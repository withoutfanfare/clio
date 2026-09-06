//! Attention commands: the Today / Needs attention surface.
//!
//! Thin adapters over `clio_core::attention` (local) or the Atlas
//! `memory_action` tool (remote). Both paths serialise the same core types,
//! so local and remote payloads stay identical by construction. Remote
//! disconnects surface as errors — never a silent fall-back to local storage.

use tauri::State;

use crate::{AppState, CommandError};

#[tauri::command]
pub async fn cmd_attention_overview(
    state: State<'_, AppState>,
    namespace: Option<String>,
) -> Result<serde_json::Value, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_action",
                serde_json::json!({ "action": "overview", "namespace": namespace }),
            )
            .await;
    }
    let app = state.local()?;
    let overview = clio_core::attention::overview(
        &app.conn,
        namespace.as_deref(),
        None,
        app.settings.attention.dormant_days,
        app.settings.attention.max_age_days,
    )?;
    Ok(serde_json::to_value(overview)?)
}

#[tauri::command]
pub async fn cmd_action_complete(
    state: State<'_, AppState>,
    id: String,
    evidence: Option<String>,
    reason: Option<String>,
) -> Result<serde_json::Value, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_action",
                serde_json::json!({
                    "action": "complete",
                    "id": id,
                    "evidence": evidence,
                    "reason": reason,
                }),
            )
            .await;
    }
    let app = state.local()?;
    let item = clio_core::attention::complete(
        &app.conn,
        &id,
        evidence.as_deref(),
        reason.as_deref(),
        Some("user"),
    )?;
    Ok(serde_json::to_value(item)?)
}

#[tauri::command]
pub async fn cmd_action_snooze(
    state: State<'_, AppState>,
    id: String,
    until: String,
) -> Result<serde_json::Value, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_action",
                serde_json::json!({ "action": "snooze", "id": id, "until": until }),
            )
            .await;
    }
    let app = state.local()?;
    let item = clio_core::attention::snooze(&app.conn, &id, &until, Some("user"))?;
    Ok(serde_json::to_value(item)?)
}

#[tauri::command]
pub async fn cmd_action_cancel(
    state: State<'_, AppState>,
    id: String,
    reason: Option<String>,
) -> Result<serde_json::Value, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_action",
                serde_json::json!({ "action": "cancel", "id": id, "reason": reason }),
            )
            .await;
    }
    let app = state.local()?;
    let item = clio_core::attention::cancel(&app.conn, &id, reason.as_deref(), Some("user"))?;
    Ok(serde_json::to_value(item)?)
}

#[tauri::command]
pub async fn cmd_link_contexts(
    state: State<'_, AppState>,
    memory_id: String,
) -> Result<serde_json::Value, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_get_links",
                serde_json::json!({ "memory_id": memory_id, "direction": "both" }),
            )
            .await;
    }
    let app = state.local()?;
    let contexts = clio_core::repository::get_link_contexts(&app.conn, &memory_id)?;
    Ok(serde_json::to_value(contexts)?)
}

/// Client-local capture-queue health, read from the spool the hook package
/// maintains. Returns `null` when the spool does not exist — the UI must show
/// "unavailable", never zero.
#[tauri::command]
pub async fn cmd_capture_queue_health() -> Result<serde_json::Value, CommandError> {
    let root = match std::env::var("CLIO_CAPTURE_SPOOL") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            let Ok(home) = std::env::var("HOME") else {
                return Ok(serde_json::Value::Null);
            };
            std::path::PathBuf::from(home).join("Library/Application Support/clio/capture-spool")
        }
    };
    Ok(clio_core::capture_queue::health(&root))
}
