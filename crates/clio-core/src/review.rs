//! Review queue for low-confidence captures.
//!
//! Items that fall below the configured confidence threshold are held in
//! the review queue rather than being stored directly as memories. Users
//! can then approve, reject, or edit them before promotion to the main
//! memory store.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{ClioError, Result};
use crate::models::{Memory, new_id, now_utc};

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/// A single item in the review queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewItem {
    pub id: String,
    pub content: String,
    pub suggested_namespace: String,
    pub suggested_kind: String,
    pub suggested_title: Option<String>,
    pub suggested_summary: Option<String>,
    pub suggested_tags: Vec<String>,
    pub suggested_importance: i32,
    pub suggested_confidence: Option<f64>,
    pub source_route: Option<String>,
    pub source_ref: Option<String>,
    pub metadata: serde_json::Value,
    pub status: String,
    pub created_at: String,
    pub reviewed_at: Option<String>,
}

/// Input for creating a review queue item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewInput {
    pub content: String,
    #[serde(default = "default_namespace")]
    pub suggested_namespace: String,
    #[serde(default = "default_kind")]
    pub suggested_kind: String,
    pub suggested_title: Option<String>,
    pub suggested_summary: Option<String>,
    #[serde(default)]
    pub suggested_tags: Vec<String>,
    #[serde(default = "default_importance")]
    pub suggested_importance: i32,
    pub suggested_confidence: Option<f64>,
    pub source_route: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default = "default_metadata")]
    pub metadata: serde_json::Value,
}

/// Edits to apply to a review item. All fields are optional; only provided
/// fields are updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewEdits {
    pub namespace: Option<String>,
    pub kind: Option<String>,
    pub title: Option<Option<String>>,
    pub summary: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<i32>,
    pub confidence: Option<Option<f64>>,
}

/// Aggregated review queue statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewStats {
    pub pending: u32,
    pub approved: u32,
    pub rejected: u32,
    pub edited: u32,
    pub total: u32,
}

fn default_namespace() -> String {
    "global".into()
}

fn default_kind() -> String {
    "note".into()
}

fn default_importance() -> i32 {
    3
}

fn default_metadata() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

// ---------------------------------------------------------------------------
// Queue operations
// ---------------------------------------------------------------------------

/// Insert a new item into the review queue.
pub fn queue_for_review(conn: &Connection, input: &ReviewInput) -> Result<ReviewItem> {
    let id = new_id();
    let now = now_utc();
    let tags_text = input.suggested_tags.join(" ");
    let metadata_str = serde_json::to_string(&input.metadata)?;

    conn.execute(
        "INSERT INTO review_queue (id, content, suggested_namespace, suggested_kind,
            suggested_title, suggested_summary, suggested_tags, suggested_importance,
            suggested_confidence, source_route, source_ref, metadata_json, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'pending', ?13)",
        params![
            id,
            input.content,
            input.suggested_namespace,
            input.suggested_kind,
            input.suggested_title,
            input.suggested_summary,
            tags_text,
            input.suggested_importance,
            input.suggested_confidence,
            input.source_route,
            input.source_ref,
            metadata_str,
            now,
        ],
    )?;

    get_review(conn, &id)
}

/// List captures awaiting approval, including edited items (oldest first).
pub fn list_pending(conn: &Connection, limit: u32) -> Result<Vec<ReviewItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, suggested_namespace, suggested_kind, suggested_title,
                suggested_summary, suggested_tags, suggested_importance, suggested_confidence,
                source_route, source_ref, metadata_json, status, created_at, reviewed_at
         FROM review_queue
         WHERE status IN ('pending', 'edited')
         ORDER BY created_at ASC, id ASC
         LIMIT ?1",
    )?;

    let rows = stmt.query_map(params![limit], row_to_review_item)?;

    let mut items = Vec::new();
    for row_result in rows {
        items.push(parse_review_row(row_result?)?);
    }
    Ok(items)
}

/// Fetch a single review item by ID.
pub fn get_review(conn: &Connection, id: &str) -> Result<ReviewItem> {
    let mut stmt = conn.prepare(
        "SELECT id, content, suggested_namespace, suggested_kind, suggested_title,
                suggested_summary, suggested_tags, suggested_importance, suggested_confidence,
                source_route, source_ref, metadata_json, status, created_at, reviewed_at
         FROM review_queue
         WHERE id = ?1",
    )?;

    let raw = stmt
        .query_row(params![id], row_to_review_item)
        .optional()?
        .ok_or_else(|| ClioError::NotFound(format!("review item {id}")))?;

    parse_review_row(raw)
}

/// Approve a review item: create a Memory from the suggested fields, then
/// mark the review item as approved.
pub fn approve_review(
    conn: &Connection,
    id: &str,
    settings: &crate::settings::Settings,
) -> Result<Memory> {
    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT approve_review"
    })?;
    let result = (|| -> Result<Memory> {
        let now = now_utc();
        let changed = conn.execute(
            "UPDATE review_queue SET status = 'approved', reviewed_at = ?1
             WHERE id = ?2 AND status IN ('pending', 'edited')",
            params![now, id],
        )?;
        if changed == 0 {
            let current = get_review(conn, id)?;
            return Err(ClioError::Validation(format!(
                "cannot approve review item with status '{}'",
                current.status
            )));
        }
        let item = get_review(conn, id)?;

        let upsert = item.source_route.is_some() && item.source_ref.is_some();
        // Provenance-backed items must reach remember() so source/source_ref
        // idempotency is preserved. Unprovenanced duplicates can reuse the
        // existing memory without creating a second row.
        // Attention data queued with the item (e.g. an inferred follow-up from
        // a checkpoint) opens attention atomically with the approval.
        let attention_data = item
            .metadata
            .get("attention")
            .filter(|value| value.is_object())
            .cloned();

        // A queued suggestion whose content already had a canonical memory at
        // capture time attaches to that memory rather than storing a
        // duplicate row (the checkpoint stamped its ID into the metadata).
        if let Some(canonical) = item
            .metadata
            .get("canonical_memory_id")
            .and_then(|value| value.as_str())
        {
            if let Ok(memory) = crate::repository::get_raw(conn, canonical) {
                crate::occurrences::record_occurrence(
                    conn,
                    &memory.id,
                    item.source_route.as_deref(),
                    item.source_ref.as_deref(),
                    None,
                )?;
                open_attention_from_metadata(conn, &memory, attention_data.as_ref())?;
                return Ok(memory);
            }
            // The canonical memory has gone; fall through to normal storage.
        }

        if !upsert {
            if let Some(existing_id) = crate::repository::find_content_duplicate(
                conn,
                &item.suggested_namespace,
                &item.content,
            )? {
                let memory = crate::repository::get(conn, &existing_id)?;
                crate::occurrences::record_occurrence(
                    conn,
                    &memory.id,
                    item.source_route.as_deref(),
                    item.source_ref.as_deref(),
                    None,
                )?;
                open_attention_from_metadata(conn, &memory, attention_data.as_ref())?;
                return Ok(memory);
            }
        }

        let input = crate::models::RememberInput {
            namespace: item.suggested_namespace,
            kind: item.suggested_kind,
            title: item.suggested_title,
            summary: item.suggested_summary,
            content: item.content,
            tags: item.suggested_tags,
            source: item.source_route,
            source_ref: item.source_ref,
            confidence: item.suggested_confidence,
            importance: item.suggested_importance,
            metadata: item.metadata,
            valid_from: None,
            valid_until: None,
            upsert,
        };

        let memory = crate::repository::remember(conn, &input, settings)?;
        crate::occurrences::record_occurrence(
            conn,
            &memory.id,
            memory.source.as_deref(),
            memory.source_ref.as_deref(),
            None,
        )?;
        open_attention_from_metadata(conn, &memory, attention_data.as_ref())?;
        Ok(memory)
    })();

    let memory = match result {
        Ok(memory) => {
            crate::db::finish_transaction(conn, owns_transaction, "approve_review")?;
            memory
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, owns_transaction, "approve_review");
            return Err(e);
        }
    };

    if settings.auto_embed {
        if let Ok(backend) = crate::embeddings::create_backend(&settings.embeddings) {
            if let Err(e) = crate::embeddings::embed_and_store(conn, backend.as_ref(), &memory) {
                tracing::warn!("review approval auto-embed failed: {e}");
            }
        }
    }

    Ok(memory)
}

/// Open attention on an approved memory when the review item's metadata
/// carries an `attention` object. Shares the approval transaction.
fn open_attention_from_metadata(
    conn: &Connection,
    memory: &Memory,
    attention: Option<&serde_json::Value>,
) -> Result<()> {
    let Some(value) = attention else {
        return Ok(());
    };
    let parsed: crate::capture::DistilledAttention =
        serde_json::from_value(value.clone()).unwrap_or_default();
    crate::attention::create_attention(
        conn,
        &crate::attention::AttentionInput {
            memory_id: memory.id.clone(),
            owner: parsed.owner,
            due_at: parsed.due_at,
            remind_at: parsed.remind_at,
            trigger: parsed.trigger,
            waiting_on: parsed.waiting_on,
            completion_condition: parsed.completion_condition,
            actor: Some("review".into()),
        },
    )?;
    Ok(())
}

/// Reject a review item.
pub fn reject_review(conn: &Connection, id: &str) -> Result<ReviewItem> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE review_queue SET status = 'rejected', reviewed_at = ?1
         WHERE id = ?2 AND status IN ('pending', 'edited')",
        params![now, id],
    )?;
    if changed == 0 {
        let current = get_review(conn, id)?;
        return Err(ClioError::Validation(format!(
            "cannot reject review item with status '{}'",
            current.status
        )));
    }

    get_review(conn, id)
}

/// Edit the suggested fields of a review item. Only provided fields are
/// updated; the status is set to 'edited'.
pub fn edit_review(conn: &Connection, id: &str, edits: &ReviewEdits) -> Result<ReviewItem> {
    // Build dynamic UPDATE query from provided edits.
    let mut set_clauses = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref ns) = edits.namespace {
        let idx = param_values.len() + 1;
        set_clauses.push(format!("suggested_namespace = ?{idx}"));
        param_values.push(Box::new(ns.clone()));
    }

    if let Some(ref kind) = edits.kind {
        let idx = param_values.len() + 1;
        set_clauses.push(format!("suggested_kind = ?{idx}"));
        param_values.push(Box::new(kind.clone()));
    }

    if let Some(ref title) = edits.title {
        let idx = param_values.len() + 1;
        set_clauses.push(format!("suggested_title = ?{idx}"));
        param_values.push(Box::new(title.clone()));
    }

    if let Some(ref summary) = edits.summary {
        let idx = param_values.len() + 1;
        set_clauses.push(format!("suggested_summary = ?{idx}"));
        param_values.push(Box::new(summary.clone()));
    }

    if let Some(ref tags) = edits.tags {
        let idx = param_values.len() + 1;
        set_clauses.push(format!("suggested_tags = ?{idx}"));
        param_values.push(Box::new(tags.join(" ")));
    }

    if let Some(importance) = edits.importance {
        let idx = param_values.len() + 1;
        set_clauses.push(format!("suggested_importance = ?{idx}"));
        param_values.push(Box::new(importance));
    }

    if let Some(ref confidence) = edits.confidence {
        let idx = param_values.len() + 1;
        set_clauses.push(format!("suggested_confidence = ?{idx}"));
        param_values.push(Box::new(*confidence));
    }

    // Always set status to 'edited'.
    let idx = param_values.len() + 1;
    set_clauses.push(format!("status = ?{idx}"));
    param_values.push(Box::new("edited".to_string()));

    let idx = param_values.len() + 1;
    let sql = format!(
        "UPDATE review_queue SET {} WHERE id = ?{idx} AND status IN ('pending', 'edited')",
        set_clauses.join(", ")
    );
    param_values.push(Box::new(id.to_string()));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let changed = conn.execute(&sql, param_refs.as_slice())?;
    if changed == 0 {
        let current = get_review(conn, id)?;
        return Err(ClioError::Validation(format!(
            "cannot edit review item with status '{}'",
            current.status
        )));
    }

    get_review(conn, id)
}

/// Count review items by status.
pub fn review_stats(conn: &Connection) -> Result<ReviewStats> {
    let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM review_queue GROUP BY status")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })?;

    let mut pending = 0u32;
    let mut approved = 0u32;
    let mut rejected = 0u32;
    let mut edited = 0u32;

    for row_result in rows {
        let (status, count) = row_result?;
        match status.as_str() {
            "pending" => pending = count,
            "approved" => approved = count,
            "rejected" => rejected = count,
            "edited" => edited = count,
            _ => {}
        }
    }

    Ok(ReviewStats {
        pending,
        approved,
        rejected,
        edited,
        total: pending + approved + rejected + edited,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Raw row from the review_queue table before tag parsing.
struct ReviewItemRow {
    id: String,
    content: String,
    suggested_namespace: String,
    suggested_kind: String,
    suggested_title: Option<String>,
    suggested_summary: Option<String>,
    suggested_tags: String,
    suggested_importance: i32,
    suggested_confidence: Option<f64>,
    source_route: Option<String>,
    source_ref: Option<String>,
    metadata_json: String,
    status: String,
    created_at: String,
    reviewed_at: Option<String>,
}

fn row_to_review_item(row: &rusqlite::Row) -> rusqlite::Result<ReviewItemRow> {
    Ok(ReviewItemRow {
        id: row.get(0)?,
        content: row.get(1)?,
        suggested_namespace: row.get(2)?,
        suggested_kind: row.get(3)?,
        suggested_title: row.get(4)?,
        suggested_summary: row.get(5)?,
        suggested_tags: row.get(6)?,
        suggested_importance: row.get(7)?,
        suggested_confidence: row.get(8)?,
        source_route: row.get(9)?,
        source_ref: row.get(10)?,
        metadata_json: row.get(11)?,
        status: row.get(12)?,
        created_at: row.get(13)?,
        reviewed_at: row.get(14)?,
    })
}

/// Parse a raw row into the public `ReviewItem`, splitting tags and parsing
/// metadata JSON.
fn parse_review_row(raw: ReviewItemRow) -> Result<ReviewItem> {
    let tags: Vec<String> = raw
        .suggested_tags
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();

    let metadata: serde_json::Value = serde_json::from_str(&raw.metadata_json)?;

    Ok(ReviewItem {
        id: raw.id,
        content: raw.content,
        suggested_namespace: raw.suggested_namespace,
        suggested_kind: raw.suggested_kind,
        suggested_title: raw.suggested_title,
        suggested_summary: raw.suggested_summary,
        suggested_tags: tags,
        suggested_importance: raw.suggested_importance,
        suggested_confidence: raw.suggested_confidence,
        source_route: raw.source_route,
        source_ref: raw.source_ref,
        metadata,
        status: raw.status,
        created_at: raw.created_at,
        reviewed_at: raw.reviewed_at,
    })
}
