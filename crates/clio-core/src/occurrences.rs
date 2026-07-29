//! Source occurrences: append-only evidence that one canonical memory was
//! observed again.
//!
//! Exact-duplicate capture no longer discards provenance — the canonical
//! memory stays single, and each sighting (per session/delta) is recorded as
//! an occurrence. Occurrence writes share the capture/review transaction of
//! their caller; merges transfer occurrences to the kept memory before the
//! duplicate row is archived.

use rusqlite::{Connection, params};

use crate::error::Result;
use crate::models::{new_id, now_utc};

/// One recorded sighting of a memory's content.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Occurrence {
    pub id: String,
    pub memory_id: String,
    pub source: Option<String>,
    pub source_ref: Option<String>,
    pub session_id: Option<String>,
    pub occurred_at: String,
}

/// Record one occurrence. Idempotent per `(memory_id, source, source_ref)`
/// when both provenance fields are present — a checkpoint replay or duplicate
/// delivery of the same delta records nothing new. Returns `Ok(None)` when
/// the write was a no-op.
pub fn record_occurrence(
    conn: &Connection,
    memory_id: &str,
    source: Option<&str>,
    source_ref: Option<&str>,
    session_id: Option<&str>,
) -> Result<Option<Occurrence>> {
    let id = new_id();
    let now = now_utc();
    let changed = conn.execute(
        "INSERT OR IGNORE INTO memory_occurrences
            (id, memory_id, source, source_ref, session_id, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, memory_id, source, source_ref, session_id, now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(Some(Occurrence {
        id,
        memory_id: memory_id.to_string(),
        source: source.map(String::from),
        source_ref: source_ref.map(String::from),
        session_id: session_id.map(String::from),
        occurred_at: now,
    }))
}

/// All occurrences for one memory, oldest first.
pub fn list_occurrences(conn: &Connection, memory_id: &str) -> Result<Vec<Occurrence>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, memory_id, source, source_ref, session_id, occurred_at
         FROM memory_occurrences WHERE memory_id = ?1 ORDER BY occurred_at, id",
    )?;
    let rows = stmt.query_map(params![memory_id], |row| {
        Ok(Occurrence {
            id: row.get(0)?,
            memory_id: row.get(1)?,
            source: row.get(2)?,
            source_ref: row.get(3)?,
            session_id: row.get(4)?,
            occurred_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// How many times this memory's content has been observed.
pub fn count_occurrences(conn: &Connection, memory_id: &str) -> Result<i64> {
    let mut stmt =
        conn.prepare_cached("SELECT COUNT(*) FROM memory_occurrences WHERE memory_id = ?1")?;
    Ok(stmt.query_row(params![memory_id], |row| row.get(0))?)
}

/// Move a merged-away memory's occurrences onto the kept memory. Runs before
/// the duplicate row is archived, so no provenance is lost. Occurrences whose
/// provenance already exists on the kept memory are dropped as duplicates.
pub fn transfer_occurrences(conn: &Connection, from_id: &str, to_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE OR IGNORE memory_occurrences SET memory_id = ?1 WHERE memory_id = ?2",
        params![to_id, from_id],
    )?;
    // Any rows left behind collided with the kept memory's provenance index.
    conn.execute(
        "DELETE FROM memory_occurrences WHERE memory_id = ?1",
        params![from_id],
    )?;
    Ok(())
}

/// Current mutation generation for a namespace: bumped by triggers on every
/// memory, link, attention and occurrence mutation, so derived views (the
/// consolidated singleton) can prove their freshness.
pub fn namespace_generation(conn: &Connection, namespace: &str) -> Result<i64> {
    let mut stmt =
        conn.prepare_cached("SELECT generation FROM namespace_state WHERE namespace = ?1")?;
    let generation = stmt
        .query_row(params![namespace], |row| row.get(0))
        .unwrap_or(0);
    Ok(generation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RememberInput;
    use crate::settings::Settings;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().unwrap()
    }

    fn remember(conn: &Connection, ns: &str, content: &str) -> crate::models::Memory {
        crate::repository::remember(
            conn,
            &RememberInput {
                namespace: ns.into(),
                kind: "fact".into(),
                title: None,
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

    #[test]
    fn occurrences_are_idempotent_per_provenance() {
        let conn = test_conn();
        let memory = remember(&conn, "project:x", "a repeated fact");

        assert!(
            record_occurrence(&conn, &memory.id, Some("s"), Some("ref-1"), None)
                .unwrap()
                .is_some()
        );
        assert!(
            record_occurrence(&conn, &memory.id, Some("s"), Some("ref-1"), None)
                .unwrap()
                .is_none(),
            "same provenance records nothing new"
        );
        assert!(
            record_occurrence(&conn, &memory.id, Some("s"), Some("ref-2"), None)
                .unwrap()
                .is_some(),
            "a different delta is fresh evidence"
        );
        assert_eq!(count_occurrences(&conn, &memory.id).unwrap(), 2);
    }

    #[test]
    fn transfer_moves_occurrences_and_drops_collisions() {
        let conn = test_conn();
        let keep = remember(&conn, "project:x", "canonical");
        let dup = remember(&conn, "project:x", "duplicate");
        record_occurrence(&conn, &keep.id, Some("s"), Some("shared"), None).unwrap();
        record_occurrence(&conn, &dup.id, Some("s"), Some("shared"), None).unwrap();
        record_occurrence(&conn, &dup.id, Some("s"), Some("unique"), None).unwrap();

        transfer_occurrences(&conn, &dup.id, &keep.id).unwrap();
        assert_eq!(count_occurrences(&conn, &keep.id).unwrap(), 2);
        assert_eq!(count_occurrences(&conn, &dup.id).unwrap(), 0);
    }

    #[test]
    fn namespace_generation_bumps_on_every_relevant_mutation() {
        let conn = test_conn();
        let g0 = namespace_generation(&conn, "project:x").unwrap();
        let a = remember(&conn, "project:x", "first");
        let b = remember(&conn, "project:x", "second");
        let g1 = namespace_generation(&conn, "project:x").unwrap();
        assert!(g1 > g0, "memory writes bump the generation");

        crate::repository::link(
            &conn,
            &crate::models::LinkInput {
                from_memory_id: a.id.clone(),
                to_memory_id: b.id.clone(),
                relationship: "supports".into(),
                metadata: serde_json::json!({}),
            },
        )
        .unwrap();
        let g2 = namespace_generation(&conn, "project:x").unwrap();
        assert!(g2 > g1, "link writes bump the generation");

        crate::attention::create_attention(
            &conn,
            &crate::attention::AttentionInput {
                memory_id: a.id.clone(),
                ..crate::attention::AttentionInput::default()
            },
        )
        .unwrap();
        let g3 = namespace_generation(&conn, "project:x").unwrap();
        assert!(g3 > g2, "attention writes bump the generation");

        record_occurrence(&conn, &a.id, Some("s"), Some("r"), None).unwrap();
        let g4 = namespace_generation(&conn, "project:x").unwrap();
        assert!(g4 > g3, "occurrence writes bump the generation");

        crate::repository::archive(&conn, &b.id).unwrap();
        let g5 = namespace_generation(&conn, "project:x").unwrap();
        assert!(g5 > g4, "archiving bumps the generation");

        assert_eq!(namespace_generation(&conn, "project:untouched").unwrap(), 0);
    }
}
