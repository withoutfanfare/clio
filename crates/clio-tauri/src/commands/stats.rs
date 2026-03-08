use std::sync::Mutex;

use tauri::State;

use clio_core::models::{MemoryStats, RecentEntry};

use crate::{AppState, CommandError};

#[tauri::command]
pub fn cmd_stats(
    state: State<'_, Mutex<AppState>>,
    namespace: Option<String>,
) -> Result<MemoryStats, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let stats = clio_core::stats::memory_stats(&app.conn, namespace.as_deref())?;
    Ok(stats)
}

#[tauri::command]
pub fn cmd_activity(
    state: State<'_, Mutex<AppState>>,
    namespace: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<RecentEntry>, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let entries =
        clio_core::stats::recent_activity(&app.conn, namespace.as_deref(), limit.unwrap_or(20))?;
    Ok(entries)
}
