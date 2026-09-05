//! Review queue adapters. All workspaces are included, as in `memory_inbox`.
use crate::{AppState, CommandError};
use clio_core::{
    models::Memory,
    review::{ReviewEdits, ReviewItem},
};
use tauri::State;

fn decode_remote_inbox(value: serde_json::Value) -> Result<Vec<ReviewItem>, CommandError> {
    #[derive(serde::Deserialize)]
    struct InboxResponse {
        items: Vec<ReviewItem>,
        includes_edited: bool,
    }
    let unsupported = || {
        CommandError::Core(
        "This backend cannot confirm that edited captures remain in the inbox. Update the backend before reviewing captures here.".into(),
    )
    };
    let response: InboxResponse = serde_json::from_value(value).map_err(|_| unsupported())?;
    if !response.includes_edited {
        return Err(unsupported());
    }
    Ok(response.items)
}

#[tauri::command]
pub async fn cmd_inbox_list(state: State<'_, AppState>) -> Result<Vec<ReviewItem>, CommandError> {
    if let Some(remote) = state.remote() {
        let response = remote
            .call_json(
                "memory_inbox",
                serde_json::json!({
                    "action": "list", "limit": 100, "response_format": "json",
                    "include_status_scope": true
                }),
            )
            .await?;
        return decode_remote_inbox(response);
    }
    let app = state.local()?;
    Ok(clio_core::review::list_pending(&app.conn, 100)?)
}

#[cfg(test)]
mod tests {
    use super::decode_remote_inbox;
    use serde_json::json;

    #[test]
    fn empty_old_backend_inbox_is_not_treated_as_a_complete_queue() {
        assert!(decode_remote_inbox(json!([])).is_err());
        assert!(decode_remote_inbox(json!({ "items": [] })).is_err());
        assert!(decode_remote_inbox(json!({ "items": [], "includes_edited": false })).is_err());
    }

    #[test]
    fn empty_inbox_is_valid_when_the_backend_confirms_edited_visibility() {
        assert!(
            decode_remote_inbox(json!({ "items": [], "includes_edited": true }))
                .unwrap()
                .is_empty()
        );
    }
}

#[tauri::command]
pub async fn cmd_inbox_approve(
    state: State<'_, AppState>,
    review_id: String,
) -> Result<Memory, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_inbox",
                serde_json::json!({"action": "approve", "review_id": review_id}),
            )
            .await;
    }
    let app = state.local()?;
    let memory = clio_core::review::approve_review(&app.conn, &review_id, &app.settings)?;
    app.cache.clear_all();
    Ok(memory)
}

#[tauri::command]
pub async fn cmd_inbox_reject(
    state: State<'_, AppState>,
    review_id: String,
) -> Result<ReviewItem, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "memory_inbox",
                serde_json::json!({"action": "reject", "review_id": review_id}),
            )
            .await;
    }
    let app = state.local()?;
    Ok(clio_core::review::reject_review(&app.conn, &review_id)?)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn cmd_inbox_edit(
    state: State<'_, AppState>,
    review_id: String,
    namespace: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    summary: Option<String>,
    tags: Option<Vec<String>>,
    importance: Option<i32>,
) -> Result<ReviewItem, CommandError> {
    if importance.is_some_and(|value| !(1..=5).contains(&value)) {
        return Err(CommandError::Core(
            "Importance must be between 1 and 5.".into(),
        ));
    }
    if let Some(remote) = state.remote() {
        return remote.call_json("memory_inbox", serde_json::json!({
            "action": "edit", "review_id": review_id, "namespace": namespace,
            "kind": kind, "title": title, "summary": summary, "tags": tags, "importance": importance,
        })).await;
    }
    let app = state.local()?;
    Ok(clio_core::review::edit_review(
        &app.conn,
        &review_id,
        &ReviewEdits {
            namespace,
            kind,
            title: title.map(Some),
            summary: summary.map(Some),
            tags,
            importance,
            confidence: None,
        },
    )?)
}
