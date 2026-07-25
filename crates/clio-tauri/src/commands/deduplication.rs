use tauri::State;

use clio_core::deduplication::{DuplicateScanResult, MergePreview};
use clio_core::models::Memory;

use crate::{AppState, CommandError};

#[tauri::command]
pub fn cmd_find_duplicates(
    state: State<'_, AppState>,
) -> Result<DuplicateScanResult, CommandError> {
    let app = state.local()?;
    Ok(clio_core::deduplication::find_duplicates(&app.conn)?)
}

#[tauri::command]
pub fn cmd_preview_merge(
    state: State<'_, AppState>,
    keep_id: String,
    merge_ids: Vec<String>,
) -> Result<MergePreview, CommandError> {
    let app = state.local()?;
    Ok(clio_core::deduplication::preview_merge(
        &app.conn, &keep_id, &merge_ids,
    )?)
}

#[tauri::command]
pub fn cmd_merge_memories(
    state: State<'_, AppState>,
    keep_id: String,
    merge_ids: Vec<String>,
) -> Result<Memory, CommandError> {
    let app = state.local()?;
    Ok(clio_core::deduplication::merge_memories(
        &app.conn, &keep_id, &merge_ids,
    )?)
}
