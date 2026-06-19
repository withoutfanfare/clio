//! Database backup and restore.
//!
//! Provides timestamped SQLite backup with configurable retention and
//! integrity-checked restore.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{ClioError, Result};

/// Result of a backup operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupResult {
    pub path: String,
    pub size_bytes: u64,
    pub timestamp: String,
}

/// Result of a restore operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RestoreResult {
    pub restored_from: String,
    pub integrity_ok: bool,
}

/// List of available backups.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupListEntry {
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
    pub created: String,
}

/// Create a timestamped backup of the SQLite database.
///
/// The backup is placed alongside the database file (or in a custom directory).
/// Retention: keeps only the last `max_backups` copies, deleting older ones.
pub fn backup(db_path: &Path, dest_dir: Option<&Path>, max_backups: u32) -> Result<BackupResult> {
    let now = time::OffsetDateTime::now_utc();
    let timestamp = now
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".into());

    let ts_file = now
        .format(
            &time::format_description::parse("[year]-[month]-[day]T[hour]-[minute]-[second]")
                .unwrap(),
        )
        .unwrap_or_else(|_| "backup".into());

    let backup_dir = dest_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            db_path
                .parent()
                .unwrap_or(Path::new("."))
                .join("backups")
        });

    std::fs::create_dir_all(&backup_dir).map_err(|e| {
        ClioError::Export(format!(
            "could not create backup directory {}: {e}",
            backup_dir.display()
        ))
    })?;

    let backup_filename = format!("clio-backup-{ts_file}.db");
    let backup_path = backup_dir.join(&backup_filename);

    // Produce a transactionally-consistent, standalone snapshot. VACUUM INTO writes
    // a complete database with no -wal/-shm sidecars, avoiding torn-copy corruption
    // that plain file copies risk under WAL mode.
    let _ = std::fs::remove_file(&backup_path); // VACUUM INTO requires the target not exist
    let src = rusqlite::Connection::open(db_path).map_err(|e| {
        ClioError::Export(format!("could not open database for backup: {e}"))
    })?;
    src.busy_timeout(Duration::from_millis(5000))
        .map_err(|e| ClioError::Export(format!("could not set busy timeout for backup: {e}")))?;
    src.execute(
        "VACUUM INTO ?1",
        rusqlite::params![backup_path.to_string_lossy()],
    )
    .map_err(|e| ClioError::Export(format!("backup VACUUM INTO failed: {e}")))?;

    let size_bytes = std::fs::metadata(&backup_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Enforce retention: delete oldest backups if exceeding max
    enforce_retention(&backup_dir, max_backups)?;

    Ok(BackupResult {
        path: backup_path.to_string_lossy().to_string(),
        size_bytes,
        timestamp,
    })
}

/// List all available backups, sorted by creation time (newest first).
pub fn list_backups(db_path: &Path, backup_dir: Option<&Path>) -> Result<Vec<BackupListEntry>> {
    let dir = backup_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            db_path
                .parent()
                .unwrap_or(Path::new("."))
                .join("backups")
        });

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<BackupListEntry> = Vec::new();

    for entry in std::fs::read_dir(&dir).map_err(|e| {
        ClioError::Export(format!("could not read backup directory: {e}"))
    })? {
        let entry = entry.map_err(|e| ClioError::Export(format!("directory entry error: {e}")))?;
        let path = entry.path();

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("clio-backup-") && name.ends_with(".db") {
                let meta = std::fs::metadata(&path)
                    .map_err(|e| ClioError::Export(format!("metadata error: {e}")))?;
                let created = meta
                    .created()
                    .or_else(|_| meta.modified())
                    .map(|t| {
                        let dt: time::OffsetDateTime = t.into();
                        dt.format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_else(|_| "unknown".into())
                    })
                    .unwrap_or_else(|_| "unknown".into());

                entries.push(BackupListEntry {
                    path: path.to_string_lossy().to_string(),
                    filename: name.to_string(),
                    size_bytes: meta.len(),
                    created,
                });
            }
        }
    }

    // Sort newest first
    entries.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(entries)
}

/// Restore the database from a backup file.
///
/// Validates the backup integrity before replacing the current database.
pub fn restore(db_path: &Path, backup_path: &Path) -> Result<RestoreResult> {
    if !backup_path.exists() {
        return Err(ClioError::Import(format!(
            "backup file does not exist: {}",
            backup_path.display()
        )));
    }

    // Validate backup integrity
    let integrity_ok = check_integrity(backup_path)?;
    if !integrity_ok {
        return Err(ClioError::Import(
            "backup file failed integrity check — aborting restore".into(),
        ));
    }

    // Safety: snapshot the current live DB before overwriting, so a bad restore is
    // recoverable. Best-effort — failure here must not block a valid restore.
    if db_path.exists() {
        let safety = db_path.with_extension("db.pre-restore");
        let _ = std::fs::remove_file(&safety);
        match rusqlite::Connection::open(db_path) {
            Err(e) => {
                tracing::warn!(
                    "could not open live database for pre-restore safety snapshot: {e}"
                );
            }
            Ok(live) => {
                let _ = live.busy_timeout(Duration::from_millis(5000));
                if let Err(e) = live.execute(
                    "VACUUM INTO ?1",
                    rusqlite::params![safety.to_string_lossy()],
                ) {
                    tracing::warn!(
                        "pre-restore safety snapshot was not written (VACUUM INTO failed): {e}"
                    );
                }
            }
        }
    }

    // Replace the current database
    std::fs::copy(backup_path, db_path).map_err(|e| {
        ClioError::Import(format!(
            "could not restore database from {}: {e}",
            backup_path.display()
        ))
    })?;

    // Also restore WAL and SHM files if they exist
    let backup_wal = backup_path.with_extension("db-wal");
    let backup_shm = backup_path.with_extension("db-shm");
    if backup_wal.exists() {
        let _ = std::fs::copy(&backup_wal, db_path.with_extension("db-wal"));
    } else {
        // Remove stale WAL/SHM from previous database
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    }
    if backup_shm.exists() {
        let _ = std::fs::copy(&backup_shm, db_path.with_extension("db-shm"));
    } else {
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    Ok(RestoreResult {
        restored_from: backup_path.to_string_lossy().to_string(),
        integrity_ok,
    })
}

/// Check SQLite integrity of a database file.
fn check_integrity(path: &Path) -> Result<bool> {
    let conn = rusqlite::Connection::open(path).map_err(|e| {
        ClioError::Import(format!("could not open backup for integrity check: {e}"))
    })?;
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| ClioError::Import(format!("integrity check failed: {e}")))?;
    Ok(result == "ok")
}

/// Delete oldest backups to keep only `max_backups` copies.
fn enforce_retention(backup_dir: &Path, max_backups: u32) -> Result<()> {
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

    for entry in std::fs::read_dir(backup_dir).map_err(|e| {
        ClioError::Export(format!("could not read backup directory: {e}"))
    })? {
        let entry = entry.map_err(|e| ClioError::Export(format!("directory entry error: {e}")))?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("clio-backup-") && name.ends_with(".db") {
                let modified = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                files.push((path, modified));
            }
        }
    }

    // Sort oldest first
    files.sort_by_key(|(_, t)| *t);

    // Delete excess
    while files.len() > max_backups as usize {
        if let Some((path, _)) = files.first() {
            let _ = std::fs::remove_file(path);
            // Also remove associated WAL/SHM
            let _ = std::fs::remove_file(path.with_extension("db-wal"));
            let _ = std::fs::remove_file(path.with_extension("db-shm"));
        }
        files.remove(0);
    }

    Ok(())
}
