use std::sync::Mutex;

use tauri::State;

use clio_core::models::{Memory, RecallResult};

use crate::{AppState, BackendState, CommandError};

#[tauri::command]
pub fn cmd_search(
    state: State<'_, Mutex<AppState>>,
    query: String,
    namespace: Option<String>,
    include_archived: Option<bool>,
    limit: Option<u32>,
) -> Result<RecallResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

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
        namespace.as_deref(),
        include_archived.unwrap_or(false),
        false,
        Some(&app.settings.scoring),
        limit.unwrap_or(10),
    )?;

    let count = items.len() as u32;
    Ok(RecallResult {
        total: count,
        count,
        offset: 0,
        limit: limit.unwrap_or(10),
        items,
    })
}

#[tauri::command]
pub fn cmd_suggest_links(
    state: State<'_, Mutex<AppState>>,
    memory_id: String,
    threshold: Option<f64>,
    limit: Option<u32>,
) -> Result<Vec<SuggestionResult>, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

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
}

#[derive(serde::Serialize)]
pub struct SuggestionResult {
    pub memory: Memory,
    pub similarity: f64,
}

#[tauri::command]
pub fn cmd_backend_status(state: State<'_, Mutex<AppState>>) -> Result<String, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    Ok(match &app.backend {
        BackendState::Ready(_) => "ready".to_string(),
        BackendState::Loading => "loading".to_string(),
        BackendState::Unavailable(reason) => format!("unavailable: {reason}"),
    })
}
