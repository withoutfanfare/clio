//! Background auto-link inference task.
//!
//! Periodically scans recently updated memories, generates embeddings if
//! missing, and creates links between semantically similar memories using
//! the "auto:relates_to" relationship prefix.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clio_core::embeddings::EmbeddingBackend;
use clio_core::settings::Settings;

/// Run the auto-link inference loop until shutdown is signalled.
pub async fn run(
    db_path: PathBuf,
    settings: Settings,
    backend: Arc<dyn EmbeddingBackend>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = settings.daemon.auto_link.clone();
    let interval_duration = Duration::from_secs(config.interval_secs);
    let mut interval = tokio::time::interval(interval_duration);
    let mut watermark: Option<String> = None;

    tracing::info!(
        interval_secs = config.interval_secs,
        threshold = config.threshold,
        batch_size = config.batch_size,
        "auto-linker started"
    );

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let db_path = db_path.clone();
                let config = config.clone();
                let backend = Arc::clone(&backend);
                let wm = watermark.clone();

                let result = tokio::task::spawn_blocking(move || {
                    let conn = clio_core::db::open(&db_path)?;
                    clio_core::embeddings::auto_link_batch(
                        &conn,
                        backend.as_ref(),
                        wm.as_deref(),
                        &config,
                    )
                })
                .await;

                match result {
                    Ok(Ok(report)) => {
                        if report.memories_processed > 0 {
                            tracing::info!(
                                processed = report.memories_processed,
                                links = report.links_created,
                                "auto-link pass complete"
                            );
                        } else {
                            tracing::debug!("auto-link pass: no new memories to process");
                        }
                        if report.last_watermark.is_some() {
                            watermark = report.last_watermark;
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("auto-link pass failed: {e}");
                    }
                    Err(e) => {
                        tracing::warn!("auto-link task panicked: {e}");
                    }
                }
            }
            _ = shutdown.recv() => {
                tracing::debug!("auto-linker shutting down");
                break;
            }
        }
    }

    Ok(())
}
