//! Cross-tool memory migration importers.
//!
//! Provides importers for Claude and ChatGPT memory exports, converting them
//! into Clio memories. Each entry is stored with `upsert: true` and a
//! deterministic `source_ref` for idempotent re-import.

use std::io::Read;

use rusqlite::Connection;

use crate::error::{ClioError, Result};
use crate::models::RememberInput;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Options controlling the migration behaviour.
#[derive(Debug, Clone)]
pub struct MigrateOptions {
    /// Override namespace for all imported entries.
    pub namespace: Option<String>,
    /// Whether to run each entry through the capture pipeline for richer
    /// classification. Requires the `capture` feature and an enabled config.
    pub classify: bool,
    /// Whether to actually store the entries (false = dry-run).
    pub store: bool,
}

impl Default for MigrateOptions {
    fn default() -> Self {
        Self {
            namespace: None,
            classify: false,
            store: true,
        }
    }
}

/// Result of a migration run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationResult {
    pub imported: u32,
    pub skipped: u32,
    pub duplicates: u32,
    pub errors: Vec<String>,
    /// Preview of entries when dry-run is active.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub preview: Vec<MigrationPreview>,
}

/// A preview entry shown during dry-run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationPreview {
    pub source: String,
    pub source_ref: String,
    pub namespace: String,
    pub kind: String,
    pub title: Option<String>,
    pub content_preview: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Produce a deterministic hash-based source_ref from the content.
///
/// Uses FNV-1a (64-bit) for a stable, version-independent hash (unlike
/// `DefaultHasher` which may change between Rust releases).
fn hash_ref(source: &str, content: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;

    let mut hash = FNV_OFFSET;
    for byte in source.as_bytes().iter().chain(b":").chain(content.as_bytes()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", hash)
}

/// Truncate text for preview display.
fn preview_text(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn build_input(
    source: &str,
    content: &str,
    namespace: &str,
    kind: &str,
    title: Option<String>,
) -> RememberInput {
    let source_ref = hash_ref(source, content);
    RememberInput {
        namespace: namespace.to_string(),
        kind: kind.to_string(),
        title,
        summary: None,
        content: content.to_string(),
        tags: Vec::new(),
        source: Some(source.to_string()),
        source_ref: Some(source_ref),
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: true,
    }
}

fn store_entry(
    conn: &Connection,
    input: &RememberInput,
    settings: &crate::settings::Settings,
) -> Result<crate::models::Memory> {
    let memory = crate::repository::remember(conn, input, settings)?;

    // Auto-embed if enabled.
    if settings.auto_embed {
        if let Ok(backend) = crate::embeddings::create_backend(&settings.embeddings) {
            if let Err(e) = crate::embeddings::embed_and_store(conn, backend.as_ref(), &memory) {
                tracing::warn!("migrate auto-embed failed for {}: {e}", memory.id);
            }
        }
    }

    Ok(memory)
}

// ---------------------------------------------------------------------------
// Claude import
// ---------------------------------------------------------------------------

/// Import memories from a Claude memory export.
///
/// Claude exports are either:
/// - One memory per line (plain text)
/// - A JSON array of strings
///
/// Each entry is stored with `source: "claude"` and a hash-based `source_ref`.
pub fn migrate_claude<R: Read>(
    conn: &Connection,
    reader: &mut R,
    options: &MigrateOptions,
    settings: &crate::settings::Settings,
) -> Result<MigrationResult> {
    let mut raw = String::new();
    reader
        .read_to_string(&mut raw)
        .map_err(|e| ClioError::Import(format!("failed to read Claude export: {e}")))?;

    let entries = parse_claude_entries(&raw)?;
    let namespace = options
        .namespace
        .as_deref()
        .unwrap_or("tool:claude");

    let mut result = MigrationResult {
        imported: 0,
        skipped: 0,
        duplicates: 0,
        errors: Vec::new(),
        preview: Vec::new(),
    };

    for entry in &entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            result.skipped += 1;
            continue;
        }

        let input = if options.classify {
            #[cfg(feature = "capture")]
            {
                classify_and_build("claude", trimmed, namespace, &settings.capture)
            }
            #[cfg(not(feature = "capture"))]
            {
                build_input("claude", trimmed, namespace, "note", None)
            }
        } else {
            build_input("claude", trimmed, namespace, "note", None)
        };

        if !options.store {
            result.preview.push(MigrationPreview {
                source: "claude".into(),
                source_ref: input.source_ref.clone().unwrap_or_default(),
                namespace: input.namespace.clone(),
                kind: input.kind.clone(),
                title: input.title.clone(),
                content_preview: preview_text(trimmed, 120),
            });
            result.imported += 1;
            continue;
        }

        match store_entry(conn, &input, settings) {
            Ok(_) => result.imported += 1,
            Err(ClioError::Conflict(_)) => result.duplicates += 1,
            Err(e) => result.errors.push(format!("claude entry: {e}")),
        }
    }

    Ok(result)
}

/// Parse Claude export entries from raw text.
fn parse_claude_entries(raw: &str) -> Result<Vec<String>> {
    let trimmed = raw.trim();

    // Try JSON array first.
    if trimmed.starts_with('[') {
        if let Ok(entries) = serde_json::from_str::<Vec<String>>(trimmed) {
            return Ok(entries);
        }
        // Also try array of objects with a "content" field.
        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
            let texts: Vec<String> = entries
                .iter()
                .filter_map(|v| {
                    v.get("content")
                        .or_else(|| v.get("text"))
                        .or_else(|| v.get("memory"))
                        .and_then(|s| s.as_str())
                        .map(String::from)
                })
                .collect();
            if !texts.is_empty() {
                return Ok(texts);
            }
        }
    }

    // Fall back to line-delimited.
    Ok(trimmed
        .lines()
        .map(|l| l.to_string())
        .collect())
}

// ---------------------------------------------------------------------------
// ChatGPT import
// ---------------------------------------------------------------------------

/// Import memories from a ChatGPT memory export.
///
/// ChatGPT exports memories as a JSON file (or a section within the data
/// export). The expected format is a JSON array of objects with at least
/// a `content` or `memory` field.
pub fn migrate_chatgpt<R: Read>(
    conn: &Connection,
    reader: &mut R,
    options: &MigrateOptions,
    settings: &crate::settings::Settings,
) -> Result<MigrationResult> {
    let mut raw = String::new();
    reader
        .read_to_string(&mut raw)
        .map_err(|e| ClioError::Import(format!("failed to read ChatGPT export: {e}")))?;

    let entries = parse_chatgpt_entries(&raw)?;
    let namespace = options
        .namespace
        .as_deref()
        .unwrap_or("tool:chatgpt");

    let mut result = MigrationResult {
        imported: 0,
        skipped: 0,
        duplicates: 0,
        errors: Vec::new(),
        preview: Vec::new(),
    };

    for entry in &entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            result.skipped += 1;
            continue;
        }

        let input = if options.classify {
            #[cfg(feature = "capture")]
            {
                classify_and_build("chatgpt", trimmed, namespace, &settings.capture)
            }
            #[cfg(not(feature = "capture"))]
            {
                build_input("chatgpt", trimmed, namespace, "fact", None)
            }
        } else {
            build_input("chatgpt", trimmed, namespace, "fact", None)
        };

        if !options.store {
            result.preview.push(MigrationPreview {
                source: "chatgpt".into(),
                source_ref: input.source_ref.clone().unwrap_or_default(),
                namespace: input.namespace.clone(),
                kind: input.kind.clone(),
                title: input.title.clone(),
                content_preview: preview_text(trimmed, 120),
            });
            result.imported += 1;
            continue;
        }

        match store_entry(conn, &input, settings) {
            Ok(_) => result.imported += 1,
            Err(ClioError::Conflict(_)) => result.duplicates += 1,
            Err(e) => result.errors.push(format!("chatgpt entry: {e}")),
        }
    }

    Ok(result)
}

/// Parse ChatGPT export entries from raw JSON.
fn parse_chatgpt_entries(raw: &str) -> Result<Vec<String>> {
    let trimmed = raw.trim();

    // Try JSON array of objects (standard ChatGPT memories export).
    if trimmed.starts_with('[') {
        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
            let texts: Vec<String> = entries
                .iter()
                .filter_map(|v| {
                    // ChatGPT memory objects may use different field names.
                    v.get("content")
                        .or_else(|| v.get("memory"))
                        .or_else(|| v.get("text"))
                        .or_else(|| v.get("value"))
                        .and_then(|s| s.as_str())
                        .map(String::from)
                })
                .collect();
            if !texts.is_empty() {
                return Ok(texts);
            }

            // Try plain string array as fallback.
            if let Ok(strings) = serde_json::from_str::<Vec<String>>(trimmed) {
                return Ok(strings);
            }
        }
    }

    // Try a single JSON object with a "memories" or "model_spec_memories" key.
    if trimmed.starts_with('{') {
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed) {
            for key in &["memories", "model_spec_memories", "data"] {
                if let Some(arr) = obj.get(*key).and_then(|v| v.as_array()) {
                    let texts: Vec<String> = arr
                        .iter()
                        .filter_map(|v| {
                            if let Some(s) = v.as_str() {
                                Some(s.to_string())
                            } else {
                                v.get("content")
                                    .or_else(|| v.get("memory"))
                                    .or_else(|| v.get("text"))
                                    .and_then(|s| s.as_str())
                                    .map(String::from)
                            }
                        })
                        .collect();
                    if !texts.is_empty() {
                        return Ok(texts);
                    }
                }
            }
        }
    }

    // Fall back to line-delimited.
    Ok(trimmed
        .lines()
        .map(|l| l.to_string())
        .collect())
}

// ---------------------------------------------------------------------------
// Capture-based classification (optional)
// ---------------------------------------------------------------------------

#[cfg(feature = "capture")]
fn classify_and_build(
    source: &str,
    content: &str,
    default_namespace: &str,
    capture_config: &crate::settings::CaptureConfig,
) -> RememberInput {
    match crate::capture::classify(content, capture_config) {
        Ok(classification) => {
            let source_ref = hash_ref(source, content);
            RememberInput {
                namespace: classification.namespace,
                kind: classification.kind,
                title: Some(classification.title),
                summary: if classification.summary.is_empty() {
                    None
                } else {
                    Some(classification.summary)
                },
                content: content.to_string(),
                tags: classification.tags,
                source: Some(source.to_string()),
                source_ref: Some(source_ref),
                confidence: Some(classification.confidence),
                importance: classification.importance,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: true,
            }
        }
        Err(e) => {
            tracing::warn!("classification failed for {source} entry, using defaults: {e}");
            build_input(source, content, default_namespace, "note", None)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::settings::Settings;

    fn test_db() -> Connection {
        db::open_in_memory().expect("failed to open in-memory DB")
    }

    fn default_settings() -> Settings {
        Settings::default()
    }

    fn default_options() -> MigrateOptions {
        MigrateOptions {
            namespace: None,
            classify: false,
            store: true,
        }
    }

    // Claude tests

    #[test]
    fn claude_line_delimited() {
        let conn = test_db();
        let data = "First memory line\nSecond memory line\nThird memory line\n";
        let mut reader = data.as_bytes();

        let result =
            migrate_claude(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 3);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn claude_json_array() {
        let conn = test_db();
        let data = r#"["Memory about Rust", "Memory about SQLite"]"#;
        let mut reader = data.as_bytes();

        let result =
            migrate_claude(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 2);
    }

    #[test]
    fn claude_json_objects() {
        let conn = test_db();
        let data = r#"[{"content": "Fact one"}, {"content": "Fact two"}]"#;
        let mut reader = data.as_bytes();

        let result =
            migrate_claude(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 2);
    }

    #[test]
    fn claude_skips_empty_lines() {
        let conn = test_db();
        let data = "First\n\n\nSecond\n";
        let mut reader = data.as_bytes();

        let result =
            migrate_claude(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 2);
    }

    #[test]
    fn claude_namespace_override() {
        let conn = test_db();
        let data = "A memory";
        let mut reader = data.as_bytes();

        let opts = MigrateOptions {
            namespace: Some("custom:ns".into()),
            ..default_options()
        };
        let result = migrate_claude(&conn, &mut reader, &opts, &default_settings()).unwrap();
        assert_eq!(result.imported, 1);

        // Verify the namespace was applied.
        let recall =
            crate::repository::recall(&conn, &crate::models::RecallQuery::default()).unwrap();
        assert_eq!(recall.items[0].memory.namespace, "custom:ns");
    }

    #[test]
    fn claude_idempotent_reimport() {
        let conn = test_db();
        let data = "Same memory";

        // Import twice.
        let mut r1 = data.as_bytes();
        let res1 =
            migrate_claude(&conn, &mut r1, &default_options(), &default_settings()).unwrap();
        assert_eq!(res1.imported, 1);

        let mut r2 = data.as_bytes();
        let res2 =
            migrate_claude(&conn, &mut r2, &default_options(), &default_settings()).unwrap();
        // Second import should still succeed (upsert), not create a duplicate.
        assert_eq!(res2.imported, 1);

        // Only one memory in DB.
        let recall =
            crate::repository::recall(&conn, &crate::models::RecallQuery::default()).unwrap();
        assert_eq!(recall.total, 1);
    }

    #[test]
    fn claude_dry_run() {
        let conn = test_db();
        let data = "Preview only";
        let mut reader = data.as_bytes();

        let opts = MigrateOptions {
            store: false,
            ..default_options()
        };
        let result = migrate_claude(&conn, &mut reader, &opts, &default_settings()).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.preview.len(), 1);
        assert_eq!(result.preview[0].source, "claude");

        // Nothing in DB.
        let recall =
            crate::repository::recall(&conn, &crate::models::RecallQuery::default()).unwrap();
        assert_eq!(recall.total, 0);
    }

    // ChatGPT tests

    #[test]
    fn chatgpt_json_objects() {
        let conn = test_db();
        let data = r#"[{"content": "User prefers Rust"}, {"content": "User lives in London"}]"#;
        let mut reader = data.as_bytes();

        let result =
            migrate_chatgpt(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn chatgpt_nested_memories_key() {
        let conn = test_db();
        let data = r#"{"memories": [{"content": "Fact A"}, {"content": "Fact B"}]}"#;
        let mut reader = data.as_bytes();

        let result =
            migrate_chatgpt(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 2);
    }

    #[test]
    fn chatgpt_string_array() {
        let conn = test_db();
        let data = r#"["Memory one", "Memory two"]"#;
        let mut reader = data.as_bytes();

        let result =
            migrate_chatgpt(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 2);
    }

    #[test]
    fn chatgpt_default_namespace() {
        let conn = test_db();
        let data = r#"[{"content": "A fact"}]"#;
        let mut reader = data.as_bytes();

        let result =
            migrate_chatgpt(&conn, &mut reader, &default_options(), &default_settings()).unwrap();
        assert_eq!(result.imported, 1);

        let recall =
            crate::repository::recall(&conn, &crate::models::RecallQuery::default()).unwrap();
        assert_eq!(recall.items[0].memory.namespace, "tool:chatgpt");
        assert_eq!(recall.items[0].memory.source.as_deref(), Some("chatgpt"));
    }

    #[test]
    fn chatgpt_dry_run() {
        let conn = test_db();
        let data = r#"[{"content": "Preview fact"}]"#;
        let mut reader = data.as_bytes();

        let opts = MigrateOptions {
            store: false,
            ..default_options()
        };
        let result = migrate_chatgpt(&conn, &mut reader, &opts, &default_settings()).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.preview.len(), 1);

        let recall =
            crate::repository::recall(&conn, &crate::models::RecallQuery::default()).unwrap();
        assert_eq!(recall.total, 0);
    }

    // Hashing tests

    #[test]
    fn hash_ref_deterministic() {
        let a = hash_ref("claude", "some content");
        let b = hash_ref("claude", "some content");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_ref_differs_by_source() {
        let a = hash_ref("claude", "same content");
        let b = hash_ref("chatgpt", "same content");
        assert_ne!(a, b);
    }
}
