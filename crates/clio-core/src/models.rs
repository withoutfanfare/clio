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

/// Fields to change on an existing memory.
///
/// Outer `None` means leave the field unchanged. For nullable fields,
/// `Some(None)` clears the stored value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nullable_patch"
    )]
    pub title: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nullable_patch"
    )]
    pub summary: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nullable_patch"
    )]
    pub source: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nullable_patch"
    )]
    pub source_ref: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nullable_patch"
    )]
    pub confidence: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nullable_patch"
    )]
    pub valid_from: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_nullable_patch"
    )]
    pub valid_until: Option<Option<String>>,
    /// Reject the update if the record changed after this timestamp was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_updated_at: Option<String>,
}

fn deserialize_nullable_patch<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
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
    /// Return only archived records; takes precedence over include_archived.
    #[serde(default)]
    pub archived_only: bool,
    /// When true, append linked memories to results.
    #[serde(default)]
    pub include_links: bool,
    /// When true, exclude memories whose `valid_until` is in the past.
    /// Defaults to false for backwards-compatible recall.
    #[serde(default)]
    pub exclude_expired: bool,
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
    /// When true, this recall does not touch `access_count`/`last_accessed_at`.
    /// Internal-only: automatic surfacing (resume briefs) must not train its
    /// own ranking. Deliberate recall stays tracked.
    #[serde(skip)]
    pub skip_access_tracking: bool,
    /// When true, a multi-word `query` matches memories containing *any* of
    /// its terms, ranked by BM25, instead of requiring every term. Internal-only:
    /// resume briefs use it for natural-language prompts, where requiring every
    /// word matches nothing.
    #[serde(skip)]
    pub match_any_term: bool,
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
            archived_only: false,
            include_links: false,
            exclude_expired: false,
            importance_min: None,
            importance_max: None,
            sort_by: None,
            limit: 10,
            offset: 0,
            scoring: None,
            skip_access_tracking: false,
            match_any_term: false,
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
    /// Every edge that connected this linked memory to a direct result,
    /// with direction, relationship and metadata preserved. Empty for
    /// direct (non-linked) results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link_context: Vec<LinkContext>,
}

/// One graph edge described relative to a recall anchor: the raw edge plus
/// which way it points from the anchor's perspective.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkContext {
    pub from_memory_id: String,
    pub to_memory_id: String,
    /// `outgoing` (anchor → linked) or `incoming` (linked → anchor).
    pub direction: String,
    pub relationship: String,
    pub metadata: serde_json::Value,
    pub created_at: String,
}

/// Paginated recall result envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    /// Confirms that the backend applied an archived-only query.
    #[serde(default)]
    pub archived_only: bool,
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
    /// Applied collection scope; absent on older backends.
    #[serde(default)]
    pub namespace: Option<String>,
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
