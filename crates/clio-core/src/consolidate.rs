//! Consolidation: roll a namespace's many atomic memories into a single,
//! AI-curated "consolidated memory" document.
//!
//! The consolidated document is a *derived cache*, not a source of truth: each
//! run reconciles it from the current atomic memories (no iterative self-edit,
//! so it can't drift). It is stored as a singleton memory per namespace
//! (`kind = summary`, upserted on `source + source_ref`), leaving the atomic
//! memories untouched.

use crate::error::{ClioError, Result};
use crate::models::{Memory, RememberInput};
use crate::settings::{CaptureConfig, Settings};
use rusqlite::Connection;
use serde::Serialize;

/// Provenance source identifying the consolidated singleton, so it can be
/// upserted in place and excluded from its own input. The `source_ref` is the
/// namespace itself, keeping the global `UNIQUE(source, source_ref)` index
/// unique per namespace (one consolidated memory each).
pub const CONSOLIDATED_SOURCE: &str = "clio-consolidate";

/// Upper bound on the characters of atomic-memory digest sent to the LLM, to
/// bound cost on large namespaces. Highest-importance, most-recent memories are
/// kept first.
const MAX_INPUT_CHARS: usize = 60_000;

const CONSOLIDATION_SYSTEM_PROMPT: &str = r#"You are a knowledge curator maintaining a single living "project memory" document for an AI coding assistant. You are given the project's atomic memories (decisions, facts, constraints, observations) gathered over time. Produce ONE coherent, well-organised Markdown document that a future assistant could read to understand the project.

Rules:
- Synthesise and deduplicate — merge overlapping points, resolve redundancy, group related items under clear headings.
- Preserve every durable, important fact, decision (with its rationale), and constraint. Do not invent anything not supported by the input.
- Prefer durable knowledge over transient activity. Drop noise.
- Be concise and skimmable: short sections, bullet points, no padding.
- Use British English.
- Output ONLY the Markdown document — no preamble, no code fences around the whole thing."#;

/// Outcome of a consolidation run.
#[derive(Debug, Clone, Serialize)]
pub struct ConsolidationResult {
    /// The upserted consolidated memory.
    pub memory: Memory,
    /// How many atomic memories were consolidated.
    pub source_count: usize,
}

struct SourceMemory {
    kind: String,
    title: Option<String>,
    content: String,
    importance: i32,
}

/// Load the atomic memories that should feed consolidation: live (non-archived)
/// memories in the namespace, excluding the consolidated singleton itself,
/// ordered by importance then recency.
fn load_source_memories(conn: &Connection, namespace: &str) -> Result<Vec<SourceMemory>> {
    let mut stmt = conn.prepare(
        "SELECT kind, title, content, importance
         FROM memories
         WHERE namespace = ?1
           AND archived_at IS NULL
           AND (source IS NULL OR source != ?2)
         ORDER BY importance DESC, updated_at DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![namespace, CONSOLIDATED_SOURCE], |row| {
        Ok(SourceMemory {
            kind: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            importance: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Build a compact, bounded digest of the atomic memories for the LLM.
fn build_digest(memories: &[SourceMemory]) -> String {
    let mut out = String::new();
    for m in memories {
        let title = m.title.as_deref().unwrap_or("(untitled)");
        let entry = format!(
            "- [{}|importance {}] {}\n  {}\n",
            m.kind, m.importance, title, m.content
        );
        if out.len() + entry.len() > MAX_INPUT_CHARS {
            break;
        }
        out.push_str(&entry);
    }
    out
}

/// Count atomic memories created since the consolidated singleton was last
/// updated (i.e. new since the last consolidation). If no consolidated memory
/// exists yet, returns the total atomic-memory count. Powers the
/// "auto-consolidate after N new" trigger.
pub fn new_since_last_consolidation(conn: &Connection, namespace: &str) -> Result<usize> {
    let last: Option<String> = conn
        .query_row(
            "SELECT updated_at FROM memories
             WHERE namespace = ?1 AND source = ?2",
            rusqlite::params![namespace, CONSOLIDATED_SOURCE],
            |row| row.get(0),
        )
        .ok();

    let count: i64 = match last {
        Some(ts) => conn.query_row(
            "SELECT COUNT(*) FROM memories
             WHERE namespace = ?1
               AND archived_at IS NULL
               AND (source IS NULL OR source != ?2)
               AND created_at > ?3",
            rusqlite::params![namespace, CONSOLIDATED_SOURCE, ts],
            |row| row.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM memories
             WHERE namespace = ?1
               AND archived_at IS NULL
               AND (source IS NULL OR source != ?2)",
            rusqlite::params![namespace, CONSOLIDATED_SOURCE],
            |row| row.get(0),
        )?,
    };

    Ok(count as usize)
}

/// Consolidate a namespace's atomic memories into the singleton consolidated
/// memory, reconciling fully from the current memories (the document is a cache,
/// not a source of truth). The atomic memories are left untouched.
#[cfg(feature = "capture")]
pub fn consolidate(
    conn: &Connection,
    namespace: &str,
    config: &CaptureConfig,
    settings: &Settings,
) -> Result<ConsolidationResult> {
    let sources = load_source_memories(conn, namespace)?;
    if sources.is_empty() {
        return Err(ClioError::Validation(format!(
            "no memories to consolidate in namespace '{namespace}'"
        )));
    }

    let digest = build_digest(&sources);
    let markdown = crate::capture::chat(CONSOLIDATION_SYSTEM_PROMPT, &digest, config)?;
    let markdown = markdown.trim().to_string();
    if markdown.is_empty() {
        return Err(ClioError::Storage(
            "consolidation produced an empty document".into(),
        ));
    }

    let input = RememberInput {
        namespace: namespace.to_string(),
        kind: "summary".into(),
        title: Some(format!("Consolidated memory — {namespace}")),
        summary: Some(format!(
            "AI-curated consolidation of {} memories",
            sources.len()
        )),
        content: markdown,
        tags: vec!["consolidated".into()],
        source: Some(CONSOLIDATED_SOURCE.into()),
        source_ref: Some(namespace.to_string()),
        confidence: Some(1.0),
        importance: 5,
        metadata: serde_json::json!({ "consolidated_from": sources.len() }),
        valid_from: None,
        valid_until: None,
        upsert: true,
    };

    let memory = crate::repository::remember(conn, &input, settings)?;
    Ok(ConsolidationResult {
        memory,
        source_count: sources.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().expect("failed to open in-memory DB")
    }

    fn insert(conn: &Connection, namespace: &str, content: &str, importance: i32) {
        let s = Settings::default();
        let input = RememberInput {
            namespace: namespace.to_string(),
            kind: "note".into(),
            title: Some("t".into()),
            summary: None,
            content: content.into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        };
        crate::repository::remember(conn, &input, &s).unwrap();
    }

    #[test]
    fn load_excludes_consolidated_singleton() {
        let conn = test_conn();
        insert(&conn, "project:x", "real memory", 3);

        // Simulate an existing consolidated singleton.
        let s = Settings::default();
        let singleton = RememberInput {
            namespace: "project:x".into(),
            kind: "summary".into(),
            title: Some("Consolidated".into()),
            summary: None,
            content: "doc".into(),
            tags: vec![],
            source: Some(CONSOLIDATED_SOURCE.into()),
            source_ref: Some("project:x".into()),
            confidence: Some(1.0),
            importance: 5,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: true,
        };
        crate::repository::remember(&conn, &singleton, &s).unwrap();

        let sources = load_source_memories(&conn, "project:x").unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].content, "real memory");
    }

    #[test]
    fn digest_respects_char_budget() {
        let big = "x".repeat(MAX_INPUT_CHARS);
        let memories = vec![
            SourceMemory {
                kind: "note".into(),
                title: Some("a".into()),
                content: big,
                importance: 5,
            },
            SourceMemory {
                kind: "note".into(),
                title: Some("b".into()),
                content: "second".into(),
                importance: 4,
            },
        ];
        let digest = build_digest(&memories);
        // The first entry alone exceeds the budget, so the second is dropped.
        assert!(!digest.contains("second"));
    }

    #[test]
    fn singletons_are_isolated_per_namespace() {
        let conn = test_conn();
        let s = Settings::default();
        let make = |ns: &str, body: &str| RememberInput {
            namespace: ns.into(),
            kind: "summary".into(),
            title: Some("Consolidated".into()),
            summary: None,
            content: body.into(),
            tags: vec![],
            source: Some(CONSOLIDATED_SOURCE.into()),
            source_ref: Some(ns.into()),
            confidence: Some(1.0),
            importance: 5,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: true,
        };
        // Two namespaces consolidated: the second must not clobber the first.
        crate::repository::remember(&conn, &make("project:a", "doc A"), &s).unwrap();
        crate::repository::remember(&conn, &make("project:b", "doc B"), &s).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE source = ?1",
                rusqlite::params![CONSOLIDATED_SOURCE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn new_since_counts_all_without_singleton() {
        let conn = test_conn();
        insert(&conn, "project:y", "one", 3);
        insert(&conn, "project:y", "two", 3);
        assert_eq!(new_since_last_consolidation(&conn, "project:y").unwrap(), 2);
    }
}
