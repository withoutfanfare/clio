use tauri::State;

use clio_core::models::{MemoryStats, RecentEntry};

use crate::{AppState, CommandError};

#[tauri::command]
pub async fn cmd_stats(
    state: State<'_, AppState>,
    namespace: Option<String>,
) -> Result<MemoryStats, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_stats",
                serde_json::json!({ "namespace": namespace, "response_format": "json" }),
            )
            .await;
    }

    let app = state.local()?;
    let stats = clio_core::stats::memory_stats(&app.conn, namespace.as_deref())?;
    Ok(stats)
}

#[tauri::command]
pub async fn cmd_activity(
    state: State<'_, AppState>,
    namespace: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<RecentEntry>, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_activity",
                serde_json::json!({
                    "namespace": namespace,
                    "limit": limit.unwrap_or(20),
                    "response_format": "json",
                }),
            )
            .await;
    }

    let app = state.local()?;
    let entries =
        clio_core::stats::recent_activity(&app.conn, namespace.as_deref(), limit.unwrap_or(20))?;
    Ok(entries)
}
