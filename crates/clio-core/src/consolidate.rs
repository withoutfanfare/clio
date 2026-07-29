//! Consolidation: roll a namespace's many atomic memories into a single,
//! AI-curated "consolidated memory" document.
//!
//! The consolidated document is a *derived, source-cited cache*, never truth:
//! every material statement must cite the IDs of current atomic memories it
//! came from, citations are validated against the bounded input before the
//! singleton is replaced, and a per-namespace mutation generation (bumped by
//! triggers on memory, link, attention and occurrence writes) proves whether
//! the document is still fresh. Contradictions stay separate and labelled
//! unresolved — the model may not resolve them. Receipts are deterministic
//! activity, not durable truth, and are excluded from the model's input.

#[cfg(feature = "capture")]
use crate::error::ClioError;
use crate::error::Result;
use crate::models::Memory;
#[cfg(any(feature = "capture", test))]
use crate::models::RememberInput;
#[cfg(feature = "capture")]
use crate::settings::CaptureConfig;
#[cfg(any(feature = "capture", test))]
use crate::settings::Settings;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Provenance source identifying the consolidated singleton, so it can be
/// upserted in place and excluded from its own input. The `source_ref` is the
/// namespace itself, keeping the global `UNIQUE(source, source_ref)` index
/// unique per namespace (one consolidated memory each).
pub const CONSOLIDATED_SOURCE: &str = "clio-consolidate";

/// Upper bound on the characters of atomic-memory digest sent to the LLM, to
/// bound cost on large namespaces. Highest-importance, most-recent memories are
/// kept first.
#[cfg(any(feature = "capture", test))]
const MAX_INPUT_CHARS: usize = 60_000;

#[cfg(feature = "capture")]
const CONSOLIDATION_SYSTEM_PROMPT: &str = r#"You are a knowledge curator maintaining a single living "project memory" document for an AI coding assistant. You are given the project's atomic memories (decisions, facts, constraints, observations), each prefixed with its ID in square brackets. Produce a structured consolidation a future assistant could rely on.

Respond ONLY with a JSON object of the form:
{"sections": [{"heading": "...", "statements": [{"text": "...", "cites": ["<memory-id>", ...]}]}]}

Rules:
- EVERY statement must cite the ID(s) of the input memories it is drawn from, using their exact bracketed IDs. Never invent an ID; never leave "cites" empty.
- Synthesise and deduplicate — merge overlapping points and group related items under clear headings.
- Preserve every durable, important fact, decision (with its rationale), and constraint. Do not invent anything not supported by the input.
- If two memories genuinely conflict, keep BOTH sides as separate statements under a heading such as "Unresolved" and say plainly that the disagreement is unresolved. Never pick a winner yourself.
- Prefer durable knowledge over transient activity. Drop noise.
- Be concise and skimmable. Use British English.
- Output ONLY valid JSON — no markdown fences, no extra text."#;

/// Outcome of a consolidation run.
#[derive(Debug, Clone, Serialize)]
pub struct ConsolidationResult {
    /// The upserted consolidated memory.
    pub memory: Memory,
    /// How many atomic memories were consolidated.
    pub source_count: usize,
    /// Whether the bounded input dropped any memories.
    pub truncated: bool,
}

/// One cited claim in the consolidated document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationStatement {
    pub text: String,
    pub cites: Vec<String>,
}

/// A heading with its cited statements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationSection {
    pub heading: String,
    pub statements: Vec<ConsolidationStatement>,
}

/// The validated structured consolidation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationDoc {
    pub sections: Vec<ConsolidationSection>,
}

#[cfg(any(feature = "capture", test))]
struct SourceMemory {
    id: String,
    kind: String,
    title: Option<String>,
    content: String,
    importance: i32,
}

/// Load the atomic memories that should feed consolidation: live (non-archived)
/// memories in the namespace, excluding expired records, the consolidated
/// singleton itself and receipts (deterministic activity, not durable truth),
/// ordered by importance
/// then recency.
#[cfg(any(feature = "capture", test))]
fn load_source_memories(conn: &Connection, namespace: &str) -> Result<Vec<SourceMemory>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, content, importance
         FROM memories
         WHERE namespace = ?1
           AND archived_at IS NULL
           AND (valid_until IS NULL OR datetime(valid_until) > datetime('now'))
           AND kind != 'receipt'
           AND (source IS NULL OR source != ?2)
         ORDER BY importance DESC, updated_at DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![namespace, CONSOLIDATED_SOURCE], |row| {
        Ok(SourceMemory {
            id: row.get(0)?,
            kind: row.get(1)?,
            title: row.get(2)?,
            content: row.get(3)?,
            importance: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Build a compact, bounded digest of the atomic memories for the LLM.
/// Returns the digest, the IDs actually included (the only IDs a valid
/// consolidation may cite) and whether anything was dropped.
#[cfg(any(feature = "capture", test))]
fn build_digest(memories: &[SourceMemory]) -> (String, Vec<String>, bool) {
    let mut out = String::new();
    let mut included = Vec::new();
    let mut truncated = false;
    for m in memories {
        let title = m.title.as_deref().unwrap_or("(untitled)");
        let entry = format!(
            "- [{}] [{}|importance {}] {}\n  {}\n",
            m.id, m.kind, m.importance, title, m.content
        );
        if out.len() + entry.len() > MAX_INPUT_CHARS {
            truncated = true;
            break;
        }
        out.push_str(&entry);
        included.push(m.id.clone());
    }
    (out, included, truncated)
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

/// Parse and validate a structured consolidation. Every statement must carry
/// at least one citation and every cited ID must belong to the bounded input;
/// otherwise the candidate is rejected and the previous view stays in place.
pub fn parse_consolidation(raw: &str, allowed_ids: &[String]) -> Result<ConsolidationDoc> {
    use crate::error::ClioError as E;

    let trimmed = raw.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    let doc: ConsolidationDoc = serde_json::from_str(json_str)
        .map_err(|e| E::Validation(format!("consolidation JSON parse error: {e}")))?;

    if doc.sections.is_empty() {
        return Err(E::Validation("consolidation produced no sections".into()));
    }
    let allowed: std::collections::HashSet<&str> = allowed_ids.iter().map(String::as_str).collect();
    for section in &doc.sections {
        if section.heading.trim().is_empty() {
            return Err(E::Validation(
                "consolidation section without heading".into(),
            ));
        }
        for statement in &section.statements {
            if statement.text.trim().is_empty() {
                return Err(E::Validation("consolidation statement without text".into()));
            }
            if statement.cites.is_empty() {
                return Err(E::Validation(format!(
                    "uncited consolidation statement rejected: \"{}\"",
                    statement.text.chars().take(80).collect::<String>()
                )));
            }
            for cite in &statement.cites {
                if !allowed.contains(cite.as_str()) {
                    return Err(E::Validation(format!(
                        "consolidation cites unknown memory id '{cite}' — candidate rejected"
                    )));
                }
            }
        }
    }
    Ok(doc)
}

/// Render the validated structure as the singleton's markdown body, keeping
/// the source IDs visible on every statement.
fn render_consolidation(doc: &ConsolidationDoc) -> String {
    let mut out = String::new();
    for section in &doc.sections {
        out.push_str(&format!("## {}\n\n", section.heading));
        for statement in &section.statements {
            out.push_str(&format!(
                "- {} _(sources: {})_\n",
                statement.text,
                statement.cites.join(", ")
            ));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Store a validated consolidation as the namespace singleton, watermarked
/// with the namespace's mutation generation so staleness is provable.
#[cfg(any(feature = "capture", test))]
pub fn store_consolidation(
    conn: &Connection,
    namespace: &str,
    doc: &ConsolidationDoc,
    source_count: usize,
    truncated: bool,
    settings: &Settings,
) -> Result<ConsolidationResult> {
    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT store_consolidation"
    })?;

    let result = (|| -> Result<ConsolidationResult> {
        // The singleton upsert below bumps the namespace generation exactly
        // once (one memories-row write; tag writes carry no trigger), so the
        // stored watermark is the generation as of this commit. Any later
        // source mutation moves the namespace past it — visibly stale.
        let generation = crate::occurrences::namespace_generation(conn, namespace)? + 1;

        let input = RememberInput {
            namespace: namespace.to_string(),
            kind: "summary".into(),
            title: Some(format!("Consolidated memory — {namespace}")),
            summary: Some(format!(
                "AI-curated, source-cited consolidation of {source_count} memories"
            )),
            content: render_consolidation(doc),
            tags: vec!["consolidated".into()],
            source: Some(CONSOLIDATED_SOURCE.into()),
            source_ref: Some(namespace.to_string()),
            confidence: Some(1.0),
            importance: 5,
            metadata: serde_json::json!({
                "consolidated_from": source_count,
                "truncated": truncated,
                "generation": generation,
                "citations": doc,
            }),
            valid_from: None,
            valid_until: None,
            upsert: true,
        };

        let memory = crate::repository::remember(conn, &input, settings)?;
        Ok(ConsolidationResult {
            memory,
            source_count,
            truncated,
        })
    })();

    match result {
        Ok(outcome) => {
            crate::db::finish_transaction(conn, owns_transaction, "store_consolidation")?;
            Ok(outcome)
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, owns_transaction, "store_consolidation");
            Err(e)
        }
    }
}

/// Whether the namespace's consolidated singleton is stale: `None` when no
/// singleton exists, otherwise `Some(current_generation > stored watermark)`.
pub fn consolidation_is_stale(conn: &Connection, namespace: &str) -> Result<Option<bool>> {
    let stored: Option<i64> = conn
        .query_row(
            "SELECT json_extract(metadata_json, '$.generation') FROM memories
             WHERE namespace = ?1 AND source = ?2",
            rusqlite::params![namespace, CONSOLIDATED_SOURCE],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    let Some(stored) = stored else {
        // A singleton without a watermark (pre-citation era) is always stale.
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE namespace = ?1 AND source = ?2",
            rusqlite::params![namespace, CONSOLIDATED_SOURCE],
            |row| row.get(0),
        )?;
        return Ok(if exists > 0 { Some(true) } else { None });
    };
    let current = crate::occurrences::namespace_generation(conn, namespace)?;
    Ok(Some(current > stored))
}

/// Consolidate a namespace's atomic memories into the cited singleton. The
/// atomic memories are left untouched; an invalid or uncited candidate is
/// rejected and the previous view stays in place (visibly stale via the
/// generation watermark).
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

    let (digest, included_ids, truncated) = build_digest(&sources);
    let raw = crate::capture::chat(CONSOLIDATION_SYSTEM_PROMPT, &digest, config, true)?;
    let doc = parse_consolidation(&raw, &included_ids)?;

    store_consolidation(
        conn,
        namespace,
        &doc,
        included_ids.len(),
        truncated,
        settings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().expect("failed to open in-memory DB")
    }

    fn insert(conn: &Connection, namespace: &str, content: &str, importance: i32) -> Memory {
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
        crate::repository::remember(conn, &input, &s).unwrap()
    }

    fn doc_citing(id: &str) -> ConsolidationDoc {
        ConsolidationDoc {
            sections: vec![ConsolidationSection {
                heading: "Decisions".into(),
                statements: vec![ConsolidationStatement {
                    text: "SQLite stays the system of record.".into(),
                    cites: vec![id.to_string()],
                }],
            }],
        }
    }

    #[test]
    fn load_excludes_singleton_and_receipts() {
        let conn = test_conn();
        insert(&conn, "project:x", "real memory", 3);

        let s = Settings::default();
        let receipt = RememberInput {
            namespace: "project:x".into(),
            kind: "receipt".into(),
            title: Some("Session receipt".into()),
            summary: None,
            content: "Did some work.".into(),
            tags: vec!["receipt".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 2,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        };
        crate::repository::remember(&conn, &receipt, &s).unwrap();

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
    fn digest_respects_char_budget_and_reports_truncation() {
        let big = "x".repeat(MAX_INPUT_CHARS - 60);
        let memories = vec![
            SourceMemory {
                id: "id-a".into(),
                kind: "note".into(),
                title: Some("a".into()),
                content: big,
                importance: 5,
            },
            SourceMemory {
                id: "id-b".into(),
                kind: "note".into(),
                title: Some("b".into()),
                content: "second".into(),
                importance: 4,
            },
        ];
        let (digest, included, truncated) = build_digest(&memories);
        // The first entry alone exceeds the budget, so the second is dropped
        // and the truncation is visible, never silent.
        assert!(!digest.contains("second"));
        assert_eq!(included, vec!["id-a".to_string()]);
        assert!(truncated);
    }

    #[test]
    fn parse_rejects_uncited_and_unknown_citations() {
        let allowed = vec!["id-a".to_string(), "id-b".to_string()];

        let valid = r#"{"sections":[{"heading":"Facts","statements":[
            {"text":"A cited fact.","cites":["id-a"]},
            {"text":"Another.","cites":["id-a","id-b"]}]}]}"#;
        assert!(parse_consolidation(valid, &allowed).is_ok());

        let uncited = r#"{"sections":[{"heading":"Facts","statements":[
            {"text":"Trust me.","cites":[]}]}]}"#;
        assert!(parse_consolidation(uncited, &allowed).is_err());

        let unknown = r#"{"sections":[{"heading":"Facts","statements":[
            {"text":"Invented.","cites":["id-z"]}]}]}"#;
        assert!(parse_consolidation(unknown, &allowed).is_err());

        assert!(parse_consolidation(r#"{"sections":[]}"#, &allowed).is_err());
        assert!(parse_consolidation("not json", &allowed).is_err());
    }

    #[test]
    fn stored_consolidation_is_fresh_then_stale_after_any_mutation() {
        let conn = test_conn();
        let settings = Settings::default();
        let atom = insert(&conn, "project:x", "a durable fact", 4);

        assert_eq!(consolidation_is_stale(&conn, "project:x").unwrap(), None);

        let result = store_consolidation(
            &conn,
            "project:x",
            &doc_citing(&atom.id),
            1,
            false,
            &settings,
        )
        .unwrap();
        assert!(
            result.memory.content.contains(&atom.id),
            "citations rendered"
        );
        assert_eq!(
            consolidation_is_stale(&conn, "project:x").unwrap(),
            Some(false),
            "fresh immediately after its own commit"
        );

        // Any source mutation moves the namespace past the watermark.
        insert(&conn, "project:x", "something new happened", 3);
        assert_eq!(
            consolidation_is_stale(&conn, "project:x").unwrap(),
            Some(true)
        );

        // Re-consolidating restores freshness in place (one singleton).
        store_consolidation(
            &conn,
            "project:x",
            &doc_citing(&atom.id),
            2,
            false,
            &settings,
        )
        .unwrap();
        assert_eq!(
            consolidation_is_stale(&conn, "project:x").unwrap(),
            Some(false)
        );
        let singletons: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE source = ?1",
                rusqlite::params![CONSOLIDATED_SOURCE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(singletons, 1);
    }

    #[test]
    fn archive_and_link_mutations_invalidate_the_consolidation() {
        let conn = test_conn();
        let settings = Settings::default();
        let a = insert(&conn, "project:x", "fact one", 3);
        let b = insert(&conn, "project:x", "fact two", 3);

        store_consolidation(&conn, "project:x", &doc_citing(&a.id), 2, false, &settings).unwrap();
        assert_eq!(
            consolidation_is_stale(&conn, "project:x").unwrap(),
            Some(false)
        );

        crate::repository::link(
            &conn,
            &crate::models::LinkInput {
                from_memory_id: a.id.clone(),
                to_memory_id: b.id.clone(),
                relationship: "supersedes".into(),
                metadata: serde_json::json!({}),
            },
        )
        .unwrap();
        assert_eq!(
            consolidation_is_stale(&conn, "project:x").unwrap(),
            Some(true),
            "a new typed link is a truth change"
        );

        store_consolidation(&conn, "project:x", &doc_citing(&a.id), 2, false, &settings).unwrap();
        crate::repository::archive(&conn, &b.id).unwrap();
        assert_eq!(
            consolidation_is_stale(&conn, "project:x").unwrap(),
            Some(true),
            "archiving a source invalidates the view"
        );
    }

    #[test]
    fn legacy_singleton_without_watermark_reads_as_stale() {
        let conn = test_conn();
        let s = Settings::default();
        let legacy = RememberInput {
            namespace: "project:legacy".into(),
            kind: "summary".into(),
            title: Some("Consolidated".into()),
            summary: None,
            content: "old uncited doc".into(),
            tags: vec![],
            source: Some(CONSOLIDATED_SOURCE.into()),
            source_ref: Some("project:legacy".into()),
            confidence: Some(1.0),
            importance: 5,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: true,
        };
        crate::repository::remember(&conn, &legacy, &s).unwrap();
        assert_eq!(
            consolidation_is_stale(&conn, "project:legacy").unwrap(),
            Some(true)
        );
    }

    #[test]
    fn singletons_are_isolated_per_namespace() {
        let conn = test_conn();
        let settings = Settings::default();
        let a = insert(&conn, "project:a", "doc A source", 3);
        let b = insert(&conn, "project:b", "doc B source", 3);
        store_consolidation(&conn, "project:a", &doc_citing(&a.id), 1, false, &settings).unwrap();
        store_consolidation(&conn, "project:b", &doc_citing(&b.id), 1, false, &settings).unwrap();

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
