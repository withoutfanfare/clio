use std::path::Path;

use rusqlite::Connection;

use crate::error::{ClioError, Result};
use crate::migrations;

/// Open (or create) the SQLite database at the given path and apply pragmas.
///
/// Creates parent directories if they don't exist. Runs pending migrations
/// automatically on every open.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ClioError::Config(format!(
                "could not create directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    let conn = Connection::open(path).map_err(|e| {
        ClioError::Storage(format!(
            "database could not be opened at {}: {e}",
            path.display()
        ))
    })?;

    apply_pragmas(&conn)?;
    migrations::run(&conn)?;

    Ok(conn)
}

/// Open an in-memory database for testing. Applies pragmas and runs migrations.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    apply_pragmas(&conn)?;
    migrations::run(&conn)?;
    Ok(conn)
}

fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}
