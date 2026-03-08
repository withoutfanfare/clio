//! Unix domain socket control server.
//!
//! Accepts newline-delimited JSON commands over a Unix socket and returns
//! JSON responses. Supports `status`, `stop`, and `health` commands.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;
use tokio_util::codec::{FramedRead, LinesCodec};

/// A command received over the control socket.
#[derive(serde::Deserialize)]
struct ControlRequest {
    command: String,
}

/// Serve the control socket until a shutdown signal is received.
pub async fn serve(
    socket_path: PathBuf,
    db_path: PathBuf,
    settings: clio_core::settings::Settings,
    started_at: Instant,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Remove stale socket file (unconditional to avoid TOCTOU race).
    match std::fs::remove_file(&socket_path) {
        Ok(()) => tracing::debug!(path = %socket_path.display(), "removed stale socket file"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(path = %socket_path.display(), "could not remove stale socket: {e}"),
    }

    // Ensure the parent directory exists.
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&socket_path)?;

    // Restrict socket to owner-only access.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::debug!(path = %socket_path.display(), "control socket bound");

    let conn_limit = Arc::new(tokio::sync::Semaphore::new(16));

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let db_path = db_path.clone();
                        let settings = settings.clone();
                        let shutdown_tx = shutdown_tx.clone();
                        let permit = conn_limit.clone();
                        tokio::spawn(async move {
                            let _permit = match permit.acquire_owned().await {
                                Ok(p) => p,
                                Err(_) => return,
                            };
                            handle_connection(
                                stream,
                                db_path,
                                settings,
                                started_at,
                                shutdown_tx,
                            ).await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!("failed to accept control connection: {e}");
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                tracing::debug!("control socket shutting down");
                break;
            }
        }
    }

    // Clean up the socket file.
    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}

/// Handle a single control socket connection. Reads newline-delimited JSON
/// commands and writes JSON responses.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    db_path: PathBuf,
    settings: clio_core::settings::Settings,
    started_at: Instant,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut frames = FramedRead::new(reader, LinesCodec::new_with_max_length(65_536));

    while let Some(Ok(line)) = frames.next().await {
        let response = match serde_json::from_str::<ControlRequest>(&line) {
            Ok(req) => handle_command(&req.command, &db_path, &settings, started_at, &shutdown_tx),
            Err(e) => serde_json::json!({
                "error": format!("invalid request: {e}")
            }),
        };

        let mut buf = serde_json::to_vec(&response).unwrap_or_else(|e| {
            tracing::error!("failed to serialise control response: {e}");
            br#"{"error":"internal serialisation failure"}"#.to_vec()
        });
        buf.push(b'\n');

        if writer.write_all(&buf).await.is_err() {
            break;
        }
    }
}

/// Dispatch a control command and return the JSON response.
fn handle_command(
    command: &str,
    db_path: &Path,
    settings: &clio_core::settings::Settings,
    started_at: Instant,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) -> serde_json::Value {
    match command {
        "status" => {
            let health = clio_core::daemon::run_health_checks(db_path, settings);
            let uptime = started_at.elapsed().as_secs();

            let status = clio_core::daemon::DaemonStatus {
                pid: Some(std::process::id()),
                uptime_secs: Some(uptime),
                db_path: db_path.display().to_string(),
                enabled_routes: build_enabled_routes(settings),
                health,
                started_at: None,
            };

            serde_json::to_value(status).unwrap_or_else(|e| {
                serde_json::json!({"error": format!("serialisation failed: {e}")})
            })
        }
        "health" => {
            let health = clio_core::daemon::run_health_checks(db_path, settings);
            serde_json::to_value(health).unwrap_or_else(|e| {
                serde_json::json!({"error": format!("serialisation failed: {e}")})
            })
        }
        "stop" => {
            tracing::info!("stop command received via control socket");
            let _ = shutdown_tx.send(());
            serde_json::json!({"ok": true})
        }
        other => {
            serde_json::json!({"error": format!("unknown command: {other}")})
        }
    }
}

/// Build the list of enabled routes/subsystems for the status response.
fn build_enabled_routes(settings: &clio_core::settings::Settings) -> Vec<String> {
    let mut routes = vec!["control_socket".to_string()];

    if !settings.daemon.inbox_paths.is_empty() {
        routes.push("inbox_watcher".to_string());
    }

    if settings.capture.enabled {
        routes.push("capture_pipeline".to_string());
    }

    if settings.daemon.http_port.is_some() {
        routes.push("http_api".to_string());
    }

    if settings.daemon.auto_link.enabled {
        routes.push("auto_linker".to_string());
    }

    routes
}
