use std::path::Path;
use std::sync::Mutex;

use tauri::State;

use clio_core::context::{self, DetectedContext};
use clio_core::models::NamespaceInfo;

use crate::{AppState, CommandError};

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
