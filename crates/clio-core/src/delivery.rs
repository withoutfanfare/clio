//! Verified external delivery outbox (Things / Linear).
//!
//! Clio never claims an item was delivered until the destination adapter has
//! created it AND read it back with a stable external ID. The outbox is the
//! state machine between one-tap approval and that proof:
//!
//! `pending` → `delivering` → `delivered` (verified read-back)
//!                          ↘ `failed` (retryable; `retry` returns to pending)
//!
//! A duplicate approval replays the one existing record (stable delivery
//! key). A crash after the external create leaves the row `delivering` with
//! its attempt history — visible, never silently lost. Failure leaves the
//! Clio attention item open. Credentials never enter this table; adapters
//! hold them (Keychain/environment) in the user process.
//!
//! Destination-specific API code lives in the Tauri adapters and is gated on
//! proving each product's create + read-back contract live.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{ClioError, Result};
use crate::events::{self, EventInput};
use crate::models::{new_id, now_utc};

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_DELIVERING: &str = "delivering";
pub const STATUS_DELIVERED: &str = "delivered";
pub const STATUS_FAILED: &str = "failed";

/// One outbox record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeliveryItem {
    pub id: String,
    pub delivery_key: String,
    pub attention_id: String,
    pub destination: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub external_id: Option<String>,
    pub readback: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
    pub delivered_at: Option<String>,
}

const COLUMNS: &str = "id, delivery_key, attention_id, destination, payload_json, status, \
                       attempts, last_error, external_id, readback_json, created_at, \
                       updated_at, delivered_at";

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeliveryItem> {
    let payload_json: String = row.get(4)?;
    let readback_json: Option<String> = row.get(9)?;
    Ok(DeliveryItem {
        id: row.get(0)?,
        delivery_key: row.get(1)?,
        attention_id: row.get(2)?,
        destination: row.get(3)?,
        payload: serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null),
        status: row.get(5)?,
        attempts: row.get(6)?,
        last_error: row.get(7)?,
        external_id: row.get(8)?,
        readback: readback_json.and_then(|raw| serde_json::from_str(&raw).ok()),
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        delivered_at: row.get(12)?,
    })
}

/// Fetch one outbox record by ID.
pub fn get_delivery(conn: &Connection, id: &str) -> Result<DeliveryItem> {
    let sql = format!("SELECT {COLUMNS} FROM delivery_outbox WHERE id = ?1");
    conn.query_row(&sql, params![id], row_to_item)
        .optional()?
        .ok_or_else(|| ClioError::NotFound(format!("delivery {id} not found")))
}

/// Queue one approved handoff. Idempotent: the stable key
/// `{destination}:{attention_id}` means a duplicate approval replays the
/// existing record instead of creating a second external item.
pub fn enqueue_delivery(
    conn: &Connection,
    attention_id: &str,
    destination: &str,
    payload: &serde_json::Value,
) -> Result<DeliveryItem> {
    if destination.is_empty() {
        return Err(ClioError::Validation("destination is required.".into()));
    }
    // The attention item must exist and be open work.
    let attention = crate::attention::resolve_attention(conn, attention_id)?;
    if attention.status != crate::attention::STATUS_OPEN
        && attention.status != crate::attention::STATUS_SNOOZED
    {
        return Err(ClioError::Validation(format!(
            "cannot route a '{}' attention item externally",
            attention.status
        )));
    }

    let delivery_key = format!("{destination}:{}", attention.id);

    // Race-safe idempotency: let the unique key arbitrate, then read back the
    // canonical row — a concurrent duplicate approval replays it instead of
    // surfacing a constraint error.
    let id = new_id();
    let now = now_utc();
    conn.execute(
        "INSERT OR IGNORE INTO delivery_outbox
            (id, delivery_key, attention_id, destination, payload_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            id,
            delivery_key,
            attention.id,
            destination,
            serde_json::to_string(payload)?,
            now,
        ],
    )?;
    let sql = format!("SELECT {COLUMNS} FROM delivery_outbox WHERE delivery_key = ?1");
    conn.query_row(&sql, params![delivery_key], row_to_item)
        .optional()?
        .ok_or_else(|| ClioError::Storage(format!("delivery {delivery_key} vanished after insert")))
}

/// Mark one attempt as started (before the adapter calls the destination).
/// A crash after the external create leaves the row visibly `delivering`
/// with its attempt recorded — never a silent loss, never a false success.
pub fn begin_attempt(conn: &Connection, id: &str) -> Result<DeliveryItem> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE delivery_outbox
         SET status = 'delivering', attempts = attempts + 1, updated_at = ?1
         WHERE id = ?2 AND status IN ('pending', 'failed')",
        params![now, id],
    )?;
    if changed == 0 {
        let current = get_delivery(conn, id)?;
        return Err(ClioError::Validation(format!(
            "cannot start delivery attempt from status '{}'",
            current.status
        )));
    }
    get_delivery(conn, id)
}

/// Record a verified delivery: the adapter created the item AND read it back.
/// Atomically stores the external ID + read-back evidence, attaches the
/// external reference to the attention item and records an `acted` event.
pub fn confirm_delivery(
    conn: &Connection,
    id: &str,
    external_id: &str,
    readback: &serde_json::Value,
) -> Result<DeliveryItem> {
    if external_id.is_empty() {
        return Err(ClioError::Validation(
            "a verified external id is required to confirm delivery.".into(),
        ));
    }

    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT confirm_delivery"
    })?;

    let result = (|| -> Result<DeliveryItem> {
        let item = get_delivery(conn, id)?;
        if item.status == STATUS_DELIVERED {
            // Only an exact-ID replay is benign; a different ID for the same
            // delivery is a reconciliation conflict that must surface.
            if item.external_id.as_deref() == Some(external_id) {
                return Ok(item);
            }
            return Err(ClioError::Validation(format!(
                "delivery {} is already confirmed as external id '{}'; refusing conflicting id \
                 '{external_id}'",
                item.id,
                item.external_id.as_deref().unwrap_or("?")
            )));
        }
        if item.status != STATUS_DELIVERING {
            return Err(ClioError::Validation(format!(
                "cannot confirm delivery from status '{}'",
                item.status
            )));
        }
        // One external identity maps to one delivery: a second delivery
        // claiming the same (destination, external_id) would let completion
        // mirroring resolve an arbitrary attention item. The partial unique
        // index is the backstop; this check gives an actionable error.
        let conflicting: Option<String> = conn
            .query_row(
                "SELECT id FROM delivery_outbox
                 WHERE destination = ?1 AND external_id = ?2 AND id != ?3",
                params![item.destination, external_id, item.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(other) = conflicting {
            return Err(ClioError::Validation(format!(
                "external id '{external_id}' in {} is already recorded on delivery {other}; \
                 reconcile before confirming",
                item.destination
            )));
        }
        let now = now_utc();
        conn.execute(
            "UPDATE delivery_outbox
             SET status = 'delivered', external_id = ?1, readback_json = ?2,
                 last_error = NULL, updated_at = ?3, delivered_at = ?3
             WHERE id = ?4",
            params![external_id, serde_json::to_string(readback)?, now, id],
        )?;
        crate::attention::attach_external(
            conn,
            &item.attention_id,
            &item.destination,
            external_id,
            Some("delivery"),
        )?;
        get_delivery(conn, id)
    })();

    match result {
        Ok(item) => {
            crate::db::finish_transaction(conn, owns_transaction, "confirm_delivery")?;
            Ok(item)
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, owns_transaction, "confirm_delivery");
            Err(e)
        }
    }
}

/// Record a failed or unverified attempt (network, auth, create or read-back
/// failure). The record stays retryable and the attention item stays open.
pub fn fail_delivery(conn: &Connection, id: &str, error: &str) -> Result<DeliveryItem> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE delivery_outbox
         SET status = 'failed', last_error = ?1, updated_at = ?2
         WHERE id = ?3 AND status = 'delivering'",
        params![error, now, id],
    )?;
    if changed == 0 {
        let current = get_delivery(conn, id)?;
        return Err(ClioError::Validation(format!(
            "cannot fail delivery from status '{}'",
            current.status
        )));
    }
    get_delivery(conn, id)
}

/// Return a failed record to pending for another attempt.
///
/// A crash-stuck `delivering` row is deliberately NOT retryable here: the
/// external create may have succeeded without its read-back, and another
/// attempt would create a duplicate external item. The adapter (or operator)
/// must first reconcile against the destination — read back by the request's
/// idempotent payload/lookup — then either `confirm_delivery` with the found
/// external ID or `fail_delivery` with evidence that nothing was created.
/// Delivered records are never retried.
pub fn retry_delivery(conn: &Connection, id: &str) -> Result<DeliveryItem> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE delivery_outbox
         SET status = 'pending', updated_at = ?1
         WHERE id = ?2 AND status = 'failed'",
        params![now, id],
    )?;
    if changed == 0 {
        let current = get_delivery(conn, id)?;
        if current.status == STATUS_DELIVERING {
            return Err(ClioError::Validation(
                "delivery is mid-attempt (possibly crashed after the external create); \
                 reconcile with the destination first, then confirm_delivery or \
                 fail_delivery before retrying"
                    .into(),
            ));
        }
        return Err(ClioError::Validation(format!(
            "cannot retry delivery from status '{}'",
            current.status
        )));
    }
    get_delivery(conn, id)
}

/// List outbox records, optionally filtered by status.
pub fn list_deliveries(
    conn: &Connection,
    status: Option<&str>,
    limit: u32,
) -> Result<Vec<DeliveryItem>> {
    let mut sql = format!("SELECT {COLUMNS} FROM delivery_outbox");
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(status) = status {
        args.push(Box::new(status.to_string()));
        sql.push_str(" WHERE status = ?1");
    }
    args.push(Box::new(limit));
    sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{}", args.len()));
    let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(refs.as_slice(), row_to_item)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Mirror a VERIFIED external completion back into Clio by stable external
/// reference: the delivered item was completed in Things/Linear, so the
/// attention item resolves with the read-back as evidence context. Unverified
/// disagreement is left alone — external execution state wins only when
/// identified by the stable ID Clio itself recorded.
pub fn mirror_external_completion(
    conn: &Connection,
    destination: &str,
    external_id: &str,
) -> Result<Option<crate::attention::AttentionItem>> {
    let delivery: Option<DeliveryItem> = {
        let sql = format!(
            "SELECT {COLUMNS} FROM delivery_outbox
             WHERE destination = ?1 AND external_id = ?2 AND status = 'delivered'"
        );
        conn.query_row(&sql, params![destination, external_id], row_to_item)
            .optional()?
    };
    let Some(delivery) = delivery else {
        // No verified delivery with this ID — never guess a match.
        return Ok(None);
    };

    let attention = crate::attention::get_attention(conn, &delivery.attention_id)?;
    if attention.status != crate::attention::STATUS_OPEN
        && attention.status != crate::attention::STATUS_SNOOZED
    {
        // Already terminal locally; conflicting states stay visible via the
        // event history rather than being overwritten.
        events::log_event(
            conn,
            &EventInput {
                memory_id: Some(attention.memory_id.clone()),
                namespace: Some(attention.namespace.clone()),
                actor: Some(format!("external:{destination}")),
                event_type: events::EVENT_ACKNOWLEDGED.into(),
                reason: Some(format!(
                    "external {destination} item {external_id} completed; local state already '{}'",
                    attention.status
                )),
                ..EventInput::default()
            },
        );
        return Ok(Some(attention));
    }

    let resolved = crate::attention::complete(
        conn,
        &attention.id,
        None,
        Some(&format!("completed in {destination} ({external_id})")),
        Some(&format!("external:{destination}")),
    )?;
    Ok(Some(resolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{AttentionInput, create_attention};
    use crate::models::RememberInput;
    use crate::settings::Settings;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().unwrap()
    }

    fn open_item(conn: &Connection) -> crate::attention::AttentionItem {
        let memory = crate::repository::remember(
            conn,
            &RememberInput {
                namespace: "project:delivery".into(),
                kind: "task".into(),
                title: Some("Route me to Things".into()),
                summary: None,
                content: "Verify the deployment".into(),
                tags: vec![],
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Settings::default(),
        )
        .unwrap();
        create_attention(
            conn,
            &AttentionInput {
                memory_id: memory.id.clone(),
                ..AttentionInput::default()
            },
        )
        .unwrap()
    }

    fn payload() -> serde_json::Value {
        serde_json::json!({ "title": "Verify the deployment" })
    }

    #[test]
    fn duplicate_approval_replays_one_outbox_record() {
        let conn = test_conn();
        let attention = open_item(&conn);

        let first = enqueue_delivery(&conn, &attention.id, "things", &payload()).unwrap();
        let second = enqueue_delivery(&conn, &attention.id, "things", &payload()).unwrap();
        assert_eq!(first.id, second.id, "one record per destination + item");
        assert_eq!(first.status, STATUS_PENDING);

        // A different destination is its own record.
        let linear = enqueue_delivery(&conn, &attention.id, "linear", &payload()).unwrap();
        assert_ne!(linear.id, first.id);
    }

    #[test]
    fn verified_delivery_attaches_external_reference_atomically() {
        let conn = test_conn();
        let attention = open_item(&conn);
        let item = enqueue_delivery(&conn, &attention.id, "things", &payload()).unwrap();

        begin_attempt(&conn, &item.id).unwrap();
        let delivered = confirm_delivery(
            &conn,
            &item.id,
            "things-abc",
            &serde_json::json!({ "title": "Verify the deployment", "id": "things-abc" }),
        )
        .unwrap();
        assert_eq!(delivered.status, STATUS_DELIVERED);
        assert_eq!(delivered.external_id.as_deref(), Some("things-abc"));
        assert!(delivered.readback.is_some(), "read-back evidence retained");

        let attention = crate::attention::get_attention(&conn, &attention.id).unwrap();
        assert_eq!(attention.external_system.as_deref(), Some("things"));
        assert_eq!(attention.external_ref.as_deref(), Some("things-abc"));
        assert_eq!(
            attention.status, "open",
            "delivery does not resolve the item"
        );

        // Confirming again is a replay, not an error.
        let replay =
            confirm_delivery(&conn, &item.id, "things-abc", &serde_json::json!({})).unwrap();
        assert_eq!(replay.delivered_at, delivered.delivered_at);
    }

    #[test]
    fn failed_readback_stays_retryable_and_attention_stays_open() {
        let conn = test_conn();
        let attention = open_item(&conn);
        let item = enqueue_delivery(&conn, &attention.id, "linear", &payload()).unwrap();

        begin_attempt(&conn, &item.id).unwrap();
        let failed = fail_delivery(&conn, &item.id, "read-back returned 404").unwrap();
        assert_eq!(failed.status, STATUS_FAILED);
        assert_eq!(failed.last_error.as_deref(), Some("read-back returned 404"));
        assert!(
            failed.external_id.is_none(),
            "no delivery claim without proof"
        );

        let attention = crate::attention::get_attention(&conn, &attention.id).unwrap();
        assert_eq!(
            attention.status, "open",
            "failed handoff leaves the loop open"
        );

        let retried = retry_delivery(&conn, &item.id).unwrap();
        assert_eq!(retried.status, STATUS_PENDING);
        assert_eq!(retried.attempts, 1, "attempt history is retained");
    }

    #[test]
    fn crash_after_external_create_is_visible_and_recoverable() {
        let conn = test_conn();
        let attention = open_item(&conn);
        let item = enqueue_delivery(&conn, &attention.id, "things", &payload()).unwrap();
        begin_attempt(&conn, &item.id).unwrap();

        // The process died between create and read-back: the row is visibly
        // stuck in `delivering` — never silently lost, never claimed done.
        let stuck = get_delivery(&conn, &item.id).unwrap();
        assert_eq!(stuck.status, STATUS_DELIVERING);
        assert_eq!(stuck.attempts, 1);

        // A blind retry is refused: the external create may have succeeded,
        // and another attempt would duplicate it.
        assert!(retry_delivery(&conn, &item.id).is_err());

        // Reconciliation path A: the destination has the item — confirm with
        // the found external ID; no second create ever happens.
        let delivered =
            confirm_delivery(&conn, &item.id, "things-xyz", &serde_json::json!({})).unwrap();
        assert_eq!(delivered.status, STATUS_DELIVERED);
        assert_eq!(delivered.attempts, 1);
    }

    #[test]
    fn crash_reconciled_as_not_created_can_then_retry() {
        let conn = test_conn();
        let attention = open_item(&conn);
        let item = enqueue_delivery(&conn, &attention.id, "things", &payload()).unwrap();
        begin_attempt(&conn, &item.id).unwrap();

        // Reconciliation path B: the destination shows nothing was created —
        // record that finding, then retry becomes legitimate.
        fail_delivery(&conn, &item.id, "reconciled: no external item found").unwrap();
        let retried = retry_delivery(&conn, &item.id).unwrap();
        assert_eq!(retried.status, STATUS_PENDING);
        begin_attempt(&conn, &item.id).unwrap();
        let delivered =
            confirm_delivery(&conn, &item.id, "things-second", &serde_json::json!({})).unwrap();
        assert_eq!(delivered.attempts, 2);
    }

    #[test]
    fn external_completion_mirrors_only_by_verified_stable_id() {
        let conn = test_conn();
        let attention = open_item(&conn);
        let item = enqueue_delivery(&conn, &attention.id, "things", &payload()).unwrap();
        begin_attempt(&conn, &item.id).unwrap();
        confirm_delivery(&conn, &item.id, "things-abc", &serde_json::json!({})).unwrap();

        // An unknown external ID never guesses a match.
        assert!(
            mirror_external_completion(&conn, "things", "not-ours")
                .unwrap()
                .is_none()
        );

        let resolved = mirror_external_completion(&conn, "things", "things-abc")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.status, "resolved");
    }

    #[test]
    fn conflicting_completion_state_stays_visible() {
        let conn = test_conn();
        let attention = open_item(&conn);
        let item = enqueue_delivery(&conn, &attention.id, "things", &payload()).unwrap();
        begin_attempt(&conn, &item.id).unwrap();
        confirm_delivery(&conn, &item.id, "things-abc", &serde_json::json!({})).unwrap();

        // Locally cancelled before the external completion arrives.
        crate::attention::cancel(&conn, &attention.id, Some("changed plan"), None).unwrap();
        let outcome = mirror_external_completion(&conn, "things", "things-abc")
            .unwrap()
            .unwrap();
        assert_eq!(
            outcome.status, "cancelled",
            "local terminal state not overwritten"
        );

        let attention_row = crate::attention::get_attention(&conn, &attention.id).unwrap();
        let history = events::list_events(&conn, &attention_row.memory_id, 20).unwrap();
        assert!(
            history.iter().any(|e| e
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("local state already 'cancelled'"))),
            "the disagreement is recorded, not hidden"
        );
    }

    #[test]
    fn external_identity_is_unique_and_conflicts_surface() {
        let conn = test_conn();
        let first = open_item(&conn);
        let second = open_item(&conn);

        let a = enqueue_delivery(&conn, &first.id, "things", &payload()).unwrap();
        begin_attempt(&conn, &a.id).unwrap();
        confirm_delivery(&conn, &a.id, "things-shared", &serde_json::json!({})).unwrap();

        // A second delivery may not claim the same verified identity —
        // completion mirroring must never resolve an arbitrary item.
        let b = enqueue_delivery(&conn, &second.id, "things", &payload()).unwrap();
        begin_attempt(&conn, &b.id).unwrap();
        assert!(confirm_delivery(&conn, &b.id, "things-shared", &serde_json::json!({})).is_err());

        // Replaying a confirmed delivery with a DIFFERENT id is a visible
        // reconciliation conflict, not a silent success.
        assert!(confirm_delivery(&conn, &a.id, "things-other", &serde_json::json!({})).is_err());
        // Exact-ID replay stays benign.
        assert!(confirm_delivery(&conn, &a.id, "things-shared", &serde_json::json!({})).is_ok());
    }

    #[test]
    fn routing_a_terminal_item_is_rejected() {
        let conn = test_conn();
        let attention = open_item(&conn);
        crate::attention::complete(&conn, &attention.id, None, None, None).unwrap();
        assert!(enqueue_delivery(&conn, &attention.id, "things", &payload()).is_err());
    }
}
