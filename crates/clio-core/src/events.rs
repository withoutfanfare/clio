//! Append-only memory event ledger.
//!
//! Events record what the system and the user did with memories — surfaced,
//! acknowledged, acted, snoozed, resolved — without ever mutating the memories
//! themselves or their access-ranking data. Automatic surfacing uses an
//! idempotency key so one session/topic records at most one event per reason.
//! Event writes that accompany a state change share the caller's transaction;
//! purely observational writes are fire-and-forget via [`log_event`].

use rusqlite::{Connection, params};

use crate::error::{ClioError, Result};
use crate::models::{new_id, now_utc};

/// Known event types. The vocabulary is deliberately narrow; extend the list
/// when a new consumer genuinely needs a new type.
pub const EVENT_ATTENTION_OPENED: &str = "attention_opened";
pub const EVENT_SURFACED: &str = "surfaced";
pub const EVENT_ACKNOWLEDGED: &str = "acknowledged";
pub const EVENT_ACTED: &str = "acted";
pub const EVENT_SNOOZED: &str = "snoozed";
pub const EVENT_DISMISSED: &str = "dismissed";
pub const EVENT_RESOLVED: &str = "resolved";
pub const EVENT_CANCELLED: &str = "cancelled";
pub const EVENT_EXTERNAL_ATTACHED: &str = "external_attached";
pub const EVENT_RESOLUTION_CANDIDATE: &str = "resolution_candidate";

const KNOWN_EVENT_TYPES: &[&str] = &[
    EVENT_ATTENTION_OPENED,
    EVENT_SURFACED,
    EVENT_ACKNOWLEDGED,
    EVENT_ACTED,
    EVENT_SNOOZED,
    EVENT_DISMISSED,
    EVENT_RESOLVED,
    EVENT_CANCELLED,
    EVENT_EXTERNAL_ATTACHED,
    EVENT_RESOLUTION_CANDIDATE,
];

/// Input for one event record.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EventInput {
    /// Optional dedup key. A second write with the same key is ignored.
    pub idempotency_key: Option<String>,
    pub memory_id: Option<String>,
    pub namespace: Option<String>,
    /// Who caused the event, e.g. `user`, `agent:claude`.
    pub actor: Option<String>,
    pub session_id: Option<String>,
    pub topic: Option<String>,
    pub event_type: String,
    /// Why the event happened — every reminder says why it appeared.
    pub reason: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// A stored event row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemoryEvent {
    pub id: String,
    pub idempotency_key: Option<String>,
    pub memory_id: Option<String>,
    pub namespace: Option<String>,
    pub actor: Option<String>,
    pub session_id: Option<String>,
    pub topic: Option<String>,
    pub event_type: String,
    pub reason: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: String,
}

/// Append one event. Returns `Ok(None)` when an idempotency key made the write
/// a no-op (the event was already recorded).
pub fn record_event(conn: &Connection, input: &EventInput) -> Result<Option<MemoryEvent>> {
    if !KNOWN_EVENT_TYPES.contains(&input.event_type.as_str()) {
        return Err(ClioError::Validation(format!(
            "unknown event type '{}'",
            input.event_type
        )));
    }

    let id = new_id();
    let now = now_utc();
    let metadata = if input.metadata.is_null() {
        "{}".to_string()
    } else {
        serde_json::to_string(&input.metadata)?
    };

    let changed = conn.execute(
        "INSERT OR IGNORE INTO memory_events
            (id, idempotency_key, memory_id, namespace, actor, session_id, topic,
             event_type, reason, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            input.idempotency_key,
            input.memory_id,
            input.namespace,
            input.actor,
            input.session_id,
            input.topic,
            input.event_type,
            input.reason,
            metadata,
            now,
        ],
    )?;

    if changed == 0 {
        return Ok(None);
    }

    Ok(Some(MemoryEvent {
        id,
        idempotency_key: input.idempotency_key.clone(),
        memory_id: input.memory_id.clone(),
        namespace: input.namespace.clone(),
        actor: input.actor.clone(),
        session_id: input.session_id.clone(),
        topic: input.topic.clone(),
        event_type: input.event_type.clone(),
        reason: input.reason.clone(),
        metadata: input.metadata.clone(),
        created_at: now,
    }))
}

/// Fire-and-forget event write: logs a warning on failure and never fails the
/// parent operation. Use for observational events outside a state-changing
/// transaction (e.g. `surfaced`).
pub fn log_event(conn: &Connection, input: &EventInput) {
    if let Err(e) = record_event(conn, input) {
        tracing::warn!("event tracking failed ({}): {e}", input.event_type);
    }
}

/// Whether an event with this idempotency key has already been recorded.
pub fn event_exists(conn: &Connection, idempotency_key: &str) -> Result<bool> {
    let mut stmt =
        conn.prepare_cached("SELECT 1 FROM memory_events WHERE idempotency_key = ?1 LIMIT 1")?;
    Ok(stmt.exists(params![idempotency_key])?)
}

/// List events for one memory, oldest first.
pub fn list_events(conn: &Connection, memory_id: &str, limit: u32) -> Result<Vec<MemoryEvent>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, idempotency_key, memory_id, namespace, actor, session_id, topic,
                event_type, reason, metadata_json, created_at
         FROM memory_events WHERE memory_id = ?1
         ORDER BY created_at, id LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![memory_id, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (
            id,
            idempotency_key,
            memory_id,
            namespace,
            actor,
            session_id,
            topic,
            event_type,
            reason,
            metadata_json,
            created_at,
        ) = row?;
        events.push(MemoryEvent {
            id,
            idempotency_key,
            memory_id,
            namespace,
            actor,
            session_id,
            topic,
            event_type,
            reason,
            metadata: serde_json::from_str(&metadata_json)?,
            created_at,
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().unwrap()
    }

    fn surfaced(key: &str) -> EventInput {
        EventInput {
            idempotency_key: Some(key.into()),
            event_type: EVENT_SURFACED.into(),
            reason: Some("overdue".into()),
            ..EventInput::default()
        }
    }

    #[test]
    fn idempotency_key_deduplicates_events() {
        let conn = test_conn();
        assert!(record_event(&conn, &surfaced("k1")).unwrap().is_some());
        assert!(record_event(&conn, &surfaced("k1")).unwrap().is_none());
        assert!(record_event(&conn, &surfaced("k2")).unwrap().is_some());
        assert!(event_exists(&conn, "k1").unwrap());
        assert!(!event_exists(&conn, "k3").unwrap());
    }

    #[test]
    fn unknown_event_type_is_rejected() {
        let conn = test_conn();
        let input = EventInput {
            event_type: "made_up".into(),
            ..EventInput::default()
        };
        assert!(record_event(&conn, &input).is_err());
    }

    #[test]
    fn events_without_key_always_append() {
        let conn = test_conn();
        let input = EventInput {
            memory_id: None,
            event_type: EVENT_ACKNOWLEDGED.into(),
            ..EventInput::default()
        };
        assert!(record_event(&conn, &input).unwrap().is_some());
        assert!(record_event(&conn, &input).unwrap().is_some());
    }
}
