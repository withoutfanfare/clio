//! Capture pipeline: accept raw unstructured text, classify it using an
//! OpenAI-compatible LLM, and store the result as a structured memory.

use crate::error::{ClioError, Result};
use crate::models::{Memory, RememberInput};
use crate::review::{ReviewInput, ReviewItem};
use crate::settings::CaptureConfig;

// ---------------------------------------------------------------------------
// Capture result
// ---------------------------------------------------------------------------

/// Outcome of the capture pipeline. When a review threshold is configured
/// and the classification confidence falls below it, the capture is routed
/// to the review queue instead of being stored immediately.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome")]
pub enum CaptureResult {
    /// The capture was stored directly as a memory.
    Stored(Memory),
    /// The capture was queued for review.
    Queued(ReviewItem),
}

// ---------------------------------------------------------------------------
// Classification result
// ---------------------------------------------------------------------------

/// The structured output returned by the LLM classification step.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClassificationResult {
    /// Memory kind — one of note, fact, decision, summary, task, observation, constraint.
    pub kind: String,
    /// Concise label (max 240 chars).
    pub title: String,
    /// One-sentence summary (max 1000 chars).
    pub summary: String,
    /// 1-5 lowercase tags.
    pub tags: Vec<String>,
    /// Suggested namespace: "global", "project:<slug>", or "topic:<slug>".
    pub namespace: String,
    /// Importance on a 1-5 scale.
    pub importance: i32,
    /// Confidence score 0.0-1.0.
    pub confidence: f64,
}

/// A single durable memory extracted from a longer body of text (e.g. a
/// session transcript). Unlike [`ClassificationResult`], which describes how to
/// file one supplied blob, each `DistilledMemory` carries its own
/// self-contained `content` — the distilled fact, decision, or insight.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DistilledMemory {
    /// The self-contained durable fact to store as the memory body.
    pub content: String,
    /// Memory kind — one of note, fact, decision, summary, task, observation, constraint.
    pub kind: String,
    /// Concise label (max 240 chars).
    pub title: String,
    /// One-sentence summary (max 1000 chars).
    pub summary: String,
    /// 1-5 lowercase tags.
    pub tags: Vec<String>,
    /// Suggested namespace: "global", "project:<slug>", or "topic:<slug>".
    pub namespace: String,
    /// Importance on a 1-5 scale.
    pub importance: i32,
    /// Confidence score 0.0-1.0.
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// System prompt
// ---------------------------------------------------------------------------

const CLASSIFICATION_SYSTEM_PROMPT: &str = r#"You are a memory classification assistant. Given unstructured text, you extract structured fields for storage in a knowledge base.

Respond ONLY with a JSON object containing these fields:
- "kind": one of "note", "fact", "decision", "summary", "task", "observation", "constraint"
- "title": a concise label (max 240 characters)
- "summary": a one-sentence summary (max 1000 characters)
- "tags": an array of 1 to 5 lowercase tags
- "namespace": suggest "global", or "project:<slug>", or "topic:<slug>"
- "importance": integer 1 to 5, calibrated strictly (see scale below)
- "confidence": float 0.0 to 1.0 — how certain you are about this classification

Importance scale (do not inflate — most items are 3):
- 5: an invariant, security/data-loss risk, or something that breaks things if forgotten
- 4: an important architectural decision or a hard-won, non-obvious fact
- 3: useful context worth keeping (the default)
- 2: a minor preference or detail
- 1: trivial

Rules:
- Tags must be lowercase, no spaces, use hyphens if needed.
- The namespace slug should be short and descriptive.
- If the text is ambiguous, prefer "note" as kind and lower confidence.
- Output ONLY valid JSON, no markdown fences, no extra text."#;

const DISTILLATION_SYSTEM_PROMPT: &str = r#"You are a knowledge curator for a long-lived, cross-tool memory shared by several AI coding assistants. You are given a digest of one working session (user prompts, assistant replies, and the tools that were run). Your job is to extract only the DURABLE KNOWLEDGE worth recalling in a completely different session weeks from now.

Capture things like:
- A decision and the reasoning behind it ("kind": "decision")
- A non-obvious fact, constraint, gotcha, or API quirk discovered ("kind": "fact" or "constraint")
- An architectural insight or how a tricky part of the system actually works ("kind": "observation")
- A durable user preference expressed during the session ("kind": "fact")

Do NOT capture:
- Routine activity, step-by-step narration, or "what was done" ("I edited file X, ran the tests")
- Lists of changed files, diff stats, or commit mechanics
- Anything trivially re-derivable by reading the current code or git history
- Transient state, in-progress work, speculation, or things specific to this one session

Each captured memory must be SELF-CONTAINED: a reader with no access to this session must understand it. Prefer fewer, higher-value memories. If the session produced nothing durable, return an empty array — this is the correct and expected outcome for most routine sessions.

Respond ONLY with a JSON array (possibly empty). Each element is an object with:
- "content": the self-contained durable fact, decision, or insight (the memory body)
- "kind": one of "note", "fact", "decision", "summary", "task", "observation", "constraint"
- "title": a concise label (max 240 characters)
- "summary": a one-sentence summary (max 1000 characters)
- "tags": an array of 1 to 5 lowercase tags (no spaces, use hyphens)
- "namespace": "global", or "project:<slug>", or "topic:<slug>"
- "importance": integer 1 to 5, calibrated strictly (see scale below)
- "confidence": float 0.0 to 1.0 — how certain you are this is durable knowledge worth keeping

Importance scale (do not inflate — if everything is a 4, the scale is useless; most items are 3):
- 5: an invariant, security/data-loss risk, or something that breaks things if forgotten
- 4: an important architectural decision or a hard-won, non-obvious fact
- 3: useful context worth keeping (the default)
- 2: a minor preference or detail
- 1: trivial

Output ONLY valid JSON, no markdown fences, no extra text. An empty session digest, or one with no durable knowledge, MUST yield []."#;

// ---------------------------------------------------------------------------
// Classify
// ---------------------------------------------------------------------------

/// Classify a single blob of text into structured memory fields.
#[cfg(feature = "capture")]
pub fn classify(text: &str, config: &CaptureConfig) -> Result<ClassificationResult> {
    parse_classification(&chat(CLASSIFICATION_SYSTEM_PROMPT, text, config)?)
}

/// Resolve the API key from config or the `OPENAI_API_KEY` environment variable.
#[cfg(feature = "capture")]
fn resolve_api_key(config: &CaptureConfig) -> Result<String> {
    match &config.api_key {
        Some(key) if !key.is_empty() => Ok(key.clone()),
        _ => std::env::var("OPENAI_API_KEY").map_err(|_| {
            ClioError::Config(
                "capture API key required: set OPENAI_API_KEY or configure capture.api_key in settings"
                    .into(),
            )
        }),
    }
}

/// Send a system + user prompt to the configured OpenAI-compatible chat
/// completions endpoint and return the assistant message content. Shared by
/// classify, distill, and consolidate. Safe to call from synchronous code.
#[cfg(feature = "capture")]
pub(crate) fn chat(system: &str, user: &str, config: &CaptureConfig) -> Result<String> {
    if !config.enabled {
        return Err(ClioError::Config("capture pipeline is not enabled".into()));
    }
    let api_key = resolve_api_key(config)?;
    get_or_create_runtime().block_on(chat_async(system, user, &api_key, config))
}

/// Reuse a single tokio runtime across all capture classify calls.
#[cfg(feature = "capture")]
fn get_or_create_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create capture async runtime")
    })
}

#[cfg(feature = "capture")]
async fn chat_async(
    system: &str,
    user: &str,
    api_key: &str,
    config: &CaptureConfig,
) -> Result<String> {
    let base_url = config.base_url.trim_end_matches('/');
    let url = format!("{base_url}/chat/completions");

    let body = serde_json::json!({
        "model": config.model,
        "temperature": 0.1,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ClioError::Storage(format!("capture API request failed: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".into());
        tracing::debug!("Capture API error body: {body}");
        return Err(ClioError::Storage(format!("capture API returned {status}")));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| ClioError::Storage(format!("capture API response parse error: {e}")))?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ClioError::Storage("capture API: missing choices[0].message.content".into()))
}

/// Distil a longer body of text (e.g. a session transcript) into zero or more
/// durable memories. An empty result is valid and expected for routine input.
#[cfg(feature = "capture")]
pub fn distill(text: &str, config: &CaptureConfig) -> Result<Vec<DistilledMemory>> {
    parse_distillation(&chat(DISTILLATION_SYSTEM_PROMPT, text, config)?)
}

/// Parse the LLM's JSON response into a `ClassificationResult`, with
/// normalisation and clamping of values.
pub fn parse_classification(raw: &str) -> Result<ClassificationResult> {
    // Strip possible markdown fences the LLM might include despite instructions.
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

    let v: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        ClioError::Validation(format!("capture classification JSON parse error: {e}"))
    })?;

    Ok(classification_from_value(&v))
}

/// Normalise and clamp the classification fields of a JSON object. Shared by
/// `parse_classification` and `parse_distillation` so both apply identical
/// rules for kind validation, title/summary truncation, tag normalisation,
/// and importance/confidence clamping.
fn classification_from_value(v: &serde_json::Value) -> ClassificationResult {
    let kind = v["kind"].as_str().unwrap_or("note").to_lowercase();
    let valid_kinds = [
        "note",
        "fact",
        "decision",
        "summary",
        "task",
        "observation",
        "constraint",
    ];
    let kind = if valid_kinds.contains(&kind.as_str()) {
        kind
    } else {
        "note".into()
    };

    let title = v["title"]
        .as_str()
        .unwrap_or("Untitled")
        .chars()
        .take(240)
        .collect::<String>();

    let summary = v["summary"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(1000)
        .collect::<String>();

    let tags: Vec<String> = v["tags"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(|t| t.trim().to_lowercase().replace(' ', "-"))
                .filter(|t| !t.is_empty() && t.len() <= 60)
                .take(5)
                .collect()
        })
        .unwrap_or_default();

    let namespace = v["namespace"].as_str().unwrap_or("global").to_string();

    let importance = v["importance"].as_i64().unwrap_or(3).clamp(1, 5) as i32;

    let confidence = v["confidence"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0);

    ClassificationResult {
        kind,
        title,
        summary,
        tags,
        namespace,
        importance,
        confidence,
    }
}

/// Parse the LLM's JSON response from the distillation step into zero or more
/// [`DistilledMemory`] values. Accepts either a bare JSON array or an object
/// wrapping the array under a `memories` key. Items whose `content` is empty or
/// whitespace-only are dropped. An empty array is valid and yields no memories.
pub fn parse_distillation(raw: &str) -> Result<Vec<DistilledMemory>> {
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

    let v: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        ClioError::Validation(format!("capture distillation JSON parse error: {e}"))
    })?;

    // Accept a bare array, or an object wrapping it under common keys.
    let array = if let Some(arr) = v.as_array() {
        arr.clone()
    } else if let Some(arr) = v["memories"].as_array() {
        arr.clone()
    } else if let Some(arr) = v["items"].as_array() {
        arr.clone()
    } else {
        return Err(ClioError::Validation(
            "capture distillation expected a JSON array of memories".into(),
        ));
    };

    let memories = array
        .iter()
        .filter_map(|item| {
            let content = item["content"].as_str().unwrap_or("").trim().to_string();
            if content.is_empty() {
                return None;
            }
            let c = classification_from_value(item);
            // Deterministic backstop: even with the distillation prompt forbidding
            // it, the LLM occasionally emits a "session summary"/"commit summary"
            // memory describing the working session itself rather than durable
            // knowledge. Drop those rather than letting them pollute recall.
            if is_session_noise(&c.title) {
                return None;
            }
            Some(DistilledMemory {
                content,
                kind: c.kind,
                title: c.title,
                summary: c.summary,
                tags: c.tags,
                namespace: c.namespace,
                importance: c.importance,
                confidence: c.confidence,
            })
        })
        .collect();

    Ok(memories)
}

/// True when a distilled memory's title describes the working session or commit
/// mechanics itself ("Stuntrocketv3 Session Summary", "Recent commits on main
/// branch", "Exploratory session in ...") rather than a piece of durable
/// knowledge. These are exactly the low-value summaries the distillation prompt
/// already forbids; this match is the deterministic safety net for when the LLM
/// ignores it.
///
/// Matching is on title phrases, not bare words: a legitimate memory may mention
/// "session" (e.g. "Session token expiry is 24h") and must not be dropped.
pub fn is_session_noise(title: &str) -> bool {
    let normalised = title.to_lowercase();
    const NOISE_PHRASES: &[&str] = &[
        "exploratory session",
        "session summary",
        "session update",
        "session recap",
        "session notes",
        "development session",
        "coding session",
        "work session",
        "session on branch",
        "commit summary",
        "summary of commits",
        "recent commits",
        "branch summary",
    ];
    NOISE_PHRASES.iter().any(|p| normalised.contains(p))
}

/// Resolve the namespace for a single distilled memory. Precedence:
/// `override_ns` (explicit `--namespace`) → the model's `"global"` promotion →
/// `default_ns` (the working directory's namespace) → the model's suggestion.
/// See [`distill_and_store`] for the rationale.
fn resolve_distill_namespace(
    override_ns: Option<&str>,
    llm_choice: &str,
    default_ns: Option<&str>,
) -> String {
    if let Some(o) = override_ns {
        o.to_string()
    } else if llm_choice == "global" {
        "global".to_string()
    } else if let Some(def) = default_ns {
        def.to_string()
    } else {
        llm_choice.to_string()
    }
}

// ---------------------------------------------------------------------------
// Full capture pipeline
// ---------------------------------------------------------------------------

/// Run the full capture pipeline: classify → route → store or queue.
///
/// If `namespace_override` is provided it takes precedence over the LLM's
/// suggestion. When `CaptureConfig::review_threshold` is set and the
/// classification confidence falls below it, the item is routed to the
/// review queue instead of being stored as a memory.
#[cfg(feature = "capture")]
pub fn capture(
    conn: &rusqlite::Connection,
    text: &str,
    config: &CaptureConfig,
    namespace_override: Option<&str>,
    settings: &crate::settings::Settings,
) -> Result<CaptureResult> {
    let classification = classify(text, config)?;
    capture_with_classification(conn, text, &classification, namespace_override, settings)
}

/// Store a memory from a classification result, or route to the review
/// queue if the confidence falls below the configured threshold.
///
/// Separated out so that dry-run logic can call `classify` independently.
pub fn capture_with_classification(
    conn: &rusqlite::Connection,
    text: &str,
    classification: &ClassificationResult,
    namespace_override: Option<&str>,
    settings: &crate::settings::Settings,
) -> Result<CaptureResult> {
    let namespace = namespace_override
        .map(String::from)
        .unwrap_or_else(|| classification.namespace.clone());

    store_or_queue(
        conn,
        text,
        classification,
        &namespace,
        "capture",
        None,
        &serde_json::json!({}),
        settings,
    )
}

/// Distil a longer body of text into zero or more durable memories and store
/// each through the same review-routing and auto-embed pipeline used by
/// [`capture`]. Returns one [`CaptureResult`] per stored or queued memory; an
/// empty vector means the text contained nothing worth remembering.
///
/// Namespace resolution for each distilled memory, in precedence order:
/// 1. `namespace_override` (an explicit `--namespace`) — always wins.
/// 2. the LLM's choice when it is `"global"` — respects a deliberate promotion
///    of a genuinely cross-project fact.
/// 3. `default_namespace` (typically the working directory's namespace) —
///    overrides the LLM's unreliable per-project guess so a session's memories
///    land in the right drawer.
/// 4. the LLM's suggested namespace — the fallback when no default is given.
///
/// `source` and `source_ref` are recorded on each memory for provenance.
#[cfg(feature = "capture")]
#[allow(clippy::too_many_arguments)]
pub fn distill_and_store(
    conn: &rusqlite::Connection,
    text: &str,
    config: &CaptureConfig,
    namespace_override: Option<&str>,
    default_namespace: Option<&str>,
    source: &str,
    source_ref: Option<&str>,
    cwd: Option<&str>,
    settings: &crate::settings::Settings,
) -> Result<Vec<CaptureResult>> {
    let memories = distill(text, config)?;

    // Record the originating working directory so namespaces can later be
    // matched to a real path (powers reliable "folder gone" cleanup).
    let metadata = match cwd {
        Some(c) => serde_json::json!({ "cwd": c }),
        None => serde_json::json!({}),
    };

    let mut results = Vec::with_capacity(memories.len());
    for (index, memory) in memories.iter().enumerate() {
        let classification = ClassificationResult {
            kind: memory.kind.clone(),
            title: memory.title.clone(),
            summary: memory.summary.clone(),
            tags: memory.tags.clone(),
            namespace: memory.namespace.clone(),
            importance: memory.importance,
            confidence: memory.confidence,
        };
        let namespace = resolve_distill_namespace(
            namespace_override,
            &classification.namespace,
            default_namespace,
        );

        // A session yields many memories, so the shared `source_ref` must be
        // made unique per memory — otherwise the UNIQUE(source, source_ref)
        // index rejects every memory after the first. The session id stays the
        // shared prefix for provenance.
        let item_ref = source_ref.map(|r| format!("{r}-{index}"));

        results.push(store_or_queue(
            conn,
            &memory.content,
            &classification,
            &namespace,
            source,
            item_ref.as_deref(),
            &metadata,
            settings,
        )?);
    }

    Ok(results)
}

/// Store a classified memory, or route it to the review queue when its
/// confidence falls below the configured threshold. Shared by the single-item
/// capture path and the multi-item distillation path so both apply identical
/// review-routing and auto-embed behaviour.
#[allow(clippy::too_many_arguments)]
fn store_or_queue(
    conn: &rusqlite::Connection,
    content: &str,
    classification: &ClassificationResult,
    namespace: &str,
    source: &str,
    source_ref: Option<&str>,
    metadata: &serde_json::Value,
    settings: &crate::settings::Settings,
) -> Result<CaptureResult> {
    // Suppress duplicate writes: if an identical, non-archived memory already
    // exists in the target namespace, return it instead of storing or queuing
    // a second copy. Runs before review-routing so a known fact never clogs the
    // inbox either.
    if let Some(existing_id) = crate::repository::find_content_duplicate(conn, namespace, content)?
    {
        let memory = crate::repository::get(conn, &existing_id)?;
        tracing::debug!(
            "capture deduplicated against existing memory {} in {}",
            existing_id,
            namespace
        );
        return Ok(CaptureResult::Stored(memory));
    }

    // If the only twin is archived, revive it rather than creating a duplicate
    // live row — a re-capture of a hidden fact should bring it back.
    if let Some(archived_id) = crate::repository::find_archived_duplicate(conn, namespace, content)?
    {
        let memory = crate::repository::unarchive(conn, &archived_id)?;
        tracing::debug!(
            "capture revived archived duplicate {} in {}",
            archived_id,
            namespace
        );
        return Ok(CaptureResult::Stored(memory));
    }

    // Check whether this capture should be routed to the review queue.
    if let Some(threshold) = settings.capture.review_threshold {
        if classification.confidence < threshold {
            let review_input = ReviewInput {
                content: content.to_string(),
                suggested_namespace: namespace.to_string(),
                suggested_kind: classification.kind.clone(),
                suggested_title: Some(classification.title.clone()),
                suggested_summary: if classification.summary.is_empty() {
                    None
                } else {
                    Some(classification.summary.clone())
                },
                suggested_tags: classification.tags.clone(),
                suggested_importance: classification.importance,
                suggested_confidence: Some(classification.confidence),
                source_route: Some(source.to_string()),
                metadata: metadata.clone(),
            };

            let review_item = crate::review::queue_for_review(conn, &review_input)?;
            return Ok(CaptureResult::Queued(review_item));
        }
    }

    let input = RememberInput {
        namespace: namespace.to_string(),
        kind: classification.kind.clone(),
        title: Some(classification.title.clone()),
        summary: if classification.summary.is_empty() {
            None
        } else {
            Some(classification.summary.clone())
        },
        content: content.to_string(),
        tags: classification.tags.clone(),
        source: Some(source.to_string()),
        source_ref: source_ref.map(String::from),
        confidence: Some(classification.confidence),
        importance: classification.importance,
        metadata: metadata.clone(),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let memory = crate::repository::remember(conn, &input, settings)?;

    // Auto-embed if enabled.
    if settings.auto_embed {
        if let Ok(backend) = crate::embeddings::create_backend(&settings.embeddings) {
            if let Err(e) = crate::embeddings::embed_and_store(conn, backend.as_ref(), &memory) {
                tracing::warn!("capture auto-embed failed: {e}");
            }
        }
    }

    Ok(CaptureResult::Stored(memory))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_classification() {
        let json = r#"{
            "kind": "decision",
            "title": "Use SQLite for storage",
            "summary": "We decided to use SQLite because it is local-first.",
            "tags": ["sqlite", "architecture"],
            "namespace": "project:clio",
            "importance": 4,
            "confidence": 0.9
        }"#;

        let result = parse_classification(json).unwrap();
        assert_eq!(result.kind, "decision");
        assert_eq!(result.title, "Use SQLite for storage");
        assert_eq!(result.tags, vec!["sqlite", "architecture"]);
        assert_eq!(result.namespace, "project:clio");
        assert_eq!(result.importance, 4);
        assert!((result.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_with_markdown_fences() {
        let json = r#"```json
{
    "kind": "note",
    "title": "Test",
    "summary": "A test note.",
    "tags": ["test"],
    "namespace": "global",
    "importance": 3,
    "confidence": 0.5
}
```"#;

        let result = parse_classification(json).unwrap();
        assert_eq!(result.kind, "note");
        assert_eq!(result.title, "Test");
    }

    #[test]
    fn parse_clamps_out_of_range_values() {
        let json = r#"{
            "kind": "note",
            "title": "Test",
            "summary": "",
            "tags": [],
            "namespace": "global",
            "importance": 10,
            "confidence": 2.5
        }"#;

        let result = parse_classification(json).unwrap();
        assert_eq!(result.importance, 5);
        assert!((result.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_unknown_kind_defaults_to_note() {
        let json = r#"{
            "kind": "banana",
            "title": "Test",
            "summary": "",
            "tags": [],
            "namespace": "global",
            "importance": 3,
            "confidence": 0.5
        }"#;

        let result = parse_classification(json).unwrap();
        assert_eq!(result.kind, "note");
    }

    #[test]
    fn parse_missing_fields_uses_defaults() {
        let json = r#"{}"#;

        let result = parse_classification(json).unwrap();
        assert_eq!(result.kind, "note");
        assert_eq!(result.title, "Untitled");
        assert_eq!(result.namespace, "global");
        assert_eq!(result.importance, 3);
    }

    #[test]
    fn distill_parses_array_of_memories() {
        let json = r#"[
            {
                "content": "Clio stores all business logic in clio-core; adapters stay thin.",
                "kind": "fact",
                "title": "Core/adapter boundary",
                "summary": "Logic lives in clio-core.",
                "tags": ["Architecture", "rust"],
                "namespace": "project:clio",
                "importance": 4,
                "confidence": 0.9
            },
            {
                "content": "Upsert is keyed on source + source_ref.",
                "kind": "constraint",
                "title": "Upsert key",
                "summary": "",
                "tags": ["upsert"],
                "namespace": "project:clio",
                "importance": 3,
                "confidence": 0.8
            }
        ]"#;

        let result = parse_distillation(json).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].kind, "fact");
        assert_eq!(result[0].tags, vec!["architecture", "rust"]);
        assert_eq!(result[1].kind, "constraint");
        assert_eq!(result[1].content, "Upsert is keyed on source + source_ref.");
    }

    #[test]
    fn distill_empty_array_yields_no_memories() {
        let result = parse_distillation("[]").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn distill_accepts_memories_wrapper_and_fences() {
        let json = r#"```json
{ "memories": [
    { "content": "A durable fact.", "kind": "note", "title": "T",
      "summary": "", "tags": [], "namespace": "global",
      "importance": 3, "confidence": 0.5 }
] }
```"#;
        let result = parse_distillation(json).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "A durable fact.");
    }

    #[test]
    fn distill_drops_items_with_empty_content() {
        let json = r#"[
            { "content": "   ", "kind": "note", "title": "blank",
              "summary": "", "tags": [], "namespace": "global",
              "importance": 1, "confidence": 0.5 },
            { "content": "Real one.", "kind": "fact", "title": "ok",
              "summary": "", "tags": [], "namespace": "global",
              "importance": 3, "confidence": 0.7 }
        ]"#;
        let result = parse_distillation(json).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "Real one.");
    }

    #[test]
    fn distill_rejects_non_array_json() {
        assert!(parse_distillation(r#"{"foo": "bar"}"#).is_err());
    }

    #[test]
    fn is_session_noise_matches_session_summaries() {
        // Real titles previously produced by the retired hook pipelines.
        for title in [
            "Exploratory Session on Branch Main",
            "Recent commits on main branch",
            "Summary of commits on branch feat/release-packaging",
            "Stuntrocketv3 Branch Commit Summary",
            "Session Summary for Stuntrocketv3 Branch",
            "Commit Summary for Stuntrocketv3",
            "Connect4 Development Session Update",
            "Connect4 Development Session",
        ] {
            assert!(is_session_noise(title), "should flag noise: {title}");
        }
    }

    #[test]
    fn is_session_noise_keeps_durable_titles() {
        // Legitimate titles, including one that mentions "session" in a durable
        // sense, must not be dropped.
        for title in [
            "Move Item Functionality in Rust",
            "Settings Panel Redesign Requirements",
            "Run Embed Backfill Command After Bulk Imports",
            "Session token expiry is 24 hours",
        ] {
            assert!(!is_session_noise(title), "should keep durable: {title}");
        }
    }

    #[test]
    fn distill_drops_session_noise_memories() {
        let json = r#"[
            { "content": "Edited 18 files and committed.", "kind": "summary",
              "title": "Stuntrocketv3 Session Summary", "summary": "",
              "tags": [], "namespace": "global", "importance": 2, "confidence": 0.6 },
            { "content": "Upsert is keyed on source + source_ref.", "kind": "constraint",
              "title": "Upsert key", "summary": "", "tags": [],
              "namespace": "project:clio", "importance": 4, "confidence": 0.9 }
        ]"#;
        let result = parse_distillation(json).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Upsert key");
    }

    #[test]
    fn resolve_namespace_explicit_override_always_wins() {
        assert_eq!(
            resolve_distill_namespace(Some("project:x"), "global", Some("project:clio")),
            "project:x"
        );
    }

    #[test]
    fn resolve_namespace_respects_global_promotion() {
        // The model promoted a cross-project fact; keep it global even though a
        // working-directory default is available.
        assert_eq!(
            resolve_distill_namespace(None, "global", Some("project:clio")),
            "global"
        );
    }

    #[test]
    fn resolve_namespace_default_overrides_llm_guess() {
        // The model guessed an unrelated project; the cwd namespace wins.
        assert_eq!(
            resolve_distill_namespace(None, "project:notes", Some("project:clio")),
            "project:clio"
        );
    }

    #[test]
    fn resolve_namespace_falls_back_to_llm_when_no_default() {
        assert_eq!(
            resolve_distill_namespace(None, "project:notes", None),
            "project:notes"
        );
    }

    #[test]
    fn parse_tags_normalised() {
        let json = r#"{
            "kind": "note",
            "title": "T",
            "summary": "",
            "tags": ["  Rust  ", "UPPER", "multi word"],
            "namespace": "global",
            "importance": 3,
            "confidence": 0.7
        }"#;

        let result = parse_classification(json).unwrap();
        assert_eq!(result.tags, vec!["rust", "upper", "multi-word"]);
    }
}
