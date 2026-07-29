use tauri::State;

use crate::{AppState, CommandError};

#[tauri::command]
pub async fn cmd_capture_preferences(
    state: State<'_, AppState>,
) -> Result<clio_core::settings::CapturePreferences, CommandError> {
    if let Some(remote) = state.remote() {
        return remote.capture_preferences().await;
    }

    let local = state.local()?;
    clio_core::settings::capture_preferences(&local.db_path).map_err(CommandError::from)
}

#[tauri::command]
pub async fn cmd_set_capture_model(
    state: State<'_, AppState>,
    model: String,
) -> Result<clio_core::settings::CapturePreferences, CommandError> {
    if let Some(remote) = state.remote() {
        return remote.set_capture_model(&model).await;
    }

    let mut local = state.local()?;
    let preferences = clio_core::settings::set_capture_model(&local.db_path, &model)?;
    local.settings.capture.model = preferences.model.clone();
    Ok(preferences)
}
