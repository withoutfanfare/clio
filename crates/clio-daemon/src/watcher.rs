//! Inbox directory watcher.
//!
//! Watches configured directories for new files, processes them through the
//! capture pipeline (or stores them as plain notes), and moves processed
//! files to a `_processed/` subdirectory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Event, EventKind, RecursiveMode, Watcher};

/// Watch inbox directories for new files and process them.
pub async fn watch(
    paths: Vec<PathBuf>,
    db_path: PathBuf,
    settings: clio_core::settings::Settings,
    embedding_backend: Option<Arc<dyn clio_core::embeddings::EmbeddingBackend>>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<PathBuf>(100);

    let mut watcher =
        notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event) => {
                if matches!(event.kind, EventKind::Create(_)) {
                    for path in event.paths {
                        let _ = tx.blocking_send(path);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("filesystem watcher error: {e}");
            }
        })?;

    let mut canonical_inbox_dirs = Vec::with_capacity(paths.len());
    for path in &paths {
        std::fs::create_dir_all(path)?;
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        tracing::info!(path = %path.display(), "watching inbox directory");
        canonical_inbox_dirs.push(path.canonicalize()?);
    }

    loop {
        tokio::select! {
            Some(file_path) = rx.recv() => {
                if should_process(&file_path, &canonical_inbox_dirs) {
                    process_file(&file_path, &db_path, &settings, &embedding_backend).await;
                }
            }
            _ = shutdown.recv() => {
                tracing::debug!("inbox watcher shutting down");
                break;
            }
        }
    }

    Ok(())
}

/// Determine whether a file should be processed. Skips hidden files,
/// directories, symlinks that resolve outside inbox dirs, and files in
/// `_processed/`.
fn should_process(path: &Path, inbox_dirs: &[PathBuf]) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    // Skip hidden files.
    if name.starts_with('.') {
        return false;
    }

    // Skip directories.
    if path.is_dir() {
        return false;
    }

    // Skip files in the _processed subdirectory.
    if path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("_processed")
    {
        return false;
    }

    // Resolve symlinks and verify the file is inside an inbox directory.
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(file = %path.display(), "could not canonicalise path: {e}");
            return false;
        }
    };
    if !inbox_dirs.iter().any(|dir| canonical.starts_with(dir)) {
        tracing::warn!(
            file = %path.display(),
            canonical = %canonical.display(),
            "file resolves outside inbox directories, skipping"
        );
        return false;
    }

    true
}

/// Read a file, run it through the capture pipeline, and move it to the
/// `_processed/` subdirectory.
async fn process_file(
    file_path: &Path,
    db_path: &Path,
    settings: &clio_core::settings::Settings,
    embedding_backend: &Option<Arc<dyn clio_core::embeddings::EmbeddingBackend>>,
) {
    tracing::info!(file = %file_path.display(), "processing inbox file");

    // Reject files larger than 10 MiB to prevent OOM from untrusted inbox drops.
    const MAX_INBOX_FILE_SIZE: u64 = 10 * 1024 * 1024;
    match std::fs::metadata(file_path) {
        Ok(m) if m.len() > MAX_INBOX_FILE_SIZE => {
            tracing::warn!(
                file = %file_path.display(),
                size = m.len(),
                "inbox file exceeds 10 MiB limit, skipping"
            );
            move_to_processed(file_path);
            return;
        }
        Err(e) => {
            tracing::warn!(file = %file_path.display(), "could not stat file: {e}");
            return;
        }
        _ => {}
    }

    // Read file content.
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(file = %file_path.display(), "could not read file: {e}");
            return;
        }
    };

    if content.trim().is_empty() {
        tracing::debug!(file = %file_path.display(), "skipping empty file");
        move_to_processed(file_path);
        return;
    }

    // Open the database once for the entire file processing pipeline.
    let conn = match clio_core::db::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("could not open database: {e}");
            return;
        }
    };

    // Run through the capture pipeline if enabled, otherwise store as a note.
    // The capture pipeline creates its own internal tokio runtime for HTTP calls,
    // so we must run it inside spawn_blocking to avoid a nested runtime panic.
    // We move the connection into the blocking task and return it so it can be
    // reused in fallback paths (rusqlite::Connection is Send).
    if settings.capture.enabled {
        let capture_content = content.clone();
        let capture_config = settings.capture.clone();
        let capture_settings = settings.clone();

        let capture_result = tokio::task::spawn_blocking(move || {
            let result = clio_core::capture::capture(
                &conn,
                &capture_content,
                &capture_config,
                None,
                &capture_settings,
            );
            (conn, result)
        })
        .await;

        match capture_result {
            Ok((_, Ok(result))) => {
                tracing::info!(
                    file = %file_path.display(),
                    ?result,
                    "capture pipeline succeeded"
                );
            }
            Ok((conn, Err(e))) => {
                tracing::warn!(
                    file = %file_path.display(),
                    "capture pipeline failed, storing as plain note: {e}"
                );
                store_as_note(&conn, &content, file_path, settings, embedding_backend);
            }
            Err(e) => {
                tracing::warn!(
                    file = %file_path.display(),
                    "capture task panicked, storing as plain note: {e}"
                );
                // Connection was lost in the panicked task; must re-open.
                if let Ok(conn) = clio_core::db::open(db_path) {
                    store_as_note(&conn, &content, file_path, settings, embedding_backend);
                } else {
                    tracing::error!("could not open database for fallback note storage");
                }
            }
        }
    } else {
        store_as_note(&conn, &content, file_path, settings, embedding_backend);
    }

    move_to_processed(file_path);
}

/// Store content as a simple note memory via the repository.
fn store_as_note(
    conn: &rusqlite::Connection,
    content: &str,
    file_path: &Path,
    settings: &clio_core::settings::Settings,
    embedding_backend: &Option<Arc<dyn clio_core::embeddings::EmbeddingBackend>>,
) {
    let title = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Inbox note");

    let input = clio_core::models::RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: Some(title.to_string()),
        summary: None,
        content: content.to_string(),
        tags: vec!["inbox".into()],
        source: Some("inbox-watcher".into()),
        source_ref: file_path.to_str().map(String::from),
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    match clio_core::repository::remember(conn, &input, settings) {
        Ok(memory) => {
            tracing::info!(id = %memory.id, "stored inbox file as note");

            // Auto-embed using the shared backend.
            if settings.auto_embed {
                if let Some(ref be) = *embedding_backend {
                    if let Err(e) =
                        clio_core::embeddings::embed_and_store(conn, be.as_ref(), &memory)
                    {
                        tracing::warn!("auto-embed failed for inbox note: {e}");
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("failed to store inbox file as note: {e}");
        }
    }
}

/// Move a processed file to the `_processed/` subdirectory alongside it.
fn move_to_processed(file_path: &Path) {
    let Some(parent) = file_path.parent() else {
        return;
    };
    let Some(file_name) = file_path.file_name() else {
        return;
    };

    let processed_dir = parent.join("_processed");
    if let Err(e) = std::fs::create_dir_all(&processed_dir) {
        tracing::warn!(
            dir = %processed_dir.display(),
            "could not create _processed directory: {e}"
        );
        return;
    }

    let mut dest = processed_dir.join(file_name);

    // Avoid overwriting a previously processed file with the same name.
    if dest.exists() {
        let stem = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = file_path.extension().and_then(|s| s.to_str());
        let dedup = uuid::Uuid::now_v7();
        let new_name = match ext {
            Some(e) => format!("{stem}-{dedup}.{e}"),
            None => format!("{stem}-{dedup}"),
        };
        dest = processed_dir.join(new_name);
    }

    if let Err(e) = std::fs::rename(file_path, &dest) {
        tracing::warn!(
            src = %file_path.display(),
            dst = %dest.display(),
            "could not move file to _processed: {e}"
        );
    } else {
        tracing::debug!(
            src = %file_path.display(),
            dst = %dest.display(),
            "moved to _processed"
        );
    }
}
