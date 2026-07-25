use tauri::State;

use crate::remote::ConnectionStatus;
use crate::{AppState, CommandError};

#[tauri::command]
pub async fn cmd_connection_status(
    state: State<'_, AppState>,
) -> Result<ConnectionStatus, CommandError> {
    if let Some(remote) = state.remote() {
        return Ok(remote.status().await);
    }

    let local = state.local()?;
    Ok(ConnectionStatus {
        backend: "local".into(),
        label: "Local".into(),
        connected: true,
        detail: Some(local.db_path.display().to_string()),
    })
}
