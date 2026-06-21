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
            suggested_confidence, source_route, metadata_json, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'pending', ?12)",
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
            metadata_str,
            now,
        ],
    )?;

    get_review(conn, &id)
}

/// List pending review items, ordered by creation time (oldest first).
pub fn list_pending(conn: &Connection, limit: u32) -> Result<Vec<ReviewItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, suggested_namespace, suggested_kind, suggested_title,
                suggested_summary, suggested_tags, suggested_importance, suggested_confidence,
                source_route, metadata_json, status, created_at, reviewed_at
         FROM review_queue
         WHERE status = 'pending'
         ORDER BY created_at ASC
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
                source_route, metadata_json, status, created_at, reviewed_at
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
    let item = get_review(conn, id)?;

    if item.status != "pending" && item.status != "edited" {
        return Err(ClioError::Validation(format!(
            "cannot approve review item with status '{}'",
            item.status
        )));
    }

    // Suppress duplicate writes: if an identical, non-archived memory already
    // exists in the suggested namespace, resolve the review against it instead
    // of inserting a second copy.
    if let Some(existing_id) =
        crate::repository::find_content_duplicate(conn, &item.suggested_namespace, &item.content)?
    {
        let memory = crate::repository::get(conn, &existing_id)?;
        let now = now_utc();
        conn.execute(
            "UPDATE review_queue SET status = 'approved', reviewed_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        return Ok(memory);
    }

    let input = crate::models::RememberInput {
        namespace: item.suggested_namespace,
        kind: item.suggested_kind,
        title: item.suggested_title,
        summary: item.suggested_summary,
        content: item.content,
        tags: item.suggested_tags,
        source: Some("capture".into()),
        source_ref: None,
        confidence: item.suggested_confidence,
        importance: item.suggested_importance,
        metadata: item.metadata,
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let memory = crate::repository::remember(conn, &input, settings)?;

    let now = now_utc();
    conn.execute(
        "UPDATE review_queue SET status = 'approved', reviewed_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;

    Ok(memory)
}

/// Reject a review item.
pub fn reject_review(conn: &Connection, id: &str) -> Result<ReviewItem> {
    let item = get_review(conn, id)?;

    if item.status != "pending" && item.status != "edited" {
        return Err(ClioError::Validation(format!(
            "cannot reject review item with status '{}'",
            item.status
        )));
    }

    let now = now_utc();
    conn.execute(
        "UPDATE review_queue SET status = 'rejected', reviewed_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;

    get_review(conn, id)
}

/// Edit the suggested fields of a review item. Only provided fields are
/// updated; the status is set to 'edited'.
pub fn edit_review(conn: &Connection, id: &str, edits: &ReviewEdits) -> Result<ReviewItem> {
    let item = get_review(conn, id)?;

    if item.status != "pending" && item.status != "edited" {
        return Err(ClioError::Validation(format!(
            "cannot edit review item with status '{}'",
            item.status
        )));
    }

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
        "UPDATE review_queue SET {} WHERE id = ?{idx}",
        set_clauses.join(", ")
    );
    param_values.push(Box::new(id.to_string()));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, param_refs.as_slice())?;

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
        metadata_json: row.get(10)?,
        status: row.get(11)?,
        created_at: row.get(12)?,
        reviewed_at: row.get(13)?,
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
        metadata,
        status: raw.status,
        created_at: raw.created_at,
        reviewed_at: raw.reviewed_at,
    })
}
