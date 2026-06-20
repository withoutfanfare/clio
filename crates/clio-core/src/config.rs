use std::path::{Path, PathBuf};

use crate::error::{ClioError, Result};

const ENV_VAR: &str = "CLIO_DB_PATH";

/// Resolve the database file path.
///
/// Resolution order:
/// 1. Explicit override (from CLI flag or runtime config)
/// 2. `CLIO_DB_PATH` environment variable
/// 3. Platform-specific default location
pub fn resolve_db_path(explicit: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(PathBuf::from(path));
    }

    if let Ok(path) = std::env::var(ENV_VAR) {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    platform_default()
}

/// Return the platform-specific default database path.
fn platform_default() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs_home()
            .ok_or_else(|| ClioError::Config("could not determine home directory".into()))?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("clio")
            .join("memory.db"))
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return Ok(PathBuf::from(xdg).join("clio").join("memory.db"));
            }
        }
        let home = dirs_home()
            .ok_or_else(|| ClioError::Config("could not determine home directory".into()))?;
        Ok(home
            .join(".local")
            .join("share")
            .join("clio")
            .join("memory.db"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata =
            std::env::var("APPDATA").map_err(|_| ClioError::Config("APPDATA not set".into()))?;
        Ok(PathBuf::from(appdata).join("clio").join("memory.db"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(ClioError::Config(
            "unsupported platform; set CLIO_DB_PATH explicitly".into(),
        ))
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Return the default inbox directory path, sibling to the database.
pub fn default_inbox_dir(db_path: &Path) -> PathBuf {
    db_path.parent().unwrap_or(Path::new(".")).join("inbox")
}

/// Create the default inbox directory and register it in settings if not
/// already configured.
pub fn ensure_inbox_dir(db_path: &Path) -> Result<PathBuf> {
    let inbox = default_inbox_dir(db_path);
    std::fs::create_dir_all(&inbox).map_err(|e| {
        ClioError::Config(format!(
            "could not create inbox directory {}: {e}",
            inbox.display()
        ))
    })?;

    let mut settings = crate::settings::load(db_path)?;
    if settings.daemon.inbox_paths.is_empty() {
        settings.daemon.inbox_paths.push(inbox.clone());
        crate::settings::save(db_path, &settings)?;
    }

    Ok(inbox)
}
