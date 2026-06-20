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
