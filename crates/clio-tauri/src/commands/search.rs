use tauri::{AppHandle, Manager, State};

use clio_core::models::{Memory, RecallResult};

use crate::{AppState, BackendState, CommandError};

/// The O(N) embedding scan runs on a blocking thread pool so it never freezes
/// the Tauri main thread. `AppHandle` is `Send + 'static`, so we resolve the
/// managed state inside the closure rather than borrowing it across the await.
#[tauri::command]
pub async fn cmd_search(
    app: AppHandle,
    query: String,
    namespace: Option<String>,
    include_archived: Option<bool>,
    limit: Option<u32>,
) -> Result<RecallResult, CommandError> {
    {
        let state = app.state::<AppState>();
        if let Some(remote) = state.remote() {
            let global = namespace.is_none();
            return remote
                .call_json(
                    "memory_search",
                    serde_json::json!({
                        "query": query,
                        "namespace": namespace,
                        "global": global,
                        "include_archived": include_archived.unwrap_or(false),
                        "limit": limit.unwrap_or(10),
                        "response_format": "json",
                    }),
                )
                .await;
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let app = state.local()?;

        let backend = match &app.backend {
            BackendState::Ready(b) => b,
            BackendState::Loading => {
                return Err(CommandError::Config(
                    "Embedding backend is still loading. Please try again shortly.".into(),
                ));
            }
            BackendState::Unavailable(reason) => {
                return Err(CommandError::Config(format!(
                    "Embedding backend unavailable: {reason}"
                )));
            }
        };

        let query_embedding = backend.embed_one(&query)?;

        let items = clio_core::embeddings::semantic_recall(
            &app.conn,
            &query,
            &query_embedding,
            backend.model_name(),
            namespace.as_deref(),
            include_archived.unwrap_or(false),
            false,
            Some(&app.settings.scoring),
            limit.unwrap_or(10),
        )?;

        let count = items.len() as u32;
        Ok(RecallResult {
            archived_only: false,
            total: count,
            count,
            offset: 0,
            limit: limit.unwrap_or(10),
            items,
        })
    })
    .await
    .map_err(|e| CommandError::Core(format!("Search task failed: {e}")))?
}

#[tauri::command]
pub async fn cmd_suggest_links(
    app: AppHandle,
    memory_id: String,
    threshold: Option<f64>,
    limit: Option<u32>,
) -> Result<Vec<SuggestionResult>, CommandError> {
    {
        let state = app.state::<AppState>();
        if let Some(remote) = state.remote() {
            return remote
                .call_json(
                    "memory_suggest_links",
                    serde_json::json!({
                        "memory_id": memory_id,
                        "threshold": threshold.unwrap_or(0.7),
                        "limit": limit.unwrap_or(5),
                        "response_format": "json",
                    }),
                )
                .await;
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let app = state.local()?;

        let backend = match &app.backend {
            BackendState::Ready(b) => b,
            BackendState::Loading => {
                return Err(CommandError::Config(
                    "Embedding backend is still loading. Please try again shortly.".into(),
                ));
            }
            BackendState::Unavailable(reason) => {
                return Err(CommandError::Config(format!(
                    "Embedding backend unavailable: {reason}"
                )));
            }
        };

        let suggestions = clio_core::embeddings::suggest_links(
            &app.conn,
            &memory_id,
            backend.as_ref(),
            threshold.unwrap_or(0.7),
            limit.unwrap_or(5),
        )?;

        Ok(suggestions
            .into_iter()
            .map(|(memory, similarity)| SuggestionResult { memory, similarity })
            .collect())
    })
    .await
    .map_err(|e| CommandError::Core(format!("Suggest-links task failed: {e}")))?
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SuggestionResult {
    pub memory: Memory,
    pub similarity: f64,
}

#[tauri::command]
pub async fn cmd_backend_status(state: State<'_, AppState>) -> Result<String, CommandError> {
    if let Some(remote) = state.remote() {
        let status = remote.status().await;
        return Ok(if status.connected {
            "ready".into()
        } else {
            format!("unavailable: {}", status.detail.unwrap_or_default())
        });
    }

    let app = state.local()?;
    Ok(match &app.backend {
        BackendState::Ready(_) => "ready".to_string(),
        BackendState::Loading => "loading".to_string(),
        BackendState::Unavailable(reason) => format!("unavailable: {reason}"),
    })
}
