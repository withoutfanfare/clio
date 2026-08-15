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
    /// Operator-authorised recovery of a retained checkpoint that arrived
    /// after a later cursor. Exact-key replay still prevents duplicates.
    #[serde(default)]
    pub recover_stale: bool,
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

        // Cursors are monotonic per session. An older cursor arriving after a
        // newer one committed means stale or overlapping client state — reject
        // it unless an operator is explicitly recovering a retained delta.
        // Exact-key replay above still makes recovery idempotent.
        let latest: Option<i64> = conn.query_row(
            "SELECT MAX(cursor) FROM session_checkpoints
             WHERE source = ?1 AND session_id = ?2",
            params![req.source, req.session_id],
            |row| row.get(0),
        )?;
        if let Some(latest) = latest {
            if req.cursor < latest && !req.recover_stale {
                return Err(ClioError::Validation(format!(
                    "cursor {} is behind this session's latest committed cursor {latest}; \
                     stale delta rejected",
                    req.cursor
                )));
            }
        }

        let mut stored_memory_ids = Vec::new();
        let mut queued_review_ids = Vec::new();

        for (index, memory) in memories.iter().enumerate() {
            let mut classification = crate::capture::ClassificationResult {
                kind: memory.kind.clone(),
                title: memory.title.clone(),
                summary: memory.summary.clone(),
                tags: memory.tags.clone(),
                namespace: memory.namespace.clone(),
                importance: memory.importance,
                confidence: memory.confidence,
            };
            // Branch/ticket context is applied deterministically after model
            // parsing — the model never invents session identifiers.
            if let Some(ticket) = &req.ticket {
                let tag = format!("ticket:{}", ticket.to_lowercase());
                if !classification.tags.contains(&tag) {
                    classification.tags.push(tag);
                }
            }
            let namespace = crate::capture::resolve_namespace(
                req.namespace_override.as_deref(),
                &classification.namespace,
                req.default_namespace.as_deref(),
            );

            let mut meta = serde_json::Map::new();
            if let Some(cwd) = &req.cwd {
                meta.insert("cwd".into(), serde_json::json!(cwd));
            }
            if let Some(branch) = &req.branch {
                meta.insert("branch".into(), serde_json::json!(branch));
            }
            if let Some(attention) = &memory.attention {
                meta.insert("attention".into(), serde_json::to_value(attention)?);
            }
            let mut metadata = serde_json::Value::Object(meta);

            // Unique provenance per atom, sharing the checkpoint key as the
            // prefix so a session's memories stay traceable to their delta.
            let item_ref = format!("{}@{}-{index}", req.session_id, req.cursor);

            // Inferred (non-explicit) open loops never become operational work
            // automatically: they queue for review, whatever their confidence.
            // Approval creates the memory and its attention in one transaction.
            let inferred_loop = memory
                .attention
                .as_ref()
                .is_some_and(|attention| !attention.is_explicit());
            if inferred_loop {
                // Even when the content matches an existing memory, the
                // suggested open loop itself still needs review. Stamp the
                // canonical memory into the metadata so approval attaches
                // attention to it instead of storing a duplicate row — the
                // follow-up is never silently dropped either way.
                if let Some(existing_id) =
                    crate::repository::find_content_duplicate(conn, &namespace, &memory.content)?
                {
                    if let Some(object) = metadata.as_object_mut() {
                        object.insert("canonical_memory_id".into(), serde_json::json!(existing_id));
                    }
                }
                let review_item = crate::review::queue_for_review(
                    conn,
                    &crate::review::ReviewInput {
                        content: memory.content.clone(),
                        suggested_namespace: namespace,
                        suggested_kind: classification.kind,
                        suggested_title: Some(classification.title),
                        suggested_summary: if classification.summary.is_empty() {
                            None
                        } else {
                            Some(classification.summary)
                        },
                        suggested_tags: classification.tags,
                        suggested_importance: classification.importance,
                        suggested_confidence: Some(classification.confidence),
                        source_route: Some(req.source.clone()),
                        source_ref: Some(item_ref),
                        metadata,
                    },
                )?;
                queued_review_ids.push(review_item.id);
                continue;
            }

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
                CaptureResult::Stored(m) => {
                    // Explicit user commitments open attention atomically.
                    if let Some(attention) = &memory.attention {
                        crate::attention::create_attention(
                            conn,
                            &crate::attention::AttentionInput {
                                memory_id: m.id.clone(),
                                owner: attention.owner.clone().or_else(|| Some("user".into())),
                                due_at: attention.due_at.clone(),
                                remind_at: attention.remind_at.clone(),
                                trigger: attention.trigger.clone(),
                                waiting_on: attention.waiting_on.clone(),
                                completion_condition: attention.completion_condition.clone(),
                                actor: Some(format!("agent:{}", req.source)),
                            },
                        )?;
                    }
                    // A model-asserted resolution is a reviewable candidate,
                    // never an automatic completion (untrusted transcript).
                    if let Some(target) = &memory.resolves {
                        record_resolution_candidate(conn, target, &m.id, &req.source);
                    }
                    stored_memory_ids.push(m.id);
                }
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

/// Record a model-asserted resolution as a REVIEWABLE candidate.
///
/// The `resolves` field is model output distilled from an untrusted
/// transcript: acting on it directly would let prompt injection or model
/// error close real work. So no state changes here — the target stays open
/// and a `resolution_candidate` event (with the storing memory as proposed
/// evidence) makes the claim visible in the item's history and the
/// Needs-attention surface for a human decision. Never fails the checkpoint.
fn record_resolution_candidate(conn: &Connection, target: &str, evidence_id: &str, source: &str) {
    match crate::attention::resolve_attention(conn, target) {
        Ok(item)
            if item.status == crate::attention::STATUS_OPEN
                || item.status == crate::attention::STATUS_SNOOZED =>
        {
            crate::events::log_event(
                conn,
                &crate::events::EventInput {
                    idempotency_key: Some(format!(
                        "resolution-candidate:{}:{evidence_id}",
                        item.memory_id
                    )),
                    memory_id: Some(item.memory_id.clone()),
                    namespace: Some(item.namespace.clone()),
                    actor: Some(format!("agent:{source}")),
                    event_type: crate::events::EVENT_RESOLUTION_CANDIDATE.into(),
                    reason: Some(format!(
                        "transcript claims this is complete; proposed evidence {evidence_id} — \
                         review and complete manually"
                    )),
                    ..crate::events::EventInput::default()
                },
            );
        }
        Ok(item) => {
            tracing::debug!(
                "resolution candidate for {target} ignored: already '{}'",
                item.status
            );
        }
        Err(_) => {
            tracing::debug!("resolution target {target} not found; leaving state unchanged");
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
    let (memories, usage) = crate::capture::distill_with_usage(digest, config)?;

    let result = store_checkpoint(conn, req, &memories, settings)?;

    if !result.replayed {
        crate::usage::record_checkpoint_usage(conn, &result.checkpoint_id, &config.model, &usage);
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
            attention: None,
            resolves: None,
        }
    }

    fn request(cursor: i64) -> CheckpointRequest {
        CheckpointRequest {
            source: "claude-session".into(),
            session_id: "session-1".into(),
            cursor,
            recover_stale: false,
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
    fn stale_cursor_behind_the_session_head_is_rejected() {
        let conn = test_conn();
        let settings = Settings::default();

        store_checkpoint(&conn, &request(20), &[atom("newer delta")], &settings).unwrap();

        // A previously unseen OLDER cursor is stale client state, not a
        // replay — rejecting it visibly beats re-applying overlapping data.
        let stale = store_checkpoint(&conn, &request(10), &[atom("old delta")], &settings);
        assert!(stale.is_err());
        assert_eq!(memory_count(&conn), 1, "stale delta stored nothing");

        // The exact committed key still replays, and later cursors advance.
        let replay = store_checkpoint(&conn, &request(20), &[atom("ignored")], &settings).unwrap();
        assert!(replay.replayed);
        let later =
            store_checkpoint(&conn, &request(30), &[atom("next delta")], &settings).unwrap();
        assert!(!later.replayed);
    }

    #[test]
    fn operator_recovery_accepts_a_missing_stale_checkpoint_exactly_once() {
        let conn = test_conn();
        let settings = Settings::default();

        store_checkpoint(&conn, &request(20), &[atom("newer delta")], &settings).unwrap();

        let mut recovery = request(10);
        recovery.recover_stale = true;
        let recovered =
            store_checkpoint(&conn, &recovery, &[atom("retained old delta")], &settings).unwrap();

        assert!(!recovered.replayed);
        assert_eq!(memory_count(&conn), 2);
        assert_eq!(checkpoint_count(&conn), 2);

        let replay = store_checkpoint(&conn, &recovery, &[atom("different")], &settings).unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.checkpoint_id, recovered.checkpoint_id);
        assert_eq!(replay.stored_memory_ids, recovered.stored_memory_ids);
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
    fn duplicate_content_suggestion_still_queues_for_review() {
        let conn = test_conn();
        let settings = Settings::default();

        // The fact already exists as a canonical memory.
        let first = store_checkpoint(
            &conn,
            &request(10),
            &[atom("shared follow-up fact")],
            &settings,
        )
        .unwrap();
        let canonical = first.stored_memory_ids[0].clone();

        // A later session suggests the same content as an open loop: the
        // suggestion must reach the review queue, not vanish into dedup.
        let mut suggested = atom("shared follow-up fact");
        suggested.attention = Some(crate::capture::DistilledAttention {
            explicitness: "suggested".into(),
            owner: Some("user".into()),
            ..crate::capture::DistilledAttention::default()
        });
        let second = store_checkpoint(&conn, &request(20), &[suggested], &settings).unwrap();
        assert_eq!(
            second.queued_review_ids.len(),
            1,
            "suggestion stays reviewable"
        );

        // Approval dedups onto the canonical memory and opens its attention.
        let memory =
            crate::review::approve_review(&conn, &second.queued_review_ids[0], &settings).unwrap();
        assert_eq!(memory.id, canonical, "no duplicate memory row");
        assert!(
            crate::attention::get_by_memory(&conn, &canonical)
                .unwrap()
                .is_some(),
            "the follow-up lives on the canonical memory"
        );
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
