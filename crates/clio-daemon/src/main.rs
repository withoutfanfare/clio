//! Clio daemon — always-on local process for background memory operations.
//!
//! Manages a Unix domain socket for control commands and optionally watches
//! inbox directories for files to ingest through the capture pipeline.

mod auto_linker;
mod control;
mod watcher;

use std::path::PathBuf;
use std::time::Instant;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse minimal CLI args.
    let db_override = parse_db_path_arg();

    // Resolve database path.
    let db_path = clio_core::config::resolve_db_path(db_override.as_deref())
        .map_err(|e| format!("failed to resolve database path: {e}"))?;

    // Load settings from the file next to the database.
    let settings = clio_core::settings::load(&db_path)
        .map_err(|e| format!("failed to load settings: {e}"))?;

    // Resolve log directory and ensure it exists.
    let log_dir = settings
        .daemon
        .log_dir
        .clone()
        .or_else(|| clio_core::daemon::default_log_dir().ok())
        .ok_or("could not determine log directory")?;
    std::fs::create_dir_all(&log_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&log_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    // Initialise tracing: file appender layer + stderr layer.
    let file_appender = tracing_appender::rolling::daily(&log_dir, "clio-daemon.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_target(false)
        .with_ansi(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!(db = %db_path.display(), "clio-daemon starting");

    // Run the async runtime.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(run(db_path, settings))
}

async fn run(
    db_path: PathBuf,
    settings: clio_core::settings::Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let started_at = Instant::now();

    // Run startup health checks.
    let health = clio_core::daemon::run_health_checks(&db_path, &settings);
    tracing::info!(?health, "startup health check");

    // Verify the database opens correctly.
    {
        let _conn = clio_core::db::open(&db_path)
            .map_err(|e| format!("database verification failed: {e}"))?;
        tracing::info!("database verified");
    }

    // Create PID file — fails if another instance is already running.
    let pid_path = clio_core::daemon::default_pid_path()
        .map_err(|e| format!("could not determine PID file path: {e}"))?;
    let pid_file = clio_core::daemon::PidFile::new(pid_path);
    pid_file
        .create(std::process::id())
        .map_err(|e| format!("PID file error: {e}"))?;
    tracing::info!(pid = std::process::id(), "PID file created");

    // Resolve socket path.
    let socket_path = settings
        .daemon
        .socket_path
        .clone()
        .or_else(|| clio_core::daemon::default_socket_path().ok())
        .ok_or("could not determine socket path")?;

    // Shutdown broadcast channel.
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    // Start the control socket server.
    let control_handle = tokio::spawn(control::serve(
        socket_path.clone(),
        db_path.clone(),
        settings.clone(),
        started_at,
        shutdown_tx.clone(),
        shutdown_tx.subscribe(),
    ));
    tracing::info!(socket = %socket_path.display(), "control socket listening");

    // Create a shared embedding backend — used by both the inbox watcher and
    // auto-linker, avoiding repeated ONNX model loads.
    let shared_backend: Option<std::sync::Arc<dyn clio_core::embeddings::EmbeddingBackend>> =
        match clio_core::embeddings::create_backend(&settings.embeddings) {
            Ok(backend) => Some(std::sync::Arc::from(backend)),
            Err(e) => {
                tracing::warn!("embedding backend unavailable: {e}");
                None
            }
        };

    // Start inbox watcher if paths are configured.
    let watcher_handle = if !settings.daemon.inbox_paths.is_empty() {
        tracing::info!(
            paths = ?settings.daemon.inbox_paths,
            "starting inbox watcher"
        );
        Some(tokio::spawn(watcher::watch(
            settings.daemon.inbox_paths.clone(),
            db_path.clone(),
            settings.clone(),
            shared_backend.clone(),
            shutdown_tx.subscribe(),
        )))
    } else {
        tracing::info!("no inbox paths configured, watcher disabled");
        None
    };

    // Start auto-linker if enabled.
    let auto_linker_handle = if settings.daemon.auto_link.enabled {
        match shared_backend.clone() {
            Some(backend) => {
                tracing::info!("starting auto-linker");
                Some(tokio::spawn(auto_linker::run(
                    db_path.clone(),
                    settings.clone(),
                    backend,
                    shutdown_tx.subscribe(),
                )))
            }
            None => {
                tracing::warn!("auto-linker disabled: embedding backend not available");
                None
            }
        }
    } else {
        tracing::info!("auto-linker not enabled");
        None
    };

    // Wait for shutdown signal (SIGTERM or SIGINT).
    shutdown_signal().await;
    tracing::info!("shutdown signal received");
    let _ = shutdown_tx.send(());

    // Wait for subsystems to finish.
    let _ = control_handle.await;
    if let Some(h) = watcher_handle {
        let _ = h.await;
    }
    if let Some(h) = auto_linker_handle {
        let _ = h.await;
    }

    // Clean up PID file and socket.
    if let Err(e) = pid_file.remove() {
        tracing::warn!("failed to remove PID file: {e}");
    }
    if socket_path.exists() {
        if let Err(e) = std::fs::remove_file(&socket_path) {
            tracing::warn!("failed to remove socket file: {e}");
        }
    }

    tracing::info!("clio-daemon stopped");
    Ok(())
}

/// Wait for SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to register SIGTERM handler — the OS does not support signal handling");
        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
    }
}

/// Parse `--db-path <path>` from command-line arguments.
fn parse_db_path_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--db-path" {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix("--db-path=") {
            return Some(value.to_string());
        }
    }
    None
}
