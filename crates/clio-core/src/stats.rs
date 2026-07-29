//! Statistics, analytics, and activity feed for the memory system.

use rusqlite::{Connection, params};

use crate::error::Result;
use crate::models::{MemoryStats, RecentEntry, WeekSummary};

// ---------------------------------------------------------------------------
// Memory statistics
// ---------------------------------------------------------------------------

/// Compute aggregate statistics about stored memories.
///
/// If `namespace` is provided, counts are scoped to that namespace only.
pub fn memory_stats(conn: &Connection, namespace: Option<&str>) -> Result<MemoryStats> {
    // Single query to get both total and active counts.
    let (total_memories, active_memories, archived_memories) = if let Some(ns) = namespace {
        let (total, active): (u32, u32) = conn.query_row(
            "SELECT COUNT(*), COUNT(CASE WHEN archived_at IS NULL THEN 1 END) FROM memories WHERE namespace = ?1",
            params![ns],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        (total, active, total - active)
    } else {
        let (total, active): (u32, u32) = conn.query_row(
            "SELECT COUNT(*), COUNT(CASE WHEN archived_at IS NULL THEN 1 END) FROM memories",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        (total, active, total - active)
    };

    let total_embeddings: u32 = if let Some(ns) = namespace {
        conn.query_row(
            "SELECT COUNT(*) FROM memory_embeddings e
             JOIN memories m ON m.id = e.memory_id
             WHERE m.namespace = ?1",
            params![ns],
            |row| row.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM memory_embeddings", [], |row| {
            row.get(0)
        })?
    };

    let embedding_coverage = if total_memories > 0 {
        (total_embeddings as f64 / total_memories as f64) * 100.0
    } else {
        0.0
    };

    // Counts by namespace.
    let by_namespace = {
        let mut stmt = conn.prepare(
            "SELECT namespace, COUNT(*) FROM memories GROUP BY namespace ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<std::result::Result<Vec<(String, u32)>, _>>()?
    };

    // Counts by kind.
    let by_kind = {
        let mut stmt = conn
            .prepare("SELECT kind, COUNT(*) FROM memories GROUP BY kind ORDER BY COUNT(*) DESC")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<std::result::Result<Vec<(String, u32)>, _>>()?
    };

    // Memories created per ISO week.
    let by_week = {
        let mut stmt = conn.prepare(
            "SELECT strftime('%Y-W%W', created_at) AS week, COUNT(*)
             FROM memories
             GROUP BY week
             ORDER BY week DESC
             LIMIT 52",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<std::result::Result<Vec<(String, u32)>, _>>()?
    };

    // Top tags.
    let top_tags = tag_frequency(conn, 20)?;

    // Link totals.
    let total_links: u32 =
        conn.query_row("SELECT COUNT(*) FROM memory_links", [], |row| row.get(0))?;

    let link_density = if total_memories > 0 {
        total_links as f64 / total_memories as f64
    } else {
        0.0
    };

    Ok(MemoryStats {
        total_memories,
        active_memories,
        archived_memories,
        total_embeddings,
        embedding_coverage,
        by_namespace,
        by_kind,
        by_week,
        top_tags,
        total_links,
        link_density,
    })
}

// ---------------------------------------------------------------------------
// Tag frequency
// ---------------------------------------------------------------------------

/// Return the most common tags with their counts.
pub fn tag_frequency(conn: &Connection, limit: u32) -> Result<Vec<(String, u32)>> {
    let mut stmt = conn.prepare(
        "SELECT tag, COUNT(*) AS cnt
         FROM memory_tags
         GROUP BY tag
         ORDER BY cnt DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

/// Return a weekly timeline of memory creation.
pub fn timeline(
    conn: &Connection,
    namespace: Option<&str>,
    weeks: u32,
) -> Result<Vec<WeekSummary>> {
    let sql = if namespace.is_some() {
        "SELECT strftime('%Y-W%W', created_at) AS week, COUNT(*)
         FROM memories
         WHERE namespace = ?1
         GROUP BY week
         ORDER BY week DESC
         LIMIT ?2"
    } else {
        "SELECT strftime('%Y-W%W', created_at) AS week, COUNT(*)
         FROM memories
         GROUP BY week
         ORDER BY week DESC
         LIMIT ?1"
    };

    let mut stmt = conn.prepare(sql)?;

    let rows = if let Some(ns) = namespace {
        stmt.query_map(params![ns, weeks], |row| {
            Ok(WeekSummary {
                week: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![weeks], |row| {
            Ok(WeekSummary {
                week: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    Ok(rows)
}

// ---------------------------------------------------------------------------
// Recent activity feed
// ---------------------------------------------------------------------------

/// Return recent memory activity: creates, updates, and archives.
///
/// Each entry indicates whether the memory was created, updated, or archived,
/// based on its timestamp fields.
pub fn recent_activity(
    conn: &Connection,
    namespace: Option<&str>,
    limit: u32,
) -> Result<Vec<RecentEntry>> {
    // We use a UNION ALL approach to capture distinct events:
    // - created_at = updated_at and archived_at IS NULL => "created"
    // - archived_at IS NOT NULL => "archived" (at archived_at time)
    // - updated_at > created_at and archived_at IS NULL => "updated"
    //
    // To keep it simple and fast, we query all memories ordered by most recent
    // timestamp and classify each one.

    let mut sql = String::from(
        "SELECT id, title, namespace, kind,
                created_at, updated_at, archived_at
         FROM memories WHERE 1=1",
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ns) = namespace {
        let idx = param_values.len() + 1;
        sql.push_str(&format!(" AND namespace = ?{idx}"));
        param_values.push(Box::new(ns.to_string()));
    }

    // Order by the most recent event timestamp.
    sql.push_str(" ORDER BY COALESCE(archived_at, updated_at) DESC");

    let idx = param_values.len() + 1;
    sql.push_str(&format!(" LIMIT ?{idx}"));
    param_values.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let id: String = row.get(0)?;
        let title: Option<String> = row.get(1)?;
        let namespace: String = row.get(2)?;
        let kind: String = row.get(3)?;
        let created_at: String = row.get(4)?;
        let updated_at: String = row.get(5)?;
        let archived_at: Option<String> = row.get(6)?;

        // Classify the action.
        let (action, timestamp) = if let Some(ref archived) = archived_at {
            ("archived".to_string(), archived.clone())
        } else if updated_at != created_at {
            ("updated".to_string(), updated_at)
        } else {
            ("created".to_string(), created_at)
        };

        Ok(RecentEntry {
            memory_id: id,
            title,
            namespace,
            kind,
            action,
            timestamp,
        })
    })?;

    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::RememberInput;
    use crate::repository;

    fn test_db() -> Connection {
        db::open_in_memory().expect("failed to open in-memory DB")
    }

    fn make_memory(
        conn: &Connection,
        ns: &str,
        kind: &str,
        tags: &[&str],
    ) -> crate::models::Memory {
        repository::remember(
            conn,
            &RememberInput {
                namespace: ns.into(),
                kind: kind.into(),
                title: Some(format!("{ns}/{kind} memory")),
                summary: None,
                content: format!("Content for {ns}/{kind}"),
                tags: tags.iter().map(|t| t.to_string()).collect(),
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &crate::settings::Settings::default(),
        )
        .unwrap()
    }

    #[test]
    fn stats_empty_db() {
        let conn = test_db();
        let stats = memory_stats(&conn, None).unwrap();
        assert_eq!(stats.total_memories, 0);
        assert_eq!(stats.active_memories, 0);
        assert_eq!(stats.archived_memories, 0);
        assert_eq!(stats.total_links, 0);
        assert_eq!(stats.embedding_coverage, 0.0);
    }

    #[test]
    fn stats_with_data() {
        let conn = test_db();

        make_memory(&conn, "global", "note", &["rust"]);
        make_memory(&conn, "global", "decision", &["rust", "sqlite"]);
        make_memory(&conn, "project:ai", "note", &["ai"]);

        let stats = memory_stats(&conn, None).unwrap();
        assert_eq!(stats.total_memories, 3);
        assert_eq!(stats.active_memories, 3);
        assert_eq!(stats.archived_memories, 0);
        assert!(stats.by_namespace.len() >= 2);
        assert!(stats.by_kind.len() >= 2);
    }

    #[test]
    fn stats_scoped_by_namespace() {
        let conn = test_db();

        make_memory(&conn, "global", "note", &[]);
        make_memory(&conn, "project:ai", "note", &[]);
        make_memory(&conn, "project:ai", "decision", &[]);

        let stats = memory_stats(&conn, Some("project:ai")).unwrap();
        assert_eq!(stats.total_memories, 2);
        assert_eq!(stats.active_memories, 2);
    }

    #[test]
    fn tag_frequency_ordering() {
        let conn = test_db();

        make_memory(&conn, "global", "note", &["rust", "sqlite"]);
        make_memory(&conn, "global", "note", &["rust"]);
        make_memory(&conn, "global", "note", &["python"]);

        let freq = tag_frequency(&conn, 10).unwrap();
        assert!(!freq.is_empty());
        // "rust" should appear first (2 uses).
        assert_eq!(freq[0].0, "rust");
        assert_eq!(freq[0].1, 2);
    }

    #[test]
    fn timeline_returns_weeks() {
        let conn = test_db();

        make_memory(&conn, "global", "note", &[]);
        make_memory(&conn, "global", "note", &[]);

        let tl = timeline(&conn, None, 12).unwrap();
        assert!(!tl.is_empty());
        // Both memories created in same week.
        assert_eq!(tl[0].count, 2);
    }

    #[test]
    fn recent_activity_classifies_actions() {
        let conn = test_db();

        // Create a memory (action: "created").
        let mem = make_memory(&conn, "global", "note", &[]);

        let activity = recent_activity(&conn, None, 10).unwrap();
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].action, "created");

        // Archive it (action: "archived").
        repository::archive(&conn, &mem.id).unwrap();

        let activity = recent_activity(&conn, None, 10).unwrap();
        assert_eq!(activity[0].action, "archived");
    }

    #[test]
    fn recent_activity_scoped_by_namespace() {
        let conn = test_db();

        make_memory(&conn, "global", "note", &[]);
        make_memory(&conn, "project:ai", "note", &[]);

        let activity = recent_activity(&conn, Some("project:ai"), 10).unwrap();
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].namespace, "project:ai");
    }
}

// ---------------------------------------------------------------------------
// Effectiveness (event-backed acceptance reporting)
// ---------------------------------------------------------------------------

/// Event-backed usefulness report. Every read is untracked: building the
/// report can never mutate rank, access or event state. Deliberate recall
/// (access counts) and automatic injection (`surfaced` events) are separate
/// measures — automatic surfacing must never train ranking. Corrupt rows are
/// counted and reported, never silently dropped from denominators.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectivenessReport {
    pub namespace: Option<String>,
    /// Server-confirmed capture attempts.
    pub checkpoints_total: i64,
    pub last_checkpoint_at: Option<String>,
    /// Attention lifecycle counts by status.
    pub attention: std::collections::BTreeMap<String, i64>,
    /// Open items untouched beyond the dormancy policy.
    pub attention_stale: i64,
    /// Append-only event counts by type (idempotent events already unique).
    pub events: std::collections::BTreeMap<String, i64>,
    /// Distinct automatic injections (unique surfaced idempotency keys).
    pub surfaced_unique: i64,
    /// Deliberate recall volume: total tracked accesses across memories.
    pub deliberate_accesses: i64,
    /// `contradicts` links where both endpoints are still active.
    pub unresolved_contradictions: i64,
    pub consolidation_stale: Option<bool>,
    /// External delivery outbox counts by status.
    pub deliveries: std::collections::BTreeMap<String, i64>,
    pub review_pending: u32,
    /// Rows whose JSON payloads failed to validate — reported, not hidden.
    pub corrupt_rows: i64,
    pub generated_at: String,
}

/// Build the effectiveness report from untracked reads only.
pub fn effectiveness(
    conn: &Connection,
    namespace: Option<&str>,
    dormant_days: u32,
) -> Result<EffectivenessReport> {
    let ns_clause = |column: &str| match namespace {
        Some(_) => format!(" AND {column} = ?1"),
        None => String::new(),
    };
    let ns_params: Vec<Box<dyn rusqlite::types::ToSql>> = match namespace {
        Some(ns) => vec![Box::new(ns.to_string())],
        None => Vec::new(),
    };
    let refs: Vec<&dyn rusqlite::types::ToSql> = ns_params.iter().map(|p| p.as_ref()).collect();

    let (checkpoints_total, last_checkpoint_at): (i64, Option<String>) = conn.query_row(
        &format!(
            "SELECT COUNT(*), MAX(created_at) FROM session_checkpoints WHERE 1=1{}",
            ns_clause("namespace")
        ),
        refs.as_slice(),
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut attention = std::collections::BTreeMap::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT status, COUNT(*) FROM attention_items WHERE 1=1{} GROUP BY status",
            ns_clause("namespace")
        ))?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            attention.insert(status, count);
        }
    }

    let attention_stale: i64 = if dormant_days == 0 {
        0 // dormancy surfacing disabled
    } else {
        match namespace {
            Some(ns) => conn.query_row(
                "SELECT COUNT(*) FROM attention_items
             WHERE status = 'open' AND namespace = ?1
               AND julianday('now') - julianday(updated_at) > ?2",
                rusqlite::params![ns, f64::from(dormant_days)],
                |row| row.get(0),
            )?,
            None => conn.query_row(
                "SELECT COUNT(*) FROM attention_items
             WHERE status = 'open'
               AND julianday('now') - julianday(updated_at) > ?1",
                rusqlite::params![f64::from(dormant_days)],
                |row| row.get(0),
            )?,
        }
    };

    let mut events = std::collections::BTreeMap::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT event_type, COUNT(*) FROM memory_events WHERE 1=1{} GROUP BY event_type",
            ns_clause("namespace")
        ))?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (event_type, count) = row?;
            events.insert(event_type, count);
        }
    }

    let surfaced_unique: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(DISTINCT idempotency_key) FROM memory_events
             WHERE event_type = 'surfaced' AND idempotency_key IS NOT NULL{}",
            ns_clause("namespace")
        ),
        refs.as_slice(),
        |row| row.get(0),
    )?;

    let deliberate_accesses: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(access_count), 0) FROM memories WHERE 1=1{}",
            ns_clause("namespace")
        ),
        refs.as_slice(),
        |row| row.get(0),
    )?;

    let unresolved_contradictions: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM memory_links l
             JOIN memories f ON f.id = l.from_memory_id
             JOIN memories t ON t.id = l.to_memory_id
             WHERE l.relationship = 'contradicts'
               AND f.archived_at IS NULL AND t.archived_at IS NULL{}",
            match namespace {
                Some(_) => " AND f.namespace = ?1".to_string(),
                None => String::new(),
            }
        ),
        refs.as_slice(),
        |row| row.get(0),
    )?;

    let consolidation_stale = match namespace {
        Some(ns) => crate::consolidate::consolidation_is_stale(conn, ns)?,
        None => None,
    };

    let mut deliveries = std::collections::BTreeMap::new();
    {
        // Deliveries are scoped through their attention item's namespace so a
        // project report never leaks another project's operational state.
        let mut stmt = conn.prepare(&format!(
            "SELECT d.status, COUNT(*) FROM delivery_outbox d
             JOIN attention_items a ON a.id = d.attention_id
             WHERE 1=1{} GROUP BY d.status",
            match namespace {
                Some(_) => " AND a.namespace = ?1",
                None => "",
            }
        ))?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            deliveries.insert(status, count);
        }
    }

    let review_pending: u32 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM review_queue WHERE status = 'pending'{}",
            match namespace {
                Some(_) => " AND suggested_namespace = ?1",
                None => "",
            }
        ),
        refs.as_slice(),
        |row| row.get(0),
    )?;

    let corrupt_rows: i64 = conn.query_row(
        "SELECT
            (SELECT COUNT(*) FROM memories WHERE json_valid(metadata_json) = 0)
          + (SELECT COUNT(*) FROM memory_events WHERE json_valid(metadata_json) = 0)
          + (SELECT COUNT(*) FROM session_checkpoints WHERE json_valid(result_json) = 0)
          + (SELECT COUNT(*) FROM delivery_outbox WHERE json_valid(payload_json) = 0)",
        [],
        |row| row.get(0),
    )?;

    Ok(EffectivenessReport {
        namespace: namespace.map(String::from),
        checkpoints_total,
        last_checkpoint_at,
        attention,
        attention_stale,
        events,
        surfaced_unique,
        deliberate_accesses,
        unresolved_contradictions,
        consolidation_stale,
        deliveries,
        review_pending,
        corrupt_rows,
        generated_at: crate::models::now_utc(),
    })
}
