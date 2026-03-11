use rusqlite::{params, Connection, OptionalExtension};

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
pub fn remember(conn: &Connection, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
    validate::remember_input(input)?;

    let tags = normalise_tags(&input.tags);
    let tags_text = tags.join(" ");
    let metadata_str = serde_json::to_string(&input.metadata)?;
    let now = now_utc();

    // Resolve the title: explicit > AI-generated > string-based extraction.
    let title = crate::title::resolve_title(input.title.clone(), &input.content, settings);

    // Upsert path: look for existing record by source + source_ref.
    if input.upsert {
        if let (Some(source), Some(source_ref)) = (&input.source, &input.source_ref) {
            if let Some(existing_id) = find_by_source_ref(conn, source, source_ref)? {
                return update_existing(conn, &existing_id, input, &tags, &tags_text, &metadata_str, &now, settings);
            }
        }
        // If upsert requested but source/source_ref incomplete, fall through to insert.
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
        Ok(()) => conn.execute_batch("RELEASE remember_insert")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO remember_insert");
            let _ = conn.execute_batch("RELEASE remember_insert");
            return Err(e);
        }
    }

    get_raw(conn, &id)
}

fn update_existing(
    conn: &Connection,
    id: &str,
    input: &RememberInput,
    tags: &[String],
    tags_text: &str,
    metadata_str: &str,
    now: &str,
    settings: &crate::settings::Settings,
) -> Result<Memory> {
    // Resolve the title: explicit > AI-generated > string-based extraction.
    let title = crate::title::resolve_title(input.title.clone(), &input.content, settings);

    conn.execute_batch("SAVEPOINT remember_update")?;
    let result = (|| -> Result<()> {
        conn.execute(
            "UPDATE memories SET namespace = ?1, kind = ?2, title = ?3, summary = ?4,
                content = ?5, tags_text = ?6, source = ?7, source_ref = ?8,
                confidence = ?9, importance = ?10, metadata_json = ?11,
                valid_from = ?12, valid_until = ?13, updated_at = ?14
             WHERE id = ?15",
            params![
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
                id,
            ],
        )?;

        // Replace tags: delete old, insert new.
        conn.execute("DELETE FROM memory_tags WHERE memory_id = ?1", params![id])?;
        insert_tags(conn, id, tags, now)?;
        Ok(())
    })();

    match result {
        Ok(()) => conn.execute_batch("RELEASE remember_update")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO remember_update");
            let _ = conn.execute_batch("RELEASE remember_update");
            return Err(e);
        }
    }

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
    tags.iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty() && seen.insert(t.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

/// Update an existing memory by ID with the provided fields.
pub fn update(conn: &Connection, id: &str, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
    // Verify memory exists.
    get_raw(conn, id)?;

    validate::remember_input(input)?;

    let tags = normalise_tags(&input.tags);
    let tags_text = tags.join(" ");
    let metadata_str = serde_json::to_string(&input.metadata)?;
    let now = now_utc();

    update_existing(conn, id, input, &tags, &tags_text, &metadata_str, &now, settings)
}

// ---------------------------------------------------------------------------
// Get
// ---------------------------------------------------------------------------

/// Fetch a single memory by id.
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
        .query_row(
            "SELECT 1 FROM memories WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
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
        append_linked_memories(conn, &mut result)?;
    }

    // Fire-and-forget access tracking for all returned items.
    let ids: Vec<&str> = result.items.iter().map(|i| i.memory.id.as_str()).collect();
    if let Err(e) = touch_accessed(conn, &ids) {
        tracing::warn!("access tracking failed in recall: {e}");
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

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, param_refs.as_slice())?;

    Ok(())
}

/// Fetch all outgoing links for multiple memory IDs in a single query.
fn get_links_bulk(conn: &Connection, memory_ids: &[String]) -> Result<Vec<MemoryLink>> {
    if memory_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: String = (1..=memory_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT from_memory_id, to_memory_id, relationship, metadata_json, created_at
         FROM memory_links WHERE from_memory_id IN ({placeholders}) ORDER BY created_at"
    );
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = memory_ids
        .iter()
        .map(|id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id.clone()) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();
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

/// Append linked memories to a recall result. For each item in the result,
/// fetch its outgoing links and add the linked memories (if not already present).
///
/// Uses batch fetching to avoid N+1 queries.
fn append_linked_memories(conn: &Connection, result: &mut RecallResult) -> Result<()> {
    use std::collections::HashSet;

    let existing_ids: HashSet<String> = result.items.iter().map(|i| i.memory.id.clone()).collect();

    // Fetch all outgoing links for current result items in one query.
    let source_ids: Vec<String> = result.items.iter().map(|i| i.memory.id.clone()).collect();
    let all_links = get_links_bulk(conn, &source_ids)?;

    // Collect target IDs and their link sources, deduplicating.
    let mut target_to_source: Vec<(String, String)> = Vec::new();
    let mut added_ids: HashSet<String> = HashSet::new();

    for link in all_links {
        let target_id = link.to_memory_id;
        if !existing_ids.contains(&target_id) && added_ids.insert(target_id.clone()) {
            target_to_source.push((target_id, link.from_memory_id));
        }
    }

    if target_to_source.is_empty() {
        return Ok(());
    }

    // Batch-fetch all linked memories in one query.
    let ids_to_fetch: Vec<String> = target_to_source.iter().map(|(id, _)| id.clone()).collect();
    let fetched = get_many(conn, &ids_to_fetch)?;

    // Build a map of id -> source for linking.
    let source_map: std::collections::HashMap<String, String> = target_to_source
        .into_iter()
        .collect();

    let mut linked_items: Vec<RecallItem> = Vec::new();
    for memory in fetched {
        if let Some(linked_from) = source_map.get(&memory.id) {
            linked_items.push(RecallItem {
                memory,
                rank: None,
                linked_from: Some(linked_from.clone()),
            });
        }
    }

    if !linked_items.is_empty() {
        let added = linked_items.len() as u32;
        result.items.extend(linked_items);
        result.count += added;
        result.total += added;
    }

    Ok(())
}

/// Sanitise a user-supplied FTS query by wrapping it in double-quotes to
/// force literal phrase matching. This prevents FTS operator injection
/// (`NOT`, `OR`, column filters) while preserving normal search.
fn sanitise_fts_query(raw: &str) -> String {
    let escaped = raw.replace('"', ' '.to_string().as_str());
    format!("\"{}\"", escaped.trim())
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

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    // Total count (uses params without LIMIT/OFFSET and scoring params).
    let count_param_end = if scoring_opt.is_some() {
        param_values.len() - 4 // skip decay, boost, limit, offset
    } else {
        param_values.len() - 2 // skip limit, offset
    };
    let count_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values[..count_param_end].iter().map(|p| p.as_ref()).collect();
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

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

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

/// Search within the detected namespace first, then fall back to `global` if
/// the scoped results are fewer than `limit`. Deduplicates by memory id,
/// giving priority to the project-scoped results.
pub fn recall_scoped(
    conn: &Connection,
    query: &RecallQuery,
    detected_namespace: &str,
) -> Result<RecallResult> {
    // If the detected namespace is already "global", just do a normal recall.
    if detected_namespace == "global" {
        return recall(conn, query);
    }

    // First: search within the detected namespace.
    let scoped_query = RecallQuery {
        namespace: Some(detected_namespace.to_string()),
        ..query.clone()
    };
    let scoped = recall(conn, &scoped_query)?;

    // If we already have enough results, return them.
    if scoped.count >= query.limit {
        return Ok(scoped);
    }

    // Second: search global namespace for additional results.
    let remaining = query.limit.saturating_sub(scoped.count);
    let global_query = RecallQuery {
        namespace: Some("global".to_string()),
        limit: remaining,
        offset: 0,
        ..query.clone()
    };
    let global = recall(conn, &global_query)?;

    // Merge results, deduplicating by id (scoped results take priority).
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut merged_items = Vec::new();

    for item in scoped.items {
        seen.insert(item.memory.id.clone());
        merged_items.push(item);
    }

    for item in global.items {
        if seen.insert(item.memory.id.clone()) {
            merged_items.push(item);
        }
    }

    let count = merged_items.len() as u32;
    let total = scoped.total + global.total;

    Ok(RecallResult {
        total,
        count,
        offset: query.offset,
        limit: query.limit,
        items: merged_items,
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
pub fn move_namespace_bulk(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<usize> {
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
    let discovered: Vec<String> = visited
        .into_iter()
        .filter(|id| id != memory_id)
        .collect();
    get_many(conn, &discovered)
}

/// Fetch multiple memories by ID in a single query. Public entry point
/// for cross-module batch lookups (e.g. semantic recall).
pub fn get_many_pub(conn: &Connection, ids: &[String]) -> Result<Vec<Memory>> {
    get_many(conn, ids)
}

/// Fetch multiple memories by ID in a single query.
fn get_many(conn: &Connection, ids: &[String]) -> Result<Vec<Memory>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: String = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, namespace, kind, title, summary, content, tags_text,
                source, source_ref, confidence, importance, metadata_json,
                valid_from, valid_until, archived_at, created_at, updated_at,
                last_accessed_at, access_count
         FROM memories WHERE id IN ({placeholders})"
    );
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = ids
        .iter()
        .map(|id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id.clone()) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();
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
    info.push_str(&format!("Archived memories: {}\n", memory_count - active_count));
    info.push_str(&format!(
        "Migrations applied: {}\n",
        versions.join(", ")
    ));

    Ok(info)
}
