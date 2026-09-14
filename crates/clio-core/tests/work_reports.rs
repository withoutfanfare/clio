use clio_core::db;

#[test]
fn work_reports_schema_is_available_in_a_temporary_database() {
    let conn = db::open_in_memory().unwrap();
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='work_reports')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(exists, "direct work-report storage is absent");
}

use clio_core::work_reports::{
    self as work, EvidenceStatus, NextActor, RunKey, State, TaskState, WorkReport,
};
use std::sync::{Arc, Barrier};

fn sample(project: &str, task: &str, run_id: &str) -> WorkReport {
    WorkReport {
        project: project.into(),
        task: task.into(),
        task_title: format!("Task {task}"),
        source: "test-agent".into(),
        session_id: format!("session-{run_id}"),
        run_id: run_id.into(),
        sequence: 0,
        observed_at: 100,
        worktree: format!("tree-{run_id}"),
        revision: "abc123".into(),
        state: State::Running,
        summary: "Working on proof".into(),
        next_step: "Run checks".into(),
        next_actor: NextActor::Agent,
        evidence: vec!["local:test".into()],
        evidence_status: EvidenceStatus::Current,
        supersedes: None,
    }
}

#[test]
fn work_reports_receipts_survive_reopen_and_replay_without_freshening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proof.sqlite");
    let initial = sample("A", "one", "r1");
    let receipt = work::report(&db::open(&path).unwrap(), &initial, 100).unwrap();
    let conn = db::open(&path).unwrap();
    assert_eq!(work::report(&conn, &initial, 400).unwrap(), receipt);
    assert_eq!(
        work::history(&conn, "test-agent", "r1").unwrap(),
        vec![receipt]
    );
    assert_eq!(
        work::overview(&conn, 400, 60).unwrap().tasks[0].state,
        TaskState::ReportingMissing
    );
    let mut changed = initial;
    changed.summary = "Different payload".into();
    assert!(work::report(&conn, &changed, 400).is_err());
    assert_eq!(work::history(&conn, "test-agent", "r1").unwrap().len(), 1);
}

#[test]
fn work_reports_require_registration_and_store_late_updates_without_regression() {
    let conn = db::open_in_memory().unwrap();
    let mut report = sample("A", "one", "r1");
    report.sequence = 2;
    assert!(work::report(&conn, &report, 100).is_err());
    report.sequence = 0;
    work::report(&conn, &report, 100).unwrap();
    report.sequence = 2;
    report.state = State::Implemented;
    report.observed_at = 120;
    work::report(&conn, &report, 120).unwrap();
    report.sequence = 1;
    report.state = State::Running;
    report.observed_at = 110;
    work::report(&conn, &report, 150).unwrap();
    let view = work::overview(&conn, 150, 60).unwrap();
    assert_eq!(view.tasks[0].state, TaskState::Implemented);
    assert_eq!(view.tasks[0].runs[0].receipt.report.sequence, 2);
    assert_eq!(work::history(&conn, "test-agent", "r1").unwrap().len(), 3);
}

#[test]
fn work_reports_reject_rebinding_and_invalid_fields_atomically() {
    let conn = db::open_in_memory().unwrap();
    let initial = sample("A", "one", "r1");
    work::report(&conn, &initial, 100).unwrap();
    for field in ["project", "task", "session", "worktree"] {
        let mut changed = initial.clone();
        changed.sequence = 1;
        match field {
            "project" => changed.project = "B".into(),
            "task" => changed.task = "two".into(),
            "session" => changed.session_id = "other".into(),
            _ => changed.worktree = "other".into(),
        }
        assert!(
            work::report(&conn, &changed, 110).is_err(),
            "rebound {field}"
        );
    }
    for case in 0..9 {
        let mut bad = sample("A", "one", &format!("invalid-{case}"));
        match case {
            0 => bad.project = " ".into(),
            1 => bad.sequence = -1,
            2 => bad.observed_at = -1,
            3 => bad.observed_at = 101,
            4 => bad.summary = "x".repeat(1001),
            5 => bad.evidence = vec!["x".into(); 33],
            6 => bad.run_id = "x".repeat(241),
            7 => bad.next_step = "x".repeat(1001),
            _ => bad.evidence = vec!["x".repeat(2001)],
        }
        assert!(
            work::report(&conn, &bad, 100).is_err(),
            "accepted case {case}"
        );
    }
    assert!(work::report(&conn, &sample("A", "one", "negative-time"), -1).is_err());
    assert_eq!(
        work::overview(&conn, 100, 60).unwrap().tasks[0].runs.len(),
        1
    );
    assert!(work::overview(&conn, -1, 60).is_err());
    assert!(work::overview(&conn, 100, -1).is_err());
    let mut revision = initial;
    revision.sequence = 1;
    revision.revision = "newrev".into();
    work::report(&conn, &revision, 110).unwrap();
}

#[test]
fn work_reports_three_connections_preserve_independent_project_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proof.sqlite");
    drop(db::open(&path).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..3)
        .map(|index| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let conn = db::open(&path).unwrap();
                let mut report = sample(
                    if index == 2 { "B" } else { "A" },
                    &format!("task-{index}"),
                    &format!("run-{index}"),
                );
                barrier.wait();
                work::report(&conn, &report, 100).unwrap();
                barrier.wait();
                if index != 2 {
                    report.sequence = 1;
                    report.observed_at = 120;
                    if index == 0 {
                        report.state = State::Implemented;
                        report.next_actor = NextActor::None;
                    } else {
                        report.state = State::Waiting;
                        report.next_actor = NextActor::User;
                        report.evidence_status = EvidenceStatus::Unavailable;
                    }
                    work::report(&conn, &report, 120).unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let overview = work::overview(&db::open(&path).unwrap(), 200, 60).unwrap();
    assert_eq!(overview.tasks.len(), 3);
    assert_eq!(
        overview
            .tasks
            .iter()
            .filter(|task| task.project == "A")
            .count(),
        2
    );
    assert_eq!(overview.tasks[0].state, TaskState::Implemented);
    assert_eq!(overview.tasks[1].state, TaskState::NeedsUser);
    assert!(overview.tasks[1].stale);
    assert!(overview.tasks[1].source_unavailable);
    assert_eq!(overview.tasks[2].state, TaskState::ReportingMissing);
}

#[test]
fn work_reports_competing_runs_conflict_and_explicit_handover_retains_history() {
    let conn = db::open_in_memory().unwrap();
    let mut first = sample("A", "one", "r1");
    work::report(&conn, &first, 100).unwrap();
    let mut second = sample("A", "one", "r2");
    second.state = State::Waiting;
    second.next_actor = NextActor::User;
    work::report(&conn, &second, 100).unwrap();
    let view = work::overview(&conn, 100, 60).unwrap();
    assert_eq!(view.tasks.len(), 1);
    assert_eq!(view.tasks[0].runs.len(), 2);
    assert_eq!(view.tasks[0].state, TaskState::Conflict);
    let mut successor = sample("A", "one", "r3");
    successor.state = State::Waiting;
    successor.next_actor = NextActor::User;
    successor.supersedes = Some(RunKey {
        source: first.source.clone(),
        run_id: first.run_id.clone(),
    });
    work::report(&conn, &successor, 100).unwrap();
    first.sequence = 1;
    first.observed_at = 120;
    first.state = State::Implemented;
    work::report(&conn, &first, 120).unwrap();
    let view = work::overview(&conn, 120, 60).unwrap();
    assert_eq!(view.tasks[0].state, TaskState::NeedsUser);
    assert_eq!(
        view.tasks[0]
            .runs
            .iter()
            .filter(|run| run.superseded)
            .count(),
        1
    );
    assert_eq!(work::history(&conn, "test-agent", "r1").unwrap().len(), 2);
    let mut wrong_task = sample("B", "two", "r4");
    wrong_task.supersedes = Some(RunKey {
        source: second.source,
        run_id: second.run_id,
    });
    assert!(work::report(&conn, &wrong_task, 100).is_err());
}

#[test]
fn work_reports_concurrent_handovers_cannot_erase_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proof.sqlite");
    work::report(
        &db::open(&path).unwrap(),
        &sample("A", "one", "predecessor"),
        100,
    )
    .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|index| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let conn = db::open(&path).unwrap();
                let mut report = sample("A", "one", &format!("successor-{index}"));
                report.supersedes = Some(RunKey {
                    source: "test-agent".into(),
                    run_id: "predecessor".into(),
                });
                barrier.wait();
                work::report(&conn, &report, 100).is_ok()
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap() as usize)
            .sum::<usize>(),
        1
    );
    let view = work::overview(&db::open(&path).unwrap(), 100, 60).unwrap();
    assert_eq!(view.tasks[0].runs.len(), 2);
    assert_eq!(
        view.tasks[0]
            .runs
            .iter()
            .filter(|run| !run.superseded)
            .count(),
        1
    );
}

#[test]
fn work_reports_delayed_payload_is_stale_and_storage_failure_has_no_receipt() {
    let conn = db::open_in_memory().unwrap();
    let report = sample("A", "one", "r1");
    work::report(&conn, &report, 500).unwrap();
    assert_eq!(
        work::overview(&conn, 500, 60).unwrap().tasks[0].state,
        TaskState::ReportingMissing
    );
    conn.execute_batch("CREATE TRIGGER refuse_proof BEFORE INSERT ON work_reports BEGIN SELECT RAISE(ABORT, 'simulated storage failure'); END;").unwrap();
    assert!(work::report(&conn, &sample("B", "two", "r2"), 100).is_err());
    assert!(conn.is_autocommit());
    assert!(work::history(&conn, "test-agent", "r2").unwrap().is_empty());
    assert_eq!(work::overview(&conn, 500, 60).unwrap().tasks.len(), 1);
    conn.execute_batch("DROP TRIGGER refuse_proof").unwrap();
    work::report(&conn, &sample("B", "two", "r2"), 100).unwrap();
}

#[test]
fn work_reports_same_identity_concurrent_replays_share_one_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proof.sqlite");
    drop(db::open(&path).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..3)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let conn = db::open(&path).unwrap();
                barrier.wait();
                work::report(&conn, &sample("A", "one", "r1"), 100).unwrap()
            })
        })
        .collect();
    let receipts: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(receipts.iter().all(|receipt| receipt == &receipts[0]));
    assert_eq!(
        work::history(&db::open(&path).unwrap(), "test-agent", "r1")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn work_reports_own_commit_and_keep_successor_binding_without_repeated_pointer() {
    let conn = db::open_in_memory().unwrap();
    let first = sample("A", "one", "r1");
    conn.execute_batch("BEGIN").unwrap();
    assert!(work::report(&conn, &first, 100).is_err());
    conn.execute_batch("ROLLBACK").unwrap();
    work::report(&conn, &first, 100).unwrap();
    let mut successor = sample("A", "one", "r2");
    successor.supersedes = Some(RunKey {
        source: first.source,
        run_id: first.run_id,
    });
    work::report(&conn, &successor, 100).unwrap();
    successor.sequence = 1;
    successor.supersedes = None;
    successor.state = State::Stopped;
    successor.evidence_status = EvidenceStatus::Unavailable;
    work::report(&conn, &successor, 100).unwrap();
    let view = work::overview(&conn, 200, 60).unwrap();
    assert_eq!(view.tasks[0].state, TaskState::Stopped);
    assert!(view.tasks[0].source_unavailable);
    assert_eq!(
        view.tasks[0]
            .runs
            .iter()
            .filter(|run| run.superseded)
            .count(),
        1
    );
    successor.sequence = 2;
    successor.supersedes = Some(RunKey {
        source: "test-agent".into(),
        run_id: "different".into(),
    });
    assert!(work::report(&conn, &successor, 100).is_err());
}

#[test]
fn work_reports_same_state_different_next_steps_conflict_without_choosing_a_writer() {
    let conn = db::open_in_memory().unwrap();
    let first = sample("A", "one", "r1");
    let mut second = sample("A", "one", "r2");
    second.next_step = "Ask for access".into();
    work::report(&conn, &first, 100).unwrap();
    work::report(&conn, &second, 100).unwrap();
    let view = work::overview(&conn, 100, 60).unwrap();
    assert_eq!(view.tasks[0].state, TaskState::Conflict);
    assert_eq!(view.tasks[0].runs.len(), 2);
}

#[test]
fn acceptance_is_receipt_scoped_durable_and_not_reopened_by_old_reports() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acceptance.sqlite");
    let conn = db::open(&path).unwrap();
    let mut item = sample("A", "one", "r1");
    item.state = State::Implemented;
    item.next_actor = NextActor::User;
    item.next_step = "Review item 3".into();
    let receipt = work::report(&conn, &item, 100).unwrap();
    let acceptance = work::Acceptance {
        receipt_id: receipt.id,
        scope: "Item 3 only".into(),
        accepted_by: "Danny".into(),
        accepted_at: 110,
        evidence: vec!["checks.md#human-acceptance".into()],
    };
    work::accept(&conn, &acceptance, 120).unwrap();
    work::accept(&conn, &acceptance, 120).unwrap();
    work::report(&conn, &item, 120).unwrap();
    let view = work::overview(&db::open(&path).unwrap(), 1000, 60).unwrap();
    assert_eq!(view.tasks[0].state, TaskState::Accepted);
    assert_eq!(view.tasks[0].next_actor, NextActor::None);
    assert!(!view.tasks[0].stale);
    assert_eq!(view.tasks[0].acceptances, vec![acceptance.clone()]);
    assert_eq!(
        work::history(&conn, "test-agent", "r1").unwrap(),
        vec![receipt]
    );
    let mut changed = acceptance.clone();
    changed.scope = "All work".into();
    assert!(work::accept(&conn, &changed, 120).is_err());
    changed.receipt_id = 999;
    assert!(work::accept(&conn, &changed, 120).is_err());
    let mut successor = sample("A", "one", "r2");
    successor.supersedes = Some(RunKey {
        source: item.source.clone(),
        run_id: item.run_id.clone(),
    });
    work::report(&conn, &successor, 120).unwrap();
    item.sequence = 1;
    work::report(&conn, &item, 120).unwrap();
    let view = work::overview(&conn, 120, 60).unwrap();
    assert_eq!(view.tasks[0].state, TaskState::Running);
    assert_eq!(view.tasks[0].next_actor, NextActor::Agent);
    assert_eq!(view.tasks[0].acceptances, vec![acceptance]);
    successor.sequence = 1;
    successor.state = State::Implemented;
    successor.next_actor = NextActor::User;
    work::report(&conn, &successor, 120).unwrap();
    assert_eq!(
        work::overview(&conn, 120, 60).unwrap().tasks[0].next_actor,
        NextActor::User,
        "acceptance must not accept later work"
    );
}

#[test]
fn recommendations_are_durable_separate_from_work_and_hidden_once_the_task_reports() {
    let conn = db::open_in_memory().unwrap();
    work::report(&conn, &sample("A", "one", "r1"), 100).unwrap();
    let recommendation = work::Recommendation {
        project: "A".into(),
        parent_task: "one".into(),
        task: "freshness".into(),
        task_title: "Show report age".into(),
        reason: "Polling age hides stale reports".into(),
        next_actor: NextActor::User,
        prompt: "Implement the freshness finding; do not start other work.".into(),
        evidence: vec!["checks.md#item-2".into()],
        checked_at: 110,
    };
    work::recommend(&conn, &recommendation, 120).unwrap();
    let view = work::overview(&conn, 120, 60).unwrap();
    assert_eq!(
        view.tasks.len(),
        1,
        "a recommendation does not register work"
    );
    assert_eq!(view.tasks[0].recommendation, Some(recommendation.clone()));
    assert_eq!(view.tasks[0].next_actor, NextActor::Agent);
    let mut invalid = recommendation.clone();
    invalid.evidence.clear();
    assert!(work::recommend(&conn, &invalid, 120).is_err());
    invalid = recommendation.clone();
    invalid.parent_task = "missing".into();
    assert!(work::recommend(&conn, &invalid, 120).is_err());
    work::report(&conn, &sample("A", "freshness", "r2"), 120).unwrap();
    assert!(
        work::overview(&conn, 120, 60)
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.task == "one")
            .unwrap()
            .recommendation
            .is_none()
    );
    assert!(
        work::recommend(&conn, &recommendation, 120).is_err(),
        "do not recommend already reported work"
    );
}

#[test]
fn evidence_opening_is_bound_to_stored_receipt_and_regular_files_in_its_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("proof file.txt"), "real evidence").unwrap();
    std::fs::write(dir.path().join("outside.txt"), "outside").unwrap();
    std::fs::write(root.join("script.sh"), "exit 0").unwrap();
    std::fs::create_dir(root.join("directory.txt")).unwrap();
    let mut report = sample("A", "evidence", "evidence-run");
    report.worktree = root.to_str().unwrap().into();
    report.evidence = vec!["proof file.txt".into()];
    let conn = db::open_in_memory().unwrap();
    let receipt = work::report(&conn, &report, 100).unwrap();
    let expected = root.join("proof file.txt").canonicalize().unwrap();
    assert_eq!(work::evidence_path(&conn, receipt.id, "proof file.txt").unwrap(), expected);
    assert_eq!(work::local_evidence_path(&report.worktree, expected.to_str().unwrap()), Some(expected));
    for reference in ["../outside.txt", "sub/../proof file.txt", "missing.txt", "directory.txt", "script.sh", "file:///proof.txt", "https://host/proof.txt", "proof file.txt:2", "proof file.txt#section", "%2e%2e/outside.txt", "..\\outside.txt", "\0proof.txt", ""] {
        assert!(work::local_evidence_path(&report.worktree, reference).is_none(), "{reference:?}");
    }
    assert!(work::local_evidence_path(&report.worktree, dir.path().join("outside.txt").to_str().unwrap()).is_none());
    assert!(work::local_evidence_path("relative", "proof file.txt").is_none());
    assert!(work::evidence_path(&conn, receipt.id + 1, "proof file.txt").is_err());
    std::fs::write(root.join("unreported.txt"), "not attached").unwrap();
    assert!(work::evidence_path(&conn, receipt.id, "unreported.txt").is_err());
    #[cfg(unix)] {
        use std::os::unix::fs::symlink;
        symlink(dir.path().join("outside.txt"), root.join("escape.txt")).unwrap();
        symlink(dir.path(), root.join("escape-dir")).unwrap();
        symlink(root.join("script.sh"), root.join("script.txt")).unwrap();
        for reference in ["escape.txt", "escape-dir/outside.txt", "script.txt"] {
            assert!(work::local_evidence_path(&report.worktree, reference).is_none());
        }
        // Revalidate at click time, even after the overview advertised this file.
        std::fs::remove_file(root.join("proof file.txt")).unwrap();
        symlink(dir.path().join("outside.txt"), root.join("proof file.txt")).unwrap();
        assert!(work::evidence_path(&conn, receipt.id, "proof file.txt").is_err());
    }
}

#[test]
fn dashboard_acceptance_is_current_receipt_scoped_durable_and_retry_safe() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("review.sqlite");
    let conn = db::open(&path).unwrap();
    let mut first = sample("A", "review", "review-one");
    let running = work::report(&conn, &first, 100).unwrap();
    assert!(work::accept_current(&conn, running.id, 110).is_err());
    first.sequence = 1; first.state = State::Implemented;
    let old = work::report(&conn, &first, 110).unwrap();
    first.sequence = 2;
    let current = work::report(&conn, &first, 120).unwrap();
    assert!(work::accept_current(&conn, old.id, 130).is_err(), "a new report invalidates an open review");
    assert!(work::accept_current(&conn, 9999, 130).is_err());
    let accepted = work::accept_current(&conn, current.id, 130).unwrap();
    assert_eq!(accepted.receipt_id, current.id);
    assert_eq!(accepted.scope, first.task_title);
    assert_eq!(accepted.accepted_by, "Danny");
    assert_eq!(work::accept_current(&conn, current.id, 140).unwrap(), accepted);
    let reopened = db::open(&path).unwrap();
    assert_eq!(work::overview(&reopened, 150, 300).unwrap().tasks[0].state, TaskState::Accepted);
    first.sequence = 3;
    let later = work::report(&conn, &first, 160).unwrap();
    assert_eq!(work::overview(&conn, 160, 300).unwrap().tasks[0].state, TaskState::Implemented);
    let mut successor = sample("A", "review", "review-two");
    successor.supersedes = Some(RunKey { source: first.source.clone(), run_id: first.run_id.clone() });
    work::report(&conn, &successor, 170).unwrap();
    assert!(work::accept_current(&conn, later.id, 180).is_err(), "handed-over runs cannot be accepted from a stale screen");
    conn.execute_batch("BEGIN").unwrap();
    assert!(work::accept_current(&conn, current.id, 180).is_err());
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM work_acceptances", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
}
