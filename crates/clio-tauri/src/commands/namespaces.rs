use std::path::Path;
use tauri::State;

use clio_core::cleanup::{self, CleanupCandidate, CleanupCriteria, CleanupReport};
use clio_core::consolidate::{self, ConsolidationResult};
use clio_core::context::{self, DetectedContext};
use clio_core::models::NamespaceInfo;

use crate::{AppState, CommandError};

/// Roll a namespace's memories into its AI-curated consolidated memory.
#[tauri::command]
pub fn cmd_consolidate_namespace(
    state: State<'_, AppState>,
    namespace: String,
) -> Result<ConsolidationResult, CommandError> {
    let app = state.local()?;
    let result =
        consolidate::consolidate(&app.conn, &namespace, &app.settings.capture, &app.settings)?;
    app.cache.clear_all();
    Ok(result)
}

/// Find namespaces matching the cleanup criteria. When no specific criterion is
/// requested, all criteria are applied.
#[tauri::command]
pub fn cmd_find_cleanup_candidates(
    state: State<'_, AppState>,
    stale_months: Option<u32>,
    archived: bool,
    folder_gone: bool,
    all: bool,
) -> Result<Vec<CleanupCandidate>, CommandError> {
    let app = state.local()?;

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
    state: State<'_, AppState>,
    namespaces: Vec<String>,
) -> Result<CleanupReport, CommandError> {
    let app = state.local()?;
    let report = cleanup::execute_cleanup(&app.conn, &app.db_path, &namespaces, 10)?;
    app.cache.clear_all();
    Ok(report)
}

#[tauri::command]
pub async fn cmd_namespaces(state: State<'_, AppState>) -> Result<Vec<String>, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json("memory_namespaces", serde_json::json!({}))
            .await;
    }

    let app = state.local()?;
    let namespaces = app.cache.list_namespaces(&app.conn)?;
    Ok(namespaces)
}

#[tauri::command]
pub async fn cmd_namespace_details(
    state: State<'_, AppState>,
) -> Result<Vec<NamespaceInfo>, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json("memory_namespace_details", serde_json::json!({}))
            .await;
    }

    let app = state.local()?;
    let details = clio_core::repository::namespace_details(&app.conn)?;
    Ok(details)
}

#[tauri::command]
pub fn cmd_rename_namespace(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<u32, CommandError> {
    let app = state.local()?;
    let count = clio_core::repository::rename_namespace(&app.conn, &from, &to)?;
    app.cache.clear_all();
    Ok(count as u32)
}

#[tauri::command]
pub fn cmd_merge_namespaces(
    state: State<'_, AppState>,
    source: String,
    target: String,
) -> Result<u32, CommandError> {
    let app = state.local()?;
    let count = app.cache.move_namespace_bulk(&app.conn, &source, &target)?;
    Ok(count as u32)
}

#[tauri::command]
pub fn cmd_delete_namespace(
    state: State<'_, AppState>,
    namespace: String,
) -> Result<bool, CommandError> {
    let app = state.local()?;
    let deleted = clio_core::repository::delete_empty_namespace(&app.conn, &namespace)?;
    app.cache.clear_all();
    Ok(deleted)
}

#[tauri::command]
pub async fn cmd_purge_namespace(
    state: State<'_, AppState>,
    namespace: String,
) -> Result<u32, CommandError> {
    if let Some(remote) = state.remote() {
        let report: CleanupReport = remote
            .call_json(
                "memory_namespace_delete",
                serde_json::json!({ "namespace": namespace }),
            )
            .await?;
        return Ok(report.memories_purged as u32);
    }

    let app = state.local()?;
    let report = purge_workspace(&app.conn, &app.db_path, &namespace)?;
    app.cache.clear_all();
    Ok(report.memories_purged as u32)
}

fn purge_workspace(
    conn: &rusqlite::Connection,
    db_path: &Path,
    namespace: &str,
) -> clio_core::error::Result<CleanupReport> {
    cleanup::execute_cleanup(conn, db_path, &[namespace.to_string()], 10)
}

#[tauri::command]
pub fn cmd_init_namespace(directory: String, namespace: String) -> Result<(), CommandError> {
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
pub fn cmd_detect_namespace(directory: String) -> Result<Option<DetectedContext>, CommandError> {
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
    state: State<'_, AppState>,
) -> Result<clio_core::integrity::IntegrityReport, CommandError> {
    let app = state.local()?;
    let report = clio_core::integrity::check(&app.conn)?;
    Ok(report)
}

#[tauri::command]
pub fn cmd_integrity_fix(
    state: State<'_, AppState>,
) -> Result<clio_core::integrity::IntegrityReport, CommandError> {
    let app = state.local()?;
    let report = clio_core::integrity::fix(&app.conn)?;
    app.cache.clear_all();
    Ok(report)
}

// ---------------------------------------------------------------------------
// Backup and restore
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn cmd_backup(
    state: State<'_, AppState>,
) -> Result<clio_core::backup::BackupResult, CommandError> {
    let app = state.local()?;
    let result = clio_core::backup::backup(&app.db_path, None, 5)?;
    Ok(result)
}

#[tauri::command]
pub fn cmd_list_backups(
    state: State<'_, AppState>,
) -> Result<Vec<clio_core::backup::BackupListEntry>, CommandError> {
    let app = state.local()?;
    let entries = clio_core::backup::list_backups(&app.db_path, None)?;
    Ok(entries)
}

#[tauri::command]
pub fn cmd_restore(
    state: State<'_, AppState>,
    backup_path: String,
) -> Result<clio_core::backup::RestoreResult, CommandError> {
    let app = state.local()?;
    let bp = std::path::Path::new(&backup_path);
    let result = clio_core::backup::restore(&app.db_path, bp)?;
    app.cache.clear_all();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clio_core::models::RememberInput;
    use clio_core::settings::Settings;

    fn remember(conn: &rusqlite::Connection, namespace: &str, content: &str) {
        clio_core::repository::remember(
            conn,
            &RememberInput {
                namespace: namespace.to_string(),
                kind: "note".into(),
                title: None,
                summary: None,
                content: content.into(),
                tags: vec![],
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Settings::default(),
        )
        .unwrap();
    }

    #[test]
    fn purge_workspace_deletes_its_memories_after_taking_a_backup() {
        let directory = tempfile::tempdir().unwrap();
        let db_path = directory.path().join("clio.db");
        let conn = clio_core::db::open(&db_path).unwrap();
        remember(&conn, "project:unused", "delete me");
        remember(&conn, "project:kept", "keep me");

        let report = purge_workspace(&conn, &db_path, "project:unused").unwrap();

        assert_eq!(report.namespaces_deleted, vec!["project:unused"]);
        assert_eq!(report.memories_purged, 1);
        let backup_path = report.backup_path.expect("cleanup should create a backup");
        assert!(Path::new(&backup_path).is_file());
        assert_eq!(
            clio_core::repository::list_namespaces(&conn).unwrap(),
            vec!["project:kept"]
        );

        let backup = rusqlite::Connection::open(backup_path).unwrap();
        assert_eq!(
            clio_core::repository::list_namespaces(&backup).unwrap(),
            vec!["project:kept", "project:unused"]
        );
    }
}
