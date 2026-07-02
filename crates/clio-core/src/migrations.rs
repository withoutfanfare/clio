use rusqlite::Connection;

use crate::error::{ClioError, Result};
use crate::models::now_utc;

/// Each migration has a version string and the SQL to apply.
struct Migration {
    version: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: "001_initial",
        sql: r#"
        CREATE TABLE memories (
            id TEXT PRIMARY KEY,
            namespace TEXT NOT NULL DEFAULT 'global',
            kind TEXT NOT NULL DEFAULT 'note',
            title TEXT,
            summary TEXT,
            content TEXT NOT NULL,
            tags_text TEXT NOT NULL DEFAULT '',
            source TEXT,
            source_ref TEXT,
            confidence REAL,
            importance INTEGER NOT NULL DEFAULT 3,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            valid_from TEXT,
            valid_until TEXT,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            CHECK (length(namespace) BETWEEN 1 AND 120),
            CHECK (length(kind) BETWEEN 1 AND 50),
            CHECK (title IS NULL OR length(title) <= 240),
            CHECK (summary IS NULL OR length(summary) <= 1000),
            CHECK (importance BETWEEN 1 AND 5),
            CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0))
        );

        CREATE TABLE memory_tags (
            memory_id TEXT NOT NULL,
            tag TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (memory_id, tag),
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            CHECK (length(tag) BETWEEN 1 AND 60)
        );

        CREATE TABLE memory_links (
            from_memory_id TEXT NOT NULL,
            to_memory_id TEXT NOT NULL,
            relationship TEXT NOT NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            PRIMARY KEY (from_memory_id, to_memory_id, relationship),
            FOREIGN KEY (from_memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (to_memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            CHECK (length(relationship) BETWEEN 1 AND 60)
        );

        CREATE INDEX idx_memories_namespace ON memories(namespace);
        CREATE INDEX idx_memories_kind ON memories(kind);
        CREATE INDEX idx_memories_updated_at ON memories(updated_at DESC);
        CREATE INDEX idx_memories_archived_at ON memories(archived_at);
        CREATE INDEX idx_memories_source ON memories(source);

        CREATE UNIQUE INDEX idx_memories_source_ref
            ON memories(source, source_ref)
            WHERE source IS NOT NULL AND source_ref IS NOT NULL;

        CREATE INDEX idx_memory_tags_tag ON memory_tags(tag);
        CREATE INDEX idx_memory_links_from ON memory_links(from_memory_id);
        CREATE INDEX idx_memory_links_to ON memory_links(to_memory_id);

        CREATE VIRTUAL TABLE memory_fts USING fts5(
            title,
            summary,
            content,
            tags_text,
            content='memories',
            content_rowid='rowid',
            tokenize='porter unicode61'
        );

        CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memory_fts(rowid, title, summary, content, tags_text)
            VALUES (new.rowid, new.title, new.summary, new.content, new.tags_text);
        END;

        CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memory_fts(memory_fts, rowid, title, summary, content, tags_text)
            VALUES ('delete', old.rowid, old.title, old.summary, old.content, old.tags_text);
        END;

        CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memory_fts(memory_fts, rowid, title, summary, content, tags_text)
            VALUES ('delete', old.rowid, old.title, old.summary, old.content, old.tags_text);
            INSERT INTO memory_fts(rowid, title, summary, content, tags_text)
            VALUES (new.rowid, new.title, new.summary, new.content, new.tags_text);
        END;
    "#,
    },
    Migration {
        version: "002_embeddings",
        sql: r#"
            CREATE TABLE memory_embeddings (
                memory_id TEXT PRIMARY KEY,
                model TEXT NOT NULL,
                dimensions INTEGER NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );
        "#,
    },
    Migration {
        version: "003_review_queue",
        sql: r#"
            CREATE TABLE IF NOT EXISTS review_queue (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                suggested_namespace TEXT NOT NULL DEFAULT 'global',
                suggested_kind TEXT NOT NULL DEFAULT 'note',
                suggested_title TEXT,
                suggested_summary TEXT,
                suggested_tags TEXT NOT NULL DEFAULT '',
                suggested_importance INTEGER NOT NULL DEFAULT 3,
                suggested_confidence REAL,
                source_route TEXT,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'approved', 'rejected', 'edited')),
                created_at TEXT NOT NULL,
                reviewed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_review_queue_status ON review_queue(status);
        "#,
    },
    Migration {
        version: "004_access_tracking",
        sql: r#"
            ALTER TABLE memories ADD COLUMN last_accessed_at TEXT;
            ALTER TABLE memories ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0;

            CREATE INDEX idx_memories_last_accessed_at
                ON memories(last_accessed_at DESC)
                WHERE last_accessed_at IS NOT NULL;
        "#,
    },
    Migration {
        version: "005_composite_indexes",
        sql: r#"
            CREATE INDEX IF NOT EXISTS idx_memories_active_namespace
                ON memories(namespace) WHERE archived_at IS NULL;

            CREATE INDEX IF NOT EXISTS idx_memories_active_kind
                ON memories(kind) WHERE archived_at IS NULL;
        "#,
    },
    Migration {
        version: "006_scoped_recall_indexes",
        sql: r#"
            CREATE INDEX IF NOT EXISTS idx_memories_active_namespace_kind
                ON memories(namespace, kind) WHERE archived_at IS NULL;

            CREATE INDEX IF NOT EXISTS idx_review_queue_created_at
                ON review_queue(created_at DESC);
        "#,
    },
    Migration {
        version: "007_content_dedup_index",
        sql: r#"
            -- Speeds up the exact-content duplicate probe (capture / review) by
            -- narrowing candidates on (namespace, content length) before the full
            -- content comparison — cheaper than indexing full content.
            CREATE INDEX IF NOT EXISTS idx_memories_content_dedup
                ON memories(namespace, length(content));
        "#,
    },
];

/// Run all pending migrations inside a transaction.
pub fn run(conn: &Connection) -> Result<()> {
    // Ensure the migrations table exists (outside the versioned migrations).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    let applied: Vec<String> = {
        let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    for migration in MIGRATIONS {
        if applied.contains(&migration.version.to_string()) {
            continue;
        }

        tracing::info!(version = migration.version, "applying migration");

        // Wrap each migration + its version record in a savepoint so that a
        // partial failure rolls back cleanly rather than leaving the schema in
        // an inconsistent state.
        conn.execute_batch("SAVEPOINT apply_migration")?;
        let result = (|| -> Result<()> {
            conn.execute_batch(migration.sql).map_err(|e| {
                ClioError::Migration(format!(
                    "failed to apply migration {}: {e}",
                    migration.version
                ))
            })?;

            conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                rusqlite::params![migration.version, now_utc()],
            )?;
            Ok(())
        })();

        match result {
            Ok(()) => conn.execute_batch("RELEASE apply_migration")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK TO apply_migration");
                let _ = conn.execute_batch("RELEASE apply_migration");
                return Err(e);
            }
        }
    }

    Ok(())
}

/// Return the list of applied migration versions.
pub fn applied_versions(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
