//! Daemon configuration, lifecycle, and health types.
//!
//! All daemon-related business logic and types live here. The actual daemon
//! runtime lives in the `clio-daemon` crate; this module provides the shared
//! contract.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ClioError, Result};
use crate::settings::{AutoLinkConfig, Settings};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the always-on daemon, stored in `clio-settings.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Whether the daemon is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Directories to watch for inbox drop files.
    #[serde(default)]
    pub inbox_paths: Vec<PathBuf>,

    /// Path to the Unix domain socket. Platform default if `None`.
    #[serde(default)]
    pub socket_path: Option<PathBuf>,

    /// Directory for daemon log files. Platform default if `None`.
    #[serde(default)]
    pub log_dir: Option<PathBuf>,

    /// Reserved compatibility field. The daemon does not currently expose an
    /// HTTP listener.
    #[serde(default)]
    pub http_port: Option<u16>,

    /// Automatic link inference configuration.
    #[serde(default)]
    pub auto_link: AutoLinkConfig,

    /// Periodic maintenance jobs (backup, integrity check).
    #[serde(default)]
    pub maintenance: MaintenanceConfig,
}

/// Configuration for periodic daemon maintenance jobs. All intervals default to
/// `0`, meaning the job is disabled — nothing runs on a timer unless opted in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceConfig {
    /// Seconds between database backups. `0` disables (default).
    #[serde(default)]
    pub backup_interval_secs: u64,

    /// How many timestamped backups to retain.
    #[serde(default = "default_max_backups")]
    pub backup_max_backups: u32,

    /// Seconds between integrity checks (log-only). `0` disables (default).
    #[serde(default)]
    pub integrity_interval_secs: u64,
}

fn default_max_backups() -> u32 {
    7
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            backup_interval_secs: 0,
            backup_max_backups: default_max_backups(),
            integrity_interval_secs: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Health types
// ---------------------------------------------------------------------------

/// Overall health status of a daemon subsystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Subsystem is operating normally.
    Healthy,
    /// Subsystem is operational but experiencing issues.
    Degraded,
    /// Subsystem is not operational.
    Unhealthy,
    /// Subsystem is not configured.
    Unconfigured,
}

/// A single health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub status: HealthStatus,
    pub message: String,
}

/// Aggregated health of all daemon subsystems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonHealth {
    pub database: HealthCheck,
    pub embeddings: HealthCheck,
    pub capture: HealthCheck,
}

/// Snapshot of the running daemon's status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
    pub db_path: String,
    pub enabled_routes: Vec<String>,
    pub health: DaemonHealth,
    pub started_at: Option<String>,
}

// ---------------------------------------------------------------------------
// PID file management
// ---------------------------------------------------------------------------

/// Manages a PID file for daemon singleton locking.
#[derive(Debug, Clone)]
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    /// Create a new `PidFile` handle at the given path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Write the given PID to the file.
    ///
    /// Fails if a PID file already exists and the recorded process is still
    /// running (prevents duplicate daemon instances).
    pub fn create(&self, pid: u32) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ClioError::Config(format!(
                    "could not create PID file directory {}: {e}",
                    parent.display()
                ))
            })?;
        }

        // Atomic create-or-fail to avoid TOCTOU race between read and write.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                use std::io::Write;
                write!(file, "{pid}").map_err(|e| {
                    ClioError::Config(format!(
                        "could not write PID file {}: {e}",
                        self.path.display()
                    ))
                })?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // File exists — check if the recorded process is still alive.
                if let Some(existing) = self.read()? {
                    if process_is_running(existing) {
                        return Err(ClioError::Config(format!(
                            "daemon already running with PID {existing} (pidfile: {})",
                            self.path.display()
                        )));
                    }
                }
                // Stale PID file — overwrite.
                tracing::debug!("removing stale PID file");
                std::fs::write(&self.path, pid.to_string()).map_err(|e| {
                    ClioError::Config(format!(
                        "could not write PID file {}: {e}",
                        self.path.display()
                    ))
                })?;
            }
            Err(e) => {
                return Err(ClioError::Config(format!(
                    "could not create PID file {}: {e}",
                    self.path.display()
                )));
            }
        }

        tracing::debug!(pid, path = %self.path.display(), "created PID file");
        Ok(())
    }

    /// Read the PID from the file, returning `None` if the file does not exist.
    pub fn read(&self) -> Result<Option<u32>> {
        if !self.path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&self.path).map_err(|e| {
            ClioError::Config(format!(
                "could not read PID file {}: {e}",
                self.path.display()
            ))
        })?;

        let pid: u32 = content.trim().parse().map_err(|e| {
            ClioError::Config(format!("invalid PID in {}: {e}", self.path.display()))
        })?;

        Ok(Some(pid))
    }

    /// Remove the PID file.
    pub fn remove(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path).map_err(|e| {
                ClioError::Config(format!(
                    "could not remove PID file {}: {e}",
                    self.path.display()
                ))
            })?;
            tracing::debug!(path = %self.path.display(), "removed PID file");
        }
        Ok(())
    }

    /// Check whether the PID recorded in the file corresponds to a running
    /// process.
    pub fn is_running(&self) -> bool {
        match self.read() {
            Ok(Some(pid)) => process_is_running(pid),
            _ => false,
        }
    }
}

/// Check whether a process with the given PID is alive by sending signal 0.
fn process_is_running(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Platform default paths
// ---------------------------------------------------------------------------

/// Return the platform-default path for the Unix domain socket.
pub fn default_socket_path() -> Result<PathBuf> {
    daemon_runtime_dir().map(|d| d.join("clio.sock"))
}

/// Return the platform-default path for the PID file.
pub fn default_pid_path() -> Result<PathBuf> {
    daemon_runtime_dir().map(|d| d.join("clio.pid"))
}

/// Return the platform-default directory for daemon log files.
pub fn default_log_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs_home()?;
        Ok(home.join("Library").join("Logs").join("clio"))
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(state) = std::env::var("XDG_STATE_HOME") {
            if !state.is_empty() {
                return Ok(PathBuf::from(state).join("clio").join("logs"));
            }
        }
        let home = dirs_home()?;
        Ok(home.join(".local").join("state").join("clio").join("logs"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(ClioError::Config(
            "unsupported platform for daemon log directory".into(),
        ))
    }
}

/// Return the runtime directory where socket and PID files live.
fn daemon_runtime_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs_home()?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("clio"))
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
            if !runtime.is_empty() {
                return Ok(PathBuf::from(runtime).join("clio"));
            }
        }
        Ok(PathBuf::from("/tmp").join("clio"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(ClioError::Config(
            "unsupported platform for daemon runtime directory".into(),
        ))
    }
}

fn dirs_home() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| ClioError::Config("could not determine home directory".into()))
}

// ---------------------------------------------------------------------------
// Health checks
// ---------------------------------------------------------------------------

/// Check database connectivity and SQLite file integrity.
pub fn check_database_health(db_path: &Path) -> HealthCheck {
    if !db_path.exists() {
        return HealthCheck {
            status: HealthStatus::Unhealthy,
            message: format!("database file not found: {}", db_path.display()),
        };
    }

    match rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    {
        Ok(conn) => {
            match conn.query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0)) {
                Ok(result) if result != "ok" => HealthCheck {
                    status: HealthStatus::Unhealthy,
                    message: format!("SQLite quick check failed: {result}"),
                },
                Ok(_) => {
                    match conn
                        .query_row("SELECT count(*) FROM memories", [], |r| r.get::<_, i64>(0))
                    {
                        Ok(count) => HealthCheck {
                            status: HealthStatus::Healthy,
                            message: format!(
                                "SQLite quick check passed; {count} memories in database"
                            ),
                        },
                        Err(e) => HealthCheck {
                            status: HealthStatus::Degraded,
                            message: format!("query failed: {e}"),
                        },
                    }
                }
                Err(e) => HealthCheck {
                    status: HealthStatus::Unhealthy,
                    message: format!("SQLite quick check failed: {e}"),
                },
            }
        }
        Err(e) => HealthCheck {
            status: HealthStatus::Unhealthy,
            message: format!("could not open database: {e}"),
        },
    }
}

/// Check whether the embedding backend is configured and usable.
pub fn check_embeddings_health(settings: &Settings) -> HealthCheck {
    use crate::embeddings::EmbeddingConfig;

    match &settings.embeddings {
        EmbeddingConfig::Local { .. } => HealthCheck {
            status: HealthStatus::Healthy,
            message: "local embedding backend configured".into(),
        },
        EmbeddingConfig::OpenAi { api_key, .. } => api_key_health(
            api_key.as_deref(),
            crate::settings::env_api_key_present(),
            "OpenAI embedding backend",
        ),
        EmbeddingConfig::Disabled => HealthCheck {
            status: HealthStatus::Unconfigured,
            message: "embeddings are disabled".into(),
        },
    }
}

/// Check whether the capture pipeline is enabled and has the required API key.
pub fn check_capture_health(settings: &Settings) -> HealthCheck {
    if !settings.capture.enabled {
        return HealthCheck {
            status: HealthStatus::Unconfigured,
            message: "capture pipeline is disabled".into(),
        };
    }

    api_key_health(
        settings.capture.api_key.as_deref(),
        crate::settings::env_api_key_present(),
        "capture pipeline",
    )
}

/// Health of a provider API key, resolved the same way the providers
/// themselves resolve one: a configured key first, then the environment.
/// Checking only the configured key reported valid environment-only setups
/// as unhealthy — the check said "missing" while every actual call
/// succeeded, which is exactly the misleading status a health check exists
/// to avoid. `env_present` is passed in rather than read here so the logic
/// stays testable without mutating process-wide environment state.
fn api_key_health(configured: Option<&str>, env_present: bool, what: &str) -> HealthCheck {
    if crate::settings::configured_api_key(configured).is_some() {
        return HealthCheck {
            status: HealthStatus::Healthy,
            message: format!("{what} has a configured API key"),
        };
    }
    if env_present {
        return HealthCheck {
            status: HealthStatus::Healthy,
            message: format!("{what} will use the environment API key"),
        };
    }
    HealthCheck {
        status: HealthStatus::Unhealthy,
        message: format!(
            "{what} has no API key in settings and none in the environment ({} or {})",
            crate::settings::CLIO_API_KEY_ENV,
            crate::settings::SHARED_API_KEY_ENV
        ),
    }
}

/// Run all health checks and return an aggregated result.
pub fn run_health_checks(db_path: &Path, settings: &Settings) -> DaemonHealth {
    DaemonHealth {
        database: check_database_health(db_path),
        embeddings: check_embeddings_health(settings),
        capture: check_capture_health(settings),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_only_key_setup_is_reported_healthy() {
        // The defect this closes: health checks examined only the configured
        // key while the providers themselves fall back to the environment, so
        // a working environment-only setup was reported unhealthy.
        let health = api_key_health(None, true, "capture pipeline");
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.message.contains("environment"), "{}", health.message);
    }

    #[test]
    fn a_blank_configured_key_with_an_environment_key_is_healthy() {
        // Blank configured keys are treated as absent everywhere else; the
        // health check must agree.
        let health = api_key_health(Some("   "), true, "capture pipeline");
        assert_eq!(health.status, HealthStatus::Healthy);
    }

    #[test]
    fn no_key_anywhere_is_unhealthy_and_names_both_channels() {
        let health = api_key_health(None, false, "capture pipeline");
        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert!(
            health.message.contains("OPENAI_API_KEY"),
            "the remedy must name the environment variables: {}",
            health.message
        );
    }

    #[test]
    fn database_health_runs_sqlite_quick_check() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("memory.db");
        crate::db::open(&path).unwrap();

        let health = check_database_health(&path);

        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.message.contains("quick check passed"));
    }
}
