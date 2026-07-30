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

    /// Maximum inferred links a memory may hold in total, across all passes — not a
    /// fresh allowance each pass. Note this bounds links *out of* a memory; because
    /// recall walks edges in both directions, a memory that many others point at can
    /// exceed this in total degree.
    #[serde(default = "default_auto_link_max")]
    pub max_links_per_memory: u32,

    /// Memories to process per pass.
    #[serde(default = "default_auto_link_batch")]
    pub batch_size: u32,

    /// Memory kinds excluded from auto-linking, as both source and target.
    ///
    /// Defaults to `["receipt"]`. Receipts are per-session write-ups of what was
    /// done; they share a great deal of boilerplate phrasing, so they attract each
    /// other strongly on similarity while carrying little conceptual content.
    /// Measured on live data at threshold 0.6, receipts averaged 4.86 links each
    /// against 2.03 for `fact` — the most substantive kind was the least connected,
    /// and receipts consumed roughly a third of all link mass. Excluding them keeps
    /// the graph about ideas rather than about sessions.
    #[serde(default = "default_auto_link_exclude_kinds")]
    pub exclude_kinds: Vec<String>,
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

fn default_auto_link_exclude_kinds() -> Vec<String> {
    vec!["receipt".to_string()]
}

impl Default for AutoLinkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: default_auto_link_threshold(),
            interval_secs: default_auto_link_interval(),
            max_links_per_memory: default_auto_link_max(),
            batch_size: default_auto_link_batch(),
            exclude_kinds: default_auto_link_exclude_kinds(),
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

/// Non-secret capture settings safe to display in user interfaces.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CapturePreferences {
    pub enabled: bool,
    pub model: String,
    pub review_threshold: Option<f64>,
}

impl From<&CaptureConfig> for CapturePreferences {
    fn from(config: &CaptureConfig) -> Self {
        Self {
            enabled: config.enabled,
            model: config.model.clone(),
            review_threshold: config.review_threshold,
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

/// Attention lifecycle policy values.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttentionConfig {
    /// Days an open attention item may sit untouched before eligibility
    /// reports it as dormant. `0` disables dormancy surfacing.
    #[serde(default = "default_dormant_days")]
    pub dormant_days: u32,
}

fn default_dormant_days() -> u32 {
    14
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            dormant_days: default_dormant_days(),
        }
    }
}

/// Configuration for AI-powered automatic title generation.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
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

/// Shared Atlas connection used by local adapters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RemoteConfig {
    /// SSH host alias.
    pub host: String,

    /// Database path on the remote host.
    pub db_path: String,

    /// MCP server binary on the remote host.
    pub mcp_binary: String,

    /// CLI binary on the remote host.
    pub cli_binary: String,

    /// Local Clio binary used as the MCP SSH bridge.
    pub bridge_command: String,
}

impl RemoteConfig {
    /// Validate a route before any adapter starts SSH.
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("host", self.host.as_str()),
            ("db_path", self.db_path.as_str()),
            ("mcp_binary", self.mcp_binary.as_str()),
            ("cli_binary", self.cli_binary.as_str()),
            ("bridge_command", self.bridge_command.as_str()),
        ] {
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                return Err(ClioError::Config(format!(
                    "remote {name} must be non-empty and contain no control characters"
                )));
            }
        }
        if self.host.chars().any(char::is_whitespace) {
            return Err(ClioError::Config(
                "remote host must not contain whitespace".into(),
            ));
        }
        for (name, value) in [
            ("db_path", self.db_path.as_str()),
            ("mcp_binary", self.mcp_binary.as_str()),
            ("cli_binary", self.cli_binary.as_str()),
            ("bridge_command", self.bridge_command.as_str()),
        ] {
            if !Path::new(value).is_absolute() {
                return Err(ClioError::Config(format!(
                    "remote {name} must be an absolute path"
                )));
            }
        }
        Ok(())
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

    /// Attention lifecycle policy.
    #[serde(default)]
    pub attention: AttentionConfig,

    /// Optional shared Atlas route for CLI, hooks and Tauri.
    #[serde(default)]
    pub remote: Option<RemoteConfig>,
}

fn default_true() -> bool {
    true
}

/// Environment variable holding a Clio-specific provider key. Preferred over the
/// shared key so Clio's spend lands on its own key and can be attributed in
/// provider billing without every other tool on the machine sharing one key.
pub const CLIO_API_KEY_ENV: &str = "OPENAI_API_KEY_CLIO";

/// The shared provider key. Still honoured, so existing installs keep working,
/// but falling back to it is warned about: a key shared across tools makes
/// per-application cost attribution impossible, and the reuse is otherwise
/// invisible.
pub const SHARED_API_KEY_ENV: &str = "OPENAI_API_KEY";

/// Which environment variable supplied a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvKeySource {
    /// From `OPENAI_API_KEY_CLIO` — the preferred, attributable key.
    Clio(String),
    /// From `OPENAI_API_KEY` — shared with other tools on the machine.
    Shared(String),
}

impl EnvKeySource {
    /// The key itself, regardless of which variable held it.
    pub fn into_key(self) -> String {
        match self {
            Self::Clio(key) | Self::Shared(key) => key,
        }
    }
}

/// Choose between a Clio-specific and a shared key, preferring the former.
///
/// Kept free of environment access so the precedence rule is testable: the
/// process environment is global, and mutating it inside Rust's parallel test
/// runner makes results depend on test ordering.
fn pick_env_key(clio: Option<String>, shared: Option<String>) -> Option<EnvKeySource> {
    let non_empty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    match (non_empty(clio), non_empty(shared)) {
        (Some(key), _) => Some(EnvKeySource::Clio(key)),
        (None, Some(key)) => Some(EnvKeySource::Shared(key)),
        (None, None) => None,
    }
}

/// Read a provider key from the environment, preferring `OPENAI_API_KEY_CLIO`.
///
/// `purpose` names the caller (for example "capture") so the warning about
/// falling back to the shared key says which part of Clio it applies to.
pub fn api_key_from_env(purpose: &str) -> Option<String> {
    let source = pick_env_key(
        std::env::var(CLIO_API_KEY_ENV).ok(),
        std::env::var(SHARED_API_KEY_ENV).ok(),
    )?;
    if matches!(source, EnvKeySource::Shared(_)) {
        tracing::warn!(
            "{purpose}: using the shared {SHARED_API_KEY_ENV}, which other tools \
             also use — provider billing cannot attribute this spend to Clio. \
             Set {CLIO_API_KEY_ENV} to a Clio-only key."
        );
    }
    Some(source.into_key())
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
            attention: AttentionConfig::default(),
            remote: None,
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
            .or_else(|| api_key_from_env("auto-title"))
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

/// Return capture settings that are safe to expose without leaking credentials.
pub fn capture_preferences(db_path: &Path) -> Result<CapturePreferences> {
    Ok(CapturePreferences::from(&load(db_path)?.capture))
}

/// Normalise and validate an OpenAI-compatible capture model ID.
pub fn normalise_capture_model(model: &str) -> Result<String> {
    let model = model.trim();
    if model.is_empty() {
        return Err(ClioError::Validation(
            "capture model cannot be empty".into(),
        ));
    }
    if model.len() > 200 || model.chars().any(char::is_control) {
        return Err(ClioError::Validation(
            "capture model must be at most 200 characters and contain no control characters".into(),
        ));
    }
    Ok(model.to_string())
}

/// Change only the capture model, preserving credentials and other settings.
pub fn set_capture_model(db_path: &Path, model: &str) -> Result<CapturePreferences> {
    let mut settings = load(db_path)?;
    settings.capture.model = normalise_capture_model(model)?;
    save(db_path, &settings)?;
    Ok(CapturePreferences::from(&settings.capture))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_default_to_local_mode() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.remote.is_none());
    }

    #[test]
    fn clio_specific_key_wins_over_the_shared_one() {
        assert_eq!(
            pick_env_key(Some("clio-key".into()), Some("shared-key".into())),
            Some(EnvKeySource::Clio("clio-key".into())),
        );
    }

    #[test]
    fn shared_key_is_used_but_flagged_as_shared() {
        assert_eq!(
            pick_env_key(None, Some("shared-key".into())),
            Some(EnvKeySource::Shared("shared-key".into())),
            "existing installs must keep working, but the source is distinguishable \
             so the caller can warn",
        );
    }

    #[test]
    fn blank_keys_are_treated_as_absent() {
        // A variable exported as "" is a common shell accident; taking it as a key
        // sends an unauthenticated request instead of falling through.
        assert_eq!(
            pick_env_key(Some("   ".into()), Some("shared-key".into())),
            Some(EnvKeySource::Shared("shared-key".into())),
        );
        assert_eq!(pick_env_key(Some(String::new()), None), None);
        assert_eq!(pick_env_key(None, None), None);
    }

    #[test]
    fn remote_settings_round_trip() {
        let settings = Settings {
            remote: Some(RemoteConfig {
                host: "atlas".into(),
                db_path: "/srv/clio/memory.db".into(),
                mcp_binary: "/srv/clio/clio-mcp".into(),
                cli_binary: "/srv/clio/clio".into(),
                bridge_command: "/Users/example/.cargo/bin/clio".into(),
            }),
            ..Default::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: Settings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.remote, settings.remote);
        decoded.remote.unwrap().validate().unwrap();
    }

    #[test]
    fn remote_settings_reject_relative_paths() {
        let remote = RemoteConfig {
            host: "atlas".into(),
            db_path: "memory.db".into(),
            mcp_binary: "/srv/clio/clio-mcp".into(),
            cli_binary: "/srv/clio/clio".into(),
            bridge_command: "/usr/local/bin/clio".into(),
        };
        assert!(remote.validate().is_err());
    }

    #[test]
    fn capture_model_update_preserves_credentials_and_trims_the_model() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("memory.db");
        let mut settings = Settings::default();
        settings.capture.enabled = true;
        settings.capture.api_key = Some("secret".into());
        settings.capture.base_url = "https://example.test/v1".into();
        settings.capture.review_threshold = Some(0.7);
        save(&db_path, &settings).unwrap();

        for model in [
            "gpt-4.1",
            "gpt-5.6-luna",
            " gpt-5.6-terra ",
            "compatible-provider/new-model",
        ] {
            let preferences = set_capture_model(&db_path, model).unwrap();
            let saved = load(&db_path).unwrap();

            assert_eq!(preferences.model, model.trim());
            assert_eq!(preferences.review_threshold, Some(0.7));
            assert_eq!(saved.capture.api_key.as_deref(), Some("secret"));
            assert_eq!(saved.capture.base_url, "https://example.test/v1");
            assert_eq!(saved.auto_title_model(), model.trim());
        }
        assert!(set_capture_model(&db_path, "  ").is_err());
        assert!(set_capture_model(&db_path, "bad\nmodel").is_err());
    }
}
