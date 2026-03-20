use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A stored memory record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub namespace: String,
    pub kind: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub content: String,
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub source_ref: Option<String>,
    pub confidence: Option<f64>,
    pub importance: i32,
    pub metadata: serde_json::Value,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: Option<String>,
    pub access_count: i32,
}

/// Input for creating or upserting a memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RememberInput {
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub source_ref: Option<String>,
    pub confidence: Option<f64>,
    #[serde(default = "default_importance")]
    pub importance: i32,
    #[serde(default = "default_metadata")]
    pub metadata: serde_json::Value,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    #[serde(default)]
    pub upsert: bool,
}

fn default_namespace() -> String {
    "global".into()
}

fn default_kind() -> String {
    "note".into()
}

fn default_importance() -> i32 {
    3
}

fn default_metadata() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Sort order for recall queries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Most recently updated first (default).
    #[default]
    UpdatedDesc,
    /// Oldest first.
    UpdatedAsc,
    /// Most important first.
    ImportanceDesc,
    /// Least important first.
    ImportanceAsc,
    /// Newest created first.
    CreatedDesc,
    /// Oldest created first.
    CreatedAsc,
}

impl SortOrder {
    /// Parse from a string, returning None for unrecognised values.
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "updated_desc" | "updated-desc" => Some(Self::UpdatedDesc),
            "updated_asc" | "updated-asc" => Some(Self::UpdatedAsc),
            "importance_desc" | "importance-desc" => Some(Self::ImportanceDesc),
            "importance_asc" | "importance-asc" => Some(Self::ImportanceAsc),
            "created_desc" | "created-desc" => Some(Self::CreatedDesc),
            "created_asc" | "created-asc" => Some(Self::CreatedAsc),
            _ => None,
        }
    }

    /// SQL ORDER BY clause fragment (without the ORDER BY keyword).
    pub fn sql_fragment(&self) -> &'static str {
        match self {
            Self::UpdatedDesc => "m.updated_at DESC",
            Self::UpdatedAsc => "m.updated_at ASC",
            Self::ImportanceDesc => "m.importance DESC, m.created_at DESC",
            Self::ImportanceAsc => "m.importance ASC, m.created_at DESC",
            Self::CreatedDesc => "m.created_at DESC",
            Self::CreatedAsc => "m.created_at ASC",
        }
    }
}

/// Query parameters for recalling memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallQuery {
    pub query: Option<String>,
    pub namespace: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_true")]
    pub match_all_tags: bool,
    #[serde(default)]
    pub include_archived: bool,
    /// When true, append linked memories to results.
    #[serde(default)]
    pub include_links: bool,
    /// Minimum importance (1–5 inclusive). None means no lower bound.
    #[serde(default)]
    pub importance_min: Option<i32>,
    /// Maximum importance (1–5 inclusive). None means no upper bound.
    #[serde(default)]
    pub importance_max: Option<i32>,
    /// Sort order. Only applied when no FTS query and no scoring config.
    #[serde(default)]
    pub sort_by: Option<SortOrder>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    /// Temporal relevance scoring config. Set by callers with access to settings;
    /// not exposed via MCP parameters.
    #[serde(skip)]
    pub scoring: Option<crate::settings::ScoringConfig>,
}

fn default_true() -> bool {
    true
}

fn default_limit() -> u32 {
    10
}

impl Default for RecallQuery {
    fn default() -> Self {
        Self {
            query: None,
            namespace: None,
            kind: None,
            tags: Vec::new(),
            match_all_tags: true,
            include_archived: false,
            include_links: false,
            importance_min: None,
            importance_max: None,
            sort_by: None,
            limit: 10,
            offset: 0,
            scoring: None,
        }
    }
}

/// A single result from a recall operation, with optional rank.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallItem {
    #[serde(flatten)]
    pub memory: Memory,
    pub rank: Option<f64>,
    /// When present, this memory was included because it is linked from the
    /// memory with this ID (graph-aware recall).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_from: Option<String>,
}

/// Paginated recall result envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    pub total: u32,
    pub count: u32,
    pub offset: u32,
    pub limit: u32,
    pub items: Vec<RecallItem>,
}

/// Input for creating a link between memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInput {
    pub from_memory_id: String,
    pub to_memory_id: String,
    #[serde(default = "default_relationship")]
    pub relationship: String,
    #[serde(default = "default_metadata")]
    pub metadata: serde_json::Value,
}

fn default_relationship() -> String {
    "relates_to".into()
}

/// A stored link between two memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLink {
    pub from_memory_id: String,
    pub to_memory_id: String,
    pub relationship: String,
    pub metadata: serde_json::Value,
    pub created_at: String,
}

/// Aggregated statistics about stored memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_memories: u32,
    pub active_memories: u32,
    pub archived_memories: u32,
    pub total_embeddings: u32,
    pub embedding_coverage: f64,
    pub by_namespace: Vec<(String, u32)>,
    pub by_kind: Vec<(String, u32)>,
    pub by_week: Vec<(String, u32)>,
    pub top_tags: Vec<(String, u32)>,
    pub total_links: u32,
    pub link_density: f64,
}

/// A weekly summary for the timeline view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeekSummary {
    pub week: String,
    pub count: u32,
}

/// A single entry in the recent activity feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    pub memory_id: String,
    pub title: Option<String>,
    pub namespace: String,
    pub kind: String,
    pub action: String,
    pub timestamp: String,
}

/// Namespace information with memory count and last activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceInfo {
    pub name: String,
    pub memory_count: u32,
    pub last_activity: Option<String>,
}

/// Generate a new UUIDv7 string.
pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}

/// Get the current UTC timestamp as ISO-8601.
pub fn now_utc() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .expect("formatting UTC timestamp should never fail")
}
