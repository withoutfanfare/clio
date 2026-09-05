use clio_core::attention::{self, AttentionInput};
use clio_core::models::{LinkInput, RememberInput};
use clio_core::repair::{
    RepairAction, RepairIntent, RepairPlan, apply_manifest, apply_manifest_with_pending_migrations,
    build_manifest_at, rollback_transaction,
};
use clio_core::{db, repository};

fn remember(conn: &rusqlite::Connection, namespace: &str, title: &str) -> String {
    repository::remember(
        conn,
        &RememberInput {
            namespace: namespace.to_string(),
            kind: "fact".to_string(),
            title: Some(title.to_string()),
            summary: Some(format!("Summary for {title}")),
            content: format!("Durable content for {title}"),
            tags: vec!["audit".to_string(), "stable".to_string()],
            source: Some("repair-test".to_string()),
            source_ref: Some(title.to_string()),
            confidence: Some(0.9),
            importance: 4,
            metadata: serde_json::json!({"preserve": true}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Default::default(),
    )
    .unwrap()
    .id
}

fn intent(memory_id: &str, action: RepairAction) -> RepairIntent {
    RepairIntent {
        memory_id: memory_id.to_string(),
        action,
        evidence: "2026-08-29 evidence-backed audit classification".to_string(),
    }
}

#[test]
fn migration_creates_immutable_repair_journal_and_global_generation() {
    let conn = db::open_in_memory().unwrap();

    for table in [
        "repair_transactions",
        "repair_journal_entries",
        "memory_store_state",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                    WHERE type = 'table' AND name = ?1
                )",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "missing table {table}");
    }

    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM memory_store_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generation, 0);

    let immutable_triggers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger'
               AND name IN (
                   'repair_transactions_immutable_update',
                   'repair_transactions_immutable_delete',
                   'repair_journal_entries_immutable_update',
                   'repair_journal_entries_immutable_delete',
                   'repair_journal_entries_closed_insert'
               )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(immutable_triggers, 5);

    conn.execute_batch(
        "BEGIN;
         INSERT INTO repair_journal_entries
         (transaction_id, sequence, entity_type, entity_key, operation, before_json)
         VALUES ('repair:immutable', 0, 'memory', 'one', 'update', '{}');
         INSERT INTO repair_transactions
         (id, kind, manifest_digest, manifest_json, created_at, committed_at)
         VALUES ('repair:immutable', 'repair', 'digest', '{}',
                 '2026-08-29T06:00:00Z', '2026-08-29T06:00:00Z');
         COMMIT;",
    )
    .unwrap();
    for statement in [
        "UPDATE repair_transactions SET committed_at = 'later' WHERE id = 'repair:immutable'",
        "DELETE FROM repair_transactions WHERE id = 'repair:immutable'",
        "UPDATE repair_journal_entries SET entity_key = 'changed' WHERE transaction_id = 'repair:immutable'",
        "DELETE FROM repair_journal_entries WHERE transaction_id = 'repair:immutable'",
    ] {
        assert!(
            conn.execute(statement, []).is_err(),
            "immutable journal statement unexpectedly succeeded: {statement}"
        );
    }

    conn.execute(
        "INSERT INTO repair_transactions
         (id, kind, manifest_digest, manifest_json, created_at, committed_at)
         VALUES ('repair:closed', 'repair', 'digest', '{}',
                 '2026-08-29T06:00:00Z', '2026-08-29T06:00:00Z')",
        [],
    )
    .unwrap();
    let late_insert = conn.execute(
        "INSERT INTO repair_journal_entries
         (transaction_id, sequence, entity_type, entity_key, operation, before_json)
         VALUES ('repair:closed', 0, 'memory', 'late', 'update', '{}')",
        [],
    );
    assert!(
        late_insert.is_err(),
        "committed journals must reject late appends"
    );
}

#[test]
fn manifest_is_deterministic_and_captures_attention_and_every_touching_link() {
    let conn = db::open_in_memory().unwrap();
    let moved = remember(&conn, "project:legacy", "moved");
    let neighbour = remember(&conn, "project:other", "neighbour");
    let archived = remember(&conn, "project:legacy", "archived");

    attention::create_attention(
        &conn,
        &AttentionInput {
            memory_id: moved.clone(),
            owner: Some("user".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    for (from, to, relationship) in [
        (&moved, &neighbour, "auto:relates_to"),
        (&neighbour, &moved, "supports"),
        (&archived, &neighbour, "auto:relates_to"),
    ] {
        repository::link(
            &conn,
            &LinkInput {
                from_memory_id: from.clone(),
                to_memory_id: to.clone(),
                relationship: relationship.to_string(),
                metadata: serde_json::json!({"origin": relationship}),
            },
        )
        .unwrap();
    }

    let plan = RepairPlan {
        intents: vec![
            intent(&archived, RepairAction::Archive),
            intent(
                &moved,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            ),
        ],
    };
    let generated_at = "2026-08-29T06:00:00Z";

    let first = build_manifest_at(&conn, &plan, generated_at).unwrap();
    let second = build_manifest_at(&conn, &plan, generated_at).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.quick_check, "ok");
    assert_eq!(first.targets.len(), 2);
    assert_eq!(first.touching_links.len(), 3);
    assert_eq!(first.removed_links.len(), 2);
    assert_eq!(
        first
            .targets
            .iter()
            .filter(|target| target.attention.is_some())
            .count(),
        1
    );
    assert!(first.transaction_id.starts_with("repair:"));
    assert_eq!(first.transaction_id, format!("repair:{}", first.digest));

    let retained = first
        .touching_links
        .iter()
        .find(|link| link.relationship == "supports")
        .unwrap();
    assert!(!first.removed_links.contains(retained));
}

#[test]
fn apply_and_rollback_preserve_semantic_state_and_are_idempotent() {
    let conn = db::open_in_memory().unwrap();
    let moved = remember(&conn, "project:legacy", "round-trip");
    let neighbour = remember(&conn, "project:other", "linked");
    let original = repository::get(&conn, &moved).unwrap();
    let attention = attention::create_attention(
        &conn,
        &AttentionInput {
            memory_id: moved.clone(),
            waiting_on: Some("approval".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: moved.clone(),
            to_memory_id: neighbour.clone(),
            relationship: "auto:relates_to".to_string(),
            metadata: serde_json::json!({"score": 0.91}),
        },
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_embeddings(memory_id, model, dimensions, embedding, created_at)
         VALUES (?1, 'test', 1, X'0000803F', '2026-08-29T05:00:00Z')",
        [&moved],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_occurrences
         (id, memory_id, source, source_ref, occurred_at, metadata_json)
         VALUES ('occurrence-1', ?1, 'test', 'one', '2026-08-29T05:00:00Z', '{}')",
        [&moved],
    )
    .unwrap();

    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![intent(
                &moved,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            )],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();

    let applied = apply_manifest(&conn, &manifest).unwrap();
    assert!(!applied.replayed);
    assert!(apply_manifest(&conn, &manifest).unwrap().replayed);

    let repaired = repository::get(&conn, &moved).unwrap();
    assert_eq!(repaired.namespace, "project:canonical");
    assert_eq!(repaired.updated_at, original.updated_at);
    assert_eq!(repaired.content, original.content);
    assert_eq!(repaired.tags, original.tags);
    assert_eq!(
        attention::get_attention(&conn, &attention.id)
            .unwrap()
            .namespace,
        "project:canonical"
    );
    assert!(repository::get_links(&conn, &moved).unwrap().is_empty());
    for table in ["memory_embeddings", "memory_occurrences"] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE memory_id = ?1"),
                [&moved],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "{table} changed during repair");
    }

    let rolled_back = rollback_transaction(&conn, &manifest.transaction_id).unwrap();
    assert!(!rolled_back.replayed);
    assert!(
        rollback_transaction(&conn, &manifest.transaction_id)
            .unwrap()
            .replayed
    );

    let restored = repository::get(&conn, &moved).unwrap();
    assert_eq!(restored.namespace, original.namespace);
    assert_eq!(restored.updated_at, original.updated_at);
    assert_eq!(
        attention::get_attention(&conn, &attention.id)
            .unwrap()
            .namespace,
        "project:legacy"
    );
    assert_eq!(repository::get_links(&conn, &moved).unwrap().len(), 1);
    assert!(
        apply_manifest(&conn, &manifest).is_err(),
        "a forward transaction that has been rolled back must not replay as repaired"
    );
}

#[test]
fn archive_apply_and_rollback_preserve_the_memory_and_restore_visibility() {
    let conn = db::open_in_memory().unwrap();
    let target = remember(&conn, "project:retired", "archive-round-trip");
    let original = repository::get(&conn, &target).unwrap();
    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![intent(&target, RepairAction::Archive)],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();

    apply_manifest(&conn, &manifest).unwrap();
    let archived = repository::get(&conn, &target).unwrap();
    assert_eq!(
        archived.archived_at.as_deref(),
        Some("2026-08-29T06:00:00Z")
    );
    assert_eq!(archived.updated_at, original.updated_at);
    assert_eq!(archived.content, original.content);

    rollback_transaction(&conn, &manifest.transaction_id).unwrap();
    let restored = repository::get(&conn, &target).unwrap();
    assert!(restored.archived_at.is_none());
    assert_eq!(restored.updated_at, original.updated_at);
    assert_eq!(restored.content, original.content);
}

#[test]
fn apply_conflict_leaves_memories_links_and_journal_unchanged() {
    let conn = db::open_in_memory().unwrap();
    let target = remember(&conn, "project:legacy", "conflict");
    let neighbour = remember(&conn, "project:canonical", "conflict-neighbour");
    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![intent(
                &target,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            )],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();

    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: target.clone(),
            to_memory_id: neighbour,
            relationship: "supports".to_string(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();

    let error = apply_manifest(&conn, &manifest).unwrap_err();
    assert!(error.to_string().contains("Conflict"));
    assert_eq!(
        repository::get(&conn, &target).unwrap().namespace,
        "project:legacy"
    );
    let journal_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM repair_transactions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(journal_count, 0);
}

#[test]
fn attention_and_existing_link_drift_each_abort_the_complete_apply() {
    let attention_conn = db::open_in_memory().unwrap();
    let attention_target = remember(&attention_conn, "project:legacy", "attention-drift");
    let item = attention::create_attention(
        &attention_conn,
        &AttentionInput {
            memory_id: attention_target.clone(),
            waiting_on: Some("review".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    let attention_manifest = build_manifest_at(
        &attention_conn,
        &RepairPlan {
            intents: vec![intent(
                &attention_target,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            )],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();
    attention_conn
        .execute(
            "UPDATE attention_items SET waiting_on = 'changed' WHERE id = ?1",
            [&item.id],
        )
        .unwrap();
    assert!(apply_manifest(&attention_conn, &attention_manifest).is_err());
    assert_eq!(
        repository::get(&attention_conn, &attention_target)
            .unwrap()
            .namespace,
        "project:legacy"
    );

    for operation in ["modify", "delete"] {
        let conn = db::open_in_memory().unwrap();
        let target = remember(&conn, "project:legacy", &format!("link-{operation}"));
        let neighbour = remember(
            &conn,
            "project:other",
            &format!("link-neighbour-{operation}"),
        );
        repository::link(
            &conn,
            &LinkInput {
                from_memory_id: target.clone(),
                to_memory_id: neighbour.clone(),
                relationship: "auto:relates_to".to_string(),
                metadata: serde_json::json!({"version": 1}),
            },
        )
        .unwrap();
        let manifest = build_manifest_at(
            &conn,
            &RepairPlan {
                intents: vec![intent(
                    &target,
                    RepairAction::Move {
                        namespace: "project:canonical".to_string(),
                    },
                )],
            },
            "2026-08-29T06:00:00Z",
        )
        .unwrap();
        if operation == "modify" {
            conn.execute(
                "UPDATE memory_links SET metadata_json = '{\"version\":2}'
                 WHERE from_memory_id = ?1 AND to_memory_id = ?2",
                [&target, &neighbour],
            )
            .unwrap();
        } else {
            conn.execute(
                "DELETE FROM memory_links WHERE from_memory_id = ?1 AND to_memory_id = ?2",
                [&target, &neighbour],
            )
            .unwrap();
        }
        assert!(apply_manifest(&conn, &manifest).is_err(), "{operation}");
        assert_eq!(
            repository::get(&conn, &target).unwrap().namespace,
            "project:legacy"
        );
        let transaction_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM repair_transactions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(transaction_count, 0, "{operation}");
    }
}

#[test]
fn an_existing_transaction_id_with_different_content_fails_closed() {
    let conn = db::open_in_memory().unwrap();
    let target = remember(&conn, "project:legacy", "transaction-collision");
    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![intent(
                &target,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            )],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO repair_transactions
         (id, kind, manifest_digest, manifest_json, created_at, committed_at)
         VALUES (?1, 'repair', 'different', '{}', ?2, ?2)",
        rusqlite::params![manifest.transaction_id, "2026-08-29T06:00:00Z"],
    )
    .unwrap();

    let error = apply_manifest(&conn, &manifest).unwrap_err();
    assert!(error.to_string().contains("different content"));
    assert_eq!(
        repository::get(&conn, &target).unwrap().namespace,
        "project:legacy"
    );
}

#[test]
fn apply_conflict_when_the_full_snapshot_changes_after_manifest_creation() {
    let conn = db::open_in_memory().unwrap();
    let target = remember(&conn, "project:legacy", "snapshot-target");
    let unrelated = remember(&conn, "project:unrelated", "snapshot-drift");
    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![intent(
                &target,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            )],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();

    repository::move_namespace(&conn, &unrelated, "project:changed").unwrap();

    let error = apply_manifest(&conn, &manifest).unwrap_err();
    assert!(error.to_string().contains("snapshot changed"));
    assert_eq!(
        repository::get(&conn, &target).unwrap().namespace,
        "project:legacy"
    );
    let journal_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM repair_transactions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(journal_count, 0);
}

#[test]
fn migration_and_repair_roll_back_together_on_a_stale_manifest() {
    let conn = db::open_in_memory().unwrap();
    let target = remember(&conn, "project:legacy", "atomic-target");
    let unrelated = remember(&conn, "project:other", "atomic-unrelated");
    conn.execute(
        "UPDATE memory_store_state SET generation = 0 WHERE singleton = 1",
        [],
    )
    .unwrap();
    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![intent(
                &target,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            )],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();

    conn.execute_batch(
        "DROP TRIGGER memory_store_memories_ai;
         DROP TRIGGER memory_store_memories_au;
         DROP TRIGGER memory_store_memories_ad;
         DROP TRIGGER memory_store_links_ai;
         DROP TRIGGER memory_store_links_au;
         DROP TRIGGER memory_store_links_ad;
         DROP TRIGGER memory_store_attention_ai;
         DROP TRIGGER memory_store_attention_au;
         DROP TRIGGER memory_store_attention_ad;
         DROP TRIGGER repair_transactions_immutable_update;
         DROP TRIGGER repair_transactions_immutable_delete;
         DROP TRIGGER repair_journal_entries_immutable_update;
         DROP TRIGGER repair_journal_entries_immutable_delete;
         DROP TRIGGER repair_journal_entries_closed_insert;
         DROP TABLE repair_journal_entries;
         DROP TABLE repair_transactions;
         DROP TABLE memory_store_state;
         DELETE FROM schema_migrations WHERE version = '015_memory_repair_journal';",
    )
    .unwrap();
    conn.execute(
        "UPDATE memories SET namespace = 'project:drifted' WHERE id = ?1",
        [&unrelated],
    )
    .unwrap();

    let error = apply_manifest_with_pending_migrations(&conn, &manifest).unwrap_err();
    assert!(error.to_string().contains("snapshot changed"));
    let repair_schema_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'repair_transactions')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !repair_schema_exists,
        "a conflicting apply must roll migration 015 back too"
    );
    let migration_recorded: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = '015_memory_repair_journal')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!migration_recorded);
    assert_eq!(
        repository::get(&conn, &target).unwrap().namespace,
        "project:legacy"
    );

    conn.execute(
        "UPDATE memories SET namespace = 'project:other' WHERE id = ?1",
        [&unrelated],
    )
    .unwrap();
    apply_manifest_with_pending_migrations(&conn, &manifest).unwrap();
    assert_eq!(
        repository::get(&conn, &target).unwrap().namespace,
        "project:canonical"
    );
    let migration_recorded: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = '015_memory_repair_journal')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(migration_recorded);
}

#[test]
fn rollback_conflict_is_atomic_when_repaired_state_has_drifted() {
    let conn = db::open_in_memory().unwrap();
    let first = remember(&conn, "project:legacy", "rollback-one");
    let second = remember(&conn, "project:legacy", "rollback-two");
    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![
                intent(
                    &first,
                    RepairAction::Move {
                        namespace: "project:canonical".to_string(),
                    },
                ),
                intent(
                    &second,
                    RepairAction::Move {
                        namespace: "project:canonical".to_string(),
                    },
                ),
            ],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();
    apply_manifest(&conn, &manifest).unwrap();

    conn.execute(
        "UPDATE memories SET namespace = 'project:drifted' WHERE id = ?1",
        [&second],
    )
    .unwrap();

    let error = rollback_transaction(&conn, &manifest.transaction_id).unwrap_err();
    assert!(error.to_string().contains("Conflict"));
    assert_eq!(
        repository::get(&conn, &first).unwrap().namespace,
        "project:canonical"
    );
    assert_eq!(
        repository::get(&conn, &second).unwrap().namespace,
        "project:drifted"
    );
    let rollback_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM repair_transactions WHERE kind = 'rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rollback_count, 0);
}

#[test]
fn rollback_replay_fails_when_the_restored_state_has_later_drifted() {
    let conn = db::open_in_memory().unwrap();
    let target = remember(&conn, "project:legacy", "rollback-replay-drift");
    let manifest = build_manifest_at(
        &conn,
        &RepairPlan {
            intents: vec![intent(
                &target,
                RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
            )],
        },
        "2026-08-29T06:00:00Z",
    )
    .unwrap();
    apply_manifest(&conn, &manifest).unwrap();
    rollback_transaction(&conn, &manifest.transaction_id).unwrap();
    conn.execute(
        "UPDATE memories SET namespace = 'project:later-work' WHERE id = ?1",
        [&target],
    )
    .unwrap();

    let error = rollback_transaction(&conn, &manifest.transaction_id).unwrap_err();
    assert!(error.to_string().contains("Conflict"));
    assert_eq!(
        repository::get(&conn, &target).unwrap().namespace,
        "project:later-work"
    );
}
