use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{ClioError, Result};
use crate::models::*;
use crate::validate;

// ---------------------------------------------------------------------------
// Remember (insert / upsert)
// ---------------------------------------------------------------------------

/// Store a new memory or upsert an existing one.
///
/// The insert (or update) and tag writes are wrapped in a savepoint so that
/// a failure in any step rolls back the entire operation atomically.
pub fn remember(
    conn: &Connection,
    input: &RememberInput,
    settings: &crate::settings::Settings,
) -> Result<Memory> {
    validate::remember_input(input)?;

    let tags = normalise_tags(&input.tags);
    let tags_text = tags.join(" ");
    let metadata_str = serde_json::to_string(&input.metadata)?;
    let now = now_utc();

    // Resolve the title: explicit > AI-generated > string-based extraction.
    let title = crate::title::resolve_title(input.title.clone(), &input.content, settings);

    // Let SQLite arbitrate source/reference conflicts atomically. A separate
    // SELECT-then-INSERT races when two MCP processes create the same source
    // reference at once.
    if input.upsert && input.source.is_some() && input.source_ref.is_some() {
        let proposed_id = new_id();
        let owns_transaction = conn.is_autocommit();
        conn.execute_batch(if owns_transaction {
            "BEGIN IMMEDIATE"
        } else {
            "SAVEPOINT remember_upsert"
        })?;
        let result = (|| -> Result<Memory> {
            let stored_id: String = conn.query_row(
                "INSERT INTO memories (id, namespace, kind, title, summary, content, tags_text,
                    source, source_ref, confidence, importance, metadata_json,
                    valid_from, valid_until, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT DO UPDATE SET
                    namespace = excluded.namespace,
                    kind = excluded.kind,
                    title = excluded.title,
                    summary = excluded.summary,
                    content = excluded.content,
                    tags_text = excluded.tags_text,
                    source = excluded.source,
                    source_ref = excluded.source_ref,
                    confidence = excluded.confidence,
                    importance = excluded.importance,
                    metadata_json = excluded.metadata_json,
                    valid_from = excluded.valid_from,
                    valid_until = excluded.valid_until,
                    updated_at = excluded.updated_at
                 RETURNING id",
                params![
                    proposed_id,
                    input.namespace,
                    input.kind,
                    title,
                    input.summary,
                    input.content,
                    tags_text,
                    input.source,
                    input.source_ref,
                    input.confidence,
                    input.importance,
                    metadata_str,
                    input.valid_from,
                    input.valid_until,
                    now,
                    now,
                ],
                |row| row.get(0),
            )?;

            conn.execute(
                "DELETE FROM memory_tags WHERE memory_id = ?1",
                params![stored_id],
            )?;
            insert_tags(conn, &stored_id, &tags, &now)?;
            get_raw(conn, &stored_id)
        })();

        return match result {
            Ok(memory) => {
                crate::db::finish_transaction(conn, owns_transaction, "remember_upsert")?;
                Ok(memory)
            }
            Err(e) => {
                crate::db::rollback_transaction(conn, owns_transaction, "remember_upsert");
                Err(e)
            }
        };
    }

    let id = new_id();

    conn.execute_batch("SAVEPOINT remember_insert")?;
    let result = (|| -> Result<()> {
        conn.execute(
            "INSERT INTO memories (id, namespace, kind, title, summary, content, tags_text,
                source, source_ref, confidence, importance, metadata_json,
                valid_from, valid_until, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                id,
                input.namespace,
                input.kind,
                title,
                input.summary,
                input.content,
                tags_text,
                input.source,
                input.source_ref,
                input.confidence,
                input.importance,
                metadata_str,
                input.valid_from,
                input.valid_until,
                now,
                now,
            ],
        )?;
        insert_tags(conn, &id, &tags, &now)?;
        Ok(())
    })();

    match result {
        Ok(()) => crate::db::finish_transaction(conn, false, "remember_insert")?,
        Err(e) => {
            crate::db::rollback_transaction(conn, false, "remember_insert");
            return Err(e);
        }
    }

    get_raw(conn, &id)
}

#[allow(clippy::too_many_arguments)]
fn update_existing(
    conn: &Connection,
    id: &str,
    input: &RememberInput,
    tags: &[String],
    tags_text: &str,
    metadata_str: &str,
    now: &str,
    expected_updated_at: Option<&str>,
) -> Result<Memory> {
    let changed = conn.execute(
        "UPDATE memories SET namespace = ?1, kind = ?2, title = ?3, summary = ?4,
            content = ?5, tags_text = ?6, source = ?7, source_ref = ?8,
            confidence = ?9, importance = ?10, metadata_json = ?11,
            valid_from = ?12, valid_until = ?13, updated_at = ?14
         WHERE id = ?15 AND (?16 IS NULL OR updated_at = ?16)",
        params![
            input.namespace,
            input.kind,
            input.title,
            input.summary,
            input.content,
            tags_text,
            input.source,
            input.source_ref,
            input.confidence,
            input.importance,
            metadata_str,
            input.valid_from,
            input.valid_until,
            now,
            id,
            expected_updated_at,
        ],
    )?;
    if changed == 0 {
        return Err(ClioError::Conflict(format!(
            "memory {id} changed after it was read; fetch the latest record and retry"
        )));
    }

    // Replace tags after the guarded record update succeeds.
    conn.execute("DELETE FROM memory_tags WHERE memory_id = ?1", params![id])?;
    insert_tags(conn, id, tags, now)?;
    get_raw(conn, id)
}

fn find_by_source_ref(conn: &Connection, source: &str, source_ref: &str) -> Result<Option<String>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM memories WHERE source = ?1 AND source_ref = ?2",
            params![source, source_ref],
            |row| row.get(0),
        )
        .optional()?;
    Ok(id)
}

/// Fetch a memory by its `(source, source_ref)` provenance pair, if present.
pub fn get_by_source_ref(
    conn: &Connection,
    source: &str,
    source_ref: &str,
) -> Result<Option<Memory>> {
    match find_by_source_ref(conn, source, source_ref)? {
        Some(id) => Ok(Some(get(conn, &id)?)),
        None => Ok(None),
    }
}

fn insert_tags(conn: &Connection, memory_id: &str, tags: &[String], now: &str) -> Result<()> {
    if tags.is_empty() {
        return Ok(());
    }

    // Single multi-row INSERT instead of per-tag loop.
    let row_placeholders: Vec<String> = (0..tags.len())
        .map(|i| {
            let base = i * 3 + 1;
            format!("(?{}, ?{}, ?{})", base, base + 1, base + 2)
        })
        .collect();

    let sql = format!(
        "INSERT OR IGNORE INTO memory_tags (memory_id, tag, created_at) VALUES {}",
        row_placeholders.join(", ")
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::with_capacity(tags.len() * 3);
    for tag in tags {
        param_values.push(Box::new(memory_id.to_string()));
        param_values.push(Box::new(tag.clone()));
        param_values.push(Box::new(now.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, param_refs.as_slice())?;

    Ok(())
}

fn normalise_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty() && seen.insert(t.clone()))
        .collect();
    out.sort_unstable();
    out
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

/// Update an existing memory by ID with the provided fields.
pub fn update(
    conn: &Connection,
    id: &str,
    patch: &UpdateInput,
    _settings: &crate::settings::Settings,
) -> Result<Memory> {
    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT memory_patch"
    })?;

    let result = (|| -> Result<Memory> {
        let existing = get_raw(conn, id)?;
        if patch
            .expected_updated_at
            .as_deref()
            .is_some_and(|expected| expected != existing.updated_at)
        {
            return Err(ClioError::Conflict(format!(
                "memory {id} changed after it was read; fetch the latest record and retry"
            )));
        }
        if patch.namespace.is_none()
            && patch.kind.is_none()
            && patch.title.is_none()
            && patch.summary.is_none()
            && patch.content.is_none()
            && patch.tags.is_none()
            && patch.source.is_none()
            && patch.source_ref.is_none()
            && patch.confidence.is_none()
            && patch.importance.is_none()
            && patch.metadata.is_none()
            && patch.valid_from.is_none()
            && patch.valid_until.is_none()
        {
            return Ok(existing);
        }

        let input = RememberInput {
            namespace: patch
                .namespace
                .clone()
                .unwrap_or_else(|| existing.namespace.clone()),
            kind: patch.kind.clone().unwrap_or_else(|| existing.kind.clone()),
            title: patch.title.clone().unwrap_or(existing.title),
            summary: patch.summary.clone().unwrap_or(existing.summary),
            content: patch
                .content
                .clone()
                .unwrap_or_else(|| existing.content.clone()),
            tags: patch.tags.clone().unwrap_or(existing.tags),
            source: patch.source.clone().unwrap_or(existing.source),
            source_ref: patch.source_ref.clone().unwrap_or(existing.source_ref),
            confidence: patch.confidence.unwrap_or(existing.confidence),
            importance: patch.importance.unwrap_or(existing.importance),
            metadata: patch.metadata.clone().unwrap_or(existing.metadata),
            valid_from: patch.valid_from.clone().unwrap_or(existing.valid_from),
            valid_until: patch.valid_until.clone().unwrap_or(existing.valid_until),
            upsert: false,
        };
        validate::remember_input(&input)?;

        let tags = normalise_tags(&input.tags);
        let tags_text = tags.join(" ");
        let metadata_str = serde_json::to_string(&input.metadata)?;
        update_existing(
            conn,
            id,
            &input,
            &tags,
            &tags_text,
            &metadata_str,
            &now_utc(),
            patch.expected_updated_at.as_deref(),
        )
    })();

    match result {
        Ok(memory) => {
            crate::db::finish_transaction(conn, owns_transaction, "memory_patch")?;
            Ok(memory)
        }
        Err(error) => {
            crate::db::rollback_transaction(conn, owns_transaction, "memory_patch");
            Err(error)
        }
    }
}

// ---------------------------------------------------------------------------
// Get
// ---------------------------------------------------------------------------

/// Fetch a single memory by id.
/// Return the id of a non-archived memory in `namespace` with byte-identical
/// content, if one exists. Used to suppress duplicate writes on the capture and
/// review-approval paths. Archived memories are excluded here; the capture path
/// handles an archived twin separately via [`find_archived_duplicate`], reviving
/// it rather than creating a fresh live row.
pub fn find_content_duplicate(
    conn: &Connection,
    namespace: &str,
    content: &str,
) -> Result<Option<String>> {
    // `length(content) = length(?2)` lets the (namespace, length(content)) index
    // prune candidates before the full content comparison.
    let id = conn
        .query_row(
            "SELECT id FROM memories
             WHERE namespace = ?1 AND length(content) = length(?2) AND content = ?2
               AND archived_at IS NULL
             ORDER BY created_at ASC LIMIT 1",
            params![namespace, content],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(id)
}

/// Return the id of an **archived** memory in `namespace` with byte-identical
/// content, if one exists (and no live twin does). The capture path uses this to
/// revive an archived duplicate rather than creating a fresh live row.
pub fn find_archived_duplicate(
    conn: &Connection,
    namespace: &str,
    content: &str,
) -> Result<Option<String>> {
    let id = conn
        .query_row(
            "SELECT id FROM memories
             WHERE namespace = ?1 AND length(content) = length(?2) AND content = ?2
               AND archived_at IS NOT NULL
             ORDER BY created_at ASC LIMIT 1",
            params![namespace, content],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Memory> {
    let memory = get_raw(conn, id)?;

    // Fire-and-forget access tracking.
    if let Err(e) = touch_accessed(conn, &[id]) {
        tracing::warn!("access tracking failed in get: {e}");
    }

    Ok(memory)
}

/// Fetch a single memory by id without triggering access tracking.
///
/// Use this for internal lookups (existence checks, linked memory resolution)
/// where we don't want to inflate access_count.
pub(crate) fn get_raw(conn: &Connection, id: &str) -> Result<Memory> {
    let mut stmt = conn.prepare(
        "SELECT id, namespace, kind, title, summary, content, tags_text,
                source, source_ref, confidence, importance, metadata_json,
                valid_from, valid_until, archived_at, created_at, updated_at,
                last_accessed_at, access_count
         FROM memories WHERE id = ?1",
    )?;

    let raw = stmt
        .query_row(params![id], row_to_memory_raw)
        .optional()?
        .ok_or_else(|| ClioError::NotFound(id.to_string()))?;

    row_to_memory(raw)
}

/// Check whether a memory exists by ID. Lightweight — no row parsing.
fn exists(conn: &Connection, id: &str) -> Result<bool> {
    let found: Option<i32> = conn
        .query_row("SELECT 1 FROM memories WHERE id = ?1", params![id], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(found.is_some())
}

/// Verify a memory exists, returning NotFound if it doesn't.
fn require_exists(conn: &Connection, id: &str) -> Result<()> {
    if exists(conn, id)? {
        Ok(())
    } else {
        Err(ClioError::NotFound(id.to_string()))
    }
}

/// Internal row struct matching the DB columns before tag resolution.
struct MemoryRow {
    id: String,
    namespace: String,
    kind: String,
    title: Option<String>,
    summary: Option<String>,
    content: String,
    tags_text: String,
    source: Option<String>,
    source_ref: Option<String>,
    confidence: Option<f64>,
    importance: i32,
    metadata_json: String,
    valid_from: Option<String>,
    valid_until: Option<String>,
    archived_at: Option<String>,
    created_at: String,
    updated_at: String,
    last_accessed_at: Option<String>,
    access_count: i32,
}

fn row_to_memory_raw(row: &rusqlite::Row) -> rusqlite::Result<MemoryRow> {
    Ok(MemoryRow {
        id: row.get(0)?,
        namespace: row.get(1)?,
        kind: row.get(2)?,
        title: row.get(3)?,
        summary: row.get(4)?,
        content: row.get(5)?,
        tags_text: row.get(6)?,
        source: row.get(7)?,
        source_ref: row.get(8)?,
        confidence: row.get(9)?,
        importance: row.get(10)?,
        metadata_json: row.get(11)?,
        valid_from: row.get(12)?,
        valid_until: row.get(13)?,
        archived_at: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        last_accessed_at: row.get(17)?,
        access_count: row.get(18)?,
    })
}

fn row_to_memory(raw: MemoryRow) -> Result<Memory> {
    // Parse tags from the denormalised tags_text column — avoids a per-row query.
    let tags: Vec<String> = if raw.tags_text.is_empty() {
        Vec::new()
    } else {
        let mut t: Vec<String> = raw.tags_text.split_whitespace().map(String::from).collect();
        t.sort();
        t
    };
    let metadata: serde_json::Value = serde_json::from_str(&raw.metadata_json)?;
    Ok(Memory {
        id: raw.id,
        namespace: raw.namespace,
        kind: raw.kind,
        title: raw.title,
        summary: raw.summary,
        content: raw.content,
        tags,
        source: raw.source,
        source_ref: raw.source_ref,
        confidence: raw.confidence,
        importance: raw.importance,
        metadata,
        valid_from: raw.valid_from,
        valid_until: raw.valid_until,
        archived_at: raw.archived_at,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        last_accessed_at: raw.last_accessed_at,
        access_count: raw.access_count,
    })
}

// ---------------------------------------------------------------------------
// Recall (FTS + recent + filters)
// ---------------------------------------------------------------------------

/// Search or list memories according to the query parameters.
///
/// When `include_links` is true, linked memories are appended to the results
/// with a `linked_from` indicator showing which result memory they are linked from.
pub fn recall(conn: &Connection, query: &RecallQuery) -> Result<RecallResult> {
    let mut result = if let Some(ref fts_query) = query.query {
        recall_fts(conn, fts_query, query)?
    } else {
        recall_recent(conn, query)?
    };

    if query.include_links {
        append_linked_memories(conn, &mut result, query)?;
    }

    // Fire-and-forget access tracking for all returned items. Automatic
    // surfacing (resume briefs) opts out so it cannot train its own ranking.
    if !query.skip_access_tracking {
        let ids: Vec<&str> = result.items.iter().map(|i| i.memory.id.as_str()).collect();
        if let Err(e) = touch_accessed(conn, &ids) {
            tracing::warn!("access tracking failed in recall: {e}");
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Access tracking
// ---------------------------------------------------------------------------

/// Record access for the given memory IDs. Updates `last_accessed_at` and
/// increments `access_count`. Throttled: skips the update if the memory was
/// accessed within the last 60 seconds.
pub fn touch_accessed(conn: &Connection, ids: &[&str]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    // Process in chunks of 500 to avoid oversized SQL parameter lists.
    for chunk in ids.chunks(500) {
        touch_accessed_chunk(conn, chunk)?;
    }

    Ok(())
}

fn touch_accessed_chunk(conn: &Connection, ids: &[&str]) -> Result<()> {
    let now = now_utc();

    // Batch update: single UPDATE with IN clause and throttle check in WHERE.
    let placeholders: String = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let now_idx = ids.len() + 1;

    let sql = format!(
        "UPDATE memories \
         SET last_accessed_at = ?{now_idx}, access_count = access_count + 1 \
         WHERE id IN ({placeholders}) \
           AND (last_accessed_at IS NULL \
                OR (julianday(?{now_idx}) - julianday(last_accessed_at)) * 86400.0 > 60.0)"
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = ids
        .iter()
        .map(|id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id.to_string()) })
        .collect();
    params.push(Box::new(now));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, param_refs.as_slice())?;

    Ok(())
}

/// Fetch all outgoing links for multiple memory IDs in a single query.
fn get_links_bulk(conn: &Connection, memory_ids: &[String]) -> Result<Vec<MemoryLink>> {
    get_links_bulk_directed(conn, memory_ids, false)
}

/// Fetch all incoming links for multiple memory IDs in a single query.
fn get_links_bulk_incoming(conn: &Connection, memory_ids: &[String]) -> Result<Vec<MemoryLink>> {
    get_links_bulk_directed(conn, memory_ids, true)
}

fn get_links_bulk_directed(
    conn: &Connection,
    memory_ids: &[String],
    incoming: bool,
) -> Result<Vec<MemoryLink>> {
    if memory_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: String = (1..=memory_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let column = if incoming {
        "to_memory_id"
    } else {
        "from_memory_id"
    };
    let sql = format!(
        "SELECT from_memory_id, to_memory_id, relationship, metadata_json, created_at
         FROM memory_links WHERE {column} IN ({placeholders}) ORDER BY created_at"
    );
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = memory_ids
        .iter()
        .map(|id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id.clone()) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(MemoryLinkRow {
            from_memory_id: row.get(0)?,
            to_memory_id: row.get(1)?,
            relationship: row.get(2)?,
            metadata_json: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;

    let mut links = Vec::new();
    for row in rows {
        let r = row?;
        let metadata: serde_json::Value = serde_json::from_str(&r.metadata_json)?;
        links.push(MemoryLink {
            from_memory_id: r.from_memory_id,
            to_memory_id: r.to_memory_id,
            relationship: r.relationship,
            metadata,
            created_at: r.created_at,
        });
    }
    Ok(links)
}

/// Append linked memories to a recall result: every memory connected to a
/// direct result by an edge in either direction, each carrying the full set
/// of edge contexts (direction, relationship, metadata) that reached it.
///
/// Linked targets reapply the parent query's archive and expiry eligibility so
/// hidden records cannot re-enter recall through graph expansion. Target
/// memories are deduplicated; their edge contexts are not.
///
/// Uses batch fetching to avoid N+1 queries.
fn append_linked_memories(
    conn: &Connection,
    result: &mut RecallResult,
    q: &RecallQuery,
) -> Result<()> {
    use std::collections::{HashMap, HashSet};

    let existing_ids: HashSet<String> = result.items.iter().map(|i| i.memory.id.clone()).collect();
    let anchor_ids: Vec<String> = result.items.iter().map(|i| i.memory.id.clone()).collect();

    let outgoing = get_links_bulk(conn, &anchor_ids)?;
    let incoming = get_links_bulk_incoming(conn, &anchor_ids)?;

    // linked id -> (first anchor, every edge context that reached it),
    // preserving first-seen order for stable output.
    let mut order: Vec<String> = Vec::new();
    let mut contexts: HashMap<String, (String, Vec<LinkContext>)> = HashMap::new();

    let mut add_edge = |linked_id: &str, anchor_id: &str, direction: &str, link: MemoryLink| {
        if existing_ids.contains(linked_id) {
            return;
        }
        let entry = contexts.entry(linked_id.to_string()).or_insert_with(|| {
            order.push(linked_id.to_string());
            (anchor_id.to_string(), Vec::new())
        });
        entry.1.push(LinkContext {
            from_memory_id: link.from_memory_id,
            to_memory_id: link.to_memory_id,
            direction: direction.to_string(),
            relationship: link.relationship,
            metadata: link.metadata,
            created_at: link.created_at,
        });
    };

    for link in outgoing {
        let (linked, anchor) = (link.to_memory_id.clone(), link.from_memory_id.clone());
        add_edge(&linked, &anchor, "outgoing", link);
    }
    for link in incoming {
        let (linked, anchor) = (link.from_memory_id.clone(), link.to_memory_id.clone());
        add_edge(&linked, &anchor, "incoming", link);
    }

    if order.is_empty() {
        return Ok(());
    }

    // Batch-fetch all linked memories in one query, reapplying the parent
    // query's archive/expiry eligibility.
    let fetched = get_many_eligible(conn, &order, q.include_archived, q.exclude_expired)?;
    let mut fetched_map: HashMap<String, Memory> =
        fetched.into_iter().map(|m| (m.id.clone(), m)).collect();

    let mut linked_items: Vec<RecallItem> = Vec::new();
    for id in &order {
        let Some(memory) = fetched_map.remove(id) else {
            continue; // hidden by eligibility
        };
        let (linked_from, link_context) = contexts.remove(id).expect("context recorded");
        linked_items.push(RecallItem {
            memory,
            rank: None,
            linked_from: Some(linked_from),
            link_context,
        });
    }

    if !linked_items.is_empty() {
        let added = linked_items.len() as u32;
        result.items.extend(linked_items);
        result.count += added;
        result.total += added;
    }

    Ok(())
}

/// Every edge touching one memory, in both directions, with relationship and
/// metadata preserved. Direction is relative to the given memory.
pub fn get_link_contexts(conn: &Connection, memory_id: &str) -> Result<Vec<LinkContext>> {
    require_exists(conn, memory_id)?;
    let ids = vec![memory_id.to_string()];
    let mut contexts = Vec::new();
    for link in get_links_bulk(conn, &ids)? {
        contexts.push(LinkContext {
            from_memory_id: link.from_memory_id,
            to_memory_id: link.to_memory_id,
            direction: "outgoing".into(),
            relationship: link.relationship,
            metadata: link.metadata,
            created_at: link.created_at,
        });
    }
    for link in get_links_bulk_incoming(conn, &ids)? {
        contexts.push(LinkContext {
            from_memory_id: link.from_memory_id,
            to_memory_id: link.to_memory_id,
            direction: "incoming".into(),
            relationship: link.relationship,
            metadata: link.metadata,
            created_at: link.created_at,
        });
    }
    contexts.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(contexts)
}

/// Sanitise a user-supplied FTS query. Each whitespace-separated term is quoted
/// individually to neutralise FTS operators (`NOT`, `OR`, column filters) while
/// joining with spaces so multi-term queries match documents containing all terms
/// (FTS5 implicit AND), preserving BM25 ranking.
fn sanitise_fts_query(raw: &str) -> String {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|t| t.replace('"', " "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect();

    if terms.is_empty() {
        // Preserve previous behaviour for an empty query: a quoted empty phrase
        // (matches nothing rather than raising an FTS syntax error).
        return "\"\"".to_string();
    }
    terms.join(" ")
}

fn recall_fts(conn: &Connection, fts_query: &str, q: &RecallQuery) -> Result<RecallResult> {
    let safe_query = sanitise_fts_query(fts_query);

    let mut sql = String::from(
        "SELECT m.id, m.namespace, m.kind, m.title, m.summary, m.content, m.tags_text,
                m.source, m.source_ref, m.confidence, m.importance, m.metadata_json,
                m.valid_from, m.valid_until, m.archived_at, m.created_at, m.updated_at,
                m.last_accessed_at, m.access_count,
                bm25(memory_fts, 4.0, 2.0, 1.0, 0.5) AS rank
         FROM memory_fts
         JOIN memories m ON m.rowid = memory_fts.rowid
         WHERE memory_fts MATCH ?1",
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(safe_query));

    append_filters(&mut sql, &mut param_values, q);

    // Count query (without LIMIT/OFFSET) — kept separate for FTS because
    // bm25() is incompatible with window functions like COUNT(*) OVER().
    let count_sql = format!("SELECT COUNT(*) FROM ({sql})");

    // Composite scoring when decay_lambda > 0.0, otherwise fall back to plain BM25.
    let scoring_opt = q.scoring.as_ref().filter(|s| s.decay_lambda > 0.0);

    if let Some(scoring) = scoring_opt {
        let decay_idx = param_values.len() + 1;
        let boost_idx = param_values.len() + 2;
        sql.push_str(&format!(
            " ORDER BY \
             ((-bm25(memory_fts, 4.0, 2.0, 1.0, 0.5)) \
              * exp(-?{decay_idx} * (julianday('now') - julianday(COALESCE(m.last_accessed_at, m.updated_at)))) \
              * (1.0 + min(0.5, ?{boost_idx} * ln(1.0 + CAST(m.access_count AS REAL)))) \
              * (CAST(m.importance AS REAL) / 3.0) \
             ) DESC"
        ));
        param_values.push(Box::new(scoring.decay_lambda));
        param_values.push(Box::new(scoring.access_boost_weight));
    } else {
        sql.push_str(" ORDER BY rank ASC, m.updated_at DESC");
    }

    let next_idx = param_values.len() + 1;
    sql.push_str(&format!(" LIMIT ?{next_idx}"));
    param_values.push(Box::new(q.limit));

    let next_idx = param_values.len() + 1;
    sql.push_str(&format!(" OFFSET ?{next_idx}"));
    param_values.push(Box::new(q.offset));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    // Total count (uses params without LIMIT/OFFSET and scoring params).
    let count_param_end = if scoring_opt.is_some() {
        param_values.len() - 4 // skip decay, boost, limit, offset
    } else {
        param_values.len() - 2 // skip limit, offset
    };
    let count_refs: Vec<&dyn rusqlite::types::ToSql> = param_values[..count_param_end]
        .iter()
        .map(|p| p.as_ref())
        .collect();
    let total: u32 = conn.query_row(&count_sql, count_refs.as_slice(), |row| row.get(0))?;

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let raw = row_to_memory_raw(row)?;
        let rank: f64 = row.get(19)?;
        Ok((raw, rank))
    })?;

    let mut items = Vec::new();
    for row_result in rows {
        let (raw, rank) = row_result?;
        let memory = row_to_memory(raw)?;
        items.push(RecallItem {
            memory,
            rank: Some(rank),
            linked_from: None,
            link_context: Vec::new(),
        });
    }

    let count = items.len() as u32;
    Ok(RecallResult {
        total,
        count,
        offset: q.offset,
        limit: q.limit,
        items,
    })
}

fn recall_recent(conn: &Connection, q: &RecallQuery) -> Result<RecallResult> {
    let mut sql = String::from(
        "SELECT m.id, m.namespace, m.kind, m.title, m.summary, m.content, m.tags_text,
                m.source, m.source_ref, m.confidence, m.importance, m.metadata_json,
                m.valid_from, m.valid_until, m.archived_at, m.created_at, m.updated_at,
                m.last_accessed_at, m.access_count,
                COUNT(*) OVER () AS total_count
         FROM memories m WHERE 1=1",
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    append_filters(&mut sql, &mut param_values, q);

    // Explicit sort_by takes priority; fall back to composite scoring or plain recency.
    if let Some(ref sort) = q.sort_by {
        sql.push_str(&format!(" ORDER BY {}", sort.sql_fragment()));
    } else if let Some(scoring) = q.scoring.as_ref().filter(|s| s.decay_lambda > 0.0) {
        let decay_idx = param_values.len() + 1;
        let boost_idx = param_values.len() + 2;
        sql.push_str(&format!(
            " ORDER BY \
             (exp(-?{decay_idx} * (julianday('now') - julianday(COALESCE(m.last_accessed_at, m.updated_at)))) \
              * (1.0 + min(0.5, ?{boost_idx} * ln(1.0 + CAST(m.access_count AS REAL)))) \
              * (CAST(m.importance AS REAL) / 3.0) \
             ) DESC"
        ));
        param_values.push(Box::new(scoring.decay_lambda));
        param_values.push(Box::new(scoring.access_boost_weight));
    } else {
        sql.push_str(" ORDER BY m.updated_at DESC");
    }

    let next_idx = param_values.len() + 1;
    sql.push_str(&format!(" LIMIT ?{next_idx}"));
    param_values.push(Box::new(q.limit));

    let next_idx = param_values.len() + 1;
    sql.push_str(&format!(" OFFSET ?{next_idx}"));
    param_values.push(Box::new(q.offset));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let raw = row_to_memory_raw(row)?;
        let total_count: u32 = row.get(19)?;
        Ok((raw, total_count))
    })?;

    let mut total: u32 = 0;
    let mut items = Vec::new();
    for row_result in rows {
        let (raw, total_count) = row_result?;
        if items.is_empty() {
            total = total_count;
        }
        let memory = row_to_memory(raw)?;
        items.push(RecallItem {
            memory,
            rank: None,
            linked_from: None,
            link_context: Vec::new(),
        });
    }

    let count = items.len() as u32;
    Ok(RecallResult {
        total,
        count,
        offset: q.offset,
        limit: q.limit,
        items,
    })
}

fn append_filters(
    sql: &mut String,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    q: &RecallQuery,
) {
    if !q.include_archived {
        sql.push_str(" AND m.archived_at IS NULL");
    }

    if q.exclude_expired {
        sql.push_str(" AND (m.valid_until IS NULL OR datetime(m.valid_until) > datetime('now'))");
    }

    if let Some(ref ns) = q.namespace {
        let idx = params.len() + 1;
        sql.push_str(&format!(" AND m.namespace = ?{idx}"));
        params.push(Box::new(ns.clone()));
    }

    if let Some(ref kind) = q.kind {
        let idx = params.len() + 1;
        sql.push_str(&format!(" AND m.kind = ?{idx}"));
        params.push(Box::new(kind.clone()));
    }

    if let Some(min) = q.importance_min {
        let idx = params.len() + 1;
        sql.push_str(&format!(" AND m.importance >= ?{idx}"));
        params.push(Box::new(min));
    }

    if let Some(max) = q.importance_max {
        let idx = params.len() + 1;
        sql.push_str(&format!(" AND m.importance <= ?{idx}"));
        params.push(Box::new(max));
    }

    if !q.tags.is_empty() {
        let tags = normalise_tags(&q.tags);
        if q.match_all_tags {
            // Match ALL tags.
            let placeholders: Vec<String> = tags
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", params.len() + 1 + i))
                .collect();
            sql.push_str(&format!(
                " AND m.id IN (SELECT memory_id FROM memory_tags WHERE tag IN ({}) GROUP BY memory_id HAVING COUNT(DISTINCT tag) = {})",
                placeholders.join(", "),
                tags.len()
            ));
            for tag in &tags {
                params.push(Box::new(tag.clone()));
            }
        } else {
            // Match ANY tag.
            let placeholders: Vec<String> = tags
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", params.len() + 1 + i))
                .collect();
            sql.push_str(&format!(
                " AND m.id IN (SELECT DISTINCT memory_id FROM memory_tags WHERE tag IN ({}))",
                placeholders.join(", ")
            ));
            for tag in &tags {
                params.push(Box::new(tag.clone()));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scoped recall (project namespace first, then global fallback)
// ---------------------------------------------------------------------------

/// Recall within the detected namespace, then fill any remaining slots from `global`.
///
/// The detected namespace takes priority; any remaining slots are filled from
/// `global`. The two passes query disjoint namespaces (a memory has exactly one
/// namespace), so `total` is the sum of both passes with each memory counted
/// once. Paging is applied in-process across the merged set, so `offset > 0`
/// pages meaningfully across the combined result.
pub fn recall_scoped(
    conn: &Connection,
    query: &RecallQuery,
    detected_namespace: &str,
) -> Result<RecallResult> {
    // If detection falls back to "global", keep default recall global-only.
    // Callers use the explicit global flag for an all-namespace search.
    if detected_namespace == "global" {
        return recall(
            conn,
            &RecallQuery {
                namespace: Some("global".to_string()),
                ..query.clone()
            },
        );
    }

    // Fetch a full window (offset + limit) from each namespace at offset 0, then
    // merge and page in-process — a single namespace's offset can't be trusted
    // to line up with the merged ordering.
    let window = query.offset.saturating_add(query.limit);

    let scoped = recall(
        conn,
        &RecallQuery {
            namespace: Some(detected_namespace.to_string()),
            limit: window,
            offset: 0,
            ..query.clone()
        },
    )?;
    let global = recall(
        conn,
        &RecallQuery {
            namespace: Some("global".to_string()),
            limit: window,
            offset: 0,
            ..query.clone()
        },
    )?;

    // Merge scoped-first, deduplicating by id (scoped results take priority).
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut merged_items = Vec::new();
    for item in scoped.items.into_iter().chain(global.items) {
        if seen.insert(item.memory.id.clone()) {
            merged_items.push(item);
        }
    }

    // Disjoint namespaces, so total matches = sum of both, each counted once.
    let total = scoped.total + global.total;

    // Apply the caller's offset/limit across the merged set.
    let paged: Vec<_> = merged_items
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();

    Ok(RecallResult {
        total,
        count: paged.len() as u32,
        offset: query.offset,
        limit: query.limit,
        items: paged,
    })
}

// ---------------------------------------------------------------------------
// Recent (convenience wrapper)
// ---------------------------------------------------------------------------

/// Return recent memories, optionally scoped to a namespace.
pub fn recent(conn: &Connection, namespace: Option<&str>, limit: u32) -> Result<RecallResult> {
    recall(
        conn,
        &RecallQuery {
            namespace: namespace.map(String::from),
            limit,
            ..Default::default()
        },
    )
}

// ---------------------------------------------------------------------------
// Archive
// ---------------------------------------------------------------------------

/// Soft-archive a memory. Idempotent.
pub fn archive(conn: &Connection, id: &str) -> Result<Memory> {
    require_exists(conn, id)?;

    let now = now_utc();
    conn.execute(
        "UPDATE memories SET archived_at = COALESCE(archived_at, ?1), updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;

    get_raw(conn, id)
}

/// Soft-archive the exact memory identified by its stable provenance.
///
/// A missing identity is a successful no-op so derived projections can retire
/// idempotently even when their optional Clio write never succeeded.
pub fn archive_by_source_ref(
    conn: &Connection,
    source: &str,
    source_ref: &str,
) -> Result<Option<Memory>> {
    if source.trim().is_empty() || source_ref.trim().is_empty() {
        return Err(ClioError::Validation(
            "source and source_ref are required to archive by provenance".into(),
        ));
    }

    crate::db::with_savepoint(conn, "archive_by_source_ref", || {
        let Some(id) = find_by_source_ref(conn, source, source_ref)? else {
            return Ok(None);
        };
        let current = get_raw(conn, &id)?;
        if current.archived_at.is_some() {
            return Ok(Some(current));
        }
        let now = now_utc();
        conn.execute(
            "UPDATE memories SET archived_at = ?1, updated_at = ?1
             WHERE id = ?2 AND archived_at IS NULL",
            params![now, id],
        )?;
        Ok(Some(get_raw(conn, &id)?))
    })
}

// ---------------------------------------------------------------------------
// Unarchive
// ---------------------------------------------------------------------------

/// Remove the archive flag from a memory. Idempotent.
pub fn unarchive(conn: &Connection, id: &str) -> Result<Memory> {
    require_exists(conn, id)?;

    let now = now_utc();
    conn.execute(
        "UPDATE memories SET archived_at = NULL, updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;

    get_raw(conn, id)
}

// ---------------------------------------------------------------------------
// Move namespace
// ---------------------------------------------------------------------------

/// Move a single memory to a different namespace. Returns the updated memory.
pub fn move_namespace(conn: &Connection, id: &str, namespace: &str) -> Result<Memory> {
    if namespace.is_empty() || namespace.len() > 120 {
        return Err(ClioError::Validation(
            "namespace must be between 1 and 120 characters.".into(),
        ));
    }

    require_exists(conn, id)?;

    let now = now_utc();
    conn.execute(
        "UPDATE memories SET namespace = ?1, updated_at = ?2 WHERE id = ?3",
        params![namespace, now, id],
    )?;

    get_raw(conn, id)
}

/// Move all memories in one namespace to another. Returns the number of
/// memories moved.
pub fn move_namespace_bulk(conn: &Connection, from: &str, to: &str) -> Result<usize> {
    if to.is_empty() || to.len() > 120 {
        return Err(ClioError::Validation(
            "namespace must be between 1 and 120 characters.".into(),
        ));
    }

    let now = now_utc();
    let count = conn.execute(
        "UPDATE memories SET namespace = ?1, updated_at = ?2 WHERE namespace = ?3",
        params![to, now, from],
    )?;

    Ok(count)
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

/// Permanently delete a memory by ID. Returns the deleted memory. Cascades to
/// tags, links, and embeddings via ON DELETE CASCADE.
pub fn delete(conn: &Connection, id: &str) -> Result<Memory> {
    let memory = get_raw(conn, id)?;
    conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
    Ok(memory)
}

// ---------------------------------------------------------------------------
// List namespaces
// ---------------------------------------------------------------------------

/// Return a sorted list of distinct namespaces in use.
pub fn list_namespaces(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT namespace FROM memories ORDER BY namespace")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Return namespace details: name, memory count, and last activity date.
pub fn namespace_details(conn: &Connection) -> Result<Vec<NamespaceInfo>> {
    let mut stmt = conn.prepare(
        "SELECT namespace, COUNT(*) as cnt,
                MAX(COALESCE(archived_at, updated_at)) as last_activity
         FROM memories
         GROUP BY namespace
         ORDER BY namespace",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(NamespaceInfo {
            name: row.get(0)?,
            memory_count: row.get(1)?,
            last_activity: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Delete a namespace if it has no memories. Returns true if deleted.
pub fn delete_empty_namespace(conn: &Connection, namespace: &str) -> Result<bool> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM memories WHERE namespace = ?1",
        params![namespace],
        |row| row.get(0),
    )?;
    if count > 0 {
        return Err(ClioError::Validation(format!(
            "cannot delete namespace '{}': still has {} memories",
            namespace, count
        )));
    }
    // Namespace only exists as a value on memories — no separate table.
    // If count is 0, the namespace is already effectively deleted.
    Ok(true)
}

/// Delete a namespace and all its memories. Returns count of deleted memories.
pub fn delete_namespace_with_memories(conn: &Connection, namespace: &str) -> Result<usize> {
    if namespace.is_empty() || namespace == "global" {
        return Err(ClioError::Validation(
            "cannot delete the global namespace".into(),
        ));
    }
    let count = conn.execute(
        "DELETE FROM memories WHERE namespace = ?1",
        params![namespace],
    )?;
    Ok(count)
}

/// Rename a namespace: update all memories from old name to new name.
pub fn rename_namespace(conn: &Connection, from: &str, to: &str) -> Result<usize> {
    if to.is_empty() || to.len() > 120 {
        return Err(ClioError::Validation(
            "namespace must be between 1 and 120 characters.".into(),
        ));
    }
    let now = now_utc();
    let count = conn.execute(
        "UPDATE memories SET namespace = ?1, updated_at = ?2 WHERE namespace = ?3",
        params![to, now, from],
    )?;
    Ok(count)
}

/// Bulk archive multiple memories in a single transaction.
pub fn archive_bulk(conn: &Connection, ids: &[String]) -> Result<u32> {
    if ids.is_empty() {
        return Ok(0);
    }
    let now = now_utc();
    crate::db::with_savepoint(conn, "bulk_archive", || {
        let mut count = 0u32;
        for id in ids {
            let affected = conn.execute(
                "UPDATE memories SET archived_at = COALESCE(archived_at, ?1), updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            count += affected as u32;
        }
        Ok(count)
    })
}

/// Bulk delete multiple memories in a single transaction.
pub fn delete_bulk(conn: &Connection, ids: &[String]) -> Result<u32> {
    if ids.is_empty() {
        return Ok(0);
    }
    crate::db::with_savepoint(conn, "bulk_delete", || {
        let mut count = 0u32;
        for id in ids {
            let affected = conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
            count += affected as u32;
        }
        Ok(count)
    })
}

/// Bulk add a tag to multiple memories.
pub fn add_tag_bulk(conn: &Connection, ids: &[String], tag: &str) -> Result<u32> {
    if ids.is_empty() || tag.is_empty() {
        return Ok(0);
    }
    let normalised = tag.trim().to_lowercase();
    let now = now_utc();
    crate::db::with_savepoint(conn, "bulk_tag_add", || {
        let mut count = 0u32;
        for id in ids {
            // Insert tag if not already present
            let affected = conn.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag, created_at) VALUES (?1, ?2, ?3)",
                params![id, normalised, now],
            )?;
            if affected > 0 {
                // Update tags_text
                let tags: Vec<String> = {
                    let mut stmt = conn
                        .prepare("SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag")?;
                    let rows = stmt.query_map(params![id], |row| row.get(0))?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()?
                };
                let tags_text = tags.join(" ");
                conn.execute(
                    "UPDATE memories SET tags_text = ?1, updated_at = ?2 WHERE id = ?3",
                    params![tags_text, now, id],
                )?;
                count += 1;
            }
        }
        Ok(count)
    })
}

/// Bulk remove a tag from multiple memories.
pub fn remove_tag_bulk(conn: &Connection, ids: &[String], tag: &str) -> Result<u32> {
    if ids.is_empty() || tag.is_empty() {
        return Ok(0);
    }
    let normalised = tag.trim().to_lowercase();
    let now = now_utc();
    crate::db::with_savepoint(conn, "bulk_tag_remove", || {
        let mut count = 0u32;
        for id in ids {
            let affected = conn.execute(
                "DELETE FROM memory_tags WHERE memory_id = ?1 AND tag = ?2",
                params![id, normalised],
            )?;
            if affected > 0 {
                let tags: Vec<String> = {
                    let mut stmt = conn
                        .prepare("SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag")?;
                    let rows = stmt.query_map(params![id], |row| row.get(0))?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()?
                };
                let tags_text = tags.join(" ");
                conn.execute(
                    "UPDATE memories SET tags_text = ?1, updated_at = ?2 WHERE id = ?3",
                    params![tags_text, now, id],
                )?;
                count += 1;
            }
        }
        Ok(count)
    })
}

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

/// Create a typed link between two memories. Idempotent.
pub fn link(conn: &Connection, input: &LinkInput) -> Result<MemoryLink> {
    // Verify both memories exist (lightweight — no row parsing or access tracking).
    require_exists(conn, &input.from_memory_id)?;
    if !exists(conn, &input.to_memory_id)? {
        return Err(ClioError::Validation(
            "cannot create link: target memory does not exist.".into(),
        ));
    }

    if input.relationship.is_empty() || input.relationship.len() > 60 {
        return Err(ClioError::Validation(
            "relationship must be between 1 and 60 characters.".into(),
        ));
    }

    if !input.metadata.is_object() {
        return Err(ClioError::Validation(
            "link metadata must be a JSON object.".into(),
        ));
    }

    let now = now_utc();
    let metadata_str = serde_json::to_string(&input.metadata)?;

    conn.execute(
        "INSERT OR REPLACE INTO memory_links (from_memory_id, to_memory_id, relationship, metadata_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            input.from_memory_id,
            input.to_memory_id,
            input.relationship,
            metadata_str,
            now,
        ],
    )?;

    Ok(MemoryLink {
        from_memory_id: input.from_memory_id.clone(),
        to_memory_id: input.to_memory_id.clone(),
        relationship: input.relationship.clone(),
        metadata: input.metadata.clone(),
        created_at: now,
    })
}

/// Get all links originating from a memory.
pub fn get_links(conn: &Connection, memory_id: &str) -> Result<Vec<MemoryLink>> {
    let mut stmt = conn.prepare(
        "SELECT from_memory_id, to_memory_id, relationship, metadata_json, created_at
         FROM memory_links WHERE from_memory_id = ?1 ORDER BY created_at",
    )?;

    let rows = stmt.query_map(params![memory_id], |row| {
        Ok(MemoryLinkRow {
            from_memory_id: row.get(0)?,
            to_memory_id: row.get(1)?,
            relationship: row.get(2)?,
            metadata_json: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;

    let mut links = Vec::new();
    for row in rows {
        let r = row?;
        let metadata: serde_json::Value = serde_json::from_str(&r.metadata_json)?;
        links.push(MemoryLink {
            from_memory_id: r.from_memory_id,
            to_memory_id: r.to_memory_id,
            relationship: r.relationship,
            metadata,
            created_at: r.created_at,
        });
    }
    Ok(links)
}

struct MemoryLinkRow {
    from_memory_id: String,
    to_memory_id: String,
    relationship: String,
    metadata_json: String,
    created_at: String,
}

// ---------------------------------------------------------------------------
// Graph neighbours
// ---------------------------------------------------------------------------

/// Traverse the link graph starting from `memory_id` up to `depth` hops.
///
/// Returns unique memories found through the graph, excluding the starting
/// memory itself. Links are followed bidirectionally (both from and to).
pub fn get_neighbours(conn: &Connection, memory_id: &str, depth: u32) -> Result<Vec<Memory>> {
    use std::collections::HashSet;

    // Cap depth to prevent excessive graph traversal.
    let depth = depth.min(5);

    // Verify the root memory exists (lightweight check).
    require_exists(conn, memory_id)?;

    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(memory_id.to_string());

    let mut frontier: Vec<String> = vec![memory_id.to_string()];

    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }

        // Batch link traversal: single UNION query for the entire frontier.
        // SQLite UNION shares the parameter namespace, so ?1 in both SELECTs
        // refers to the same bound value — no need to duplicate params.
        let placeholders: String = (1..=frontier.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT to_memory_id FROM memory_links WHERE from_memory_id IN ({placeholders}) \
             UNION \
             SELECT from_memory_id FROM memory_links WHERE to_memory_id IN ({placeholders})"
        );

        let params: Vec<Box<dyn rusqlite::types::ToSql>> = frontier
            .iter()
            .map(|id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id.clone()) })
            .collect();

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let id_rows = stmt.query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))?;

        let mut next_frontier = Vec::new();
        for id_result in id_rows {
            let id = id_result?;
            if visited.insert(id.clone()) {
                next_frontier.push(id);
            }
        }

        frontier = next_frontier;
    }

    // Collect all discovered memories (excluding the root) in a single batch.
    let discovered: Vec<String> = visited.into_iter().filter(|id| id != memory_id).collect();
    get_many(conn, &discovered)
}

/// Fetch multiple memories by ID in a single query. Public entry point
/// for cross-module batch lookups (e.g. semantic recall).
pub fn get_many_pub(conn: &Connection, ids: &[String]) -> Result<Vec<Memory>> {
    get_many(conn, ids)
}

/// Fetch multiple memories by ID in a single query.
fn get_many(conn: &Connection, ids: &[String]) -> Result<Vec<Memory>> {
    get_many_eligible(conn, ids, true, false)
}

/// Fetch multiple memories by ID, applying archive/expiry eligibility in SQL.
/// The eligibility clauses mirror `append_filters` exactly.
fn get_many_eligible(
    conn: &Connection,
    ids: &[String],
    include_archived: bool,
    exclude_expired: bool,
) -> Result<Vec<Memory>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: String = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "SELECT id, namespace, kind, title, summary, content, tags_text,
                source, source_ref, confidence, importance, metadata_json,
                valid_from, valid_until, archived_at, created_at, updated_at,
                last_accessed_at, access_count
         FROM memories WHERE id IN ({placeholders})"
    );
    if !include_archived {
        sql.push_str(" AND archived_at IS NULL");
    }
    if exclude_expired {
        sql.push_str(" AND (valid_until IS NULL OR datetime(valid_until) > datetime('now'))");
    }
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = ids
        .iter()
        .map(|id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id.clone()) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), row_to_memory_raw)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row_to_memory(row?)?);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Schema info
// ---------------------------------------------------------------------------

/// Return a human-readable summary of the database schema.
pub fn schema_info(conn: &Connection) -> Result<String> {
    let versions = crate::migrations::applied_versions(conn)?;

    let table_count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'memory_fts%'",
        [],
        |row| row.get(0),
    )?;

    // Single query for both total and active counts.
    let (memory_count, active_count): (u32, u32) = conn.query_row(
        "SELECT COUNT(*), COUNT(CASE WHEN archived_at IS NULL THEN 1 END) FROM memories",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut info = String::new();
    info.push_str("# Clio Database Schema\n\n");
    info.push_str(&format!("Tables: {table_count}\n"));
    info.push_str(&format!("Total memories: {memory_count}\n"));
    info.push_str(&format!("Active memories: {active_count}\n"));
    info.push_str(&format!(
        "Archived memories: {}\n",
        memory_count - active_count
    ));
    info.push_str(&format!("Migrations applied: {}\n", versions.join(", ")));

    Ok(info)
}
