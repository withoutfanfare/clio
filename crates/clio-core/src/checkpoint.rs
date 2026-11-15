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
    /// Machine the working directory belongs to, recorded beside it.
    #[serde(default)]
    pub host: Option<String>,
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
                if let Some(host) = &req.host {
                    meta.insert("host".into(), serde_json::json!(host));
                }
            }
            if let Some(branch) = &req.branch {
                meta.insert("branch".into(), serde_json::json!(branch));
            }
            if let Some(attention) = &memory.attention {
                meta.insert("attention".into(), serde_json::to_value(attention)?);
            }
            memory.stamp_expiry(&mut meta);
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
                // attention to it instead of storing a duplicate row.
                let live_twin =
                    crate::repository::find_content_duplicate(conn, &namespace, &memory.content)?;
                if let Some(existing_id) = &live_twin {
                    if let Some(object) = metadata.as_object_mut() {
                        object.insert("canonical_memory_id".into(), serde_json::json!(existing_id));
                    }
                }
                let input = crate::review::ReviewInput {
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
                };
                // With no live twin, an archived one makes this a revival
                // proposal like any other capture: one open item per hidden
                // memory, and none once a person has rejected it.
                let archived_twin = match live_twin {
                    Some(_) => None,
                    None => crate::capture::propose_revival(conn, &input)?,
                };
                match archived_twin {
                    Some(crate::capture::ArchivedTwin::Proposed(item)) => {
                        queued_review_ids.push(item.id)
                    }
                    Some(crate::capture::ArchivedTwin::Rejected(_)) => {}
                    None => {
                        queued_review_ids.push(crate::review::queue_for_review(conn, &input)?.id)
                    }
                }
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
                // Content of an archived memory whose revival a person
                // rejected: the sighting was recorded and nothing else is.
                CaptureResult::Stored(m) if m.archived_at.is_some() => {}
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
        match crate::repository::get_raw(conn, id) {
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

/// Most titles from earlier deltas of a session shown to the distiller.
const PRIOR_TITLES_LIMIT: u32 = 20;
/// Longest title shown. With the limit above, the added prompt text stays
/// under about 1,800 characters however long the session runs.
const PRIOR_TITLE_CHARS: usize = 80;

/// Prefix `digest` with the titles already stored from earlier deltas of the
/// same session, so the distiller does not store the same fact again in new
/// words. A long session is distilled one slice at a time and each slice
/// would otherwise be judged with no sight of the others.
///
/// Only deltas with a lower cursor than this request count as earlier, so an
/// older delta recovered late is not shown titles from what came after it.
/// The list is a hint, not a complete record: a fact this session restated
/// that was deduplicated onto another session's memory, or that is waiting
/// in the review queue, is not in it.
///
/// Returns the digest untouched when the session has stored nothing yet, and
/// on any lookup failure: this is a hint, never a reason to lose a capture.
pub fn digest_with_prior_titles(
    conn: &Connection,
    req: &CheckpointRequest,
    digest: &str,
) -> String {
    // A session's memories share the `{session_id}@` provenance prefix. The
    // range is that prefix as an index scan: `A` is the character after `@`.
    // The cursor follows the `@`; casting what is left reads its digits.
    let titles = (|| -> Result<Vec<String>> {
        let mut stmt = conn.prepare_cached(
            "SELECT title FROM memories
             WHERE source = ?1 AND source_ref >= ?2 AND source_ref < ?3
               AND CAST(substr(source_ref, ?4) AS INTEGER) < ?5
               AND kind != 'receipt' AND title IS NOT NULL
             ORDER BY created_at DESC LIMIT ?6",
        )?;
        let rows = stmt.query_map(
            params![
                req.source,
                format!("{}@", req.session_id),
                format!("{}A", req.session_id),
                req.session_id.chars().count() + 2,
                req.cursor,
                PRIOR_TITLES_LIMIT
            ],
            |row| row.get::<_, String>(0),
        )?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    })();

    let titles = match titles {
        Ok(titles) if !titles.is_empty() => titles,
        Ok(_) => return digest.to_string(),
        Err(e) => {
            tracing::warn!("could not look up this session's earlier memories: {e}");
            return digest.to_string();
        }
    };

    let mut text = String::from("ALREADY STORED from earlier in this session (titles only):\n");
    for title in titles {
        let title: String = title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(PRIOR_TITLE_CHARS)
            .collect();
        text.push_str(&format!("- {title}\n"));
    }
    text.push_str("\nSESSION DIGEST:\n");
    text.push_str(digest);
    text
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
    let digest = digest_with_prior_titles(conn, req, digest);
    let (memories, usage) = crate::capture::distill_with_usage(&digest, config)?;

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
            lifetime: crate::capture::Lifetime::Durable,
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
            host: Some("mbpro".into()),
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
    fn checkpoint_records_cwd_with_its_host() {
        let conn = test_conn();
        let settings = Settings::default();

        store_checkpoint(&conn, &request(10), &[atom("fact one")], &settings).unwrap();

        let meta: String = conn
            .query_row("SELECT metadata_json FROM memories", [], |r| r.get(0))
            .unwrap();
        let meta: serde_json::Value = serde_json::from_str(&meta).unwrap();
        assert_eq!(meta["cwd"], "/tmp/project");
        assert_eq!(meta["host"], "mbpro");
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
    fn status_notes_expire_and_durable_knowledge_does_not() {
        let conn = test_conn();
        let mut settings = Settings::default();
        settings.capture.review_threshold = Some(0.5);

        let status = |content: &str, confidence: f64| DistilledMemory {
            lifetime: crate::capture::Lifetime::Status,
            confidence,
            ..atom(content)
        };
        let result = store_checkpoint(
            &conn,
            &request(10),
            &[
                atom("the queue is a spool directory"),
                status("PR 12 passed review today", 1.0),
                status("staging is on revision abc", 0.1),
            ],
            &settings,
        )
        .unwrap();

        let get = |id: &str| crate::repository::get_raw(&conn, id).unwrap();
        assert_eq!(get(&result.stored_memory_ids[0]).valid_until, None);
        let until = get(&result.stored_memory_ids[1]).valid_until.unwrap();
        let days: f64 = conn
            .query_row("SELECT julianday(?1) - julianday('now')", [&until], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((29.9..=30.1).contains(&days), "expires in {days} days");

        // A status note that waited in the review queue keeps its expiry.
        let approved =
            crate::review::approve_review(&conn, &result.queued_review_ids[0], &settings).unwrap();
        assert!(approved.valid_until.is_some());

        // Expiry hides a memory from recall that excludes expired rows; the
        // row itself is still there.
        conn.execute(
            "UPDATE memories SET valid_until = '2000-01-01T00:00:00Z' WHERE id = ?1",
            [&result.stored_memory_ids[1]],
        )
        .unwrap();
        let recalled = crate::repository::recall(
            &conn,
            &crate::models::RecallQuery {
                namespace: Some("project:checkpoint-test".into()),
                exclude_expired: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            recalled
                .items
                .iter()
                .all(|m| m.memory.id != result.stored_memory_ids[1])
        );
        assert!(get(&result.stored_memory_ids[1]).archived_at.is_none());
    }

    #[test]
    fn later_slices_are_told_what_the_session_already_stored() {
        let conn = test_conn();
        let settings = Settings::default();
        let digest = "## User\nmore work";

        // Nothing stored yet: the digest is passed through byte for byte.
        assert_eq!(
            digest_with_prior_titles(&conn, &request(10), digest),
            digest
        );

        let receipt = DistilledMemory {
            kind: "receipt".into(),
            ..atom("did the work")
        };
        store_checkpoint(&conn, &request(10), &[atom("fact one"), receipt], &settings).unwrap();
        let mut other = request(10);
        other.session_id = "session-10".into();
        store_checkpoint(&conn, &other, &[atom("another session")], &settings).unwrap();

        let text = digest_with_prior_titles(&conn, &request(20), digest);
        assert!(text.starts_with("ALREADY STORED"));
        assert!(text.contains("- Title: fact one\n"));
        assert!(text.ends_with(&format!("SESSION DIGEST:\n{digest}")));
        // Receipts are per-slice records, and other sessions are not ours.
        assert!(!text.contains("did the work"));
        assert!(!text.contains("another session"));

        // An older delta recovered late sees only what came before it.
        store_checkpoint(&conn, &request(40), &[atom("fact two")], &settings).unwrap();
        let recovered = digest_with_prior_titles(&conn, &request(30), digest);
        assert!(recovered.contains("fact one") && !recovered.contains("fact two"));
        assert_eq!(
            digest_with_prior_titles(&conn, &request(10), digest),
            digest
        );

        // However much a session stores, the added text stays bounded.
        let long = "x".repeat(200);
        for cursor in 0..30 {
            let facts: Vec<_> = (0..3)
                .map(|n| atom(&format!("{long} {cursor} {n}")))
                .collect();
            store_checkpoint(&conn, &request(100 + cursor), &facts, &settings).unwrap();
        }
        let added = digest_with_prior_titles(&conn, &request(500), digest).len() - digest.len();
        assert!(added < 1_800, "added {added} characters");
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
    fn a_suggested_follow_up_on_archived_content_shares_the_one_revival_proposal() {
        let conn = test_conn();
        let settings = Settings::default();
        let suggested = || DistilledMemory {
            attention: Some(crate::capture::DistilledAttention {
                explicitness: "suggested".into(),
                owner: Some("user".into()),
                ..crate::capture::DistilledAttention::default()
            }),
            ..atom("chase the vendor")
        };
        let review_rows = || -> i64 {
            conn.query_row("SELECT COUNT(*) FROM review_queue", [], |r| r.get(0))
                .unwrap()
        };

        let stored =
            store_checkpoint(&conn, &request(10), &[atom("chase the vendor")], &settings).unwrap();
        let archived = stored.stored_memory_ids[0].clone();
        crate::repository::archive(&conn, &archived).unwrap();

        // An ordinary sighting opens the proposal; it carries no follow-up.
        let plain =
            store_checkpoint(&conn, &request(20), &[atom("chase the vendor")], &settings).unwrap();
        let proposal = plain.queued_review_ids[0].clone();

        // Suggested follow-ups on the same content, in two later deltas, join
        // that proposal instead of queueing one item per checkpoint.
        for cursor in [30, 40] {
            let result =
                store_checkpoint(&conn, &request(cursor), &[suggested()], &settings).unwrap();
            assert_eq!(result.queued_review_ids, [proposal.clone()]);
        }
        assert_eq!(review_rows(), 1);
        let item = crate::review::get_review(&conn, &proposal).unwrap();
        assert_eq!(item.metadata["revive_memory_id"], archived.as_str());
        assert_eq!(item.metadata["attention"]["owner"], "user");

        // Approval revives the one memory and opens the follow-up on it.
        let memory = crate::review::approve_review(&conn, &proposal, &settings).unwrap();
        assert_eq!(memory.id, archived);
        assert!(memory.archived_at.is_none());
        assert!(
            crate::attention::get_by_memory(&conn, &archived)
                .unwrap()
                .is_some()
        );
        assert_eq!(memory_count(&conn), 1);
    }

    #[test]
    fn content_whose_revival_was_rejected_opens_nothing_on_later_sightings() {
        let conn = test_conn();
        let settings = Settings::default();
        let stored =
            store_checkpoint(&conn, &request(10), &[atom("chase the vendor")], &settings).unwrap();
        let archived = stored.stored_memory_ids[0].clone();
        crate::repository::archive(&conn, &archived).unwrap();
        let proposed =
            store_checkpoint(&conn, &request(20), &[atom("chase the vendor")], &settings).unwrap();
        crate::review::reject_review(&conn, &proposed.queued_review_ids[0]).unwrap();

        let with_attention = |explicitness: &str| DistilledMemory {
            attention: Some(crate::capture::DistilledAttention {
                explicitness: explicitness.into(),
                ..crate::capture::DistilledAttention::default()
            }),
            ..atom("chase the vendor")
        };
        let later = store_checkpoint(
            &conn,
            &request(30),
            &[with_attention("explicit"), with_attention("suggested")],
            &settings,
        )
        .unwrap();

        assert!(later.stored_memory_ids.is_empty());
        assert!(later.queued_review_ids.is_empty());
        assert!(
            crate::attention::get_by_memory(&conn, &archived)
                .unwrap()
                .is_none(),
            "no follow-up is opened on a memory a person chose to keep hidden"
        );
        assert!(
            crate::repository::get_raw(&conn, &archived)
                .unwrap()
                .archived_at
                .is_some()
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
