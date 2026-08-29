use std::process::Command;

use clio_core::models::RememberInput;
use clio_core::repair::{RepairAction, RepairIntent, RepairManifest, RepairPlan};
use clio_core::{db, repository};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_clio"))
        .args(args)
        .output()
        .unwrap()
}

fn namespace_at(db_path: &std::path::Path, memory_id: &str) -> String {
    rusqlite::Connection::open(db_path)
        .unwrap()
        .query_row(
            "SELECT namespace FROM memories WHERE id = ?1",
            [memory_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn repair_cli_requires_confirmation_and_round_trips_through_private_files() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("clio.db");
    let conn = db::open(&db_path).unwrap();
    let memory = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:legacy".to_string(),
            kind: "fact".to_string(),
            title: Some("CLI repair".to_string()),
            summary: None,
            content: "Preserve this content".to_string(),
            tags: vec!["repair".to_string()],
            source: Some("repair-cli-test".to_string()),
            source_ref: Some("one".to_string()),
            confidence: Some(1.0),
            importance: 4,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Default::default(),
    )
    .unwrap();
    drop(conn);

    let plan_path = directory.path().join("plan.json");
    let manifest_path = directory.path().join("manifest.json");
    let journal_path = directory.path().join("rollback.json");
    let exported_journal_path = directory.path().join("rollback-export.json");
    let rollback_journal_path = directory.path().join("rollback-result.json");
    let backup_dir = directory.path().join("backups");
    std::fs::write(
        &plan_path,
        serde_json::to_vec_pretty(&RepairPlan {
            intents: vec![RepairIntent {
                memory_id: memory.id.clone(),
                action: RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
                evidence: "integration test evidence".to_string(),
            }],
        })
        .unwrap(),
    )
    .unwrap();

    let manifest_output = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "--json",
        "repair",
        "manifest",
        "--plan",
        plan_path.to_str().unwrap(),
        "--output",
        manifest_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(
        manifest_output.status.success(),
        "{}",
        String::from_utf8_lossy(&manifest_output.stderr)
    );
    let manifest: RepairManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();

    let refused = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "apply",
        "--manifest",
        manifest_path.to_str().unwrap(),
        "--confirm",
        "wrong-digest",
        "--rollback-output",
        journal_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("confirmation"));
    assert_eq!(namespace_at(&db_path, &memory.id), "project:legacy");

    let applied = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "apply",
        "--manifest",
        manifest_path.to_str().unwrap(),
        "--confirm",
        &manifest.digest,
        "--rollback-output",
        journal_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(journal_path.exists());
    assert_eq!(namespace_at(&db_path, &memory.id), "project:canonical");

    let exported = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "export",
        "--transaction",
        &manifest.transaction_id,
        "--output",
        exported_journal_path.to_str().unwrap(),
    ]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert_eq!(
        std::fs::read(&exported_journal_path).unwrap(),
        std::fs::read(&journal_path).unwrap()
    );

    let refused_rollback = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "rollback",
        "--transaction",
        &manifest.transaction_id,
        "--confirm",
        "wrong-transaction",
        "--rollback-output",
        rollback_journal_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(!refused_rollback.status.success());
    assert!(String::from_utf8_lossy(&refused_rollback.stderr).contains("confirmation"));
    assert_eq!(namespace_at(&db_path, &memory.id), "project:canonical");

    let rolled_back = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "rollback",
        "--transaction",
        &manifest.transaction_id,
        "--confirm",
        &manifest.transaction_id,
        "--rollback-output",
        rollback_journal_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(
        rolled_back.status.success(),
        "{}",
        String::from_utf8_lossy(&rolled_back.stderr)
    );
    assert_eq!(namespace_at(&db_path, &memory.id), "project:legacy");
    assert!(rollback_journal_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            manifest_path,
            journal_path,
            exported_journal_path,
            rollback_journal_path,
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let backups = std::fs::read_dir(&backup_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "db"))
            .collect::<Vec<_>>();
        assert_eq!(
            backups.len(),
            3,
            "manifest, apply and rollback must retain distinct safety snapshots"
        );
        for path in backups {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600,
                "database backups contain private memory"
            );
        }
    }
}

#[test]
fn stale_manifest_fails_without_a_partial_repair() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("clio.db");
    let conn = db::open(&db_path).unwrap();
    let target = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:legacy".to_string(),
            kind: "fact".to_string(),
            title: Some("Stale CLI target".to_string()),
            summary: None,
            content: "Preserve target".to_string(),
            tags: Vec::new(),
            source: Some("repair-cli-test".to_string()),
            source_ref: Some("stale-target".to_string()),
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Default::default(),
    )
    .unwrap();
    let unrelated = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:other".to_string(),
            kind: "fact".to_string(),
            source_ref: Some("stale-unrelated".to_string()),
            title: Some("Unrelated".to_string()),
            summary: None,
            content: "Unrelated state".to_string(),
            tags: Vec::new(),
            source: Some("repair-cli-test".to_string()),
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Default::default(),
    )
    .unwrap();
    drop(conn);

    let plan_path = directory.path().join("plan.json");
    let manifest_path = directory.path().join("manifest.json");
    let journal_path = directory.path().join("journal.json");
    let backup_dir = directory.path().join("backups");
    std::fs::write(
        &plan_path,
        serde_json::to_vec(&RepairPlan {
            intents: vec![RepairIntent {
                memory_id: target.id.clone(),
                action: RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
                evidence: "stale CLI test".to_string(),
            }],
        })
        .unwrap(),
    )
    .unwrap();
    let built = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "manifest",
        "--plan",
        plan_path.to_str().unwrap(),
        "--output",
        manifest_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(built.status.success());
    let manifest: RepairManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    repository::move_namespace(
        &db::open(&db_path).unwrap(),
        &unrelated.id,
        "project:changed",
    )
    .unwrap();
    let journal_mode: String = rusqlite::Connection::open(&db_path)
        .unwrap()
        .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "delete");

    let applied = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "apply",
        "--manifest",
        manifest_path.to_str().unwrap(),
        "--confirm",
        &manifest.digest,
        "--rollback-output",
        journal_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(!applied.status.success());
    assert!(String::from_utf8_lossy(&applied.stderr).contains("snapshot changed"));
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    assert_eq!(namespace_at(&db_path, &target.id), "project:legacy");
    let transactions: i64 = conn
        .query_row("SELECT COUNT(*) FROM repair_transactions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(transactions, 0);
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "delete");
}

#[test]
fn repair_export_is_read_only_and_preserves_a_non_wal_database() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("clio.db");
    let conn = db::open(&db_path).unwrap();
    drop(conn);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "delete");
    drop(conn);

    let output_path = directory.path().join("missing.json");
    let exported = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "export",
        "--transaction",
        "missing-transaction",
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert!(!exported.status.success());
    assert!(!output_path.exists());

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "delete");
}

#[test]
fn committed_apply_can_reexport_its_journal_after_output_failure() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("clio.db");
    let conn = db::open(&db_path).unwrap();
    let target = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:legacy".to_string(),
            kind: "fact".to_string(),
            title: Some("Export recovery".to_string()),
            summary: None,
            content: "Recover the committed journal".to_string(),
            tags: Vec::new(),
            source: Some("repair-cli-test".to_string()),
            source_ref: Some("export-recovery".to_string()),
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Default::default(),
    )
    .unwrap();
    drop(conn);

    let plan_path = directory.path().join("plan.json");
    let manifest_path = directory.path().join("manifest.json");
    let blocked_journal_path = directory.path().join("blocked-journal.json");
    let recovered_journal_path = directory.path().join("recovered-journal.json");
    let backup_dir = directory.path().join("backups");
    std::fs::write(
        &plan_path,
        serde_json::to_vec(&RepairPlan {
            intents: vec![RepairIntent {
                memory_id: target.id.clone(),
                action: RepairAction::Move {
                    namespace: "project:canonical".to_string(),
                },
                evidence: "post-commit export recovery test".to_string(),
            }],
        })
        .unwrap(),
    )
    .unwrap();
    let built = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "manifest",
        "--plan",
        plan_path.to_str().unwrap(),
        "--output",
        manifest_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(built.status.success());
    let manifest: RepairManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    std::fs::write(&blocked_journal_path, b"different existing content").unwrap();

    let applied = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "apply",
        "--manifest",
        manifest_path.to_str().unwrap(),
        "--confirm",
        &manifest.digest,
        "--rollback-output",
        blocked_journal_path.to_str().unwrap(),
        "--backup-dir",
        backup_dir.to_str().unwrap(),
    ]);
    assert!(!applied.status.success());
    let stderr = String::from_utf8_lossy(&applied.stderr);
    assert!(stderr.contains("committed"), "{stderr}");
    assert!(stderr.contains("repair export"), "{stderr}");
    assert_eq!(namespace_at(&db_path, &target.id), "project:canonical");

    let exported = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "repair",
        "export",
        "--transaction",
        &manifest.transaction_id,
        "--output",
        recovered_journal_path.to_str().unwrap(),
    ]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(recovered_journal_path.exists());
}

#[test]
fn archive_cli_accepts_the_stable_source_reference_without_a_memory_id() {
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("clio.db");
    let conn = db::open(&db_path).unwrap();
    let memory = repository::remember(
        &conn,
        &RememberInput {
            source: Some("waypoint-worktree".to_string()),
            source_ref: Some("worktree:wt_demo:current".to_string()),
            upsert: true,
            namespace: "project:scooda".to_string(),
            kind: "summary".to_string(),
            title: Some("Current worktree".to_string()),
            summary: None,
            content: "Derived projection".to_string(),
            tags: Vec::new(),
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
        },
        &Default::default(),
    )
    .unwrap();
    drop(conn);

    let output = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "--json",
        "archive",
        "--source",
        "waypoint-worktree",
        "--source-ref",
        "worktree:wt_demo:current",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        repository::get(&db::open(&db_path).unwrap(), &memory.id)
            .unwrap()
            .archived_at
            .is_some()
    );

    let missing = run(&[
        "--local",
        "--db-path",
        db_path.to_str().unwrap(),
        "archive",
        "--source",
        "waypoint-worktree",
        "--source-ref",
        "worktree:missing:current",
    ]);
    assert!(
        missing.status.success(),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}
