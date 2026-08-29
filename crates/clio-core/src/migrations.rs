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
    Migration {
        version: "014_checkpoint_usage",
        sql: r#"
            ALTER TABLE session_checkpoints ADD COLUMN model TEXT;
            ALTER TABLE session_checkpoints ADD COLUMN input_tokens INTEGER;
            ALTER TABLE session_checkpoints ADD COLUMN cached_input_tokens INTEGER;
            ALTER TABLE session_checkpoints ADD COLUMN output_tokens INTEGER;
            ALTER TABLE session_checkpoints ADD COLUMN reasoning_tokens INTEGER;
        "#,
    },
    Migration {
        version: "015_memory_repair_journal",
        sql: r#"
            CREATE TABLE repair_transactions (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK (kind IN ('repair', 'rollback')),
                manifest_digest TEXT NOT NULL,
                forward_transaction_id TEXT,
                manifest_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                committed_at TEXT NOT NULL,
                FOREIGN KEY (forward_transaction_id) REFERENCES repair_transactions(id)
            );

            CREATE UNIQUE INDEX idx_repair_transactions_forward_rollback
                ON repair_transactions(forward_transaction_id)
                WHERE kind = 'rollback';

            CREATE TABLE repair_journal_entries (
                transaction_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                entity_type TEXT NOT NULL
                    CHECK (entity_type IN ('memory', 'attention', 'link')),
                entity_key TEXT NOT NULL,
                operation TEXT NOT NULL
                    CHECK (operation IN ('update', 'delete', 'insert')),
                before_json TEXT,
                after_json TEXT,
                evidence_json TEXT NOT NULL DEFAULT '{}',
                PRIMARY KEY (transaction_id, sequence),
                UNIQUE (transaction_id, entity_type, entity_key),
                FOREIGN KEY (transaction_id) REFERENCES repair_transactions(id)
                    ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
                CHECK (before_json IS NOT NULL OR after_json IS NOT NULL)
            );

            CREATE TRIGGER repair_transactions_immutable_update
            BEFORE UPDATE ON repair_transactions BEGIN
                SELECT RAISE(ABORT, 'repair transactions are immutable');
            END;

            CREATE TRIGGER repair_transactions_immutable_delete
            BEFORE DELETE ON repair_transactions BEGIN
                SELECT RAISE(ABORT, 'repair transactions are immutable');
            END;

            CREATE TRIGGER repair_journal_entries_immutable_update
            BEFORE UPDATE ON repair_journal_entries BEGIN
                SELECT RAISE(ABORT, 'repair journal entries are immutable');
            END;

            CREATE TRIGGER repair_journal_entries_immutable_delete
            BEFORE DELETE ON repair_journal_entries BEGIN
                SELECT RAISE(ABORT, 'repair journal entries are immutable');
            END;

            CREATE TRIGGER repair_journal_entries_closed_insert
            BEFORE INSERT ON repair_journal_entries
            WHEN EXISTS (
                SELECT 1 FROM repair_transactions
                WHERE id = new.transaction_id
            ) BEGIN
                SELECT RAISE(ABORT, 'committed repair journals are closed');
            END;

            CREATE TABLE memory_store_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                generation INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            INSERT INTO memory_store_state(singleton, generation, updated_at)
            VALUES (1, 0, strftime('%Y-%m-%dT%H:%M:%fZ','now'));

            CREATE TRIGGER memory_store_memories_ai AFTER INSERT ON memories BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_memories_au AFTER UPDATE ON memories BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_memories_ad AFTER DELETE ON memories BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_links_ai AFTER INSERT ON memory_links BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_links_au AFTER UPDATE ON memory_links BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_links_ad AFTER DELETE ON memory_links BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_attention_ai AFTER INSERT ON attention_items BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_attention_au AFTER UPDATE ON attention_items BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;

            CREATE TRIGGER memory_store_attention_ad AFTER DELETE ON attention_items BEGIN
                UPDATE memory_store_state SET
                    generation = generation + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE singleton = 1;
            END;
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
    let result = run_pending_in_transaction(conn);

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

/// Apply pending migrations inside a transaction already owned by the caller.
///
/// The repair path uses this so first installation of the repair schema and the
/// reviewed data mutation either commit together or both roll back.
pub(crate) fn run_pending_in_transaction(conn: &Connection) -> Result<()> {
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
