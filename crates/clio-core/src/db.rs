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

    enable_wal(&conn)?;
    apply_connection_pragmas(&conn)?;
    migrations::run(&conn)?;

    Ok(conn)
}

/// Open an existing database without applying pending migrations or changing
/// its persistent journal mode.
///
/// This is intentionally narrow: evidence-backed repair commands need to put
/// migration installation and data mutation inside one outer transaction.
pub fn open_existing_unmigrated(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(ClioError::Config(format!(
            "database does not exist at {}",
            path.display()
        )));
    }
    let conn = Connection::open(path).map_err(|e| {
        ClioError::Storage(format!(
            "database could not be opened at {}: {e}",
            path.display()
        ))
    })?;
    apply_connection_pragmas(&conn)?;
    Ok(conn)
}

/// Open an existing database read-only without migrations or persistent
/// pragmas. Repair journal export uses this path so evidence retrieval cannot
/// change the database it is inspecting.
pub fn open_existing_read_only(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(ClioError::Config(format!(
            "database does not exist at {}",
            path.display()
        )));
    }
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| {
            ClioError::Storage(format!(
                "database could not be opened read-only at {}: {e}",
                path.display()
            ))
        })?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.execute_batch("PRAGMA query_only = ON; PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

/// Open an in-memory database for testing. Applies pragmas and runs migrations.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    enable_wal(&conn)?;
    apply_connection_pragmas(&conn)?;
    migrations::run(&conn)?;
    Ok(conn)
}

fn enable_wal(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    Ok(())
}

fn apply_connection_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA wal_autocheckpoint = 1000;",
    )?;
    Ok(())
}

/// Commit an owned transaction or release a nested savepoint.
///
/// SQLite can leave the transaction open when the commit itself fails, so
/// always unwind it before returning the original error.
pub(crate) fn finish_transaction(
    conn: &Connection,
    owns_transaction: bool,
    savepoint: &str,
) -> Result<()> {
    let result = if owns_transaction {
        conn.execute_batch("COMMIT")
    } else {
        conn.execute_batch(&format!("RELEASE {savepoint}"))
    };
    if let Err(error) = result {
        rollback_transaction(conn, owns_transaction, savepoint);
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn rollback_transaction(conn: &Connection, owns_transaction: bool, savepoint: &str) {
    if owns_transaction {
        let _ = conn.execute_batch("ROLLBACK");
    } else {
        let _ = conn.execute_batch(&format!("ROLLBACK TO {savepoint}; RELEASE {savepoint};"));
    }
}

pub(crate) fn with_savepoint<T>(
    conn: &Connection,
    savepoint: &str,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    conn.execute_batch(&format!("SAVEPOINT {savepoint}"))?;
    match operation() {
        Ok(value) => {
            finish_transaction(conn, false, savepoint)?;
            Ok(value)
        }
        Err(error) => {
            rollback_transaction(conn, false, savepoint);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::finish_transaction;

    #[test]
    fn failed_commit_does_not_leave_the_connection_in_a_transaction() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE parents (id INTEGER PRIMARY KEY);
             CREATE TABLE children (
                 parent_id INTEGER,
                 FOREIGN KEY (parent_id) REFERENCES parents(id)
                     DEFERRABLE INITIALLY DEFERRED
             );
             BEGIN IMMEDIATE;
             INSERT INTO children (parent_id) VALUES (1);",
        )
        .unwrap();

        assert!(finish_transaction(&conn, true, "").is_err());
        assert!(conn.is_autocommit());
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM children", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
