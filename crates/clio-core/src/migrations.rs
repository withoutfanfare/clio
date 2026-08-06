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
    Migration {
        version: "008_review_source_ref",
        sql: r#"
            ALTER TABLE review_queue ADD COLUMN source_ref TEXT;
        "#,
    },
    Migration {
        version: "009_session_checkpoints",
        sql: r#"
            CREATE TABLE session_checkpoints (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                session_id TEXT NOT NULL,
                cursor INTEGER NOT NULL,
                namespace TEXT,
                branch TEXT,
                ticket TEXT,
                result_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE UNIQUE INDEX idx_session_checkpoints_identity
                ON session_checkpoints(source, session_id, cursor);
        "#,
    },
    Migration {
        version: "010_attention_and_events",
        sql: r#"
            CREATE TABLE attention_items (
                id TEXT PRIMARY KEY,
                memory_id TEXT NOT NULL UNIQUE,
                namespace TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'open'
                    CHECK (status IN ('open', 'snoozed', 'resolved', 'cancelled')),
                owner TEXT,
                due_at TEXT,
                remind_at TEXT,
                trigger_kind TEXT,
                waiting_on TEXT,
                completion_condition TEXT,
                external_system TEXT,
                external_ref TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                resolved_at TEXT,
                FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE INDEX idx_attention_status ON attention_items(status, namespace);

            CREATE TABLE memory_events (
                id TEXT PRIMARY KEY,
                idempotency_key TEXT,
                memory_id TEXT,
                namespace TEXT,
                actor TEXT,
                session_id TEXT,
                topic TEXT,
                event_type TEXT NOT NULL CHECK (length(event_type) BETWEEN 1 AND 40),
                reason TEXT,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );

            CREATE UNIQUE INDEX idx_memory_events_idempotency
                ON memory_events(idempotency_key) WHERE idempotency_key IS NOT NULL;
            CREATE INDEX idx_memory_events_memory ON memory_events(memory_id, created_at);
        "#,
    },
    Migration {
        version: "011_occurrences_and_namespace_state",
        sql: r#"
            CREATE TABLE memory_occurrences (
                id TEXT PRIMARY KEY,
                memory_id TEXT NOT NULL,
                source TEXT,
                source_ref TEXT,
                session_id TEXT,
                occurred_at TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE INDEX idx_memory_occurrences_memory
                ON memory_occurrences(memory_id, occurred_at);

            CREATE UNIQUE INDEX idx_memory_occurrences_provenance
                ON memory_occurrences(memory_id, source, source_ref)
                WHERE source IS NOT NULL AND source_ref IS NOT NULL;

            CREATE TABLE namespace_state (
                namespace TEXT PRIMARY KEY,
                generation INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE TRIGGER memories_gen_ai AFTER INSERT ON memories BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                VALUES (new.namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;

            CREATE TRIGGER memories_gen_au AFTER UPDATE ON memories BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                VALUES (old.namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
                INSERT INTO namespace_state(namespace, generation, updated_at)
                SELECT new.namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE new.namespace != old.namespace
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;

            CREATE TRIGGER memories_gen_ad AFTER DELETE ON memories BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                VALUES (old.namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;

            CREATE TRIGGER links_gen_ai AFTER INSERT ON memory_links BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                SELECT namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now')
                FROM memories WHERE id IN (new.from_memory_id, new.to_memory_id)
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;

            CREATE TRIGGER links_gen_ad AFTER DELETE ON memory_links BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                SELECT namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now')
                FROM memories WHERE id IN (old.from_memory_id, old.to_memory_id)
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;

            CREATE TRIGGER attention_gen_ai AFTER INSERT ON attention_items BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                VALUES (new.namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;

            CREATE TRIGGER attention_gen_au AFTER UPDATE ON attention_items BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                VALUES (new.namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;

            CREATE TRIGGER occurrences_gen_ai AFTER INSERT ON memory_occurrences BEGIN
                INSERT INTO namespace_state(namespace, generation, updated_at)
                SELECT namespace, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now')
                FROM memories WHERE id = new.memory_id
                ON CONFLICT(namespace) DO UPDATE SET
                    generation = generation + 1, updated_at = excluded.updated_at;
            END;
        "#,
    },
    Migration {
        version: "012_delivery_outbox",
        sql: r#"
            CREATE TABLE delivery_outbox (
                id TEXT PRIMARY KEY,
                delivery_key TEXT NOT NULL UNIQUE,
                attention_id TEXT NOT NULL,
                destination TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'delivering', 'delivered', 'failed')),
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                external_id TEXT,
                readback_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                delivered_at TEXT,
                FOREIGN KEY (attention_id) REFERENCES attention_items(id) ON DELETE CASCADE
            );

            CREATE INDEX idx_delivery_outbox_status ON delivery_outbox(status, destination);
        "#,
    },
    Migration {
        version: "013_delivery_external_identity",
        sql: r#"
            CREATE UNIQUE INDEX idx_delivery_outbox_external
                ON delivery_outbox(destination, external_id)
                WHERE external_id IS NOT NULL;
        "#,
    },
];

/// Run all pending migrations inside a transaction.
pub fn run(conn: &Connection) -> Result<()> {
    // The common startup path is read-only. Only contend for SQLite's write
    // lock when this process actually observes a pending migration.
    if migrations_current(conn)? {
        return Ok(());
    }

    // Acquire the write lock before reading migration state so simultaneous
    // MCP process starts cannot both decide that the same migration is pending.
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL
            );",
        )?;

        let applied: Vec<String> = {
            let mut stmt =
                conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };

        for migration in MIGRATIONS {
            if applied.iter().any(|version| version == migration.version) {
                continue;
            }

            tracing::info!(version = migration.version, "applying migration");
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
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            crate::db::finish_transaction(conn, true, "")?;
            Ok(())
        }
        Err(e) => {
            crate::db::rollback_transaction(conn, true, "");
            Err(e)
        }
    }
}

fn migrations_current(conn: &Connection) -> Result<bool> {
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'schema_migrations'
         )",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Ok(false);
    }

    let applied = applied_versions(conn)?;
    Ok(MIGRATIONS
        .iter()
        .all(|migration| applied.iter().any(|version| version == migration.version)))
}

/// Return the list of applied migration versions.
pub fn applied_versions(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
