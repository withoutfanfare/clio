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
    if !root.exists() {
        return Ok(serde_json::Value::Null);
    }

    let count = |bucket: &str| -> (u64, Option<f64>) {
        let mut n = 0u64;
        let mut oldest: Option<f64> = None;
        if let Ok(entries) = std::fs::read_dir(root.join(bucket)) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    n += 1;
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(age) = modified.elapsed() {
                                let secs = age.as_secs_f64();
                                if oldest.is_none_or(|o| secs > o) {
                                    oldest = Some(secs);
                                }
                            }
                        }
                    }
                }
            }
        }
        (n, oldest)
    };

    let (pending, oldest_pending) = count("pending");
    let (processing, _) = count("processing");
    let (dead, _) = count("dead");

    Ok(serde_json::json!({
        "pending": pending,
        "processing": processing,
        "dead": dead,
        "oldest_pending_age_secs": oldest_pending,
    }))
}
