//! Periodic maintenance jobs: database backup and integrity checks.
//!
//! Both are pure-local (no network, no LLM) and safe to run on a timer. Each is
//! disabled unless its interval is configured (> 0). Consolidation is not run
//! here — it is triggered per session by the session-stop hook.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clio_core::settings::Settings;

/// Base granularity at which the scheduler wakes to check whether any job is due.
const TICK_SECS: u64 = 60;

/// Run the maintenance scheduler until shutdown is signalled.
pub async fn run(
    db_path: PathBuf,
    settings: Settings,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = settings.daemon.maintenance.clone();

    // Nothing enabled → idle until shutdown rather than spinning a timer.
    if cfg.backup_interval_secs == 0 && cfg.integrity_interval_secs == 0 {
        let _ = shutdown.recv().await;
        return Ok(());
    }

    tracing::info!(
        backup_interval_secs = cfg.backup_interval_secs,
        integrity_interval_secs = cfg.integrity_interval_secs,
        "maintenance scheduler started"
    );

    let mut tick = tokio::time::interval(Duration::from_secs(TICK_SECS));
    let start = Instant::now();
    let mut last_backup = start;
    let mut last_integrity = start;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let now = Instant::now();

                if cfg.backup_interval_secs > 0
                    && now.duration_since(last_backup).as_secs() >= cfg.backup_interval_secs
                {
                    last_backup = now;
                    run_backup(db_path.clone(), cfg.backup_max_backups).await;
                }

                if cfg.integrity_interval_secs > 0
                    && now.duration_since(last_integrity).as_secs() >= cfg.integrity_interval_secs
                {
                    last_integrity = now;
                    run_integrity(db_path.clone()).await;
                }
            }
            _ = shutdown.recv() => {
                tracing::debug!("maintenance scheduler shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn run_backup(db_path: PathBuf, max_backups: u32) {
    let result =
        tokio::task::spawn_blocking(move || clio_core::backup::backup(&db_path, None, max_backups))
            .await;

    match result {
        Ok(Ok(b)) => {
            tracing::info!(path = %b.path, size = b.size_bytes, "scheduled backup complete")
        }
        Ok(Err(e)) => tracing::warn!("scheduled backup failed: {e}"),
        Err(e) => tracing::warn!("backup task panicked: {e}"),
    }
}

async fn run_integrity(db_path: PathBuf) {
    let result = tokio::task::spawn_blocking(move || {
        let conn = clio_core::db::open(&db_path)?;
        clio_core::integrity::check(&conn)
    })
    .await;

    match result {
        Ok(Ok(report)) => {
            if report.issues_found > 0 {
                tracing::warn!(
                    checked = report.total_checked,
                    issues = report.issues_found,
                    "integrity check found issues (run `clio integrity --fix` to repair)"
                );
            } else {
                tracing::info!(checked = report.total_checked, "integrity check clean");
            }
        }
        Ok(Err(e)) => tracing::warn!("integrity check failed: {e}"),
        Err(e) => tracing::warn!("integrity task panicked: {e}"),
    }
}
