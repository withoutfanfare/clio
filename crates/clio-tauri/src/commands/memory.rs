use std::sync::Mutex;

use tauri::State;

use clio_core::capture::CaptureResult;
use clio_core::models::{LinkInput, Memory, MemoryLink, RecallQuery, RecallResult, RememberInput, SortOrder};

use crate::{AppState, BackendState, CommandError};

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn cmd_remember(
    state: State<'_, Mutex<AppState>>,
    namespace: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    summary: Option<String>,
    content: String,
    tags: Option<Vec<String>>,
    source: Option<String>,
    source_ref: Option<String>,
    confidence: Option<f64>,
    importance: Option<i32>,
    metadata: Option<serde_json::Value>,
    upsert: Option<bool>,
) -> Result<Memory, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

    let input = RememberInput {
        namespace: namespace.unwrap_or_else(|| "global".into()),
        kind: kind.unwrap_or_else(|| "note".into()),
        title,
        summary,
        content,
        tags: tags.unwrap_or_default(),
        source,
        source_ref,
        confidence,
        importance: importance.unwrap_or(3),
        metadata: metadata.unwrap_or(serde_json::Value::Object(Default::default())),
        valid_from: None,
        valid_until: None,
        upsert: upsert.unwrap_or(false),
    };

    let memory = app.cache.remember(&app.conn, &input)?;

    // Auto-embed using the cached backend (skipped silently if still loading).
    if app.settings.auto_embed {
        if let BackendState::Ready(ref backend) = app.backend {
            if let Err(e) =
                clio_core::embeddings::embed_and_store(&app.conn, backend.as_ref(), &memory)
            {
                tracing::warn!("Auto-embed failed: {e}");
            }
        }
    }

    Ok(memory)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn cmd_update(
    state: State<'_, Mutex<AppState>>,
    memory_id: String,
    namespace: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    summary: Option<String>,
    content: String,
    tags: Option<Vec<String>>,
    source: Option<String>,
    source_ref: Option<String>,
    confidence: Option<f64>,
    importance: Option<i32>,
    metadata: Option<serde_json::Value>,
) -> Result<Memory, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

    let input = RememberInput {
        namespace: namespace.unwrap_or_else(|| "global".into()),
        kind: kind.unwrap_or_else(|| "note".into()),
        title,
        summary,
        content,
        tags: tags.unwrap_or_default(),
        source,
        source_ref,
        confidence,
        importance: importance.unwrap_or(3),
        metadata: metadata.unwrap_or(serde_json::Value::Object(Default::default())),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let memory = app.cache.update(&app.conn, &memory_id, &input)?;

    // Auto-embed using the cached backend.
    if app.settings.auto_embed {
        if let BackendState::Ready(ref backend) = app.backend {
            if let Err(e) =
                clio_core::embeddings::embed_and_store(&app.conn, backend.as_ref(), &memory)
            {
                tracing::warn!("Auto-embed failed: {e}");
            }
        }
    }

    Ok(memory)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn cmd_recall(
    state: State<'_, Mutex<AppState>>,
    query: Option<String>,
    namespace: Option<String>,
    kind: Option<String>,
    tags: Option<Vec<String>>,
    match_all_tags: Option<bool>,
    importance_min: Option<i32>,
    importance_max: Option<i32>,
    sort_by: Option<String>,
    include_archived: Option<bool>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<RecallResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

    let q = RecallQuery {
        query,
        namespace,
        kind,
        tags: tags.unwrap_or_default(),
        match_all_tags: match_all_tags.unwrap_or(true),
        include_archived: include_archived.unwrap_or(false),
        importance_min,
        importance_max,
        sort_by: sort_by.as_deref().and_then(SortOrder::from_str_opt),
        limit: limit.unwrap_or(10),
        offset: offset.unwrap_or(0),
        include_links: false,
        scoring: Some(app.settings.scoring.clone()),
    };

    let result = app.cache.recall(&app.conn, &q)?;
    Ok(result)
}

#[tauri::command]
pub fn cmd_get(
    state: State<'_, Mutex<AppState>>,
    memory_id: String,
) -> Result<Memory, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let memory = app.cache.get(&app.conn, &memory_id)?;
    Ok(memory)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn cmd_recent(
    state: State<'_, Mutex<AppState>>,
    namespace: Option<String>,
    kind: Option<String>,
    tags: Option<Vec<String>>,
    match_all_tags: Option<bool>,
    importance_min: Option<i32>,
    importance_max: Option<i32>,
    sort_by: Option<String>,
    include_archived: Option<bool>,
    limit: Option<u32>,
) -> Result<RecallResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

    let q = RecallQuery {
        query: None,
        namespace,
        kind,
        tags: tags.unwrap_or_default(),
        match_all_tags: match_all_tags.unwrap_or(true),
        include_archived: include_archived.unwrap_or(false),
        importance_min,
        importance_max,
        sort_by: sort_by.as_deref().and_then(SortOrder::from_str_opt),
        limit: limit.unwrap_or(10),
        offset: 0,
        include_links: false,
        scoring: Some(app.settings.scoring.clone()),
    };

    let result = app.cache.recall(&app.conn, &q)?;
    Ok(result)
}

#[tauri::command]
pub fn cmd_archive(
    state: State<'_, Mutex<AppState>>,
    memory_id: String,
) -> Result<Memory, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let memory = app.cache.archive(&app.conn, &memory_id)?;
    Ok(memory)
}

#[tauri::command]
pub fn cmd_unarchive(
    state: State<'_, Mutex<AppState>>,
    memory_id: String,
) -> Result<Memory, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let memory = app.cache.unarchive(&app.conn, &memory_id)?;
    Ok(memory)
}

#[tauri::command]
pub fn cmd_delete(
    state: State<'_, Mutex<AppState>>,
    memory_id: String,
) -> Result<Memory, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let memory = app.cache.delete(&app.conn, &memory_id)?;
    Ok(memory)
}

#[tauri::command]
pub fn cmd_link(
    state: State<'_, Mutex<AppState>>,
    from_memory_id: String,
    to_memory_id: String,
    relationship: Option<String>,
    metadata: Option<serde_json::Value>,
) -> Result<MemoryLink, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

    let input = LinkInput {
        from_memory_id,
        to_memory_id,
        relationship: relationship.unwrap_or_else(|| "relates_to".into()),
        metadata: metadata.unwrap_or(serde_json::Value::Object(Default::default())),
    };

    let link = app.cache.link(&app.conn, &input)?;
    Ok(link)
}

#[tauri::command]
pub fn cmd_get_links(
    state: State<'_, Mutex<AppState>>,
    memory_id: String,
) -> Result<Vec<MemoryLink>, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    let links = app.cache.get_links(&app.conn, &memory_id)?;
    Ok(links)
}

#[tauri::command]
pub fn cmd_capture(
    state: State<'_, Mutex<AppState>>,
    text: String,
    namespace: Option<String>,
) -> Result<CaptureResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;

    if !app.settings.capture.enabled {
        return Err(CommandError::Config(
            "Capture pipeline is not enabled. Configure it in settings.".into(),
        ));
    }

    let result = clio_core::capture::capture(
        &app.conn,
        &text,
        &app.settings.capture,
        namespace.as_deref(),
        &app.settings,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn cmd_cache_clear(
    state: State<'_, Mutex<AppState>>,
) -> Result<clio_core::cache::CacheClearResult, CommandError> {
    let app = state
        .lock()
        .map_err(|e| CommandError::Core(format!("Lock poisoned: {e}")))?;
    Ok(app.cache.clear_all())
}
