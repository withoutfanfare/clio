//! Exact-once session checkpoints.
//!
//! A checkpoint records the durable outcome of distilling one session delta,
//! keyed by `(source, session_id, cursor)`. Model extraction happens outside
//! the write transaction; the accepted atoms, review items and the checkpoint
//! record itself commit atomically. A repeated or concurrently delivered key
//! replays the stored result instead of storing duplicates, so a client that
//! lost the response can safely retry. An intentionally empty extraction is a
//! successful checkpoint and is never redistilled.

use rusqlite::{Connection, OptionalExtension, params};

use crate::capture::{CaptureResult, DistilledMemory};
use crate::error::{ClioError, Result};
use crate::models::{new_id, now_utc};

/// Identity and context for one checkpoint attempt. The digest text itself is
/// deliberately not part of this struct: it is model input, never stored.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckpointRequest {
    /// Capturing agent, e.g. `claude-session` or `codex-session`.
    pub source: String,
    /// Client session identifier.
    pub session_id: String,
    /// Monotonic transcript cursor: how far into the session this delta ends.
    pub cursor: i64,
    /// Explicit namespace override — always wins.
    pub namespace_override: Option<String>,
    /// Working-directory namespace used when the model does not promote a
    /// memory to `global`.
    pub default_namespace: Option<String>,
    /// Originating working directory, recorded in memory metadata.
    pub cwd: Option<String>,
    /// Git branch active during the session, if any.
    pub branch: Option<String>,
    /// Ticket/issue identifier associated with the work, if any.
    pub ticket: Option<String>,
}

/// The stored, replayable outcome of a checkpoint.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CheckpointResult {
    pub checkpoint_id: String,
    /// True when this call returned a previously stored result rather than
    /// storing new atoms.
    pub replayed: bool,
    pub stored_memory_ids: Vec<String>,
    pub queued_review_ids: Vec<String>,
    pub created_at: String,
}

/// The envelope persisted in `session_checkpoints.result_json`. IDs and counts
/// only — never transcript text or secrets.
#[derive(serde::Serialize, serde::Deserialize)]
struct ResultEnvelope {
    stored_memory_ids: Vec<String>,
    queued_review_ids: Vec<String>,
}

fn validate_request(req: &CheckpointRequest) -> Result<()> {
    if req.source.is_empty() {
        return Err(ClioError::Validation("source is required.".into()));
    }
    if req.session_id.is_empty() {
        return Err(ClioError::Validation("session_id is required.".into()));
    }
    if req.cursor < 0 {
        return Err(ClioError::Validation(
            "cursor must be zero or positive.".into(),
        ));
    }
    Ok(())
}

/// Look up a completed checkpoint for the request key. Used as a cheap
/// preflight before the model call and rechecked inside the write transaction.
pub fn find_checkpoint(
    conn: &Connection,
    source: &str,
    session_id: &str,
    cursor: i64,
) -> Result<Option<CheckpointResult>> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT id, result_json, created_at FROM session_checkpoints
             WHERE source = ?1 AND session_id = ?2 AND cursor = ?3",
            params![source, session_id, cursor],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;

    match row {
        None => Ok(None),
        Some((id, result_json, created_at)) => {
            let envelope: ResultEnvelope = serde_json::from_str(&result_json)?;
            Ok(Some(CheckpointResult {
                checkpoint_id: id,
                replayed: true,
                stored_memory_ids: envelope.stored_memory_ids,
                queued_review_ids: envelope.queued_review_ids,
                created_at,
            }))
        }
    }
}

/// Store the extracted atoms and the checkpoint record atomically.
///
/// Model extraction must happen in the caller, outside this transaction. Every
/// accepted memory, review item and the checkpoint row commit together; any
/// failure rolls the whole attempt back so no partial session can persist.
/// Auto-embedding is deferred to the caller (post-commit, best-effort).
pub fn store_checkpoint(
    conn: &Connection,
    req: &CheckpointRequest,
    memories: &[DistilledMemory],
    settings: &crate::settings::Settings,
) -> Result<CheckpointResult> {
    validate_request(req)?;

    let owns_transaction = conn.is_autocommit();
    conn.execute_batch(if owns_transaction {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT store_checkpoint"
    })?;

    let result = (|| -> Result<CheckpointResult> {
        // Recheck under the write lock: a concurrent attempt may have
        // committed this key after our preflight.
        if let Some(existing) = find_checkpoint(conn, &req.source, &req.session_id, req.cursor)? {
            return Ok(existing);
        }

        let metadata = match &req.cwd {
            Some(c) => serde_json::json!({ "cwd": c }),
            None => serde_json::json!({}),
        };

        let mut stored_memory_ids = Vec::new();
        let mut queued_review_ids = Vec::new();

        for (index, memory) in memories.iter().enumerate() {
            let classification = crate::capture::ClassificationResult {
                kind: memory.kind.clone(),
                title: memory.title.clone(),
                summary: memory.summary.clone(),
                tags: memory.tags.clone(),
                namespace: memory.namespace.clone(),
                importance: memory.importance,
                confidence: memory.confidence,
            };
            let namespace = crate::capture::resolve_distill_namespace(
                req.namespace_override.as_deref(),
                &classification.namespace,
                req.default_namespace.as_deref(),
            );

            // Unique provenance per atom, sharing the checkpoint key as the
            // prefix so a session's memories stay traceable to their delta.
            let item_ref = format!("{}@{}-{index}", req.session_id, req.cursor);

            let outcome = crate::capture::store_or_queue(
                conn,
                &memory.content,
                &classification,
                &namespace,
                &req.source,
                Some(&item_ref),
                &metadata,
                settings,
                false, // embed after commit, never inside the transaction
            )?;
            match outcome {
                CaptureResult::Stored(m) => stored_memory_ids.push(m.id),
                CaptureResult::Queued(item) => queued_review_ids.push(item.id),
            }
        }

        let envelope = ResultEnvelope {
            stored_memory_ids: stored_memory_ids.clone(),
            queued_review_ids: queued_review_ids.clone(),
        };
        let id = new_id();
        let now = now_utc();
        conn.execute(
            "INSERT INTO session_checkpoints
                (id, source, session_id, cursor, namespace, branch, ticket, result_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                req.source,
                req.session_id,
                req.cursor,
                req.namespace_override
                    .as_deref()
                    .or(req.default_namespace.as_deref()),
                req.branch,
                req.ticket,
                serde_json::to_string(&envelope)?,
                now,
            ],
        )?;

        Ok(CheckpointResult {
            checkpoint_id: id,
            replayed: false,
            stored_memory_ids,
            queued_review_ids,
            created_at: now,
        })
    })();

    match result {
        Ok(outcome) => {
            crate::db::finish_transaction(conn, owns_transaction, "store_checkpoint")?;
            Ok(outcome)
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, owns_transaction, "store_checkpoint");
            Err(e)
        }
    }
}

/// Best-effort post-commit auto-embedding for freshly stored checkpoint
/// memories. Failures are logged and never affect the committed checkpoint.
pub fn embed_checkpoint_memories(
    conn: &Connection,
    memory_ids: &[String],
    settings: &crate::settings::Settings,
) {
    if memory_ids.is_empty() || !settings.auto_embed {
        return;
    }
    let backend = match crate::embeddings::create_backend(&settings.embeddings) {
        Ok(backend) => backend,
        Err(e) => {
            tracing::warn!("checkpoint auto-embed unavailable: {e}");
            return;
        }
    };
    for id in memory_ids {
        match crate::repository::get(conn, id) {
            Ok(memory) => {
                if let Err(e) = crate::embeddings::embed_and_store(conn, backend.as_ref(), &memory)
                {
                    tracing::warn!("checkpoint auto-embed failed for {id}: {e}");
                }
            }
            Err(e) => tracing::warn!("checkpoint auto-embed could not load {id}: {e}"),
        }
    }
}

/// Distil one session delta and commit it as an exact-once checkpoint.
///
/// Preflights the key before calling the model, rechecks it inside the write
/// transaction, and replays the stored result whenever the key already
/// completed — including the empty-extraction case.
#[cfg(feature = "capture")]
pub fn checkpoint(
    conn: &Connection,
    req: &CheckpointRequest,
    digest: &str,
    config: &crate::settings::CaptureConfig,
    settings: &crate::settings::Settings,
) -> Result<CheckpointResult> {
    validate_request(req)?;

    // Preflight: never pay for the model when the result already exists.
    if let Some(existing) = find_checkpoint(conn, &req.source, &req.session_id, req.cursor)? {
        return Ok(existing);
    }

    // Provider work strictly outside the write transaction.
    let memories = crate::capture::distill(digest, config)?;

    let result = store_checkpoint(conn, req, &memories, settings)?;

    if !result.replayed {
        embed_checkpoint_memories(conn, &result.stored_memory_ids, settings);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn test_conn() -> Connection {
        crate::db::open_in_memory().expect("failed to open in-memory DB")
    }

    fn atom(content: &str) -> DistilledMemory {
        DistilledMemory {
            content: content.into(),
            kind: "fact".into(),
            title: format!("Title: {content}"),
            summary: String::new(),
            tags: vec!["test".into()],
            namespace: "project:checkpoint-test".into(),
            importance: 3,
            confidence: 1.0,
        }
    }

    fn request(cursor: i64) -> CheckpointRequest {
        CheckpointRequest {
            source: "claude-session".into(),
            session_id: "session-1".into(),
            cursor,
            namespace_override: None,
            default_namespace: Some("project:checkpoint-test".into()),
            cwd: Some("/tmp/project".into()),
            branch: Some("develop".into()),
            ticket: None,
        }
    }

    fn memory_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
            .unwrap()
    }

    fn checkpoint_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM session_checkpoints", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn first_checkpoint_stores_atoms_and_record() {
        let conn = test_conn();
        let settings = Settings::default();

        let result = store_checkpoint(
            &conn,
            &request(10),
            &[atom("fact one"), atom("fact two")],
            &settings,
        )
        .unwrap();

        assert!(!result.replayed);
        assert_eq!(result.stored_memory_ids.len(), 2);
        assert!(result.queued_review_ids.is_empty());
        assert_eq!(memory_count(&conn), 2);
        assert_eq!(checkpoint_count(&conn), 1);
    }

    #[test]
    fn same_key_replays_original_result() {
        let conn = test_conn();
        let settings = Settings::default();

        let first = store_checkpoint(&conn, &request(10), &[atom("fact one")], &settings).unwrap();
        // A retry may re-extract different atoms; the stored result wins.
        let second = store_checkpoint(
            &conn,
            &request(10),
            &[atom("different fact"), atom("another")],
            &settings,
        )
        .unwrap();

        assert!(second.replayed);
        assert_eq!(second.checkpoint_id, first.checkpoint_id);
        assert_eq!(second.stored_memory_ids, first.stored_memory_ids);
        assert_eq!(memory_count(&conn), 1);
        assert_eq!(checkpoint_count(&conn), 1);
    }

    #[test]
    fn later_cursor_is_accepted() {
        let conn = test_conn();
        let settings = Settings::default();

        store_checkpoint(&conn, &request(10), &[atom("fact one")], &settings).unwrap();
        let later = store_checkpoint(&conn, &request(20), &[atom("fact two")], &settings).unwrap();

        assert!(!later.replayed);
        assert_eq!(memory_count(&conn), 2);
        assert_eq!(checkpoint_count(&conn), 2);
    }

    #[test]
    fn empty_extraction_is_a_successful_checkpoint() {
        let conn = test_conn();
        let settings = Settings::default();

        let empty = store_checkpoint(&conn, &request(10), &[], &settings).unwrap();
        assert!(!empty.replayed);
        assert!(empty.stored_memory_ids.is_empty());
        assert_eq!(checkpoint_count(&conn), 1);

        // A retry of the same key must replay the empty result, not
        // redistil — even if the client somehow supplies atoms this time.
        let replay =
            store_checkpoint(&conn, &request(10), &[atom("late fact")], &settings).unwrap();
        assert!(replay.replayed);
        assert!(replay.stored_memory_ids.is_empty());
        assert_eq!(memory_count(&conn), 0);
    }

    #[test]
    fn middle_failure_rolls_back_every_write() {
        let conn = test_conn();
        let settings = Settings::default();

        // Empty content fails remember() validation after the first atom
        // has already been written inside the transaction.
        let invalid = DistilledMemory {
            content: String::new(),
            ..atom("unused")
        };
        let result = store_checkpoint(
            &conn,
            &request(10),
            &[atom("stored before failure"), invalid],
            &settings,
        );

        assert!(result.is_err());
        assert_eq!(memory_count(&conn), 0, "no partial atoms may survive");
        assert_eq!(
            checkpoint_count(&conn),
            0,
            "no checkpoint may claim success"
        );
        // The connection must be usable and the key still open for retry.
        let retry =
            store_checkpoint(&conn, &request(10), &[atom("clean retry")], &settings).unwrap();
        assert!(!retry.replayed);
        assert_eq!(memory_count(&conn), 1);
    }

    #[test]
    fn low_confidence_atoms_queue_for_review_atomically() {
        let conn = test_conn();
        let settings = Settings::default(); // review_threshold default applies

        let mut doubtful = atom("possibly durable");
        doubtful.confidence = 0.1;

        let result =
            store_checkpoint(&conn, &request(10), &[atom("certain"), doubtful], &settings).unwrap();

        if settings.capture.review_threshold.is_some() {
            assert_eq!(result.stored_memory_ids.len(), 1);
            assert_eq!(result.queued_review_ids.len(), 1);
        } else {
            assert_eq!(result.stored_memory_ids.len(), 2);
        }
    }

    #[test]
    fn rejects_invalid_identity() {
        let conn = test_conn();
        let settings = Settings::default();

        let mut req = request(10);
        req.source = String::new();
        assert!(store_checkpoint(&conn, &req, &[], &settings).is_err());

        let mut req = request(-1);
        req.source = "claude-session".into();
        assert!(store_checkpoint(&conn, &req, &[], &settings).is_err());
    }
}
