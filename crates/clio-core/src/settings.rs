//! Clio settings management.
//!
//! Settings are stored as a JSON file alongside the database. They control
//! embedding backend selection and other configurable behaviour.

use std::path::{Path, PathBuf};

use crate::daemon::DaemonConfig;
use crate::embeddings::EmbeddingConfig;
use crate::error::{ClioError, Result};

/// Configuration for temporal relevance scoring in recall queries.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScoringConfig {
    /// Exponential decay rate. 0.01 = gentle (75% at 30 days). 0.0 = disabled.
    #[serde(default = "default_decay_lambda")]
    pub decay_lambda: f64,

    /// Weight for access frequency boost. 0.0 = disabled.
    #[serde(default = "default_access_boost")]
    pub access_boost_weight: f64,
}

fn default_decay_lambda() -> f64 {
    0.01
}

fn default_access_boost() -> f64 {
    0.1
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            decay_lambda: default_decay_lambda(),
            access_boost_weight: default_access_boost(),
        }
    }
}

/// Configuration for automatic link inference in the daemon.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoLinkConfig {
    /// Whether auto-link inference is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Cosine similarity threshold for auto-linking (0.0–1.0).
    #[serde(default = "default_auto_link_threshold")]
    pub threshold: f64,

    /// Seconds between auto-link passes.
    #[serde(default = "default_auto_link_interval")]
    pub interval_secs: u64,

    /// Max links to create per memory per pass.
    #[serde(default = "default_auto_link_max")]
    pub max_links_per_memory: u32,

    /// Memories to process per pass.
    #[serde(default = "default_auto_link_batch")]
    pub batch_size: u32,
}

fn default_auto_link_threshold() -> f64 {
    0.80
}

fn default_auto_link_interval() -> u64 {
    3600
}

fn default_auto_link_max() -> u32 {
    3
}

fn default_auto_link_batch() -> u32 {
    50
}

impl Default for AutoLinkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: default_auto_link_threshold(),
            interval_secs: default_auto_link_interval(),
            max_links_per_memory: default_auto_link_max(),
            batch_size: default_auto_link_batch(),
        }
    }
}

/// Configuration for the LLM-based capture pipeline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaptureConfig {
    /// Whether capture is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// OpenAI API key (or compatible provider key).
    #[serde(default)]
    pub api_key: Option<String>,

    /// Base URL for the API. Default: "https://api.openai.com/v1".
    #[serde(default = "default_capture_base_url")]
    pub base_url: String,

    /// Model to use for classification. Default: "gpt-4o-mini".
    #[serde(default = "default_capture_model")]
    pub model: String,

    /// Optional confidence threshold for the review queue. When set,
    /// captures with a classification confidence below this value are
    /// routed to the review queue instead of being stored directly.
    /// `None` means the review queue is disabled and all captures are
    /// stored immediately.
    #[serde(default)]
    pub review_threshold: Option<f64>,
}

fn default_capture_base_url() -> String {
    "https://api.openai.com/v1".into()
}

fn default_capture_model() -> String {
    "gpt-4o-mini".into()
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            base_url: default_capture_base_url(),
            model: default_capture_model(),
            review_threshold: None,
        }
    }
}

/// Configuration for automatic namespace detection from the working directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextConfig {
    /// Whether to auto-detect namespace from the working directory.
    #[serde(default = "default_true")]
    pub auto_detect: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self { auto_detect: true }
    }
}

/// Configuration for AI-powered automatic title generation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoTitleConfig {
    /// Whether AI title generation is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// API key. Falls back to capture.api_key if None.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Base URL for the API. Falls back to capture.base_url if None.
    #[serde(default)]
    pub base_url: Option<String>,

    /// Model to use. Falls back to capture.model if None.
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for AutoTitleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            base_url: None,
            model: None,
        }
    }
}

/// Configuration for namespace cleanup (stale-namespace detection and purge).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CleanupConfig {
    /// A namespace with no activity for this many months is "stale by age".
    #[serde(default = "default_stale_months")]
    pub stale_months: u32,

    /// Directory roots scanned to decide whether a `project:<slug>` namespace's
    /// folder still exists on disk (the "folder gone" heuristic). `~` is
    /// expanded to the home directory.
    #[serde(default = "default_dev_roots")]
    pub dev_roots: Vec<String>,

    /// Record the working directory in memory metadata at capture time, so
    /// future namespaces can be matched to a real path reliably.
    #[serde(default = "default_true")]
    pub record_cwd: bool,
}

fn default_stale_months() -> u32 {
    6
}

fn default_dev_roots() -> Vec<String> {
    vec![
        "~/Development".into(),
        "~/Projects".into(),
        "~/Code".into(),
        "~/dev".into(),
        "~/src".into(),
    ]
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            stale_months: default_stale_months(),
            dev_roots: default_dev_roots(),
            record_cwd: true,
        }
    }
}

/// Configuration for memory consolidation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConsolidateConfig {
    /// Consolidate a namespace automatically once it has accrued at least this
    /// many new memories since the last consolidation (used by `--if-due`).
    #[serde(default = "default_consolidate_threshold")]
    pub auto_threshold: u32,
}

fn default_consolidate_threshold() -> u32 {
    10
}

impl Default for ConsolidateConfig {
    fn default() -> Self {
        Self {
            auto_threshold: default_consolidate_threshold(),
        }
    }
}

/// All configurable Clio settings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// Embedding configuration.
    #[serde(default)]
    pub embeddings: EmbeddingConfig,

    /// Whether to auto-embed memories on write.
    #[serde(default = "default_true")]
    pub auto_embed: bool,

    /// Capture pipeline configuration.
    #[serde(default)]
    pub capture: CaptureConfig,

    /// Automatic title generation configuration.
    #[serde(default)]
    pub auto_title: AutoTitleConfig,

    /// Context detection configuration.
    #[serde(default)]
    pub context: ContextConfig,

    /// Temporal relevance scoring configuration.
    #[serde(default)]
    pub scoring: ScoringConfig,

    /// Daemon configuration.
    #[serde(default)]
    pub daemon: DaemonConfig,

    /// Namespace cleanup configuration.
    #[serde(default)]
    pub cleanup: CleanupConfig,

    /// Memory consolidation configuration.
    #[serde(default)]
    pub consolidate: ConsolidateConfig,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            embeddings: EmbeddingConfig::default(),
            auto_embed: true,
            capture: CaptureConfig::default(),
            auto_title: AutoTitleConfig::default(),
            context: ContextConfig::default(),
            scoring: ScoringConfig::default(),
            daemon: DaemonConfig::default(),
            cleanup: CleanupConfig::default(),
            consolidate: ConsolidateConfig::default(),
        }
    }
}

impl Settings {
    /// Resolve the API key for auto-title, falling back to capture config.
    pub fn auto_title_api_key(&self) -> Option<String> {
        self.auto_title
            .api_key
            .clone()
            .or_else(|| self.capture.api_key.clone())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
    }

    /// Resolve the base URL for auto-title, falling back to capture config.
    pub fn auto_title_base_url(&self) -> String {
        self.auto_title
            .base_url
            .clone()
            .unwrap_or_else(|| self.capture.base_url.clone())
    }

    /// Resolve the model for auto-title, falling back to capture config.
    pub fn auto_title_model(&self) -> String {
        self.auto_title
            .model
            .clone()
            .unwrap_or_else(|| self.capture.model.clone())
    }
}

/// Derive the settings file path from a database path.
/// If DB is at `/path/to/memory.db`, settings are at `/path/to/clio-settings.json`.
pub fn settings_path(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("clio-settings.json")
}

/// Load settings from the file next to the database. Returns defaults if
/// the file doesn't exist.
pub fn load(db_path: &Path) -> Result<Settings> {
    let path = settings_path(db_path);

    // Open directly, handle NotFound → return defaults.
    // This avoids a TOCTOU race between exists() and read().
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "settings file not found, using defaults");
            return Ok(Settings::default());
        }
        Err(e) => {
            return Err(ClioError::Config(format!(
                "could not read settings from {}: {e}",
                path.display()
            )));
        }
    };

    let settings: Settings = serde_json::from_str(&content)
        .map_err(|e| ClioError::Config(format!("invalid settings in {}: {e}", path.display())))?;

    tracing::debug!(path = %path.display(), "loaded settings");
    Ok(settings)
}

/// Save settings to the file next to the database.
///
/// Uses atomic write (temp file + rename) to prevent corruption on crash.
/// On Unix, restricts file permissions to owner-only (0o600) to protect
/// plaintext API keys.
pub fn save(db_path: &Path, settings: &Settings) -> Result<()> {
    let path = settings_path(db_path);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ClioError::Config(format!(
                "could not create directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    let content = serde_json::to_string_pretty(settings)?;

    // Atomic write: write to a temp file in the same directory, then rename.
    let parent = path.parent().unwrap_or(Path::new("."));
    let tmp_path = parent.join(".clio-settings.tmp");
    std::fs::write(&tmp_path, &content).map_err(|e| {
        ClioError::Config(format!(
            "could not write temp settings to {}: {e}",
            tmp_path.display()
        ))
    })?;

    // Set restrictive permissions before rename (Unix only).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        if let Err(e) = std::fs::set_permissions(&tmp_path, perms) {
            tracing::warn!("could not set permissions on settings file: {e}");
        }
    }

    std::fs::rename(&tmp_path, &path).map_err(|e| {
        ClioError::Config(format!(
            "could not rename temp settings to {}: {e}",
            path.display()
        ))
    })?;

    tracing::debug!(path = %path.display(), "saved settings");
    Ok(())
}
