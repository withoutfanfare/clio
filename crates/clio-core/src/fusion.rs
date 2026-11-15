//! One fused retrieval for task-matched knowledge.
//!
//! Any-word keyword search finds what names the same things as the query;
//! semantic search finds what means the same. Each misses what the other
//! finds, so both run and their results are merged by rank position
//! (reciprocal rank fusion), which needs no scaling between a BM25 score and a
//! cosine: an item scores `1 / (scoring.fusion_k + position)` in each leg that
//! returned it. Without a query vector the keyword leg stands alone.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::error::Result;
use crate::models::{Memory, RecallItem, RecallQuery};
use crate::settings::ScoringConfig;

/// Results each leg contributes before merging.
const POOL: u32 = 30;

/// Chosen query terms one keyword match must share for the keyword leg to
/// count as evidence by itself: one or two shared words out of many turn up
/// by chance. A query with fewer chosen terms needs all of them, or a short
/// follow-up could never qualify.
const AGREEING_TERMS: usize = 3;

/// Embedding threads still running, including any that overran their budget.
static EMBEDDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Counts one embedding thread for as long as it lives, panics included.
struct Embedding;

impl Embedding {
    fn start() -> Self {
        EMBEDDING.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

impl Drop for Embedding {
    fn drop(&mut self) {
        EMBEDDING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// End a one-shot process that may have left an embedding behind.
///
/// A query embedding that overran its budget is still loading or running the
/// model on its own thread. Returning from `main`, or calling
/// `std::process::exit`, then tears the embedding runtime down underneath it,
/// and the process dies with a segmentation fault after printing a perfectly
/// good answer (9 of 30 runs when forced). With such a thread alive this
/// flushes standard output and exits with `code` at once, skipping that
/// teardown. Otherwise it does nothing and the caller ends as usual.
pub fn finish_one_shot(code: i32) {
    if EMBEDDING.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        return;
    }
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    #[cfg(unix)]
    // SAFETY: `_exit` only ends the process. Every command value has been
    // dropped by the time a caller reaches the end of `main`.
    unsafe {
        libc::_exit(code)
    }
    #[cfg(not(unix))]
    let _ = code;
}

/// A query vector, the model that produced it, and when the semantic leg must
/// have finished by.
#[derive(Debug, Clone)]
pub struct QueryEmbedding {
    pub vector: Vec<f32>,
    pub model: String,
    pub deadline: Option<Instant>,
}

/// A fused retrieval request: exactly what both legs honour. Filters that
/// only keyword recall understands (kind, tags, importance, archived-only,
/// paging) have no field here, so they cannot be passed and half-applied.
#[derive(Debug, Clone, Default)]
pub struct FusedQuery {
    pub query: String,
    /// One namespace, or every namespace when `None`. A bare project name is
    /// canonicalised.
    pub namespace: Option<String>,
    pub include_archived: bool,
    pub exclude_expired: bool,
    /// Kinds neither leg may return, left out before either list is cut.
    pub exclude_kinds: Vec<String>,
    /// Results to return.
    pub limit: u32,
    /// Candidates each leg contributes before merging; `0` means the default
    /// of 30. The keyword leg contributes at least `limit`.
    pub pool: u32,
    pub scoring: Option<ScoringConfig>,
    pub skip_access_tracking: bool,
}

/// One fused result and whether the keyword leg found it.
#[derive(Debug, Clone)]
pub struct FusedItem {
    pub item: RecallItem,
    /// Found by the keyword leg; otherwise by meaning alone.
    pub keyword: bool,
}

/// Embed `query` for fused retrieval within `scoring.fusion_budget_ms`.
///
/// `None` means "use keywords alone": fusion is off, the query is empty, or
/// `embed` failed or overran the budget. `embed` runs on its own thread so a
/// slow model load or provider call cannot hold the caller; when it overruns
/// it is left to finish unobserved, so a one-shot caller must end with
/// [`finish_one_shot`].
pub fn query_embedding<F>(
    scoring: &ScoringConfig,
    query: Option<&str>,
    embed: F,
) -> Option<QueryEmbedding>
where
    F: FnOnce(&str) -> Result<(Vec<f32>, String)> + Send + 'static,
{
    if !scoring.fusion {
        return None;
    }
    let query = query.map(str::trim).filter(|q| !q.is_empty())?.to_string();
    let budget = Duration::from_millis(scoring.fusion_budget_ms);
    let deadline = (scoring.fusion_budget_ms > 0).then(|| Instant::now() + budget);
    let (tx, rx) = std::sync::mpsc::channel();
    let running = Embedding::start();
    std::thread::spawn(move || {
        let _running = running;
        let _ = tx.send(embed(&query));
    });
    let embedded = if deadline.is_some() {
        rx.recv_timeout(budget).ok()
    } else {
        rx.recv().ok()
    };
    match embedded {
        Some(Ok((vector, model))) => Some(QueryEmbedding {
            vector,
            model,
            deadline,
        }),
        Some(Err(e)) => {
            tracing::debug!("no query embedding, using keywords alone: {e}");
            None
        }
        None => {
            tracing::warn!("query embedding overran its budget, using keywords alone");
            None
        }
    }
}

/// [`query_embedding`] for a caller whose backend cannot move to another
/// thread: `embed` runs on the calling thread, so the caller waits for it
/// however long it takes. The answer is the same: an embedding that arrives
/// after the budget is discarded and keywords are used alone.
pub fn query_embedding_inline<F>(
    scoring: &ScoringConfig,
    query: Option<&str>,
    embed: F,
) -> Option<QueryEmbedding>
where
    F: FnOnce(&str) -> Result<(Vec<f32>, String)>,
{
    if !scoring.fusion {
        return None;
    }
    let query = query.map(str::trim).filter(|q| !q.is_empty())?;
    let deadline = (scoring.fusion_budget_ms > 0)
        .then(|| Instant::now() + Duration::from_millis(scoring.fusion_budget_ms));
    match embed(query) {
        Ok(_) if deadline.is_some_and(|d| Instant::now() >= d) => {
            tracing::warn!("query embedding overran its budget, using keywords alone");
            None
        }
        Ok((vector, model)) => Some(QueryEmbedding {
            vector,
            model,
            deadline,
        }),
        Err(e) => {
            tracing::debug!("no query embedding, using keywords alone: {e}");
            None
        }
    }
}

/// Fused recall over the memories `query` covers.
///
/// Each leg is ordered the way its own search orders it today. If the
/// semantic leg fails or overruns the embedding's deadline, the keyword
/// results are returned on their own.
///
/// A keyword match containing an identifier the query names ranks ahead of
/// the merged order (`scoring.identifiers_first`): rank fusion would
/// otherwise put the one memory that names the ticket below every memory
/// both legs half-agree on.
///
/// Nothing is returned when `scoring.quiet_similarity` is above zero, the
/// most similar memory searched is below it, and no keyword match either
/// shares [`AGREEING_TERMS`] of the query's terms (all of them, when it has
/// fewer) or contains an identifier the query names: any-word matching always finds something, and with no
/// signal behind it that something is noise. An exact identifier match is
/// never noise. The rule needs the semantic leg to have compared at least
/// one memory, so a store with no usable embeddings keeps its keyword
/// results.
pub fn fused_recall(
    conn: &Connection,
    query: &FusedQuery,
    embedding: Option<&QueryEmbedding>,
) -> Result<Vec<FusedItem>> {
    let pool = if query.pool == 0 { POOL } else { query.pool };
    let keyword_query = RecallQuery {
        query: Some(query.query.clone()),
        namespace: query
            .namespace
            .as_deref()
            .map(crate::context::canonical_namespace),
        include_archived: query.include_archived,
        exclude_expired: query.exclude_expired,
        limit: pool.max(query.limit),
        scoring: query.scoring.clone(),
        skip_access_tracking: true,
        match_any_term: true,
        ..Default::default()
    };
    let base = &keyword_query;
    let (mut semantic, nearest) = match embedding {
        Some(e) => semantic_leg(conn, base, &query.exclude_kinds, pool, e).unwrap_or_else(|err| {
            tracing::warn!("semantic leg failed, using keywords alone: {err}");
            (Vec::new(), None)
        }),
        None => (Vec::new(), None),
    };
    let (mut keyword, terms) =
        crate::repository::recall_any_terms(conn, &keyword_query, &query.exclude_kinds)?;

    // How the query's terms sit on the keyword matches: every term when the
    // quiet rule needs their agreement, otherwise only its identifiers.
    let defaults = ScoringConfig::default();
    let scoring = base.scoring.as_ref().unwrap_or(&defaults);
    let quiet = base.scoring.as_ref().map_or(0.0, |s| s.quiet_similarity);
    // Cosine can be negative, so a zero threshold must be checked as "off".
    let far = quiet > 0.0 && nearest.is_some_and(|similarity| similarity < quiet);
    let agreeing = AGREEING_TERMS.min(terms.len()).max(1);
    let weighed: Vec<String> = terms
        .into_iter()
        .filter(|t| far || (scoring.identifiers_first && crate::repository::is_identifier(t)))
        .collect();
    let ids: Vec<&str> = keyword.iter().map(|i| i.memory.id.as_str()).collect();
    let evidence = crate::repository::term_evidence(conn, &keyword_query, &weighed, &ids)?;
    if far {
        // Nothing is close in meaning, so each keyword match earns its own
        // place by agreeing with the query or naming an identifier; one
        // strong match does not let the weaker ones through with it.
        keyword.retain(|item| {
            evidence
                .get(item.memory.id.as_str())
                .is_some_and(|(shared, named)| *named || *shared >= agreeing)
        });
        if keyword.is_empty() {
            return Ok(Vec::new());
        }
        semantic.clear();
    }
    let names_identifier =
        |id: &str| scoring.identifiers_first && evidence.get(id).is_some_and(|(_, named)| *named);

    let k = scoring.fusion_k.max(0.0);
    let mut fused: HashMap<String, (f64, FusedItem)> = HashMap::new();
    for (position, item) in keyword.into_iter().enumerate() {
        let score = 1.0 / (k + position as f64 + 1.0);
        fused.insert(
            item.memory.id.clone(),
            (
                score,
                FusedItem {
                    item,
                    keyword: true,
                },
            ),
        );
    }
    for (position, memory) in semantic.into_iter().enumerate() {
        let score = 1.0 / (k + position as f64 + 1.0);
        let entry = fused.entry(memory.id.clone()).or_insert_with(|| {
            (
                0.0,
                FusedItem {
                    item: RecallItem {
                        memory,
                        rank: None,
                        linked_from: None,
                        link_context: Vec::new(),
                    },
                    keyword: false,
                },
            )
        });
        entry.0 += score;
    }

    let mut merged: Vec<(f64, FusedItem)> = fused.into_values().collect();
    merged.sort_by(|a, b| {
        names_identifier(&b.1.item.memory.id)
            .cmp(&names_identifier(&a.1.item.memory.id))
            .then_with(|| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.1.item.memory.id.cmp(&b.1.item.memory.id))
    });
    let items: Vec<FusedItem> = merged
        .into_iter()
        .take(query.limit as usize)
        .map(|(score, mut fused)| {
            fused.item.rank = Some(score);
            fused
        })
        .collect();

    if !query.skip_access_tracking {
        let ids: Vec<&str> = items.iter().map(|f| f.item.memory.id.as_str()).collect();
        if let Err(e) = crate::repository::touch_accessed(conn, &ids) {
            tracing::warn!("access tracking failed in fused recall: {e}");
        }
    }
    Ok(items)
}

/// Fused recall in the detected namespace, then remaining slots from `global`,
/// as scoped keyword and semantic recall do. A detected `global` stays global.
pub fn fused_recall_scoped(
    conn: &Connection,
    query: &FusedQuery,
    detected_namespace: &str,
    embedding: Option<&QueryEmbedding>,
) -> Result<Vec<FusedItem>> {
    let namespace = crate::context::canonical_namespace(detected_namespace);
    let within = |namespace: &str, limit: u32| {
        fused_recall(
            conn,
            &FusedQuery {
                namespace: Some(namespace.to_string()),
                limit,
                ..query.clone()
            },
            embedding,
        )
    };
    let mut items = within(&namespace, query.limit)?;
    let remaining = query.limit.saturating_sub(items.len() as u32);
    if namespace != "global" && remaining > 0 {
        items.extend(within("global", remaining)?);
    }
    Ok(items)
}

/// Fused search as the `search` command and `memory_search` tool run it:
/// scoped to `scope` then `global` when given, otherwise over
/// `query.namespace` (one namespace, or every namespace when `None`).
pub fn search(
    conn: &Connection,
    query: &FusedQuery,
    scope: Option<&str>,
    embedding: Option<&QueryEmbedding>,
) -> Result<Vec<RecallItem>> {
    let fused = match scope {
        Some(namespace) => fused_recall_scoped(conn, query, namespace, embedding)?,
        None => fused_recall(conn, query, embedding)?,
    };
    Ok(fused.into_iter().map(|f| f.item).collect())
}

/// The semantic leg's memories, best first, and the similarity of the nearest
/// memory compared.
type SemanticLeg = (Vec<Memory>, Option<f64>);

/// The semantic leg: the `pool` nearest memories above the similarity floor,
/// ordered as semantic search orders them (cosine times the composite
/// multiplier), and the similarity of the nearest memory compared, floor or
/// not. `base` is the keyword leg's query: its namespace is canonical.
fn semantic_leg(
    conn: &Connection,
    base: &RecallQuery,
    exclude_kinds: &[String],
    pool: u32,
    embedding: &QueryEmbedding,
) -> Result<SemanticLeg> {
    let mut hits = crate::embeddings::semantic_search_until(
        conn,
        &embedding.vector,
        &embedding.model,
        base.namespace.as_deref(),
        base.include_archived,
        base.exclude_expired,
        exclude_kinds,
        pool,
        embedding.deadline,
    )?;
    let nearest = hits.first().map(|h| h.similarity);
    let scoring = base.scoring.as_ref();
    if let Some(floor) = scoring.map(|s| s.min_similarity).filter(|f| *f > 0.0) {
        hits.retain(|h| h.similarity >= floor);
    }
    let similarity: HashMap<String, f64> = hits
        .iter()
        .map(|h| (h.memory_id.clone(), h.similarity))
        .collect();
    let ids: Vec<String> = hits.into_iter().map(|h| h.memory_id).collect();
    let now = time::OffsetDateTime::now_utc();
    let mut scored: Vec<(f64, Memory)> = crate::repository::get_many_pub(conn, &ids)?
        .into_iter()
        .map(|memory| {
            let cosine = similarity.get(&memory.id).copied().unwrap_or(0.0);
            let multiplier = scoring.map_or(1.0, |s| {
                crate::scoring::composite_multiplier(&memory, s, now)
            });
            (cosine * multiplier, memory)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.id.cmp(&b.1.id))
    });
    let ordered = scored.into_iter().map(|(_, memory)| memory).collect();
    Ok((ordered, nearest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::store_embedding;
    use crate::models::RememberInput;

    fn store(conn: &Connection, namespace: &str, content: &str, vector: &[f32]) -> Memory {
        let input: RememberInput =
            serde_json::from_value(serde_json::json!({"namespace": namespace, "content": content}))
                .unwrap();
        let memory =
            crate::repository::remember(conn, &input, &crate::settings::Settings::default())
                .unwrap();
        store_embedding(conn, &memory.id, "test", vector.len(), vector).unwrap();
        memory
    }

    fn query(text: &str) -> FusedQuery {
        FusedQuery {
            query: text.into(),
            namespace: Some("project:x".into()),
            limit: 10,
            scoring: Some(ScoringConfig {
                decay_lambda: 0.0,
                ..Default::default()
            }),
            skip_access_tracking: true,
            ..Default::default()
        }
    }

    fn embedding(vector: &[f32]) -> QueryEmbedding {
        QueryEmbedding {
            vector: vector.to_vec(),
            model: "test".into(),
            deadline: None,
        }
    }

    #[test]
    fn fusion_keeps_what_either_leg_alone_would_miss_and_ranks_agreement_first() {
        let conn = crate::db::open_in_memory().unwrap();
        // Names the identifier, means something else.
        let named = store(&conn, "project:x", "SCO-123 rollout checklist", &[0.0, 1.0]);
        // Means the same, shares no word with the query.
        let meant = store(
            &conn,
            "project:x",
            "card payments declined at checkout",
            &[1.0, 0.0],
        );
        // Both: shares a word and the meaning.
        let both = store(&conn, "project:x", "stripe SCO-123 webhook", &[0.9, 0.1]);
        store(&conn, "project:x", "unrelated gardening note", &[-1.0, 0.0]);

        let q = query("SCO-123 stripe signature failing");
        let fused = fused_recall(&conn, &q, Some(&embedding(&[1.0, 0.0]))).unwrap();
        let ids: Vec<&str> = fused.iter().map(|f| f.item.memory.id.as_str()).collect();
        assert_eq!(ids[0], both.id, "found by both legs ranks first");
        assert!(ids.contains(&named.id.as_str()) && ids.contains(&meant.id.as_str()));
        assert_eq!(
            ids.len(),
            3,
            "the unrelated note is below the similarity floor"
        );
        let meant_item = fused.iter().find(|f| f.item.memory.id == meant.id).unwrap();
        assert!(!meant_item.keyword);

        // No vector: exactly the keyword result, no error.
        let keywords = fused_recall(&conn, &q, None).unwrap();
        let ids: Vec<&str> = keywords.iter().map(|f| f.item.memory.id.as_str()).collect();
        assert!(ids.contains(&named.id.as_str()) && !ids.contains(&meant.id.as_str()));
    }

    #[test]
    fn an_overrun_or_failed_semantic_leg_falls_back_to_keywords() {
        let conn = crate::db::open_in_memory().unwrap();
        let named = store(&conn, "project:x", "lease expiry rules", &[0.0, 1.0]);
        store(
            &conn,
            "project:x",
            "something similar in meaning",
            &[1.0, 0.0],
        );
        let late = QueryEmbedding {
            deadline: Some(Instant::now() - Duration::from_secs(1)),
            ..embedding(&[1.0, 0.0])
        };
        let fused = fused_recall(&conn, &query("lease expiry"), Some(&late)).unwrap();
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].item.memory.id, named.id);
    }

    #[test]
    fn a_memory_naming_the_querys_identifier_ranks_first() {
        let conn = crate::db::open_in_memory().unwrap();
        // Both legs half-agree on these; neither names the ticket.
        let agreed: Vec<Memory> = (0..3)
            .map(|i| {
                store(
                    &conn,
                    "project:x",
                    &format!("stripe webhook retries {i}"),
                    &[1.0, 0.0],
                )
            })
            .collect();
        let named = store(
            &conn,
            "project:x",
            "SCO-123 was closed as a duplicate",
            &[0.0, 1.0],
        );
        let q = query("SCO-123 stripe webhook retries");
        let near = embedding(&[1.0, 0.0]);

        let fused = fused_recall(&conn, &q, Some(&near)).unwrap();
        assert_eq!(fused[0].item.memory.id, named.id);
        assert_eq!(fused.len(), 4);

        let plain = FusedQuery {
            scoring: Some(ScoringConfig {
                decay_lambda: 0.0,
                identifiers_first: false,
                ..Default::default()
            }),
            ..q.clone()
        };
        let fused = fused_recall(&conn, &plain, Some(&near)).unwrap();
        assert!(agreed.iter().any(|m| m.id == fused[0].item.memory.id));
        assert_eq!(fused[3].item.memory.id, named.id);
    }

    #[test]
    fn stays_quiet_when_nothing_searched_is_close_in_meaning() {
        let conn = crate::db::open_in_memory().unwrap();
        // Shares a word with the query, means something else (cosine 0.3).
        store(&conn, "project:x", "lease the office car", &[0.3, 0.954]);
        let q = query("lease expiry for agents");
        let near = embedding(&[1.0, 0.0]);
        assert!(fused_recall(&conn, &q, Some(&near)).unwrap().is_empty());

        // The keyword match returns once the rule is off, or with no vector.
        let off = FusedQuery {
            scoring: Some(ScoringConfig {
                decay_lambda: 0.0,
                quiet_similarity: 0.0,
                ..Default::default()
            }),
            ..q.clone()
        };
        assert_eq!(fused_recall(&conn, &off, Some(&near)).unwrap().len(), 1);
        assert_eq!(fused_recall(&conn, &q, None).unwrap().len(), 1);

        // No memory with a comparable embedding: the rule has no evidence.
        let other_model = QueryEmbedding {
            model: "another-model".into(),
            ..near.clone()
        };
        assert_eq!(
            fused_recall(&conn, &q, Some(&other_model)).unwrap().len(),
            1
        );

        // Three of the query's words agreeing on one memory is evidence by
        // itself, however far its meaning sits. It does not bring the
        // one-word match in with it.
        let agreed = store(
            &conn,
            "project:x",
            "lease expiry notes for agents",
            &[0.0, 1.0],
        );
        let found = fused_recall(&conn, &q, Some(&near)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].item.memory.id, agreed.id);
        crate::repository::archive(&conn, &agreed.id).unwrap();
        assert!(fused_recall(&conn, &q, Some(&near)).unwrap().is_empty());

        // Something close in meaning lets the keyword match through as well.
        store(
            &conn,
            "project:x",
            "tokens time out after an hour",
            &[0.9, 0.436],
        );
        assert_eq!(fused_recall(&conn, &q, Some(&near)).unwrap().len(), 2);
    }

    #[test]
    fn an_exact_identifier_is_never_silenced() {
        let conn = crate::db::open_in_memory().unwrap();
        // The only memory naming the ticket points the opposite way in
        // meaning, and nothing else is near the query either.
        let ticket = store(&conn, "project:x", "CLIO-123", &[-1.0, 0.0]);
        let near = embedding(&[1.0, 0.0]);
        let q = query("CLIO-123");
        let found = fused_recall(&conn, &q, Some(&near)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].item.memory.id, ticket.id);

        // The veto does not depend on identifier-first ordering.
        let unordered = FusedQuery {
            scoring: Some(ScoringConfig {
                decay_lambda: 0.0,
                identifiers_first: false,
                ..Default::default()
            }),
            ..q
        };
        assert_eq!(
            fused_recall(&conn, &unordered, Some(&near)).unwrap().len(),
            1
        );
    }

    #[test]
    fn a_zero_threshold_switches_the_quiet_rule_off_for_negative_cosines() {
        let conn = crate::db::open_in_memory().unwrap();
        store(&conn, "project:x", "lease the office car", &[-1.0, 0.0]);
        let near = embedding(&[1.0, 0.0]);
        let q = query("lease expiry for agents");
        assert!(fused_recall(&conn, &q, Some(&near)).unwrap().is_empty());
        let off = FusedQuery {
            scoring: Some(ScoringConfig {
                decay_lambda: 0.0,
                quiet_similarity: 0.0,
                ..Default::default()
            }),
            ..q
        };
        assert_eq!(fused_recall(&conn, &off, Some(&near)).unwrap().len(), 1);
    }

    #[test]
    fn a_short_query_needs_only_its_own_terms_to_agree() {
        let conn = crate::db::open_in_memory().unwrap();
        let rule = store(
            &conn,
            "project:x",
            "ship releases with the script",
            &[0.0, 1.0],
        );
        let near = embedding(&[1.0, 0.0]);
        // One chosen term ("it" is too short to count), and the memory has it.
        let found = fused_recall(&conn, &query("Yes, ship it"), Some(&near)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].item.memory.id, rule.id);
        // Two chosen terms, only one shared: still quiet.
        assert!(
            fused_recall(&conn, &query("ship yesterday"), Some(&near))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn excluded_kinds_leave_before_either_list_is_cut() {
        let conn = crate::db::open_in_memory().unwrap();
        for i in 0..(POOL + 5) {
            let input: RememberInput = serde_json::from_value(serde_json::json!({
                "namespace": "project:x", "kind": "receipt", "content": format!("session write-up {i}")
            }))
            .unwrap();
            let receipt =
                crate::repository::remember(&conn, &input, &crate::settings::Settings::default())
                    .unwrap();
            store_embedding(&conn, &receipt.id, "test", 2, &[1.0, 0.0]).unwrap();
        }
        // Less similar than every receipt, and shares no word with the query.
        let fact = store(
            &conn,
            "project:x",
            "tokens time out after an hour",
            &[0.9, 0.436],
        );
        let near = embedding(&[1.0, 0.0]);
        let q = query("how long do logins last");

        let all = fused_recall(&conn, &q, Some(&near)).unwrap();
        assert!(all.iter().all(|f| f.item.memory.kind == "receipt"));

        let without = FusedQuery {
            exclude_kinds: vec!["receipt".into()],
            ..q
        };
        let found = fused_recall(&conn, &without, Some(&near)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].item.memory.id, fact.id);
    }

    #[test]
    fn inline_embedding_follows_the_same_switch_budget_and_failure() {
        let on = ScoringConfig {
            fusion_budget_ms: 50,
            ..Default::default()
        };
        let ok = |_: &str| Ok((vec![1.0_f32], "test".to_string()));
        assert!(query_embedding_inline(&on, Some("task"), ok).is_some());
        assert!(query_embedding_inline(&on, Some("  "), ok).is_none());
        let off = ScoringConfig {
            fusion: false,
            ..on.clone()
        };
        assert!(query_embedding_inline(&off, Some("task"), ok).is_none());
        let failing = |_: &str| Err(crate::error::ClioError::Config("still loading".into()));
        assert!(query_embedding_inline(&on, Some("task"), failing).is_none());
        let slow = |_: &str| {
            std::thread::sleep(Duration::from_millis(120));
            Ok((vec![1.0_f32], "test".to_string()))
        };
        assert!(query_embedding_inline(&on, Some("task"), slow).is_none());
    }

    #[test]
    fn query_embedding_respects_the_switch_the_budget_and_failure() {
        let on = ScoringConfig {
            fusion_budget_ms: 50,
            ..Default::default()
        };
        let ok = |_: &str| Ok((vec![1.0_f32], "test".to_string()));
        assert!(query_embedding(&on, Some("task"), ok).is_some());
        assert!(query_embedding(&on, Some("  "), ok).is_none());
        assert!(query_embedding(&on, None, ok).is_none());
        let off = ScoringConfig {
            fusion: false,
            ..on.clone()
        };
        assert!(query_embedding(&off, Some("task"), ok).is_none());
        let failing = |_: &str| {
            Err(crate::error::ClioError::Config(
                "embeddings are disabled".into(),
            ))
        };
        assert!(query_embedding(&on, Some("task"), failing).is_none());
        let slow = |_: &str| {
            std::thread::sleep(Duration::from_millis(400));
            Ok((vec![1.0_f32], "test".to_string()))
        };
        assert!(query_embedding(&on, Some("task"), slow).is_none());
    }

    #[test]
    fn scoped_fusion_fills_from_global_after_the_project() {
        let conn = crate::db::open_in_memory().unwrap();
        let project = store(&conn, "project:x", "lease expiry rules", &[1.0, 0.0]);
        let global = store(&conn, "global", "lease etiquette everywhere", &[1.0, 0.0]);
        let fused = fused_recall_scoped(
            &conn,
            &FusedQuery {
                namespace: None,
                ..query("lease")
            },
            "x",
            Some(&embedding(&[1.0, 0.0])),
        )
        .unwrap();
        let ids: Vec<&str> = fused.iter().map(|f| f.item.memory.id.as_str()).collect();
        assert_eq!(ids, vec![project.id.as_str(), global.id.as_str()]);
    }
}
