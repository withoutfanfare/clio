use std::path::Path;
use std::sync::Mutex;

use tauri::State;

use clio_core::cleanup::{self, CleanupCandidate, CleanupCriteria, CleanupReport};
use clio_core::consolidate::{self, ConsolidationResult};
use clio_core::context::{self, DetectedContext};
use clio_core::models::NamespaceInfo;

use crate::{AppState, CommandError};

/// Roll a namespace's memories into its AI-curated consolidated memory.
#[tauri::command]
pub fn cmd_consolidate_namespace(
    state: State<'_, Mutex<AppState>>,
    namespace: String,
) -> Result<ConsolidationResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let result =
        consolidate::consolidate(&app.conn, &namespace, &app.settings.capture, &app.settings)?;
    app.cache.clear_all();
    Ok(result)
}

/// Find namespaces matching the cleanup criteria. When no specific criterion is
/// requested, all criteria are applied.
#[tauri::command]
pub fn cmd_find_cleanup_candidates(
    state: State<'_, Mutex<AppState>>,
    stale_months: Option<u32>,
    archived: bool,
    folder_gone: bool,
    all: bool,
) -> Result<Vec<CleanupCandidate>, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

    let any_specific = archived || folder_gone || stale_months.is_some();
    let use_all = all || !any_specific;

    let criteria = CleanupCriteria {
        stale_months: if use_all || stale_months.is_some() {
            Some(stale_months.unwrap_or(app.settings.cleanup.stale_months))
        } else {
            None
        },
        all_archived: use_all || archived,
        folder_gone: use_all || folder_gone,
    };

    let dev_roots = cleanup::expand_dev_roots(&app.settings.cleanup.dev_roots);
    let candidates = cleanup::find_candidates_now(&app.conn, &criteria, &dev_roots)?;
    Ok(candidates)
}

/// Purge the given namespaces (and all their memories), taking a backup first.
/// The caller passes the explicit list the user confirmed.
#[tauri::command]
pub fn cmd_run_cleanup(
    state: State<'_, Mutex<AppState>>,
    namespaces: Vec<String>,
) -> Result<CleanupReport, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let report = cleanup::execute_cleanup(&app.conn, &app.db_path, &namespaces, 10)?;
    app.cache.clear_all();
    Ok(report)
}

#[tauri::command]
pub fn cmd_namespaces(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<String>, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let namespaces = app.cache.list_namespaces(&app.conn)?;
    Ok(namespaces)
}

#[tauri::command]
pub fn cmd_namespace_details(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<NamespaceInfo>, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let details = clio_core::repository::namespace_details(&app.conn)?;
    Ok(details)
}

#[tauri::command]
pub fn cmd_rename_namespace(
    state: State<'_, Mutex<AppState>>,
    from: String,
    to: String,
) -> Result<u32, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let count = clio_core::repository::rename_namespace(&app.conn, &from, &to)?;
    app.cache.clear_all();
    Ok(count as u32)
}

#[tauri::command]
pub fn cmd_merge_namespaces(
    state: State<'_, Mutex<AppState>>,
    source: String,
    target: String,
) -> Result<u32, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let count = app.cache.move_namespace_bulk(&app.conn, &source, &target)?;
    Ok(count as u32)
}

#[tauri::command]
pub fn cmd_delete_namespace(
    state: State<'_, Mutex<AppState>>,
    namespace: String,
) -> Result<bool, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let deleted = clio_core::repository::delete_empty_namespace(&app.conn, &namespace)?;
    app.cache.clear_all();
    Ok(deleted)
}

#[tauri::command]
pub fn cmd_purge_namespace(
    state: State<'_, Mutex<AppState>>,
    namespace: String,
) -> Result<u32, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let count =
        clio_core::repository::delete_namespace_with_memories(&app.conn, &namespace)?;
    app.cache.clear_all();
    Ok(count as u32)
}

#[tauri::command]
pub fn cmd_init_namespace(
    directory: String,
    namespace: String,
) -> Result<(), CommandError> {
    let dir = Path::new(&directory);
    if !dir.is_dir() {
        return Err(CommandError::Config(format!(
            "Directory does not exist: {directory}"
        )));
    }
    if namespace.is_empty() {
        return Err(CommandError::Config(
            "Namespace must not be empty".to_string(),
        ));
    }
    context::init_namespace(dir, &namespace)
        .map_err(|e| CommandError::Core(format!("Failed to create namespace: {e}")))
}

#[tauri::command]
pub fn cmd_detect_namespace(
    directory: String,
) -> Result<Option<DetectedContext>, CommandError> {
    let dir = Path::new(&directory);
    if !dir.is_dir() {
        return Err(CommandError::Config(format!(
            "Directory does not exist: {directory}"
        )));
    }
    Ok(context::detect_namespace(dir))
}

// ---------------------------------------------------------------------------
// Integrity checks
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn cmd_integrity_check(
    state: State<'_, Mutex<AppState>>,
) -> Result<clio_core::integrity::IntegrityReport, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let report = clio_core::integrity::check(&app.conn)?;
    Ok(report)
}

#[tauri::command]
pub fn cmd_integrity_fix(
    state: State<'_, Mutex<AppState>>,
) -> Result<clio_core::integrity::IntegrityReport, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let report = clio_core::integrity::fix(&app.conn)?;
    app.cache.clear_all();
    Ok(report)
}

// ---------------------------------------------------------------------------
// Backup and restore
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn cmd_backup(
    state: State<'_, Mutex<AppState>>,
) -> Result<clio_core::backup::BackupResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let result = clio_core::backup::backup(&app.db_path, None, 5)?;
    Ok(result)
}

#[tauri::command]
pub fn cmd_list_backups(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<clio_core::backup::BackupListEntry>, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let entries = clio_core::backup::list_backups(&app.db_path, None)?;
    Ok(entries)
}

#[tauri::command]
pub fn cmd_restore(
    state: State<'_, Mutex<AppState>>,
    backup_path: String,
) -> Result<clio_core::backup::RestoreResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let bp = std::path::Path::new(&backup_path);
    let result = clio_core::backup::restore(&app.db_path, bp)?;
    app.cache.clear_all();
    Ok(result)
}
