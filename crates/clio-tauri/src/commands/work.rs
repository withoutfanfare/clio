//! Work reporting and receipt-scoped local review; remote failures never fall back to local data.
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;

use crate::{AppState, CommandError};

#[tauri::command]
pub async fn cmd_work_overview(
    state: State<'_, AppState>,
    project: Option<String>,
    stale_after_secs: Option<i64>,
) -> Result<serde_json::Value, CommandError> {
    if let Some(remote) = state.remote() {
        return remote
            .call_json(
                "work_overview",
                serde_json::json!({ "project": project, "stale_after_secs": stale_after_secs }),
            )
            .await;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CommandError::Core(error.to_string()))?
        .as_secs() as i64;
    let app = state.local()?;
    let overview = clio_core::work_reports::overview_for_project(
        &app.conn,
        now,
        stale_after_secs.unwrap_or(clio_core::work_reports::DEFAULT_STALE_AFTER_SECS),
        project.as_deref(),
    )?;
    let mut value = serde_json::to_value(&overview)?;
    value["can_accept"] = serde_json::json!(true);
    for (task, json_task) in overview.tasks.iter().zip(value["tasks"].as_array_mut().unwrap()) {
        for (run, json_run) in task.runs.iter().zip(json_task["runs"].as_array_mut().unwrap()) {
            let report = &run.receipt.report;
            json_run["openable_evidence"] = serde_json::json!(report.evidence.iter().filter(|reference|
                clio_core::work_reports::local_evidence_path(&report.worktree, reference).is_some()
            ).collect::<Vec<_>>());
        }
    }
    Ok(value)
}


#[tauri::command]
pub async fn cmd_open_work_evidence(
    state: State<'_, AppState>,
    receipt_id: i64,
    reference: String,
) -> Result<(), CommandError> {
    let path = {
        let app = state.local()?;
        clio_core::work_reports::evidence_path(&app.conn, receipt_id, &reference)?
    };
    let status = tokio::process::Command::new("/usr/bin/open")
        .arg(path)
        .status().await
        .map_err(|_| CommandError::Core("Could not start the local file opener".into()))?;
    if !status.success() {
        return Err(CommandError::Core("The local file opener could not open this evidence".into()));
    }
    Ok(())
}


#[tauri::command]
pub fn cmd_accept_work_change(
    state: State<'_, AppState>,
    receipt_id: i64,
) -> Result<clio_core::work_reports::Acceptance, CommandError> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|error| CommandError::Core(error.to_string()))?.as_secs() as i64;
    let app = state.local()?;
    Ok(clio_core::work_reports::accept_current(&app.conn, receipt_id, now)?)
}
