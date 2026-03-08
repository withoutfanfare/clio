use std::path::Path;
use std::sync::Mutex;

use tauri::State;

use clio_core::context::{self, DetectedContext};

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
