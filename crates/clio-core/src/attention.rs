//! Narrow attention lifecycle for follow-up memories.
//!
//! An attention item marks one memory as operationally open: something the
//! user committed to, is waiting on, or must decide. Statuses are only
//! `open`, `snoozed`, `resolved` and `cancelled`. Completion never rewrites
//! the source memory — it records a resolution event and, when evidence is
//! supplied, a `resolved_by` link. Eligibility is a pure read that returns a
//! machine-readable reason for every item it surfaces.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{ClioError, Result};
use crate::events::{self, EventInput};
use crate::models::{new_id, now_utc};

pub const STATUS_OPEN: &str = "open";
pub const STATUS_SNOOZED: &str = "snoozed";
pub const STATUS_RESOLVED: &str = "resolved";
pub const STATUS_CANCELLED: &str = "cancelled";

/// Trigger value meaning "surface at the next session in this project".
pub const TRIGGER_PROJECT_SESSION: &str = "project-session";

/// Relationship recorded from the followed-up memory to its evidence.
pub const REL_RESOLVED_BY: &str = "resolved_by";

/// Input for opening attention on a memory.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AttentionInput {
    pub memory_id: String,
    /// Who owns the follow-up, e.g. `user`.
    pub owner: Option<String>,
    /// Hard due date (ISO-8601 UTC).
    pub due_at: Option<String>,
    /// Reminder time (ISO-8601 UTC).
    pub remind_at: Option<String>,
    /// Non-time trigger, e.g. [`TRIGGER_PROJECT_SESSION`].
    pub trigger: Option<String>,
    /// What or whom this is waiting on.
    pub waiting_on: Option<String>,
    /// What would prove this complete.
    pub completion_condition: Option<String>,
    /// Actor recorded on the initial event.
    pub actor: Option<String>,
}

/// One attention row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AttentionItem {
    pub id: String,
    pub memory_id: String,
    pub namespace: String,
    pub status: String,
    pub owner: Option<String>,
    pub due_at: Option<String>,
    pub remind_at: Option<String>,
    pub trigger: Option<String>,
    pub waiting_on: Option<String>,
    pub completion_condition: Option<String>,
    pub external_system: Option<String>,
    pub external_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

/// Why an item is eligible to surface now. Serialised snake_case so adapters
/// and hooks can act on it without string matching prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EligibilityReason {
    Overdue,
    ReminderDue,
    ProjectSession,
    Dormant,
}

impl EligibilityReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            EligibilityReason::Overdue => "overdue",
            EligibilityReason::ReminderDue => "reminder_due",
            EligibilityReason::ProjectSession => "project_session",
            EligibilityReason::Dormant => "dormant",
        }
    }
}

/// An eligible item with its reason.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EligibleAttention {
    #[serde(flatten)]
    pub item: AttentionItem,
    pub reason: EligibilityReason,
}

/// Context for one eligibility evaluation. `now` is explicit so policy is
/// testable at fixed times.
#[derive(Debug, Clone, Default)]
pub struct EligibilityContext {
    pub namespace: Option<String>,
    /// Session or topic scope used for once-per-scope surfacing.
    pub scope: Option<String>,
    pub now: String,
    pub dormant_days: u32,
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttentionItem> {
    Ok(AttentionItem {
        id: row.get(0)?,
        memory_id: row.get(1)?,
        namespace: row.get(2)?,
        status: row.get(3)?,
        owner: row.get(4)?,
        due_at: row.get(5)?,
        remind_at: row.get(6)?,
        trigger: row.get(7)?,
        waiting_on: row.get(8)?,
        completion_condition: row.get(9)?,
        external_system: row.get(10)?,
        external_ref: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        resolved_at: row.get(14)?,
    })
}

const ITEM_COLUMNS: &str = "id, memory_id, namespace, status, owner, due_at, remind_at, \
                            trigger_kind, waiting_on, completion_condition, external_system, \
                            external_ref, created_at, updated_at, resolved_at";

/// Fetch an attention item by its ID.
pub fn get_attention(conn: &Connection, id: &str) -> Result<AttentionItem> {
    let sql = format!("SELECT {ITEM_COLUMNS} FROM attention_items WHERE id = ?1");
    conn.query_row(&sql, params![id], row_to_item)
        .optional()?
        .ok_or_else(|| ClioError::NotFound(format!("attention item {id} not found")))
}

/// Fetch the attention item attached to a memory, if any.
pub fn get_by_memory(conn: &Connection, memory_id: &str) -> Result<Option<AttentionItem>> {
    let sql = format!("SELECT {ITEM_COLUMNS} FROM attention_items WHERE memory_id = ?1");
    Ok(conn
        .query_row(&sql, params![memory_id], row_to_item)
        .optional()?)
}

/// Resolve either an attention ID or a memory ID to the attention item.
pub fn resolve_attention(conn: &Connection, id: &str) -> Result<AttentionItem> {
    match get_attention(conn, id) {
        Ok(item) => Ok(item),
        Err(ClioError::NotFound(_)) => get_by_memory(conn, id)?
            .ok_or_else(|| ClioError::NotFound(format!("no attention item for id or memory {id}"))),
        Err(e) => Err(e),
    }
}

/// Open attention on a memory. Idempotent: if the memory already has an
/// attention item (any status), that item is returned unchanged. The row and
/// its initial event commit atomically.
pub fn create_attention(conn: &Connection, input: &AttentionInput) -> Result<AttentionItem> {
    if input.memory_id.is_empty() {
        return Err(ClioError::Validation("memory_id is required.".into()));
    }

    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT create_attention"
    })?;

    let result = (|| -> Result<AttentionItem> {
        if let Some(existing) = get_by_memory(conn, &input.memory_id)? {
            return Ok(existing);
        }

        // The memory must exist and provides the namespace. Untracked read:
        // opening attention is not a recall.
        let memory = crate::repository::get_raw(conn, &input.memory_id)?;

        let id = new_id();
        let now = now_utc();
        conn.execute(
            "INSERT INTO attention_items
                (id, memory_id, namespace, status, owner, due_at, remind_at, trigger_kind,
                 waiting_on, completion_condition, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'open', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                id,
                input.memory_id,
                memory.namespace,
                input.owner,
                input.due_at,
                input.remind_at,
                input.trigger,
                input.waiting_on,
                input.completion_condition,
                now,
            ],
        )?;

        events::record_event(
            conn,
            &EventInput {
                memory_id: Some(input.memory_id.clone()),
                namespace: Some(memory.namespace.clone()),
                actor: input.actor.clone(),
                event_type: events::EVENT_ATTENTION_OPENED.into(),
                ..EventInput::default()
            },
        )?;

        get_attention(conn, &id)
    })();

    match result {
        Ok(item) => {
            crate::db::finish_transaction(conn, owns_transaction, "create_attention")?;
            Ok(item)
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, owns_transaction, "create_attention");
            Err(e)
        }
    }
}

/// List attention items, optionally filtered by namespace and status.
pub fn list_attention(
    conn: &Connection,
    namespace: Option<&str>,
    status: Option<&str>,
    limit: u32,
) -> Result<Vec<AttentionItem>> {
    let mut sql = format!("SELECT {ITEM_COLUMNS} FROM attention_items WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(ns) = namespace {
        args.push(Box::new(ns.to_string()));
        sql.push_str(&format!(" AND namespace = ?{}", args.len()));
    }
    if let Some(st) = status {
        args.push(Box::new(st.to_string()));
        sql.push_str(&format!(" AND status = ?{}", args.len()));
    }
    args.push(Box::new(limit));
    sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{}", args.len()));

    let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(refs.as_slice(), row_to_item)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// One guarded status transition + event, in one transaction.
#[allow(clippy::too_many_arguments)]
fn transition(
    conn: &Connection,
    id: &str,
    allowed_from: &[&str],
    to_status: &str,
    set_extra: &str,
    extra_params: &[&dyn rusqlite::types::ToSql],
    event_type: &str,
    reason: Option<&str>,
    actor: Option<&str>,
) -> Result<AttentionItem> {
    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT attention_transition"
    })?;

    let result = (|| -> Result<AttentionItem> {
        let item = resolve_attention(conn, id)?;
        if !allowed_from.contains(&item.status.as_str()) {
            return Err(ClioError::Validation(format!(
                "cannot move attention item {} from '{}' to '{}'",
                item.id, item.status, to_status
            )));
        }

        let now = now_utc();
        let sql = format!(
            "UPDATE attention_items SET status = ?1, updated_at = ?2{set_extra} WHERE id = ?3"
        );
        let mut args: Vec<&dyn rusqlite::types::ToSql> = vec![&to_status, &now, &item.id];
        args.extend_from_slice(extra_params);
        conn.execute(&sql, args.as_slice())?;

        events::record_event(
            conn,
            &EventInput {
                memory_id: Some(item.memory_id.clone()),
                namespace: Some(item.namespace.clone()),
                actor: actor.map(String::from),
                event_type: event_type.into(),
                reason: reason.map(String::from),
                ..EventInput::default()
            },
        )?;

        get_attention(conn, &item.id)
    })();

    match result {
        Ok(item) => {
            crate::db::finish_transaction(conn, owns_transaction, "attention_transition")?;
            Ok(item)
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, owns_transaction, "attention_transition");
            Err(e)
        }
    }
}

/// Snooze an open or snoozed item until `until` (ISO-8601 UTC).
pub fn snooze(
    conn: &Connection,
    id: &str,
    until: &str,
    actor: Option<&str>,
) -> Result<AttentionItem> {
    if until.is_empty() {
        return Err(ClioError::Validation("snooze time is required.".into()));
    }
    transition(
        conn,
        id,
        &[STATUS_OPEN, STATUS_SNOOZED],
        STATUS_SNOOZED,
        ", remind_at = ?4",
        &[&until],
        events::EVENT_SNOOZED,
        Some(until),
        actor,
    )
}

/// Resolve an open or snoozed item. The source memory is never rewritten;
/// supplied evidence is recorded as a `resolved_by` link from the followed-up
/// memory to the evidence memory, inside the same transaction.
pub fn complete(
    conn: &Connection,
    id: &str,
    evidence_memory_id: Option<&str>,
    reason: Option<&str>,
    actor: Option<&str>,
) -> Result<AttentionItem> {
    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT attention_complete"
    })?;

    let result = (|| -> Result<AttentionItem> {
        let now = now_utc();
        let item = transition(
            conn,
            id,
            &[STATUS_OPEN, STATUS_SNOOZED],
            STATUS_RESOLVED,
            ", resolved_at = ?4",
            &[&now],
            events::EVENT_RESOLVED,
            reason,
            actor,
        )?;

        if let Some(evidence) = evidence_memory_id {
            crate::repository::link(
                conn,
                &crate::models::LinkInput {
                    from_memory_id: item.memory_id.clone(),
                    to_memory_id: evidence.to_string(),
                    relationship: REL_RESOLVED_BY.into(),
                    metadata: serde_json::json!({}),
                },
            )?;
        }
        Ok(item)
    })();

    match result {
        Ok(item) => {
            crate::db::finish_transaction(conn, owns_transaction, "attention_complete")?;
            Ok(item)
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, owns_transaction, "attention_complete");
            Err(e)
        }
    }
}

/// Cancel an open or snoozed item.
pub fn cancel(
    conn: &Connection,
    id: &str,
    reason: Option<&str>,
    actor: Option<&str>,
) -> Result<AttentionItem> {
    transition(
        conn,
        id,
        &[STATUS_OPEN, STATUS_SNOOZED],
        STATUS_CANCELLED,
        "",
        &[],
        events::EVENT_CANCELLED,
        reason,
        actor,
    )
}

/// Attach a verified external reference (e.g. a Things or Linear item).
pub fn attach_external(
    conn: &Connection,
    id: &str,
    system: &str,
    external_ref: &str,
    actor: Option<&str>,
) -> Result<AttentionItem> {
    if system.is_empty() || external_ref.is_empty() {
        return Err(ClioError::Validation(
            "external system and reference are required.".into(),
        ));
    }
    transition(
        conn,
        id,
        &[STATUS_OPEN, STATUS_SNOOZED],
        STATUS_OPEN,
        ", external_system = ?4, external_ref = ?5",
        &[&system, &external_ref],
        events::EVENT_EXTERNAL_ATTACHED,
        Some(external_ref),
        actor,
    )
}

/// Idempotency key for surfacing `item` in `scope` for `reason`. The item's
/// `updated_at` is part of the key, so a state change re-arms surfacing.
pub fn surfaced_key(item: &AttentionItem, scope: &str, reason: &str) -> String {
    format!(
        "surfaced:{}:{}:{}:{}",
        item.memory_id, scope, reason, item.updated_at
    )
}

/// Record that an eligible item was actually surfaced. Idempotent per
/// scope/reason/state; fire-and-forget (never fails the surfacing read).
pub fn record_surfaced(
    conn: &Connection,
    item: &AttentionItem,
    scope: &str,
    reason: &str,
    actor: Option<&str>,
) {
    events::log_event(
        conn,
        &EventInput {
            idempotency_key: Some(surfaced_key(item, scope, reason)),
            memory_id: Some(item.memory_id.clone()),
            namespace: Some(item.namespace.clone()),
            actor: actor.map(String::from),
            session_id: Some(scope.to_string()),
            event_type: events::EVENT_SURFACED.into(),
            reason: Some(reason.to_string()),
            ..EventInput::default()
        },
    );
}

/// Pure read: which items deserve attention now, and why.
///
/// Rules, in order of precedence per item:
/// - snoozed items are eligible only once `remind_at` has passed (`reminder_due`)
/// - open items past `due_at` are `overdue`
/// - open items past `remind_at` are `reminder_due`
/// - open items with a `project-session` trigger surface in their project (`project_session`)
/// - open items untouched for `dormant_days` are `dormant`
///
/// Items already surfaced in `ctx.scope` for the same reason and state are
/// skipped, so one session/topic sees an item at most once unless its state
/// changes. This function never mutates anything — including access ranking.
pub fn eligible(conn: &Connection, ctx: &EligibilityContext) -> Result<Vec<EligibleAttention>> {
    if ctx.now.is_empty() {
        return Err(ClioError::Validation(
            "eligibility requires an explicit now timestamp.".into(),
        ));
    }

    let candidates = list_attention(conn, ctx.namespace.as_deref(), None, 500)?;
    let mut eligible_items = Vec::new();

    for item in candidates {
        let reason = match item.status.as_str() {
            STATUS_SNOOZED => match &item.remind_at {
                Some(remind) if remind.as_str() <= ctx.now.as_str() => {
                    Some(EligibilityReason::ReminderDue)
                }
                _ => None,
            },
            STATUS_OPEN => {
                if item
                    .due_at
                    .as_deref()
                    .is_some_and(|due| due <= ctx.now.as_str())
                {
                    Some(EligibilityReason::Overdue)
                } else if item
                    .remind_at
                    .as_deref()
                    .is_some_and(|remind| remind <= ctx.now.as_str())
                {
                    Some(EligibilityReason::ReminderDue)
                } else if item.trigger.as_deref() == Some(TRIGGER_PROJECT_SESSION)
                    && ctx.namespace.as_deref() == Some(item.namespace.as_str())
                {
                    Some(EligibilityReason::ProjectSession)
                } else if ctx.dormant_days > 0
                    && dormant_before(&ctx.now, ctx.dormant_days).as_str()
                        > item.updated_at.as_str()
                {
                    Some(EligibilityReason::Dormant)
                } else {
                    None
                }
            }
            _ => None,
        };

        let Some(reason) = reason else { continue };

        if let Some(scope) = &ctx.scope {
            if events::event_exists(conn, &surfaced_key(&item, scope, reason.as_str()))? {
                continue;
            }
        }

        eligible_items.push(EligibleAttention { item, reason });
    }

    Ok(eligible_items)
}

/// ISO-8601 timestamp `days` before `now`. Falls back to `now` on a
/// malformed input timestamp (never panics inside eligibility).
fn dormant_before(now: &str, days: u32) -> String {
    use time::format_description::well_known::Rfc3339;
    match time::OffsetDateTime::parse(now, &Rfc3339) {
        Ok(ts) => {
            let earlier = ts - time::Duration::days(i64::from(days));
            earlier.format(&Rfc3339).unwrap_or_else(|_| now.to_string())
        }
        Err(_) => now.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RememberInput;
    use crate::settings::Settings;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().unwrap()
    }

    fn remember(conn: &Connection, content: &str) -> crate::models::Memory {
        crate::repository::remember(
            conn,
            &RememberInput {
                namespace: "project:attention-test".into(),
                kind: "task".into(),
                title: Some(content.into()),
                summary: None,
                content: content.into(),
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
        .unwrap()
    }

    fn open_attention(conn: &Connection, memory_id: &str) -> AttentionItem {
        create_attention(
            conn,
            &AttentionInput {
                memory_id: memory_id.into(),
                owner: Some("user".into()),
                ..AttentionInput::default()
            },
        )
        .unwrap()
    }

    const NOW: &str = "2026-07-29T12:00:00Z";

    fn ctx(namespace: Option<&str>, scope: Option<&str>) -> EligibilityContext {
        EligibilityContext {
            namespace: namespace.map(String::from),
            scope: scope.map(String::from),
            now: NOW.into(),
            dormant_days: 0,
        }
    }

    #[test]
    fn attention_creation_is_idempotent_and_audited() {
        let conn = test_conn();
        let memory = remember(&conn, "Follow up with the review");

        let first = open_attention(&conn, &memory.id);
        let second = open_attention(&conn, &memory.id);
        assert_eq!(first.id, second.id);
        assert_eq!(first.status, STATUS_OPEN);
        assert_eq!(first.namespace, "project:attention-test");

        let events = events::list_events(&conn, &memory.id, 10).unwrap();
        assert_eq!(events.len(), 1, "idempotent creation records one event");
        assert_eq!(events[0].event_type, "attention_opened");
    }

    #[test]
    fn creation_requires_an_existing_memory() {
        let conn = test_conn();
        let result = create_attention(
            &conn,
            &AttentionInput {
                memory_id: "missing".into(),
                ..AttentionInput::default()
            },
        );
        assert!(result.is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM memory_events", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "failed creation must not leak events"
        );
    }

    #[test]
    fn due_and_reminder_items_become_eligible() {
        let conn = test_conn();
        let due = remember(&conn, "Overdue item");
        let later = remember(&conn, "Not yet due");
        create_attention(
            &conn,
            &AttentionInput {
                memory_id: due.id.clone(),
                due_at: Some("2026-07-28T00:00:00Z".into()),
                ..AttentionInput::default()
            },
        )
        .unwrap();
        create_attention(
            &conn,
            &AttentionInput {
                memory_id: later.id.clone(),
                due_at: Some("2026-08-05T00:00:00Z".into()),
                ..AttentionInput::default()
            },
        )
        .unwrap();

        let eligible_items = eligible(&conn, &ctx(None, None)).unwrap();
        assert_eq!(eligible_items.len(), 1);
        assert_eq!(eligible_items[0].item.memory_id, due.id);
        assert_eq!(eligible_items[0].reason, EligibilityReason::Overdue);
    }

    #[test]
    fn project_session_trigger_surfaces_in_matching_namespace_only() {
        let conn = test_conn();
        let memory = remember(&conn, "Pick this up next session");
        create_attention(
            &conn,
            &AttentionInput {
                memory_id: memory.id.clone(),
                trigger: Some(TRIGGER_PROJECT_SESSION.into()),
                ..AttentionInput::default()
            },
        )
        .unwrap();

        let same_project = eligible(&conn, &ctx(Some("project:attention-test"), None)).unwrap();
        assert_eq!(same_project.len(), 1);
        assert_eq!(same_project[0].reason, EligibilityReason::ProjectSession);

        let other_project = eligible(&conn, &ctx(Some("project:other"), None)).unwrap();
        assert!(other_project.is_empty());
    }

    #[test]
    fn snooze_hides_until_expiry_then_resurfaces() {
        let conn = test_conn();
        let memory = remember(&conn, "Snoozed follow-up");
        let item = open_attention(&conn, &memory.id);

        // Snoozed into the future: not eligible.
        snooze(&conn, &item.id, "2026-08-01T00:00:00Z", Some("user")).unwrap();
        assert!(eligible(&conn, &ctx(None, None)).unwrap().is_empty());

        // Snooze expiry has passed: eligible again with reminder_due.
        snooze(&conn, &item.id, "2026-07-29T00:00:00Z", Some("user")).unwrap();
        let items = eligible(&conn, &ctx(None, None)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].reason, EligibilityReason::ReminderDue);
    }

    #[test]
    fn surfacing_is_once_per_scope_until_state_changes() {
        let conn = test_conn();
        let memory = remember(&conn, "Surface once");
        create_attention(
            &conn,
            &AttentionInput {
                memory_id: memory.id.clone(),
                due_at: Some("2026-07-28T00:00:00Z".into()),
                ..AttentionInput::default()
            },
        )
        .unwrap();

        let context = ctx(None, Some("session-a"));
        let first = eligible(&conn, &context).unwrap();
        assert_eq!(first.len(), 1);
        record_surfaced(
            &conn,
            &first[0].item,
            "session-a",
            first[0].reason.as_str(),
            None,
        );

        // Same scope, same state: suppressed.
        assert!(eligible(&conn, &context).unwrap().is_empty());
        // Different scope still sees it.
        assert_eq!(
            eligible(&conn, &ctx(None, Some("session-b")))
                .unwrap()
                .len(),
            1
        );

        // A state change re-arms the same scope.
        let item = resolve_attention(&conn, &memory.id).unwrap();
        snooze(&conn, &item.id, "2026-07-28T00:00:00Z", None).unwrap();
        assert_eq!(eligible(&conn, &context).unwrap().len(), 1);
    }

    #[test]
    fn dormant_items_surface_after_the_configured_gap() {
        let conn = test_conn();
        let memory = remember(&conn, "Quietly forgotten");
        open_attention(&conn, &memory.id);

        let mut context = ctx(None, None);
        context.dormant_days = 14;
        // Freshly created: not dormant.
        assert!(eligible(&conn, &context).unwrap().is_empty());

        // Age the row artificially.
        conn.execute(
            "UPDATE attention_items SET updated_at = '2026-06-01T00:00:00Z'",
            [],
        )
        .unwrap();
        let items = eligible(&conn, &context).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].reason, EligibilityReason::Dormant);
    }

    #[test]
    fn complete_records_resolution_and_keeps_history() {
        let conn = test_conn();
        let memory = remember(&conn, "Ship the fix");
        let evidence = remember(&conn, "Fix shipped in commit abc123");
        let item = open_attention(&conn, &memory.id);

        let resolved = complete(
            &conn,
            &item.id,
            Some(&evidence.id),
            Some("shipped"),
            Some("user"),
        )
        .unwrap();
        assert_eq!(resolved.status, STATUS_RESOLVED);
        assert!(resolved.resolved_at.is_some());

        // Source memory untouched; resolution recorded as link + event.
        let unchanged = crate::repository::get(&conn, &memory.id).unwrap();
        assert_eq!(unchanged.content, "Ship the fix");
        let links = crate::repository::get_links(&conn, &memory.id).unwrap();
        assert!(
            links
                .iter()
                .any(|l| l.relationship == REL_RESOLVED_BY && l.to_memory_id == evidence.id)
        );
        let history = events::list_events(&conn, &memory.id, 10).unwrap();
        assert!(history.iter().any(|e| e.event_type == "resolved"));
        assert!(history.iter().any(|e| e.event_type == "attention_opened"));

        // Resolved items are terminal.
        assert!(complete(&conn, &item.id, None, None, None).is_err());
        assert!(snooze(&conn, &item.id, "2026-08-01T00:00:00Z", None).is_err());
        assert!(cancel(&conn, &item.id, None, None).is_err());
    }

    #[test]
    fn cancel_is_terminal_and_audited() {
        let conn = test_conn();
        let memory = remember(&conn, "No longer needed");
        let item = open_attention(&conn, &memory.id);

        let cancelled = cancel(&conn, &item.id, Some("obsolete"), Some("user")).unwrap();
        assert_eq!(cancelled.status, STATUS_CANCELLED);
        assert!(eligible(&conn, &ctx(None, None)).unwrap().is_empty());
        assert!(complete(&conn, &item.id, None, None, None).is_err());
    }

    #[test]
    fn attach_external_keeps_the_item_open() {
        let conn = test_conn();
        let memory = remember(&conn, "Route to Things");
        let item = open_attention(&conn, &memory.id);

        let attached =
            attach_external(&conn, &item.id, "things", "things-id-1", Some("user")).unwrap();
        assert_eq!(attached.status, STATUS_OPEN);
        assert_eq!(attached.external_system.as_deref(), Some("things"));
        assert_eq!(attached.external_ref.as_deref(), Some("things-id-1"));
    }

    #[test]
    fn commands_accept_memory_id_as_an_alias() {
        let conn = test_conn();
        let memory = remember(&conn, "Aliased by memory id");
        open_attention(&conn, &memory.id);
        let via_memory = resolve_attention(&conn, &memory.id).unwrap();
        assert_eq!(via_memory.memory_id, memory.id);
        let resolved = complete(&conn, &memory.id, None, None, None).unwrap();
        assert_eq!(resolved.status, STATUS_RESOLVED);
    }
}
