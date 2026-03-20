use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

mod commands;

/// State of the embedding backend, which loads asynchronously on startup.
pub enum BackendState {
    /// Backend loaded and ready.
    Ready(Box<dyn clio_core::embeddings::EmbeddingBackend>),
    /// Backend is loading on a background thread.
    Loading,
    /// Backend failed to load or embeddings are disabled.
    Unavailable(String),
}

/// Shared application state: holds a persistent DB connection, cached settings,
/// a deferred embedding backend, and an in-memory cache so commands avoid
/// per-request reinit.
pub struct AppState {
    pub db_path: PathBuf,
    pub conn: Connection,
    pub settings: clio_core::settings::Settings,
    pub backend: BackendState,
    pub cache: clio_core::cache::ClioCache,
}

/// Serialisable error type for Tauri command returns.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("{0}")]
    Core(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Configuration error: {0}")]
    Config(String),
}

impl serde::Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<clio_core::error::ClioError> for CommandError {
    fn from(e: clio_core::error::ClioError) -> Self {
        CommandError::Core(e.to_string())
    }
}

impl From<serde_json::Error> for CommandError {
    fn from(e: serde_json::Error) -> Self {
        CommandError::Core(e.to_string())
    }
}

/// Run the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Clipboard handled via cmd_copy_to_clipboard (pbcopy) for
        // reliable macOS clipboard manager compatibility.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed
                        && shortcut.matches(
                            tauri_plugin_global_shortcut::Modifiers::SUPER
                                | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                            tauri_plugin_global_shortcut::Code::KeyM,
                        )
                    {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let db_path = resolve_db_path();
            let conn = clio_core::db::open(&db_path)
                .map_err(|e| format!("failed to open Clio database: {e}"))?;

            let settings = clio_core::settings::load(&db_path).unwrap_or_default();
            tracing::info!("Embeddings provider: {}", match &settings.embeddings {
                clio_core::embeddings::EmbeddingConfig::Local { model } => format!("Local ({model})"),
                clio_core::embeddings::EmbeddingConfig::OpenAi { model, .. } => format!("OpenAI ({model})"),
                clio_core::embeddings::EmbeddingConfig::Disabled => "Disabled".to_string(),
            });
            let embedding_config = settings.embeddings.clone();
            let cache = clio_core::cache::ClioCache::with_defaults();

            app.manage(Mutex::new(AppState {
                db_path,
                conn,
                settings,
                backend: BackendState::Loading,
                cache,
            }));

            // Register global hotkey: Cmd+Shift+M to show/hide the window.
            app.global_shortcut().register("CmdOrCtrl+Shift+M")
                .map_err(|e| format!("failed to register global shortcut: {e}"))?;

            // Load embedding backend on a background thread so the window
            // appears immediately without waiting for model init.
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let state_handle = app_handle.state::<Mutex<AppState>>();
                let result = clio_core::embeddings::create_backend(&embedding_config);
                let mut app_state = state_handle.inner().lock().expect("AppState lock poisoned");
                app_state.backend = match result {
                    Ok(b) => {
                        tracing::info!("Embedding backend loaded successfully");
                        BackendState::Ready(b)
                    }
                    Err(e) => {
                        tracing::error!("Embedding backend unavailable: {e}");
                        BackendState::Unavailable(e.to_string())
                    }
                };
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::memory::cmd_remember,
            commands::memory::cmd_update,
            commands::memory::cmd_recall,
            commands::memory::cmd_get,
            commands::memory::cmd_recent,
            commands::memory::cmd_archive,
            commands::memory::cmd_unarchive,
            commands::memory::cmd_delete,
            commands::memory::cmd_link,
            commands::memory::cmd_get_links,
            commands::memory::cmd_capture,
            commands::memory::cmd_cache_clear,
            commands::memory::cmd_bulk_archive,
            commands::memory::cmd_bulk_delete,
            commands::memory::cmd_bulk_add_tag,
            commands::memory::cmd_bulk_remove_tag,
            commands::memory::cmd_export_memories,
            commands::memory::cmd_import_memories,
            commands::search::cmd_search,
            commands::search::cmd_suggest_links,
            commands::search::cmd_backend_status,
            commands::stats::cmd_stats,
            commands::stats::cmd_activity,
            commands::namespaces::cmd_namespaces,
            commands::namespaces::cmd_namespace_details,
            commands::namespaces::cmd_rename_namespace,
            commands::namespaces::cmd_merge_namespaces,
            commands::namespaces::cmd_delete_namespace,
            commands::namespaces::cmd_init_namespace,
            commands::namespaces::cmd_detect_namespace,
            commands::namespaces::cmd_integrity_check,
            commands::namespaces::cmd_integrity_fix,
            commands::namespaces::cmd_backup,
            commands::namespaces::cmd_list_backups,
            commands::namespaces::cmd_restore,
            commands::clipboard::cmd_copy_to_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("Failed to run Clio");
}

/// Resolve the database path from env or platform default.
fn resolve_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLIO_DB_PATH") {
        return PathBuf::from(p);
    }
    clio_core::config::resolve_db_path(None).expect("Could not resolve database path")
}
