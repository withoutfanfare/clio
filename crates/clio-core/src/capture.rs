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
- "importance": integer 1 to 5 (1=trivial, 5=critical)
- "confidence": float 0.0 to 1.0 — how certain you are about this classification

Rules:
- Tags must be lowercase, no spaces, use hyphens if needed.
- The namespace slug should be short and descriptive.
- If the text is ambiguous, prefer "note" as kind and lower confidence.
- Output ONLY valid JSON, no markdown fences, no extra text."#;

// ---------------------------------------------------------------------------
// Classify
// ---------------------------------------------------------------------------

/// Call an OpenAI-compatible chat completions endpoint to classify raw text.
///
/// Uses a mini tokio runtime for the HTTP call (same pattern as the OpenAI
/// embedding backend) so this function is safe to call from synchronous code.
#[cfg(feature = "capture")]
pub fn classify(text: &str, config: &CaptureConfig) -> Result<ClassificationResult> {
    if !config.enabled {
        return Err(ClioError::Config("capture pipeline is not enabled".into()));
    }

    let api_key = match &config.api_key {
        Some(key) if !key.is_empty() => key.clone(),
        _ => std::env::var("OPENAI_API_KEY").map_err(|_| {
            ClioError::Config(
                "capture API key required: set OPENAI_API_KEY or configure capture.api_key in settings"
                    .into(),
            )
        })?,
    };

    get_or_create_runtime().block_on(classify_async(text, &api_key, config))
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
async fn classify_async(
    text: &str,
    api_key: &str,
    config: &CaptureConfig,
) -> Result<ClassificationResult> {
    let base_url = config.base_url.trim_end_matches('/');
    let url = format!("{base_url}/chat/completions");

    let body = serde_json::json!({
        "model": config.model,
        "temperature": 0.1,
        "messages": [
            { "role": "system", "content": CLASSIFICATION_SYSTEM_PROMPT },
            { "role": "user", "content": text }
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
        return Err(ClioError::Storage(format!(
            "capture API returned {status}"
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| ClioError::Storage(format!("capture API response parse error: {e}")))?;

    let content_str = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| {
            ClioError::Storage("capture API: missing choices[0].message.content".into())
        })?;

    parse_classification(content_str)
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

    let kind = v["kind"]
        .as_str()
        .unwrap_or("note")
        .to_lowercase();
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

    let namespace = v["namespace"]
        .as_str()
        .unwrap_or("global")
        .to_string();

    let importance = v["importance"]
        .as_i64()
        .unwrap_or(3)
        .clamp(1, 5) as i32;

    let confidence = v["confidence"]
        .as_f64()
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);

    Ok(ClassificationResult {
        kind,
        title,
        summary,
        tags,
        namespace,
        importance,
        confidence,
    })
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

    // Check whether this capture should be routed to the review queue.
    if let Some(threshold) = settings.capture.review_threshold {
        if classification.confidence < threshold {
            let review_input = ReviewInput {
                content: text.to_string(),
                suggested_namespace: namespace,
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
                source_route: Some("capture".into()),
                metadata: serde_json::json!({}),
            };

            let review_item = crate::review::queue_for_review(conn, &review_input)?;
            return Ok(CaptureResult::Queued(review_item));
        }
    }

    let input = RememberInput {
        namespace,
        kind: classification.kind.clone(),
        title: Some(classification.title.clone()),
        summary: if classification.summary.is_empty() {
            None
        } else {
            Some(classification.summary.clone())
        },
        content: text.to_string(),
        tags: classification.tags.clone(),
        source: Some("capture".into()),
        source_ref: None,
        confidence: Some(classification.confidence),
        importance: classification.importance,
        metadata: serde_json::json!({}),
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
