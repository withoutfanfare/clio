//! Clio MCP Server
//!
//! A Model Context Protocol server exposing the Clio shared memory system.
//! Communicates over stdin/stdout using JSON-RPC. All logging goes to stderr.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    Implementation, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
    RawResource, RawResourceTemplate, ReadResourceRequestParams, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate, ServerCapabilities, ServerInfo,
};
use rmcp::{ErrorData, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use clio_core::error::ClioError;
use clio_core::models::{
    LinkInput, Memory, RecallItem, RecallQuery, RecallResult, RememberInput, SortOrder,
};

// ---------------------------------------------------------------------------
// Parameter types for MCP tools
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct RememberParams {
    /// Namespace. Auto-detected if omitted.
    #[serde(default)]
    namespace: Option<String>,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,

    /// Memory kind: note, decision, snippet.
    #[serde(default = "default_kind")]
    kind: String,

    /// Short title.
    #[serde(default)]
    title: Option<String>,

    /// Summary.
    #[serde(default)]
    summary: Option<String>,

    /// Content (required).
    content: String,

    /// Tags.
    #[serde(default)]
    tags: Vec<String>,

    /// Source system.
    #[serde(default)]
    source: Option<String>,

    /// Source reference ID.
    #[serde(default)]
    source_ref: Option<String>,

    /// Confidence 0.0–1.0.
    #[serde(default)]
    confidence: Option<f64>,

    /// Importance 1–5.
    #[serde(default = "default_importance")]
    importance: i32,

    /// Metadata JSON.
    #[serde(default = "default_metadata")]
    metadata: serde_json::Value,

    /// Valid-from ISO-8601.
    #[serde(default)]
    valid_from: Option<String>,

    /// Valid-until ISO-8601.
    #[serde(default)]
    valid_until: Option<String>,

    /// Upsert by source+source_ref.
    #[serde(default)]
    upsert: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecallParams {
    /// Search query. Omit for recent.
    #[serde(default)]
    query: Option<String>,

    /// Namespace filter.
    #[serde(default)]
    namespace: Option<String>,

    /// Search across all namespaces (ignores namespace and cwd).
    #[serde(default)]
    global: bool,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,

    /// Kind filter.
    #[serde(default)]
    kind: Option<String>,

    /// Tag filter.
    #[serde(default)]
    tags: Vec<String>,

    /// Require all tags to match.
    #[serde(default = "default_true")]
    match_all_tags: bool,

    /// Minimum importance (1–5).
    #[serde(default)]
    importance_min: Option<i32>,

    /// Maximum importance (1–5).
    #[serde(default)]
    importance_max: Option<i32>,

    /// Sort order: updated_desc, updated_asc, importance_desc, importance_asc, created_desc, created_asc.
    #[serde(default)]
    sort_by: Option<String>,

    /// Include archived.
    #[serde(default)]
    include_archived: bool,

    /// Max results.
    #[serde(default = "default_limit")]
    limit: u32,

    /// Pagination offset.
    #[serde(default)]
    offset: u32,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetParams {
    /// Memory ID.
    memory_id: String,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecentParams {
    /// Namespace filter.
    #[serde(default)]
    namespace: Option<String>,

    /// Kind filter.
    #[serde(default)]
    kind: Option<String>,

    /// Tag filter.
    #[serde(default)]
    tags: Vec<String>,

    /// Require all tags to match.
    #[serde(default = "default_true")]
    match_all_tags: bool,

    /// Minimum importance (1–5).
    #[serde(default)]
    importance_min: Option<i32>,

    /// Maximum importance (1–5).
    #[serde(default)]
    importance_max: Option<i32>,

    /// Sort order: updated_desc, updated_asc, importance_desc, importance_asc, created_desc, created_asc.
    #[serde(default)]
    sort_by: Option<String>,

    /// Include archived.
    #[serde(default)]
    include_archived: bool,

    /// Max results.
    #[serde(default = "default_limit")]
    limit: u32,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LinkParams {
    /// Source memory ID.
    from_memory_id: String,

    /// Target memory ID.
    to_memory_id: String,

    /// Relationship type.
    #[serde(default = "default_relationship")]
    relationship: String,

    /// Link metadata.
    #[serde(default = "default_metadata")]
    metadata: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ArchiveParams {
    /// Memory ID.
    memory_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UnarchiveParams {
    /// Memory ID.
    memory_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteParams {
    /// Memory ID.
    memory_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MoveNamespaceParams {
    /// Memory ID to move.
    memory_id: String,
    /// Target namespace (e.g. "project:my-app", "global").
    namespace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetLinksParams {
    /// Memory ID.
    memory_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CaptureParams {
    /// Text to classify.
    text: String,

    /// Namespace override.
    #[serde(default)]
    namespace: Option<String>,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Search query.
    query: String,

    /// Namespace filter.
    #[serde(default)]
    namespace: Option<String>,

    /// Search across all namespaces (ignores namespace and cwd).
    #[serde(default)]
    global: bool,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,

    /// Include archived.
    #[serde(default)]
    include_archived: bool,

    /// Max results.
    #[serde(default = "default_limit")]
    limit: u32,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatsParams {
    /// Namespace filter.
    #[serde(default)]
    namespace: Option<String>,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ActivityParams {
    /// Namespace filter.
    #[serde(default)]
    namespace: Option<String>,

    /// Max entries.
    #[serde(default = "default_activity_limit")]
    limit: u32,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SuggestLinksParams {
    /// Memory ID.
    memory_id: String,

    /// Min similarity 0.0–1.0.
    #[serde(default = "default_threshold")]
    threshold: f64,

    /// Max suggestions.
    #[serde(default = "default_suggest_limit")]
    limit: u32,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ContextParams {
    /// Namespace scope.
    #[serde(default)]
    namespace: Option<String>,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,

    /// Preset: project-brief, person-brief, decision-history, active-constraints, recent-activity, custom.
    #[serde(default = "default_preset")]
    preset: String,

    /// FTS query for custom preset.
    #[serde(default)]
    query: Option<String>,

    /// Max memories to include.
    #[serde(default = "default_max_items")]
    max_items: u32,

    /// Include linked memories.
    #[serde(default)]
    include_links: bool,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InboxListParams {
    /// Max pending items to return.
    #[serde(default = "default_inbox_limit")]
    limit: u32,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InboxApproveParams {
    /// Review item ID.
    #[serde(alias = "id")]
    review_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InboxRejectParams {
    /// Review item ID.
    #[serde(alias = "id")]
    review_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InboxEditParams {
    /// Review item ID.
    #[serde(alias = "id")]
    review_id: String,

    /// Override suggested namespace.
    #[serde(default)]
    namespace: Option<String>,

    /// Override suggested kind.
    #[serde(default)]
    kind: Option<String>,

    /// Override suggested title.
    #[serde(default)]
    title: Option<String>,

    /// Override suggested summary.
    #[serde(default)]
    summary: Option<String>,

    /// Override suggested tags.
    #[serde(default)]
    tags: Option<Vec<String>>,

    /// Override suggested importance (1-5).
    #[serde(default)]
    importance: Option<i32>,

    /// Override suggested confidence (0.0-1.0).
    #[serde(default)]
    confidence: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CacheClearParams {}

// ---------------------------------------------------------------------------
// Serde default helpers
// ---------------------------------------------------------------------------

/// Maximum limit for any query to prevent runaway scans.
const MAX_LIMIT: u32 = 500;

fn default_kind() -> String {
    "note".into()
}

fn default_importance() -> i32 {
    3
}

fn default_metadata() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn default_true() -> bool {
    true
}

fn default_limit() -> u32 {
    10
}

fn default_response_format() -> String {
    "markdown".into()
}

fn default_activity_limit() -> u32 {
    20
}

fn default_suggest_limit() -> u32 {
    5
}

fn default_threshold() -> f64 {
    0.7
}

fn default_relationship() -> String {
    "relates_to".into()
}

fn default_preset() -> String {
    "project-brief".into()
}

fn default_max_items() -> u32 {
    20
}

fn default_inbox_limit() -> u32 {
    20
}

// ---------------------------------------------------------------------------
// Input validation helpers (MCP boundary)
// ---------------------------------------------------------------------------

/// Validate that a memory ID is not empty or whitespace-only.
fn validate_memory_id(id: &str, field_name: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err(format!("{field_name} must not be empty."));
    }
    Ok(())
}

/// Validate a response_format value.
fn validate_response_format(format: &str) -> Result<(), String> {
    match format {
        "markdown" | "json" => Ok(()),
        other => Err(format!(
            "Invalid response_format '{other}'. Must be 'markdown' or 'json'."
        )),
    }
}

/// Cap a limit value to prevent runaway queries.
fn cap_limit(limit: u32) -> u32 {
    limit.min(MAX_LIMIT)
}

/// Validate threshold is within 0.0–1.0 range.
fn validate_threshold(value: f64, field_name: &str) -> Result<(), String> {
    if !(0.0..=1.0).contains(&value) {
        return Err(format!("{field_name} must be between 0.0 and 1.0."));
    }
    Ok(())
}

/// Validate importance is within 1–5 range.
fn validate_importance(value: i32) -> Result<(), String> {
    if !(1..=5).contains(&value) {
        return Err("importance must be between 1 and 5.".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cached settings (reloaded when stale)
// ---------------------------------------------------------------------------

struct CachedSettings {
    settings: clio_core::settings::Settings,
    loaded_at: std::time::Instant,
}

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ClioServer {
    db_path: Arc<PathBuf>,
    conn: Arc<Mutex<rusqlite::Connection>>,
    cache: Arc<clio_core::cache::ClioCache>,
    settings_cache: Arc<Mutex<CachedSettings>>,
    embedding_backend: Arc<Option<Box<dyn clio_core::embeddings::EmbeddingBackend>>>,
    tool_router: ToolRouter<Self>,
}

impl ClioServer {
    /// Load settings, returning cached value if fresh (< 30s).
    fn settings(&self) -> Result<clio_core::settings::Settings, String> {
        let mut cache = self
            .settings_cache
            .lock()
            .map_err(|e| format!("settings cache lock error: {e}"))?;
        if cache.loaded_at.elapsed().as_secs() < 30 {
            return Ok(cache.settings.clone());
        }
        match clio_core::settings::load(&self.db_path) {
            Ok(s) => {
                cache.settings = s.clone();
                cache.loaded_at = std::time::Instant::now();
                Ok(s)
            }
            Err(e) => {
                tracing::warn!("failed to reload settings, using cached: {e}");
                Ok(cache.settings.clone())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

/// Truncate a string to at most `max` characters, safe for multi-byte UTF-8.
fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Convert a `ClioError` into an actionable, user-facing error message.
fn format_clio_error(err: &ClioError) -> String {
    match err {
        ClioError::Validation(msg) => format!("Validation error: {msg}"),
        ClioError::NotFound(id) => format!("Memory not found: {id}"),
        ClioError::Conflict(msg) => format!("Conflict: {msg}"),
        ClioError::Storage(msg) => format!("Storage error: {msg}"),
        ClioError::Config(msg) => format!("Configuration error: {msg}"),
        ClioError::Migration(msg) => format!("Migration error: {msg}"),
        ClioError::Export(msg) => format!("Export error: {msg}"),
        ClioError::Import(msg) => format!("Import error: {msg}"),
    }
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

/// Render a single memory as a Markdown summary card (used in list views).
fn memory_card_md(item: &RecallItem) -> String {
    let m = &item.memory;
    let heading = m.title.as_deref().unwrap_or(&m.id);
    let tags_str = if m.tags.is_empty() {
        "none".to_string()
    } else {
        m.tags.join(", ")
    };

    let mut md = format!("### {heading}\n");
    md.push_str(&format!("- **ID:** {}\n", m.id));
    md.push_str(&format!("- **Namespace:** {}\n", m.namespace));
    md.push_str(&format!("- **Kind:** {}\n", m.kind));
    md.push_str(&format!("- **Tags:** {tags_str}\n"));
    md.push_str(&format!("- **Updated:** {}\n", m.updated_at));

    if let Some(rank) = item.rank {
        md.push_str(&format!("- **Rank:** {rank:.3}\n"));
    }

    md.push('\n');

    let preview = m.summary.as_deref().unwrap_or(&m.content);
    let truncated = if preview.len() > 200 {
        format!("{}...", truncate_chars(preview, 200))
    } else {
        preview.to_string()
    };
    md.push_str(&format!("> {}\n", truncated.replace('\n', "\n> ")));

    md
}

/// Render a recall result as Markdown.
fn recall_result_md(result: &RecallResult) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "**{} of {} memories** (offset {}, limit {})\n\n",
        result.count, result.total, result.offset, result.limit
    ));

    for item in &result.items {
        md.push_str(&memory_card_md(item));
        md.push('\n');
    }

    md
}

/// Render a single memory as a detailed Markdown page.
fn memory_detail_md(m: &Memory) -> String {
    let heading = m.title.as_deref().unwrap_or(&m.id);
    let tags_str = if m.tags.is_empty() {
        "none".to_string()
    } else {
        m.tags.join(", ")
    };
    let confidence_str = m
        .confidence
        .map(|c| format!("{c:.2}"))
        .unwrap_or_else(|| "n/a".into());
    let source_str = m.source.as_deref().unwrap_or("n/a");

    let mut md = format!("# {heading}\n\n");
    md.push_str("| Field | Value |\n");
    md.push_str("|---|---|\n");
    md.push_str(&format!("| ID | {} |\n", m.id));
    md.push_str(&format!("| Namespace | {} |\n", m.namespace));
    md.push_str(&format!("| Kind | {} |\n", m.kind));
    md.push_str(&format!("| Tags | {tags_str} |\n"));
    md.push_str(&format!("| Importance | {} |\n", m.importance));
    md.push_str(&format!("| Confidence | {confidence_str} |\n"));
    md.push_str(&format!("| Source | {source_str} |\n"));
    md.push_str(&format!("| Created | {} |\n", m.created_at));
    md.push_str(&format!("| Updated | {} |\n", m.updated_at));

    if let Some(ref archived) = m.archived_at {
        md.push_str(&format!("| Archived | {archived} |\n"));
    }
    if let Some(ref vf) = m.valid_from {
        md.push_str(&format!("| Valid from | {vf} |\n"));
    }
    if let Some(ref vu) = m.valid_until {
        md.push_str(&format!("| Valid until | {vu} |\n"));
    }

    md.push_str("\n## Content\n\n");
    md.push_str(&m.content);
    md.push_str("\n\n## Metadata\n\n```json\n");
    md.push_str(&serde_json::to_string_pretty(&m.metadata).unwrap_or_default());
    md.push_str("\n```\n");

    md
}

/// Format a response as either Markdown or JSON, depending on `response_format`.
fn format_recall_response(result: &RecallResult, format: &str) -> String {
    if format == "json" {
        serde_json::to_string_pretty(result).unwrap_or_else(|e| format!("Serialisation error: {e}"))
    } else {
        recall_result_md(result)
    }
}

/// Format a single memory as either Markdown or JSON.
fn format_memory_response(memory: &Memory, format: &str) -> String {
    if format == "json" {
        serde_json::to_string_pretty(memory).unwrap_or_else(|e| format!("Serialisation error: {e}"))
    } else {
        memory_detail_md(memory)
    }
}

/// Format memory stats as Markdown.
fn format_stats_md(stats: &clio_core::models::MemoryStats) -> String {
    let mut md = String::from(
        "# Memory Statistics

",
    );
    md.push_str("| Metric | Value |\n");
    md.push_str("|---|---|\n");
    md.push_str(&format!(
        "| Total memories | {} |
",
        stats.total_memories
    ));
    md.push_str(&format!(
        "| Active | {} |
",
        stats.active_memories
    ));
    md.push_str(&format!(
        "| Archived | {} |
",
        stats.archived_memories
    ));
    md.push_str(&format!(
        "| Total embeddings | {} |
",
        stats.total_embeddings
    ));
    md.push_str(&format!(
        "| Embedding coverage | {:.1}% |
",
        stats.embedding_coverage
    ));
    md.push_str(&format!(
        "| Total links | {} |
",
        stats.total_links
    ));
    md.push_str(&format!(
        "| Link density | {:.2} links/memory |
",
        stats.link_density
    ));

    if !stats.by_namespace.is_empty() {
        md.push_str(
            "
## By Namespace

",
        );
        md.push_str(
            "| Namespace | Count |
|---|---|
",
        );
        for (ns, count) in &stats.by_namespace {
            md.push_str(&format!(
                "| {ns} | {count} |
"
            ));
        }
    }

    if !stats.by_kind.is_empty() {
        md.push_str(
            "
## By Kind

",
        );
        md.push_str(
            "| Kind | Count |
|---|---|
",
        );
        for (kind, count) in &stats.by_kind {
            md.push_str(&format!(
                "| {kind} | {count} |
"
            ));
        }
    }

    if !stats.top_tags.is_empty() {
        md.push_str(
            "
## Top Tags

",
        );
        md.push_str(
            "| Tag | Count |
|---|---|
",
        );
        for (tag, count) in &stats.top_tags {
            md.push_str(&format!(
                "| {tag} | {count} |
"
            ));
        }
    }

    md
}

/// Format activity entries as Markdown.
fn format_activity_md(entries: &[clio_core::models::RecentEntry]) -> String {
    if entries.is_empty() {
        return "No recent activity.".to_string();
    }

    let mut md = String::from(
        "# Recent Activity

",
    );
    md.push_str(
        "| Action | ID | Namespace | Kind | Title | Timestamp |
",
    );
    md.push_str(
        "|---|---|---|---|---|---|
",
    );

    for entry in entries {
        let title = entry.title.as_deref().unwrap_or("(untitled)");
        let id_short = &entry.memory_id[..std::cmp::min(entry.memory_id.len(), 8)];
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |
",
            entry.action, id_short, entry.namespace, entry.kind, title, entry.timestamp
        ));
    }

    md
}

/// Format link suggestions as Markdown.
fn format_suggestions_md(suggestions: &[(Memory, f64)]) -> String {
    if suggestions.is_empty() {
        return "No link suggestions found above the threshold.".to_string();
    }

    let mut md = String::from(
        "# Suggested Links

",
    );
    md.push_str(
        "| ID | Namespace | Kind | Title | Similarity |
",
    );
    md.push_str(
        "|---|---|---|---|---|
",
    );

    for (mem, similarity) in suggestions {
        let title = mem.title.as_deref().unwrap_or("(untitled)");
        let id_short = &mem.id[..std::cmp::min(mem.id.len(), 8)];
        md.push_str(&format!(
            "| {} | {} | {} | {} | {:.4} |
",
            id_short, mem.namespace, mem.kind, title, similarity
        ));
    }

    md
}

/// Format a context brief as Markdown.
fn format_brief_md(brief: &clio_core::assembly::ContextBrief) -> String {
    let mut md = format!("# Context Brief: {}\n\n", brief.preset);
    md.push_str(&format!(
        "**Namespace:** {} | **Generated:** {} | **Memories:** {}\n\n",
        brief.namespace, brief.generated_at, brief.total_memories_used
    ));

    for section in &brief.sections {
        md.push_str(&format!("## {}\n\n", section.heading));

        if section.items.is_empty() {
            md.push_str("_No memories._\n\n");
            continue;
        }

        for mem in &section.items {
            let title = mem.title.as_deref().unwrap_or("(untitled)");
            let preview = mem.summary.as_deref().unwrap_or(&mem.content);
            let truncated = if preview.len() > 200 {
                format!("{}...", truncate_chars(preview, 200))
            } else {
                preview.to_string()
            };
            md.push_str(&format!("### {title}\n"));
            md.push_str(&format!("- **ID:** {}\n", mem.id));
            md.push_str(&format!("- **Kind:** {}\n", mem.kind));
            md.push_str(&format!("- **Updated:** {}\n\n", mem.updated_at));
            md.push_str(&format!("> {}\n\n", truncated.replace('\n', "\n> ")));
        }
    }

    md
}

/// Render a review item as Markdown.
fn review_item_md(item: &clio_core::review::ReviewItem) -> String {
    let heading = item.suggested_title.as_deref().unwrap_or(&item.id);
    let tags_str = if item.suggested_tags.is_empty() {
        "none".to_string()
    } else {
        item.suggested_tags.join(", ")
    };
    let confidence_str = item
        .suggested_confidence
        .map(|c| format!("{c:.2}"))
        .unwrap_or_else(|| "n/a".into());

    let mut md = format!("### [review] {heading}\n");
    md.push_str(&format!("- **ID:** {}\n", item.id));
    md.push_str(&format!("- **Status:** {}\n", item.status));
    md.push_str(&format!("- **Namespace:** {}\n", item.suggested_namespace));
    md.push_str(&format!("- **Kind:** {}\n", item.suggested_kind));
    md.push_str(&format!("- **Tags:** {tags_str}\n"));
    md.push_str(&format!(
        "- **Importance:** {}\n",
        item.suggested_importance
    ));
    md.push_str(&format!("- **Confidence:** {confidence_str}\n"));
    md.push_str(&format!("- **Created:** {}\n", item.created_at));
    md.push('\n');

    let preview = item.suggested_summary.as_deref().unwrap_or(&item.content);
    let truncated = if preview.len() > 200 {
        format!("{}...", truncate_chars(preview, 200))
    } else {
        preview.to_string()
    };
    md.push_str(&format!("> {}\n", truncated.replace('\n', "\n> ")));

    md
}

/// Render a list of review items as Markdown.
fn review_list_md(items: &[clio_core::review::ReviewItem]) -> String {
    if items.is_empty() {
        return "No pending review items.".to_string();
    }

    let mut md = format!("**{} pending item(s)**\n\n", items.len());
    for item in items {
        md.push_str(&review_item_md(item));
        md.push('\n');
    }
    md
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[tool_router]
impl ClioServer {
    fn new(
        db_path: PathBuf,
        conn: rusqlite::Connection,
        settings: clio_core::settings::Settings,
        backend: Option<Box<dyn clio_core::embeddings::EmbeddingBackend>>,
    ) -> Self {
        Self {
            db_path: Arc::new(db_path),
            conn: Arc::new(Mutex::new(conn)),
            cache: Arc::new(clio_core::cache::ClioCache::with_defaults()),
            settings_cache: Arc::new(Mutex::new(CachedSettings {
                settings,
                loaded_at: std::time::Instant::now(),
            })),
            embedding_backend: Arc::new(backend),
            tool_router: Self::tool_router(),
        }
    }

    /// Store a memory record.
    #[tool(description = "Store a memory. Upserts if upsert=true with source+source_ref.")]
    async fn memory_remember(
        &self,
        Parameters(params): Parameters<RememberParams>,
    ) -> Result<String, String> {
        // MCP boundary validation (defence-in-depth on top of core validation).
        if params.content.trim().is_empty() {
            return Err("content must not be empty.".into());
        }
        if params.content.len() > 1_048_576 {
            return Err("content must not exceed 1 MiB.".into());
        }
        if params.tags.len() > 50 {
            return Err("at most 50 tags are allowed.".into());
        }
        if let Ok(serialised) = serde_json::to_string(&params.metadata) {
            if serialised.len() > 65_536 {
                return Err("metadata must not exceed 64 KiB when serialised.".into());
            }
        }
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        let settings = self.settings()?;
        let backend = self.embedding_backend.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let namespace = clio_core::context::resolve_namespace(
                params.namespace.as_deref(),
                cwd_path,
                settings.context.auto_detect,
            );
            let input = RememberInput {
                namespace,
                kind: params.kind,
                title: params.title,
                summary: params.summary,
                content: params.content,
                tags: params.tags,
                source: params.source,
                source_ref: params.source_ref,
                confidence: params.confidence,
                importance: params.importance,
                metadata: params.metadata,
                valid_from: params.valid_from,
                valid_until: params.valid_until,
                upsert: params.upsert,
            };
            let memory = cache
                .remember(&conn, &input, &settings)
                .map_err(|e| format_clio_error(&e))?;

            // Auto-embed if enabled.
            if settings.auto_embed {
                if let Some(ref be) = *backend {
                    if let Err(e) =
                        clio_core::embeddings::embed_and_store(&conn, be.as_ref(), &memory)
                    {
                        tracing::warn!("auto-embed failed: {e}");
                    }
                }
            }

            serde_json::to_string_pretty(&memory).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Full-text search and filter memories.
    #[tool(description = "Full-text search and filter memories.")]
    async fn memory_recall(
        &self,
        Parameters(params): Parameters<RecallParams>,
    ) -> Result<String, String> {
        validate_response_format(&params.response_format)?;
        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let detected_ns = clio_core::context::resolve_namespace(
                params.namespace.as_deref(),
                cwd_path,
                settings.context.auto_detect,
            );

            let scoring = Some(settings.scoring.clone());
            let sort_by = params.sort_by.as_deref().and_then(SortOrder::from_str_opt);
            let query = RecallQuery {
                query: params.query,
                namespace: None,
                kind: params.kind,
                tags: params.tags,
                match_all_tags: params.match_all_tags,
                include_archived: params.include_archived,
                include_links: false,
                importance_min: params.importance_min,
                importance_max: params.importance_max,
                sort_by,
                limit,
                offset: params.offset,
                scoring,
            };

            // --global: search all namespaces without scoping.
            // --namespace: search only that exact namespace.
            // Neither: use scoped-then-global recall (project namespace + global fallback).
            let result = if params.global {
                cache
                    .recall(&conn, &query)
                    .map_err(|e| format_clio_error(&e))?
            } else if params.namespace.is_some() {
                let scoped_query = RecallQuery {
                    namespace: Some(detected_ns),
                    ..query
                };
                cache
                    .recall(&conn, &scoped_query)
                    .map_err(|e| format_clio_error(&e))?
            } else {
                cache
                    .recall_scoped(&conn, &query, &detected_ns)
                    .map_err(|e| format_clio_error(&e))?
            };

            Ok(format_recall_response(&result, &params.response_format))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Get a memory by ID.
    #[tool(description = "Get a memory by ID.")]
    async fn memory_get(
        &self,
        Parameters(params): Parameters<GetParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        validate_response_format(&params.response_format)?;
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let memory = cache
                .get(&conn, &params.memory_id)
                .map_err(|e| format_clio_error(&e))?;
            Ok(format_memory_response(&memory, &params.response_format))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// List recent memories.
    #[tool(description = "List recent memories.")]
    async fn memory_recent(
        &self,
        Parameters(params): Parameters<RecentParams>,
    ) -> Result<String, String> {
        validate_response_format(&params.response_format)?;
        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let sort_by = params.sort_by.as_deref().and_then(SortOrder::from_str_opt);
            let query = RecallQuery {
                namespace: params.namespace,
                kind: params.kind,
                tags: params.tags,
                match_all_tags: params.match_all_tags,
                importance_min: params.importance_min,
                importance_max: params.importance_max,
                sort_by,
                include_archived: params.include_archived,
                limit,
                scoring: Some(settings.scoring.clone()),
                ..Default::default()
            };
            let result = cache
                .recall(&conn, &query)
                .map_err(|e| format_clio_error(&e))?;
            Ok(format_recall_response(&result, &params.response_format))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Link two memories.
    #[tool(description = "Link two memories. Idempotent on (from, to, rel).")]
    async fn memory_link(
        &self,
        Parameters(params): Parameters<LinkParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.from_memory_id, "from_memory_id")?;
        validate_memory_id(&params.to_memory_id, "to_memory_id")?;
        if params.from_memory_id == params.to_memory_id {
            return Err("Cannot link a memory to itself.".into());
        }
        if params.relationship.trim().is_empty() {
            return Err("relationship must not be empty.".into());
        }
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let input = LinkInput {
                from_memory_id: params.from_memory_id,
                to_memory_id: params.to_memory_id,
                relationship: params.relationship,
                metadata: params.metadata,
            };
            let link = cache
                .link(&conn, &input)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&link).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Archive a memory.
    #[tool(description = "Archive a memory by ID.")]
    async fn memory_archive(
        &self,
        Parameters(params): Parameters<ArchiveParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let memory = cache
                .archive(&conn, &params.memory_id)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&memory).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Unarchive a memory.
    #[tool(description = "Unarchive a memory by ID.")]
    async fn memory_unarchive(
        &self,
        Parameters(params): Parameters<UnarchiveParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let memory = cache
                .unarchive(&conn, &params.memory_id)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&memory).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Permanently delete a memory.
    #[tool(description = "Permanently delete a memory by ID. Cascades to links and embeddings.")]
    async fn memory_delete(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let memory = cache
                .delete(&conn, &params.memory_id)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&memory).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Move a memory to a different namespace.
    #[tool(description = "Move a memory to a different namespace.")]
    async fn memory_move(
        &self,
        Parameters(params): Parameters<MoveNamespaceParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        if params.namespace.trim().is_empty() {
            return Err("namespace must not be empty.".into());
        }
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let memory = cache
                .move_namespace(&conn, &params.memory_id, &params.namespace)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&memory).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// List namespaces.
    #[tool(description = "List all namespaces.")]
    async fn memory_namespaces(&self) -> Result<String, String> {
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let namespaces = cache
                .list_namespaces(&conn)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&namespaces)
                .map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Get links from a memory.
    #[tool(description = "Get links from a memory.")]
    async fn memory_get_links(
        &self,
        Parameters(params): Parameters<GetLinksParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let links = cache
                .get_links(&conn, &params.memory_id)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&links).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// LLM-classify text into a memory (or queue for review if below threshold).
    #[tool(
        description = "LLM-classify unstructured text into a structured memory. May queue for review if confidence is low."
    )]
    async fn memory_capture(
        &self,
        Parameters(params): Parameters<CaptureParams>,
    ) -> Result<String, String> {
        if params.text.trim().is_empty() {
            return Err("text must not be empty.".into());
        }
        let conn = self.conn.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            // Resolve namespace from cwd if not explicitly provided.
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let ns_override = if params.namespace.is_some() {
                params.namespace
            } else if settings.context.auto_detect {
                cwd_path
                    .and_then(clio_core::context::detect_namespace)
                    .map(|ctx| ctx.namespace)
            } else {
                None
            };

            let result = clio_core::capture::capture(
                &conn,
                &params.text,
                &settings.capture,
                ns_override.as_deref(),
                &settings,
            )
            .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&result).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Semantic vector search.
    #[tool(description = "Semantic vector search for similar memories.")]
    async fn memory_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<String, String> {
        if params.query.trim().is_empty() {
            return Err("query must not be empty for semantic search.".into());
        }
        validate_response_format(&params.response_format)?;
        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        let settings = self.settings()?;
        let backend = self.embedding_backend.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;

            let ns_filter = if params.global {
                None
            } else {
                let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
                let resolved_ns = clio_core::context::resolve_namespace(
                    params.namespace.as_deref(),
                    cwd_path,
                    settings.context.auto_detect,
                );
                if params.namespace.is_some() || resolved_ns != "global" {
                    Some(resolved_ns)
                } else {
                    None
                }
            };

            let be = backend.as_ref().as_ref().ok_or_else(|| {
                "Embedding backend not available. Ensure embeddings are configured in settings."
                    .to_string()
            })?;

            let query_embedding = be
                .embed_one(&params.query)
                .map_err(|e| format_clio_error(&e))?;

            let items = clio_core::embeddings::semantic_recall(
                &conn,
                &params.query,
                &query_embedding,
                ns_filter.as_deref(),
                params.include_archived,
                limit,
            )
            .map_err(|e| format_clio_error(&e))?;

            let len = items.len() as u32;
            let result = RecallResult {
                items,
                total: len,
                limit,
                offset: 0,
                count: len,
            };

            Ok(format_recall_response(&result, &params.response_format))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Memory statistics.
    #[tool(description = "Memory statistics: counts, breakdowns, coverage.")]
    async fn memory_stats(
        &self,
        Parameters(params): Parameters<StatsParams>,
    ) -> Result<String, String> {
        validate_response_format(&params.response_format)?;
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let stats = clio_core::stats::memory_stats(&conn, params.namespace.as_deref())
                .map_err(|e| format_clio_error(&e))?;

            if params.response_format == "json" {
                serde_json::to_string_pretty(&stats)
                    .map_err(|e| format!("Serialisation error: {e}"))
            } else {
                Ok(format_stats_md(&stats))
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Recent activity feed.
    #[tool(description = "Recent activity feed.")]
    async fn memory_activity(
        &self,
        Parameters(params): Parameters<ActivityParams>,
    ) -> Result<String, String> {
        validate_response_format(&params.response_format)?;
        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let entries =
                clio_core::stats::recent_activity(&conn, params.namespace.as_deref(), limit)
                    .map_err(|e| format_clio_error(&e))?;

            if params.response_format == "json" {
                serde_json::to_string_pretty(&entries)
                    .map_err(|e| format!("Serialisation error: {e}"))
            } else {
                Ok(format_activity_md(&entries))
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Suggest links by similarity.
    #[tool(description = "Suggest links based on embedding similarity.")]
    async fn memory_suggest_links(
        &self,
        Parameters(params): Parameters<SuggestLinksParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        validate_threshold(params.threshold, "threshold")?;
        validate_response_format(&params.response_format)?;
        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        let backend = self.embedding_backend.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let be = backend.as_ref().as_ref().ok_or_else(|| {
                "Embedding backend not available. Ensure embeddings are configured in settings."
                    .to_string()
            })?;

            let suggestions = clio_core::embeddings::suggest_links(
                &conn,
                &params.memory_id,
                be.as_ref(),
                params.threshold,
                limit,
            )
            .map_err(|e| format_clio_error(&e))?;

            if params.response_format == "json" {
                let items: Vec<serde_json::Value> = suggestions
                    .iter()
                    .map(|(mem, sim)| {
                        serde_json::json!({
                            "memory": mem,
                            "similarity": sim,
                        })
                    })
                    .collect();
                serde_json::to_string_pretty(&items)
                    .map_err(|e| format!("Serialisation error: {e}"))
            } else {
                Ok(format_suggestions_md(&suggestions))
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Build a context brief.
    #[tool(
        description = "Build a scoped context brief combining recent, important, and filtered memories."
    )]
    async fn memory_context(
        &self,
        Parameters(params): Parameters<ContextParams>,
    ) -> Result<String, String> {
        validate_response_format(&params.response_format)?;
        let max_items = cap_limit(params.max_items);
        let conn = self.conn.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let namespace = clio_core::context::resolve_namespace(
                params.namespace.as_deref(),
                cwd_path,
                settings.context.auto_detect,
            );

            let preset: clio_core::assembly::ContextPreset = params
                .preset
                .parse()
                .map_err(|e: clio_core::error::ClioError| format_clio_error(&e))?;

            let request = clio_core::assembly::ContextRequest {
                namespace: Some(namespace),
                preset,
                query: params.query,
                max_items,
                include_links: params.include_links,
                scoring: Some(settings.scoring.clone()),
            };

            let brief = clio_core::assembly::build_context(&conn, &request)
                .map_err(|e| format_clio_error(&e))?;

            if params.response_format == "json" {
                serde_json::to_string_pretty(&brief)
                    .map_err(|e| format!("Serialisation error: {e}"))
            } else {
                Ok(format_brief_md(&brief))
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// List pending review items.
    #[tool(
        description = "List pending review queue items (low-confidence captures awaiting review)."
    )]
    async fn memory_inbox_list(
        &self,
        Parameters(params): Parameters<InboxListParams>,
    ) -> Result<String, String> {
        validate_response_format(&params.response_format)?;
        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let items =
                clio_core::review::list_pending(&conn, limit).map_err(|e| format_clio_error(&e))?;

            if params.response_format == "json" {
                serde_json::to_string_pretty(&items)
                    .map_err(|e| format!("Serialisation error: {e}"))
            } else {
                Ok(review_list_md(&items))
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Approve a review item (converts to a memory).
    #[tool(description = "Approve a review queue item by ID, converting it to a stored memory.")]
    async fn memory_inbox_approve(
        &self,
        Parameters(params): Parameters<InboxApproveParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.review_id, "review_id")?;
        let conn = self.conn.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let memory = clio_core::review::approve_review(&conn, &params.review_id, &settings)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&memory).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Reject a review item.
    #[tool(description = "Reject a review queue item by ID.")]
    async fn memory_inbox_reject(
        &self,
        Parameters(params): Parameters<InboxRejectParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.review_id, "review_id")?;
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let item = clio_core::review::reject_review(&conn, &params.review_id)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&item).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Edit a review item's suggested fields.
    #[tool(description = "Edit a review queue item's suggested fields before approval.")]
    async fn memory_inbox_edit(
        &self,
        Parameters(params): Parameters<InboxEditParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.review_id, "review_id")?;
        if let Some(importance) = params.importance {
            validate_importance(importance)?;
        }
        if let Some(confidence) = params.confidence {
            validate_threshold(confidence, "confidence")?;
        }
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let edits = clio_core::review::ReviewEdits {
                namespace: params.namespace,
                kind: params.kind,
                title: params.title.map(Some),
                summary: params.summary.map(Some),
                tags: params.tags,
                importance: params.importance,
                confidence: params.confidence.map(Some),
            };
            let item = clio_core::review::edit_review(&conn, &params.review_id, &edits)
                .map_err(|e| format_clio_error(&e))?;
            serde_json::to_string_pretty(&item).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Clear all in-memory caches.
    #[tool(
        description = "Clear all in-memory caches (memory, recall, namespace, embedding). Returns counts of entries cleared."
    )]
    async fn memory_cache_clear(
        &self,
        Parameters(_params): Parameters<CacheClearParams>,
    ) -> Result<String, String> {
        let cache = self.cache.clone();
        let result = cache.clear_all();
        serde_json::to_string_pretty(&result).map_err(|e| format!("Serialisation error: {e}"))
    }
}

// ---------------------------------------------------------------------------
// ServerHandler implementation (resources + server info)
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for ClioServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: Implementation {
                name: "clio_mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "Clio: local memory system. remember=store, recall=keyword search, \
                 search=semantic search, capture=LLM classify, link=relate memories, \
                 context=build scoped context briefs, inbox=review low-confidence captures."
                    .into(),
            ),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            meta: None,
            next_cursor: None,
            resources: vec![Resource {
                raw: RawResource {
                    uri: "memory://schema".into(),
                    name: "Database Schema".into(),
                    title: None,
                    description: Some(
                        "Summary of the Clio database schema, table counts, and memory statistics."
                            .into(),
                    ),
                    mime_type: Some("text/markdown".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                annotations: None,
            }],
        })
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult {
            meta: None,
            next_cursor: None,
            resource_templates: vec![
                ResourceTemplate {
                    raw: RawResourceTemplate {
                        uri_template: "memory://item/{id}".into(),
                        name: "Memory Detail".into(),
                        title: None,
                        description: Some("Fetch a single memory by its ID.".into()),
                        mime_type: Some("text/markdown".into()),
                        icons: None,
                    },
                    annotations: None,
                },
                ResourceTemplate {
                    raw: RawResourceTemplate {
                        uri_template: "memory://recent/{namespace}".into(),
                        name: "Recent Memories".into(),
                        title: None,
                        description: Some("Recent memories for a given namespace.".into()),
                        mime_type: Some("text/markdown".into()),
                        icons: None,
                    },
                    annotations: None,
                },
            ],
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let uri = request.uri.clone();
        let conn = self.conn.clone();
        let cache = self.cache.clone();

        let content = tokio::task::spawn_blocking(move || -> Result<String, ClioError> {
            let conn = conn
                .lock()
                .map_err(|e| ClioError::Storage(format!("lock error: {e}")))?;

            if uri == "memory://schema" {
                return clio_core::repository::schema_info(&conn);
            }

            if let Some(id) = uri.strip_prefix("memory://item/") {
                if id.is_empty() || id.len() > 256 {
                    return Err(ClioError::Validation(
                        "invalid memory ID in resource URI.".into(),
                    ));
                }
                let memory = cache.get(&conn, id)?;
                return Ok(memory_detail_md(&memory));
            }

            if let Some(namespace) = uri.strip_prefix("memory://recent/") {
                let result = cache.recent(&conn, Some(namespace), 10)?;
                return Ok(recall_result_md(&result));
            }

            Err(ClioError::NotFound(format!("Unknown resource URI: {uri}")))
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("Task failed: {e}"), None))?
        .map_err(|e| match e {
            ClioError::NotFound(msg) => ErrorData::resource_not_found(msg, None),
            other => ErrorData::internal_error(format_clio_error(&other), None),
        })?;

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: request.uri,
                mime_type: Some("text/markdown".into()),
                text: content,
                meta: None,
            }],
        })
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logging to stderr only -- stdout is the MCP transport.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    // Resolve the database path (supports CLIO_DB_PATH env var and platform defaults).
    let db_path = clio_core::config::resolve_db_path(None).map_err(|e| {
        tracing::error!("Failed to resolve database path: {e}");
        e
    })?;

    tracing::info!(path = %db_path.display(), "Clio MCP server starting");

    // Open one shared connection (runs migrations on first use).
    let (conn, settings, backend) = {
        let path = db_path.clone();
        tokio::task::spawn_blocking(move || {
            let conn =
                clio_core::db::open(&path).map_err(|e| format!("failed to open database: {e}"))?;
            let settings = match clio_core::settings::load(&path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("failed to load settings, using defaults: {e}");
                    clio_core::settings::Settings::default()
                }
            };
            let backend = clio_core::embeddings::create_backend(&settings.embeddings)
                .map_err(|e| {
                    tracing::warn!("embedding backend unavailable at startup: {e}");
                    e
                })
                .ok();
            Ok::<_, String>((conn, settings, backend))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?
    };

    tracing::info!("Database ready");

    let server = ClioServer::new(db_path, conn, settings, backend);
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await.inspect_err(|e| {
        tracing::error!("Server failed to start: {e}");
    })?;

    tracing::info!("Clio MCP server running");
    service.waiting().await?;

    tracing::info!("Clio MCP server shut down");
    Ok(())
}
