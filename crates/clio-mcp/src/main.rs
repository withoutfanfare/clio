//! Clio MCP Server
//!
//! A Model Context Protocol server exposing the Clio shared memory system.
//! Communicates over stdin/stdout using JSON-RPC. All logging goes to stderr.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::fd::AsRawFd;

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
    LinkInput, Memory, RecallItem, RecallQuery, RecallResult, RememberInput, SortOrder, UpdateInput,
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

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,

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
struct UpdateParams {
    /// Memory ID to update in place.
    memory_id: String,

    /// Reject the write if the memory has changed since this timestamp.
    expected_updated_at: String,

    #[serde(default)]
    namespace: Option<String>,

    #[serde(default)]
    kind: Option<String>,

    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    title: Option<Option<String>>,

    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    summary: Option<Option<String>>,

    #[serde(default)]
    content: Option<String>,

    #[serde(default)]
    tags: Option<Vec<String>>,

    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    source: Option<Option<String>>,

    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    source_ref: Option<Option<String>>,

    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    confidence: Option<Option<f64>>,

    #[serde(default)]
    importance: Option<i32>,

    #[serde(default)]
    metadata: Option<serde_json::Value>,

    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    valid_from: Option<Option<String>>,

    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    valid_until: Option<Option<String>>,
}

fn deserialize_nullable_patch<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
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

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,

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

    /// Edge direction: outgoing (default, compatible shape), incoming, or
    /// both. Non-default directions return edge contexts with a `direction`
    /// field relative to this memory.
    #[serde(default)]
    direction: Option<String>,
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

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SessionCheckpointParams {
    /// Redacted session-delta digest to distil.
    text: String,

    /// Capturing agent, e.g. claude-session.
    source: String,

    /// Client session identifier.
    session_id: String,

    /// Monotonic transcript cursor marking where this delta ends.
    cursor: i64,

    /// Namespace override applied to every extracted memory.
    #[serde(default)]
    namespace: Option<String>,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,

    /// Git branch active during the session.
    #[serde(default)]
    branch: Option<String>,

    /// Ticket/issue identifier associated with the work.
    #[serde(default)]
    ticket: Option<String>,

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,
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

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,

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

    /// Minimum cosine similarity (0.0–1.0). Only suggestions at or above this are
    /// returned: lower = more permissive (more, looser links); 0.9+ = near-identical.
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

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,

    /// Preset: project-brief, person-brief, decision-history, active-constraints, recent-activity, handoff, custom.
    #[serde(default = "default_preset")]
    preset: String,

    /// FTS query for the custom and handoff presets (handoff requires it — pass the ticket id or topic).
    #[serde(default)]
    query: Option<String>,

    /// Max memories to include.
    #[serde(default = "default_max_items")]
    max_items: u32,

    /// Character budget for the whole brief (sections truncated greedily once reached).
    #[serde(default)]
    char_budget: Option<u32>,

    /// Include linked memories.
    #[serde(default)]
    include_links: bool,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InboxParams {
    /// Action: list | approve | reject | edit.
    action: String,

    /// Review item ID. Required for approve/reject/edit; ignored for list.
    #[serde(default, alias = "id")]
    review_id: Option<String>,

    /// Max pending items to return (list only).
    #[serde(default = "default_inbox_limit")]
    limit: u32,

    /// Format: markdown|json (list only).
    #[serde(default = "default_response_format")]
    response_format: String,

    /// Edit override: suggested namespace.
    #[serde(default)]
    namespace: Option<String>,

    /// Edit override: suggested kind.
    #[serde(default)]
    kind: Option<String>,

    /// Edit override: suggested title.
    #[serde(default)]
    title: Option<String>,

    /// Edit override: suggested summary.
    #[serde(default)]
    summary: Option<String>,

    /// Edit override: suggested tags.
    #[serde(default)]
    tags: Option<Vec<String>>,

    /// Edit override: suggested importance (1-5).
    #[serde(default)]
    importance: Option<i32>,

    /// Edit override: suggested confidence (0.0-1.0).
    #[serde(default)]
    confidence: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ResumeParams {
    /// Prompt/task context for the relevant-knowledge section. Omit at
    /// session start; pass the user's task on the first substantive prompt.
    #[serde(default)]
    query: Option<String>,

    /// Namespace scope. Auto-detected from cwd if omitted.
    #[serde(default)]
    namespace: Option<String>,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,

    /// Session or topic scope: suppresses repeats within the scope and
    /// records idempotent `surfaced` events.
    #[serde(default)]
    session_id: Option<String>,

    /// Maximum items across all sections.
    #[serde(default = "default_inbox_limit")]
    max_items: u32,

    /// Character budget over the serialised items.
    #[serde(default)]
    char_budget: Option<u32>,

    /// Format: markdown|json.
    #[serde(default = "default_response_format")]
    response_format: String,

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,
}

fn resume_brief_md(brief: &clio_core::assembly::ResumeBrief) -> String {
    if brief.sections.is_empty() {
        return format!(
            "# Resume — {}\n\nNothing to resume: no open work or relevant context.",
            brief.namespace
        );
    }
    let mut out = format!("# Resume — {}\n", brief.namespace);
    for section in &brief.sections {
        out.push_str(&format!("\n## {}\n\n", section.heading));
        for item in &section.items {
            let title = item.title.as_deref().unwrap_or("(untitled)");
            out.push_str(&format!(
                "- [{}] **{}** ({})\n",
                item.kind, title, item.memory_id
            ));
            out.push_str(&format!("  why: {}\n", item.reason));
            out.push_str(&format!("  {}\n", item.content));
        }
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ActionParams {
    /// Action: add | list | eligible | complete | snooze | cancel | attach_external | history.
    action: String,

    /// Attention item ID or memory ID. Required for complete, snooze, cancel,
    /// attach_external and history.
    #[serde(default, alias = "id")]
    action_id: Option<String>,

    /// Content for a new task memory (add). Alternative to memory_id.
    #[serde(default)]
    content: Option<String>,

    /// Existing memory to attach attention to (add).
    #[serde(default)]
    memory_id: Option<String>,

    /// Namespace: context for add/eligible, filter for list.
    #[serde(default)]
    namespace: Option<String>,

    /// Working dir for namespace detection.
    #[serde(default)]
    cwd: Option<String>,

    /// Who owns the follow-up (add), e.g. user.
    #[serde(default)]
    owner: Option<String>,

    /// Hard due date, ISO-8601 UTC (add).
    #[serde(default)]
    due_at: Option<String>,

    /// Reminder time, ISO-8601 UTC (add).
    #[serde(default)]
    remind_at: Option<String>,

    /// Non-time trigger (add), e.g. project-session.
    #[serde(default)]
    trigger: Option<String>,

    /// What or whom this waits on (add).
    #[serde(default)]
    waiting_on: Option<String>,

    /// What would prove this complete (add).
    #[serde(default)]
    completion_condition: Option<String>,

    /// Session or topic scope for once-per-scope suppression (eligible).
    #[serde(default)]
    scope: Option<String>,

    /// Evidence memory ID recording completion proof (complete).
    #[serde(default)]
    evidence: Option<String>,

    /// Wake time, ISO-8601 UTC (snooze).
    #[serde(default)]
    until: Option<String>,

    /// Why the state changed (complete/cancel).
    #[serde(default)]
    reason: Option<String>,

    /// External system name (attach_external), e.g. things or linear.
    #[serde(default)]
    external_system: Option<String>,

    /// Stable external item ID (attach_external).
    #[serde(default)]
    external_ref: Option<String>,

    /// Status filter (list): open, snoozed, resolved, cancelled.
    #[serde(default)]
    status: Option<String>,

    /// Max items/events to return (list/history).
    #[serde(default = "default_inbox_limit")]
    limit: u32,

    /// Namespace detected by a local remote bridge.
    #[serde(default, rename = "_clio_namespace")]
    #[schemars(skip)]
    clio_namespace: Option<String>,
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

fn resolve_mcp_namespace(
    explicit: Option<&str>,
    local_detected: Option<&str>,
    cwd: Option<&std::path::Path>,
    auto_detect: bool,
) -> String {
    let local_detected = if auto_detect { local_detected } else { None };
    clio_core::context::resolve_namespace(explicit.or(local_detected), cwd, auto_detect)
}

fn validate_memory_payload(params: &RememberParams) -> Result<(), String> {
    if params.content.trim().is_empty() {
        return Err("content must not be empty.".into());
    }
    if params.content.len() > 1_048_576 {
        return Err("content must not exceed 1 MiB.".into());
    }
    if params.tags.len() > 50 {
        return Err("at most 50 tags are allowed.".into());
    }
    if serde_json::to_string(&params.metadata).is_ok_and(|value| value.len() > 65_536) {
        return Err("metadata must not exceed 64 KiB when serialised.".into());
    }
    Ok(())
}

fn validate_update_payload(params: &UpdateParams) -> Result<(), String> {
    if params.expected_updated_at.trim().is_empty() {
        return Err("expected_updated_at must not be empty.".into());
    }
    if let Some(content) = params.content.as_deref() {
        if content.trim().is_empty() {
            return Err("content must not be empty.".into());
        }
        if content.len() > 1_048_576 {
            return Err("content must not exceed 1 MiB.".into());
        }
    }
    if params.tags.as_ref().is_some_and(|tags| tags.len() > 50) {
        return Err("at most 50 tags are allowed.".into());
    }
    if params.metadata.as_ref().is_some_and(|metadata| {
        serde_json::to_string(metadata).is_ok_and(|value| value.len() > 65_536)
    }) {
        return Err("metadata must not exceed 64 KiB when serialised.".into());
    }
    Ok(())
}

fn remember_input(params: RememberParams, namespace: String, upsert: bool) -> RememberInput {
    RememberInput {
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
        upsert,
    }
}

#[cfg(test)]
mod namespace_tests {
    use clio_core::models::UpdateInput;

    use super::{UpdateParams, resolve_mcp_namespace};

    #[test]
    fn local_detected_namespace_respects_precedence_and_setting() {
        let cases = [
            (
                Some("project:explicit"),
                Some("project:detected"),
                true,
                "project:explicit",
            ),
            (None, Some("project:detected"), true, "project:detected"),
            (None, Some("project:detected"), false, "global"),
        ];

        for (explicit, detected, auto_detect, expected) in cases {
            assert_eq!(
                resolve_mcp_namespace(explicit, detected, None, auto_detect),
                expected
            );
        }
    }

    #[test]
    fn action_params_default_optional_fields() {
        let params: super::ActionParams = serde_json::from_value(serde_json::json!({
            "action": "eligible"
        }))
        .unwrap();
        assert!(params.action_id.is_none());
        assert!(params.namespace.is_none());
        assert!(params.scope.is_none());

        // `id` is accepted as an alias for action_id.
        let aliased: super::ActionParams = serde_json::from_value(serde_json::json!({
            "action": "complete",
            "id": "0195-some-id"
        }))
        .unwrap();
        assert_eq!(aliased.action_id.as_deref(), Some("0195-some-id"));
    }

    #[test]
    fn session_checkpoint_params_default_optional_context() {
        let params: super::SessionCheckpointParams = serde_json::from_value(serde_json::json!({
            "text": "digest",
            "source": "claude-session",
            "session_id": "session-1",
            "cursor": 40
        }))
        .unwrap();
        assert!(params.namespace.is_none());
        assert!(params.branch.is_none());
        assert!(params.ticket.is_none());
        assert!(params.clio_namespace.is_none());

        // The local remote bridge injects the detected namespace on the wire.
        let bridged: super::SessionCheckpointParams = serde_json::from_value(serde_json::json!({
            "text": "digest",
            "source": "claude-session",
            "session_id": "session-1",
            "cursor": 40,
            "_clio_namespace": "project:clio"
        }))
        .unwrap();
        assert_eq!(bridged.clio_namespace.as_deref(), Some("project:clio"));
    }

    #[test]
    fn update_distinguishes_omitted_nullable_fields_from_clear() {
        let omitted: UpdateParams = serde_json::from_value(serde_json::json!({
            "memory_id": "memory-id",
            "expected_updated_at": "2026-07-28T12:00:00Z"
        }))
        .unwrap();
        let cleared: UpdateParams = serde_json::from_value(serde_json::json!({
            "memory_id": "memory-id",
            "expected_updated_at": "2026-07-28T12:00:00Z",
            "title": null
        }))
        .unwrap();

        assert!(omitted.title.is_none());
        assert!(matches!(cleared.title, Some(None)));

        let wire_patch = serde_json::to_value(UpdateInput {
            title: Some(None),
            ..UpdateInput::default()
        })
        .unwrap();
        assert!(wire_patch.get("title").is_some_and(|value| value.is_null()));
        assert!(wire_patch.get("summary").is_none());

        assert!(
            serde_json::from_value::<UpdateParams>(serde_json::json!({
                "memory_id": "memory-id",
                "title": "unguarded"
            }))
            .is_err()
        );
    }
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
            Ok(mut s) => {
                if s.embeddings != cache.settings.embeddings {
                    tracing::warn!(
                        "embedding settings changed; restart this MCP process to load the new backend"
                    );
                    s.embeddings = cache.settings.embeddings.clone();
                }
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

/// Generate a record embedding without holding the shared SQLite mutex, then
/// persist it only if the record has not changed while provider work ran.
fn embed_memory_if_current(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    backend: &dyn clio_core::embeddings::EmbeddingBackend,
    memory: &Memory,
) -> Result<(), String> {
    let passage = clio_core::embeddings::build_passage(memory);
    let embedding = backend
        .embed_one(&passage)
        .map_err(|e| format_clio_error(&e))?;

    let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
    let current = clio_core::repository::get_many_pub(&conn, std::slice::from_ref(&memory.id))
        .map_err(|e| format_clio_error(&e))?
        .into_iter()
        .next();
    if current.as_ref().map(|m| &m.updated_at) != Some(&memory.updated_at) {
        tracing::debug!(
            memory_id = %memory.id,
            "memory changed before auto-embedding completed; skipping stale vector"
        );
        return Ok(());
    }

    clio_core::embeddings::store_embedding(
        &conn,
        &memory.id,
        backend.model_name(),
        backend.dimensions(),
        &embedding,
    )
    .map_err(|e| format_clio_error(&e))
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

/// Render a single memory as a Markdown summary card (used in list views).
fn memory_card_md(item: &RecallItem) -> String {
    let m = &item.memory;
    let heading = m.title.as_deref().unwrap_or(&m.id);

    // Compact one-line metadata: `id` · namespace/kind · tags. Rank and full
    // timestamps are dropped from the card to save tokens — they remain in the
    // `response_format:"json"` payload for callers that need them.
    let mut meta = format!("`{}` · {}/{}", m.id, m.namespace, m.kind);
    if !m.tags.is_empty() {
        meta.push_str(&format!(" · {}", m.tags.join(", ")));
    }

    let preview = m.summary.as_deref().unwrap_or(&m.content);
    let truncated = if preview.len() > 200 {
        format!("{}...", truncate_chars(preview, 200))
    } else {
        preview.to_string()
    };

    format!(
        "### {heading}\n{meta}\n> {}\n",
        truncated.replace('\n', "\n> ")
    )
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
    #[tool(
        description = "Store a memory. Upsert (replace-in-place) requires BOTH `source` and \
                       `source_ref`; without both, a new record is always inserted."
    )]
    async fn memory_remember(
        &self,
        Parameters(params): Parameters<RememberParams>,
    ) -> Result<String, String> {
        // MCP boundary validation (defence-in-depth on top of core validation).
        validate_memory_payload(&params)?;
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        let settings = self.settings()?;
        let backend = self.embedding_backend.clone();
        tokio::task::spawn_blocking(move || {
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let namespace = resolve_mcp_namespace(
                params.namespace.as_deref(),
                params.clio_namespace.as_deref(),
                cwd_path,
                settings.context.auto_detect,
            );
            let upsert = params.upsert;
            let input = remember_input(params, namespace, upsert);
            let memory = {
                let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
                cache
                    .remember(&conn, &input, &settings)
                    .map_err(|e| format_clio_error(&e))?
            };

            // Auto-embed if enabled.
            if settings.auto_embed {
                if let Some(ref be) = *backend {
                    if let Err(e) = embed_memory_if_current(&conn, be.as_ref(), &memory) {
                        tracing::warn!("auto-embed failed: {e}");
                    }
                }
            }

            serde_json::to_string_pretty(&memory).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Patch a memory in place by ID.
    #[tool(
        description = "Patch only the supplied fields of a memory without creating a duplicate. expected_updated_at is required and rejects stale writes."
    )]
    async fn memory_update(
        &self,
        Parameters(params): Parameters<UpdateParams>,
    ) -> Result<String, String> {
        validate_memory_id(&params.memory_id, "memory_id")?;
        validate_update_payload(&params)?;
        let memory_id = params.memory_id;
        let input = UpdateInput {
            namespace: params.namespace,
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
            expected_updated_at: Some(params.expected_updated_at),
        };
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        let settings = self.settings()?;
        let backend = self.embedding_backend.clone();
        tokio::task::spawn_blocking(move || {
            let memory = {
                let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
                cache
                    .update(&conn, &memory_id, &input, &settings)
                    .map_err(|e| format_clio_error(&e))?
            };

            if settings.auto_embed {
                if let Some(ref be) = *backend {
                    if let Err(e) = embed_memory_if_current(&conn, be.as_ref(), &memory) {
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
    #[tool(
        description = "Keyword/full-text (FTS) search and filter memories. Omit `query` for a \
                       recent-style listing. Pass `cwd` for namespace auto-detection."
    )]
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
            let detected_ns = resolve_mcp_namespace(
                params.namespace.as_deref(),
                params.clio_namespace.as_deref(),
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
                exclude_expired: false,
                importance_min: params.importance_min,
                importance_max: params.importance_max,
                sort_by,
                limit,
                offset: params.offset,
                scoring,
                skip_access_tracking: false,
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
    #[tool(
        description = "Deprecated: use memory_recall with no `query` instead. Lists recent memories."
    )]
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
        let direction = params
            .direction
            .as_deref()
            .unwrap_or("outgoing")
            .to_string();
        if !matches!(direction.as_str(), "outgoing" | "incoming" | "both") {
            return Err(format!(
                "unknown direction '{direction}'. Expected outgoing, incoming, or both."
            ));
        }
        let conn = self.conn.clone();
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            if direction == "outgoing" {
                // Compatible shape for existing callers.
                let links = cache
                    .get_links(&conn, &params.memory_id)
                    .map_err(|e| format_clio_error(&e))?;
                return serde_json::to_string_pretty(&links)
                    .map_err(|e| format!("Serialisation error: {e}"));
            }
            let mut contexts = clio_core::repository::get_link_contexts(&conn, &params.memory_id)
                .map_err(|e| format_clio_error(&e))?;
            if direction == "incoming" {
                contexts.retain(|c| c.direction == "incoming");
            }
            serde_json::to_string_pretty(&contexts).map_err(|e| format!("Serialisation error: {e}"))
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
        let backend = self.embedding_backend.clone();
        tokio::task::spawn_blocking(move || {
            // The explicit namespace and the cwd-detected default stay separate:
            // core resolves them with the same precedence as capture/distill
            // everywhere else (override → model's global promotion → cwd → model).
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let default_ns = if settings.context.auto_detect {
                params.clio_namespace.or_else(|| {
                    cwd_path
                        .and_then(clio_core::context::detect_namespace)
                        .map(|ctx| ctx.namespace)
                })
            } else {
                None
            };

            let classification = clio_core::capture::classify(&params.text, &settings.capture)
                .map_err(|e| format_clio_error(&e))?;
            let result = {
                let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
                clio_core::capture::capture_with_classification_deferred_embedding(
                    &conn,
                    &params.text,
                    &classification,
                    params.namespace.as_deref(),
                    default_ns.as_deref(),
                    &settings,
                )
                .map_err(|e| format_clio_error(&e))?
            };

            if settings.auto_embed {
                if let (clio_core::capture::CaptureResult::Stored(memory), Some(be)) =
                    (&result, backend.as_ref())
                {
                    if let Err(e) = embed_memory_if_current(&conn, be.as_ref(), memory) {
                        tracing::warn!(memory_id = %memory.id, "capture auto-embed failed: {e}");
                    }
                }
            }
            serde_json::to_string_pretty(&result).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    #[tool(
        description = "Distil a session delta and commit it as an exact-once checkpoint keyed by \
                       source + session_id + cursor. Safe to retry: a completed key replays the \
                       stored result (memory and review IDs) instead of creating duplicates. An \
                       empty extraction is a successful checkpoint. Requires a configured capture \
                       model; returns a configuration error otherwise."
    )]
    async fn memory_session_checkpoint(
        &self,
        Parameters(params): Parameters<SessionCheckpointParams>,
    ) -> Result<String, String> {
        if params.text.trim().is_empty() {
            return Err("text must not be empty.".into());
        }
        if params.source.is_empty() {
            return Err("source is required.".into());
        }
        if params.session_id.is_empty() {
            return Err("session_id is required.".into());
        }
        if params.cursor < 0 {
            return Err("cursor must be zero or positive.".into());
        }
        let conn = self.conn.clone();
        let settings = self.settings()?;
        let backend = self.embedding_backend.clone();
        tokio::task::spawn_blocking(move || {
            // Resolve the default namespace from cwd like memory_capture, but
            // keep an explicit namespace as the override that always wins.
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let default_namespace = if settings.context.auto_detect {
                params.clio_namespace.clone().or_else(|| {
                    cwd_path
                        .and_then(clio_core::context::detect_namespace)
                        .map(|ctx| ctx.namespace)
                })
            } else {
                None
            };

            let request = clio_core::checkpoint::CheckpointRequest {
                source: params.source,
                session_id: params.session_id,
                cursor: params.cursor,
                namespace_override: params.namespace,
                default_namespace,
                cwd: params.cwd,
                branch: params.branch,
                ticket: params.ticket,
            };

            if let Some(existing) = {
                let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
                clio_core::checkpoint::find_checkpoint(
                    &conn,
                    &request.source,
                    &request.session_id,
                    request.cursor,
                )
                .map_err(|e| format_clio_error(&e))?
            } {
                return serde_json::to_string_pretty(&existing)
                    .map_err(|e| format!("Serialisation error: {e}"));
            }

            let memories = clio_core::capture::distill(&params.text, &settings.capture)
                .map_err(|e| format_clio_error(&e))?;
            let (result, stored_memories) = {
                let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
                let result =
                    clio_core::checkpoint::store_checkpoint(&conn, &request, &memories, &settings)
                        .map_err(|e| format_clio_error(&e))?;
                let stored_memories = if !result.replayed && settings.auto_embed {
                    clio_core::repository::get_many_pub(&conn, &result.stored_memory_ids)
                        .map_err(|e| format_clio_error(&e))?
                } else {
                    Vec::new()
                };
                (result, stored_memories)
            };

            if let Some(ref be) = *backend {
                for memory in &stored_memories {
                    if let Err(e) = embed_memory_if_current(&conn, be.as_ref(), memory) {
                        tracing::warn!(memory_id = %memory.id, "checkpoint auto-embed failed: {e}");
                    }
                }
            }

            serde_json::to_string_pretty(&result).map_err(|e| format!("Serialisation error: {e}"))
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Semantic vector search.
    #[tool(
        description = "Semantic (vector) search for similar memories. Requires a configured \
                       embedding backend; returns a configuration error otherwise (use \
                       memory_recall for keyword search when embeddings are unavailable)."
    )]
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
            let be = backend.as_ref().as_ref().ok_or_else(|| {
                "Embedding backend not available. Ensure embeddings are configured in settings."
                    .to_string()
            })?;

            let query_embedding = be
                .embed_one(&params.query)
                .map_err(|e| format_clio_error(&e))?;

            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;

            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let detected_ns = resolve_mcp_namespace(
                params.namespace.as_deref(),
                params.clio_namespace.as_deref(),
                cwd_path,
                settings.context.auto_detect,
            );

            let items = if params.global {
                clio_core::embeddings::semantic_recall(
                    &conn,
                    &params.query,
                    &query_embedding,
                    be.model_name(),
                    None,
                    params.include_archived,
                    false,
                    Some(&settings.scoring),
                    limit,
                )
            } else if params.namespace.is_some() {
                clio_core::embeddings::semantic_recall(
                    &conn,
                    &params.query,
                    &query_embedding,
                    be.model_name(),
                    Some(&detected_ns),
                    params.include_archived,
                    false,
                    Some(&settings.scoring),
                    limit,
                )
            } else {
                clio_core::embeddings::semantic_recall_scoped(
                    &conn,
                    &params.query,
                    &query_embedding,
                    be.model_name(),
                    &detected_ns,
                    params.include_archived,
                    false,
                    Some(&settings.scoring),
                    limit,
                )
            }
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
    #[tool(
        description = "Suggest links based on embedding similarity. Requires a configured \
                       embedding backend; returns a configuration error otherwise."
    )]
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
            let namespace = resolve_mcp_namespace(
                params.namespace.as_deref(),
                params.clio_namespace.as_deref(),
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
                char_budget: params.char_budget,
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

    /// Review the capture inbox: list / approve / reject / edit.
    #[tool(
        description = "Review the capture inbox (low-confidence memories awaiting review). \
                       action=\"list\" returns pending items; \"approve\" stores an item; \
                       \"reject\" discards it; \"edit\" adjusts suggested fields before approval. \
                       approve/reject/edit require review_id."
    )]
    async fn memory_inbox(
        &self,
        Parameters(params): Parameters<InboxParams>,
    ) -> Result<String, String> {
        let action = params.action.to_lowercase();

        // Validate up front (before offloading to the blocking pool).
        match action.as_str() {
            "list" => validate_response_format(&params.response_format)?,
            "approve" | "reject" | "edit" => {
                let id = params
                    .review_id
                    .as_deref()
                    .ok_or_else(|| format!("inbox action '{action}' requires review_id"))?;
                validate_memory_id(id, "review_id")?;
                if action == "edit" {
                    if let Some(importance) = params.importance {
                        validate_importance(importance)?;
                    }
                    if let Some(confidence) = params.confidence {
                        validate_threshold(confidence, "confidence")?;
                    }
                }
            }
            other => {
                return Err(format!(
                    "unknown inbox action '{other}'. Expected list, approve, reject, or edit."
                ));
            }
        }

        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            match action.as_str() {
                "list" => {
                    let items = clio_core::review::list_pending(&conn, limit)
                        .map_err(|e| format_clio_error(&e))?;
                    if params.response_format == "json" {
                        serde_json::to_string_pretty(&items)
                            .map_err(|e| format!("Serialisation error: {e}"))
                    } else {
                        Ok(review_list_md(&items))
                    }
                }
                "approve" => {
                    let id = params.review_id.as_deref().expect("validated present");
                    let memory = clio_core::review::approve_review(&conn, id, &settings)
                        .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&memory)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "reject" => {
                    let id = params.review_id.as_deref().expect("validated present");
                    let item = clio_core::review::reject_review(&conn, id)
                        .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&item)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "edit" => {
                    let id = params.review_id.clone().expect("validated present");
                    let edits = clio_core::review::ReviewEdits {
                        namespace: params.namespace,
                        kind: params.kind,
                        title: params.title.map(Some),
                        summary: params.summary.map(Some),
                        tags: params.tags,
                        importance: params.importance,
                        confidence: params.confidence.map(Some),
                    };
                    let item = clio_core::review::edit_review(&conn, &id, &edits)
                        .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&item)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                _ => unreachable!("action validated above"),
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    #[tool(
        description = "Build a resume brief: eligible open work, blocked items, constraints, \
                       recent decisions, prompt-relevant knowledge and recent activity — each \
                       with the reason it appears now. Call at session start (no query), and \
                       again with `query` on the first substantive task prompt. Reads are \
                       untracked and repeats are suppressed per session_id scope."
    )]
    async fn memory_resume(
        &self,
        Parameters(params): Parameters<ResumeParams>,
    ) -> Result<String, String> {
        validate_response_format(&params.response_format)?;
        let conn = self.conn.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let namespace = params.namespace.clone().or_else(|| {
                if settings.context.auto_detect {
                    params.clio_namespace.clone().or_else(|| {
                        cwd_path
                            .and_then(clio_core::context::detect_namespace)
                            .map(|ctx| ctx.namespace)
                    })
                } else {
                    None
                }
            });

            let request = clio_core::assembly::ResumeRequest {
                namespace,
                query: params.query,
                session_id: params.session_id,
                max_items: cap_limit(params.max_items),
                char_budget: params.char_budget,
                scoring: Some(settings.scoring.clone()),
                dormant_days: settings.attention.dormant_days,
                now: None,
            };

            let brief = clio_core::assembly::build_resume_brief(&conn, &request)
                .map_err(|e| format_clio_error(&e))?;
            if params.response_format == "json" {
                serde_json::to_string_pretty(&brief)
                    .map_err(|e| format!("Serialisation error: {e}"))
            } else {
                Ok(resume_brief_md(&brief))
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    #[tool(
        description = "Manage follow-up attention on memories (open loops). Actions: add (open \
                       attention on new or existing memory), list, eligible (what needs attention \
                       now, with a machine-readable reason per item), complete (with optional \
                       evidence memory), snooze (until a time), cancel, attach_external (record a \
                       verified Things/Linear reference), history (event audit for an item), \
                       overview (eligible + open items, review depth and consolidation \
                       freshness in one call). \
                       Statuses: open, snoozed, resolved, cancelled. Completion never rewrites \
                       the source memory."
    )]
    async fn memory_action(
        &self,
        Parameters(params): Parameters<ActionParams>,
    ) -> Result<String, String> {
        use clio_core::attention;

        let action = params.action.to_lowercase();
        match action.as_str() {
            "add" => {
                if params.content.is_none() && params.memory_id.is_none() {
                    return Err("action 'add' requires content or memory_id".into());
                }
            }
            "complete" | "snooze" | "cancel" | "attach_external" | "history" => {
                params
                    .action_id
                    .as_deref()
                    .ok_or_else(|| format!("action '{action}' requires an id"))?;
                if action == "snooze" && params.until.is_none() {
                    return Err("action 'snooze' requires until".into());
                }
                if action == "attach_external"
                    && (params.external_system.is_none() || params.external_ref.is_none())
                {
                    return Err(
                        "action 'attach_external' requires external_system and external_ref".into(),
                    );
                }
            }
            "list" | "eligible" | "overview" => {}
            other => {
                return Err(format!(
                    "unknown action '{other}'. Expected add, list, eligible, overview, complete, \
                     snooze, cancel, attach_external, or history."
                ));
            }
        }

        let limit = cap_limit(params.limit);
        let conn = self.conn.clone();
        let settings = self.settings()?;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
            let cwd_path = params.cwd.as_deref().map(std::path::Path::new);
            let detected_namespace = params.namespace.clone().or_else(|| {
                if settings.context.auto_detect {
                    params.clio_namespace.clone().or_else(|| {
                        cwd_path
                            .and_then(clio_core::context::detect_namespace)
                            .map(|ctx| ctx.namespace)
                    })
                } else {
                    None
                }
            });

            match action.as_str() {
                "add" => {
                    conn.execute_batch("BEGIN IMMEDIATE")
                        .map_err(|e| format!("lock error: {e}"))?;
                    let result = (|| -> Result<attention::AttentionItem, String> {
                        let memory_id = match (&params.memory_id, &params.content) {
                            (Some(id), _) => id.clone(),
                            (None, Some(text)) => {
                                clio_core::repository::remember(
                                    &conn,
                                    &clio_core::models::RememberInput {
                                        namespace: detected_namespace
                                            .clone()
                                            .unwrap_or_else(|| "global".into()),
                                        kind: "task".into(),
                                        title: None,
                                        summary: None,
                                        content: text.clone(),
                                        tags: vec!["follow-up".into()],
                                        source: None,
                                        source_ref: None,
                                        confidence: None,
                                        importance: 3,
                                        metadata: serde_json::json!({}),
                                        valid_from: None,
                                        valid_until: None,
                                        upsert: false,
                                    },
                                    &settings,
                                )
                                .map_err(|e| format_clio_error(&e))?
                                .id
                            }
                            (None, None) => unreachable!("validated above"),
                        };
                        attention::create_attention(
                            &conn,
                            &attention::AttentionInput {
                                memory_id,
                                owner: params.owner.clone(),
                                due_at: params.due_at.clone(),
                                remind_at: params.remind_at.clone(),
                                trigger: params.trigger.clone(),
                                waiting_on: params.waiting_on.clone(),
                                completion_condition: params.completion_condition.clone(),
                                actor: Some("agent".into()),
                            },
                        )
                        .map_err(|e| format_clio_error(&e))
                    })();
                    match result {
                        Ok(item) => {
                            conn.execute_batch("COMMIT")
                                .map_err(|e| format!("commit error: {e}"))?;
                            serde_json::to_string_pretty(&item)
                                .map_err(|e| format!("Serialisation error: {e}"))
                        }
                        Err(e) => {
                            let _ = conn.execute_batch("ROLLBACK");
                            Err(e)
                        }
                    }
                }
                "list" => {
                    let items = attention::list_attention(
                        &conn,
                        detected_namespace.as_deref(),
                        params.status.as_deref(),
                        limit,
                    )
                    .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&items)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "overview" => {
                    let overview = attention::overview(
                        &conn,
                        detected_namespace.as_deref(),
                        params.scope.as_deref(),
                        settings.attention.dormant_days,
                    )
                    .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&overview)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "eligible" => {
                    let context = attention::EligibilityContext {
                        namespace: detected_namespace,
                        scope: params.scope.clone(),
                        now: clio_core::models::now_utc(),
                        dormant_days: settings.attention.dormant_days,
                    };
                    let items =
                        attention::eligible(&conn, &context).map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&items)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "complete" => {
                    let id = params.action_id.as_deref().expect("validated present");
                    let item = attention::complete(
                        &conn,
                        id,
                        params.evidence.as_deref(),
                        params.reason.as_deref(),
                        Some("agent"),
                    )
                    .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&item)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "snooze" => {
                    let id = params.action_id.as_deref().expect("validated present");
                    let until = params.until.as_deref().expect("validated present");
                    let item = attention::snooze(&conn, id, until, Some("agent"))
                        .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&item)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "cancel" => {
                    let id = params.action_id.as_deref().expect("validated present");
                    let item =
                        attention::cancel(&conn, id, params.reason.as_deref(), Some("agent"))
                            .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&item)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "attach_external" => {
                    let id = params.action_id.as_deref().expect("validated present");
                    let item = attention::attach_external(
                        &conn,
                        id,
                        params.external_system.as_deref().expect("validated"),
                        params.external_ref.as_deref().expect("validated"),
                        Some("agent"),
                    )
                    .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&item)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                "history" => {
                    let id = params.action_id.as_deref().expect("validated present");
                    let item = attention::resolve_attention(&conn, id)
                        .map_err(|e| format_clio_error(&e))?;
                    let events = clio_core::events::list_events(&conn, &item.memory_id, limit)
                        .map_err(|e| format_clio_error(&e))?;
                    serde_json::to_string_pretty(&events)
                        .map_err(|e| format!("Serialisation error: {e}"))
                }
                _ => unreachable!("action validated above"),
            }
        })
        .await
        .map_err(|e| format!("Internal error: task failed: {e}"))?
    }

    /// Clear all in-memory caches. Individual record reads are always fresh.
    #[tool(
        description = "Clear bounded recall and namespace caches. Individual memory and embedding reads are not cached. Returns counts of entries cleared."
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
                "Clio: local shared-memory store for AI tools (SQLite-backed).\n\n\
                 NAMESPACE: memories are scoped per project. Resolution order is \
                 explicit `namespace` -> auto-detect from `cwd` -> `global`. Always pass \
                 `cwd` so recall and storage land in the right project.\n\n\
                 CHOOSING A TOOL:\n\
                 - memory_recall: keyword/FTS search. Omit `query` for a recent-style listing.\n\
                 - memory_search: semantic (vector) search; requires a configured embedding \
                 backend and returns a configuration error if none is set.\n\
                 - memory_remember: deliberate store. Upsert needs BOTH `source` and \
                 `source_ref`, else a new record is inserted.\n\
                 - memory_capture: LLM-classified store; low-confidence items queue to the \
                 inbox for review instead of storing immediately.\n\
                 - memory_context: assemble a scoped brief. Presets: project-brief, \
                 person-brief, decision-history, active-constraints, recent-activity, handoff, custom. \
                 The handoff preset requires `query` (a ticket id or topic) and returns a pickup \
                 brief: relevant memories, active constraints, recent receipts.\n\
                 - memory_inbox: review queued captures (list/approve/reject/edit via `action`).\n\
                 - memory_session_checkpoint: distil a session delta exactly once, keyed by \
                 source + session_id + cursor. Retries replay the stored result; an empty \
                 extraction is a successful checkpoint. Requires a configured capture model.\n\
                 - memory_resume: evidence-backed pickup brief (open work, blockers, \
                 constraints, relevant knowledge — each with why-now). Prefer it over ad hoc \
                 recall when resuming work; it is untracked and repeat-suppressed per session.\n\
                 - memory_action: follow-up attention on memories (open loops). When the user \
                 states an explicit decision or commitment, store it IMMEDIATELY with \
                 memory_remember or memory_action(add) — do not wait for end-of-session \
                 distillation, which is only the safety net. Complete with evidence when done. \
                 action:eligible reports what needs attention now and why.\n\n\
                 TICKET CONVENTION: when working a tracked issue, tag stored memories \
                 `ticket:<issue-id>` (lowercase). Tags are FTS-indexed, so a later handoff \
                 brief for that id finds them.\n\n\
                 Archive is a soft-delete: archived records are hidden and excluded from recall \
                 by default. Pass `response_format:\"json\"` for structured processing (cheaper \
                 tokens); markdown is for human display."
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

#[cfg(unix)]
fn acquire_database_lease(db_path: &std::path::Path) -> std::io::Result<File> {
    let mut lock_path = db_path.as_os_str().to_os_string();
    lock_path.push(".maintenance.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    // SAFETY: flock only reads the valid file descriptor and the File keeps it
    // open for the lifetime of the server.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

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

    #[cfg(unix)]
    let _database_lease = acquire_database_lease(&db_path).map_err(|e| {
        tracing::error!("Failed to acquire database maintenance lease: {e}");
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

#[cfg(all(test, unix))]
mod maintenance_lock_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn database_lease_blocks_exclusive_maintenance() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path =
            std::env::temp_dir().join(format!("clio-mcp-lock-{}-{unique}.db", std::process::id()));
        let lease = acquire_database_lease(&db_path).unwrap();
        let mut lock_path = db_path.as_os_str().to_os_string();
        lock_path.push(".maintenance.lock");
        let maintenance = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();

        // SAFETY: maintenance is open for the duration of each flock call.
        assert_eq!(
            unsafe { libc::flock(maintenance.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB,) },
            -1
        );
        drop(lease);
        // SAFETY: maintenance remains open and now owns the exclusive lock.
        assert_eq!(
            unsafe { libc::flock(maintenance.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB,) },
            0
        );
        drop(maintenance);
        std::fs::remove_file(lock_path).unwrap();
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver, SyncSender};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct BlockingBackend {
        started: Arc<Barrier>,
        release: Arc<Barrier>,
    }

    struct SignallingBackend {
        started: SyncSender<()>,
        release: Mutex<Receiver<()>>,
    }

    impl clio_core::embeddings::EmbeddingBackend for SignallingBackend {
        fn model_name(&self) -> &str {
            "signalling-test"
        }

        fn dimensions(&self) -> usize {
            1
        }

        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            self.started.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(vec![1.0])
        }

        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(vec![vec![1.0]; texts.len()])
        }
    }

    impl clio_core::embeddings::EmbeddingBackend for BlockingBackend {
        fn model_name(&self) -> &str {
            "blocking-test"
        }

        fn dimensions(&self) -> usize {
            1
        }

        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            self.started.wait();
            self.release.wait();
            Ok(vec![1.0])
        }

        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(vec![vec![1.0]; texts.len()])
        }
    }

    fn capture_endpoint(
        started: Arc<Barrier>,
        release: Arc<Barrier>,
        assistant_content: serde_json::Value,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                if read == 0 || request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = request
                .windows(4)
                .position(|bytes| bytes == b"\r\n\r\n")
                .unwrap()
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }

            started.wait();
            release.wait();

            let body = serde_json::json!({
                "choices": [{"message": {"content": assistant_content.to_string()}}],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "completion_tokens_details": {"reasoning_tokens": 0},
                    "prompt_tokens_details": {"cached_tokens": 0}
                }
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            stream.flush().unwrap();
        });

        (format!("http://{address}"), handle)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn semantic_embedding_does_not_block_unrelated_database_reads() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "clio-mcp-concurrency-{}-{unique}.db",
            std::process::id()
        ));
        let conn = clio_core::db::open(&db_path).unwrap();
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let server = ClioServer::new(
            db_path.clone(),
            conn,
            clio_core::settings::Settings::default(),
            Some(Box::new(BlockingBackend {
                started: started.clone(),
                release: release.clone(),
            })),
        );

        let search_server = server.clone();
        let search = tokio::spawn(async move {
            search_server
                .memory_search(Parameters(SearchParams {
                    query: "concurrency".into(),
                    namespace: None,
                    global: true,
                    cwd: None,
                    clio_namespace: None,
                    include_archived: false,
                    limit: 10,
                    response_format: "json".into(),
                }))
                .await
        });

        started.wait();
        let namespace_result =
            tokio::time::timeout(Duration::from_millis(500), server.memory_namespaces()).await;
        release.wait();
        search.await.unwrap().unwrap();

        assert!(
            namespace_result.is_ok(),
            "provider work held the SQLite mutex and blocked an unrelated read"
        );

        drop(server);
        std::fs::remove_file(db_path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn automatic_embedding_does_not_block_unrelated_database_reads() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "clio-mcp-auto-embed-{}-{unique}.db",
            std::process::id()
        ));
        let conn = clio_core::db::open(&db_path).unwrap();
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let server = ClioServer::new(
            db_path.clone(),
            conn,
            clio_core::settings::Settings::default(),
            Some(Box::new(BlockingBackend {
                started: started.clone(),
                release: release.clone(),
            })),
        );

        let remember_server = server.clone();
        let remember = tokio::spawn(async move {
            remember_server
                .memory_remember(Parameters(RememberParams {
                    namespace: Some("global".into()),
                    cwd: None,
                    clio_namespace: None,
                    kind: "fact".into(),
                    title: Some("Concurrency".into()),
                    summary: None,
                    content: "Provider work should not block SQLite reads.".into(),
                    tags: Vec::new(),
                    source: None,
                    source_ref: None,
                    confidence: None,
                    importance: 3,
                    metadata: serde_json::json!({}),
                    valid_from: None,
                    valid_until: None,
                    upsert: false,
                }))
                .await
        });

        started.wait();
        let namespace_result =
            tokio::time::timeout(Duration::from_millis(500), server.memory_namespaces()).await;
        release.wait();
        remember.await.unwrap().unwrap();

        assert!(
            namespace_result.is_ok(),
            "automatic embedding held the SQLite mutex and blocked an unrelated read"
        );

        drop(server);
        std::fs::remove_file(db_path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_provider_does_not_block_unrelated_database_reads() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "clio-mcp-capture-{}-{unique}.db",
            std::process::id()
        ));
        let conn = clio_core::db::open(&db_path).unwrap();
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let (base_url, endpoint) = capture_endpoint(
            started.clone(),
            release.clone(),
            serde_json::json!({
                "kind": "fact",
                "title": "Concurrency",
                "summary": "Provider calls do not hold the database lock.",
                "tags": ["mcp"],
                "namespace": "global",
                "importance": 3,
                "confidence": 1.0
            }),
        );
        let mut settings = clio_core::settings::Settings::default();
        settings.auto_embed = false;
        settings.capture = clio_core::settings::CaptureConfig {
            enabled: true,
            api_key: Some("test-key".into()),
            base_url,
            model: "gpt-4.1".into(),
            review_threshold: None,
        };
        let server = ClioServer::new(db_path.clone(), conn, settings, None);

        let capture_server = server.clone();
        let capture = tokio::spawn(async move {
            capture_server
                .memory_capture(Parameters(CaptureParams {
                    text: "Remember the concurrency contract.".into(),
                    namespace: Some("global".into()),
                    cwd: None,
                    clio_namespace: None,
                }))
                .await
        });

        started.wait();
        let namespace_result =
            tokio::time::timeout(Duration::from_millis(500), server.memory_namespaces()).await;
        release.wait();
        capture.await.unwrap().unwrap();
        endpoint.join().unwrap();

        assert!(
            namespace_result.is_ok(),
            "capture provider work held the SQLite mutex and blocked an unrelated read"
        );

        drop(server);
        std::fs::remove_file(db_path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_auto_embedding_uses_cached_backend_without_blocking_database_reads() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "clio-mcp-capture-embedding-{}-{unique}.db",
            std::process::id()
        ));
        let conn = clio_core::db::open(&db_path).unwrap();
        let classification_started = Arc::new(Barrier::new(2));
        let classification_release = Arc::new(Barrier::new(2));
        let (base_url, endpoint) = capture_endpoint(
            classification_started.clone(),
            classification_release.clone(),
            serde_json::json!({
                "kind": "fact",
                "title": "Capture embedding",
                "summary": "Capture reuses the MCP embedding backend.",
                "tags": ["mcp"],
                "namespace": "global",
                "importance": 3,
                "confidence": 1.0
            }),
        );
        let mut settings = clio_core::settings::Settings::default();
        settings.auto_embed = true;
        settings.capture = clio_core::settings::CaptureConfig {
            enabled: true,
            api_key: Some("test-key".into()),
            base_url,
            model: "gpt-4.1".into(),
            review_threshold: None,
        };
        let (embedding_started_tx, embedding_started_rx) = mpsc::sync_channel(1);
        let (embedding_release_tx, embedding_release_rx) = mpsc::sync_channel(1);
        let server = ClioServer::new(
            db_path.clone(),
            conn,
            settings,
            Some(Box::new(SignallingBackend {
                started: embedding_started_tx,
                release: Mutex::new(embedding_release_rx),
            })),
        );

        let capture_server = server.clone();
        let capture = tokio::spawn(async move {
            capture_server
                .memory_capture(Parameters(CaptureParams {
                    text: "Remember that capture embedding is non-blocking.".into(),
                    namespace: Some("global".into()),
                    cwd: None,
                    clio_namespace: None,
                }))
                .await
        });

        classification_started.wait();
        classification_release.wait();
        let embedding_started = tokio::task::spawn_blocking(move || {
            embedding_started_rx.recv_timeout(Duration::from_secs(1))
        })
        .await
        .unwrap();
        if embedding_started.is_ok() {
            let namespace_result =
                tokio::time::timeout(Duration::from_millis(500), server.memory_namespaces()).await;
            embedding_release_tx.send(()).unwrap();
            assert!(
                namespace_result.is_ok(),
                "capture auto-embedding held the SQLite mutex and blocked an unrelated read"
            );
        }

        capture.await.unwrap().unwrap();
        endpoint.join().unwrap();
        assert!(
            embedding_started.is_ok(),
            "capture did not reuse the MCP server's cached embedding backend"
        );

        drop(server);
        std::fs::remove_file(db_path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn checkpoint_provider_does_not_block_unrelated_database_reads() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "clio-mcp-checkpoint-{}-{unique}.db",
            std::process::id()
        ));
        let conn = clio_core::db::open(&db_path).unwrap();
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let (base_url, endpoint) = capture_endpoint(
            started.clone(),
            release.clone(),
            serde_json::json!({"memories": []}),
        );
        let mut settings = clio_core::settings::Settings::default();
        settings.auto_embed = false;
        settings.capture = clio_core::settings::CaptureConfig {
            enabled: true,
            api_key: Some("test-key".into()),
            base_url,
            model: "gpt-4.1".into(),
            review_threshold: None,
        };
        let server = ClioServer::new(db_path.clone(), conn, settings, None);

        let checkpoint_server = server.clone();
        let checkpoint = tokio::spawn(async move {
            checkpoint_server
                .memory_session_checkpoint(Parameters(SessionCheckpointParams {
                    text: "No durable changes in this session.".into(),
                    source: "test-session".into(),
                    session_id: "session-1".into(),
                    cursor: 1,
                    namespace: Some("global".into()),
                    cwd: None,
                    branch: None,
                    ticket: None,
                    clio_namespace: None,
                }))
                .await
        });

        started.wait();
        let namespace_result =
            tokio::time::timeout(Duration::from_millis(500), server.memory_namespaces()).await;
        release.wait();
        checkpoint.await.unwrap().unwrap();
        endpoint.join().unwrap();

        assert!(
            namespace_result.is_ok(),
            "checkpoint provider work held the SQLite mutex and blocked an unrelated read"
        );

        drop(server);
        std::fs::remove_file(db_path).unwrap();
    }
}
