//! Disposable, offline proof. Never opens the user's configured Clio database.
use clio_core::{db, work_reports::*};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Barrier},
    thread,
};

fn fixture(project: &str, task: &str, title: &str, run: &str) -> WorkReport {
    WorkReport {
        project: project.into(),
        task: task.into(),
        task_title: title.into(),
        source: "simulated-agent".into(),
        session_id: format!("session-{run}"),
        run_id: run.into(),
        sequence: 0,
        observed_at: 1000,
        worktree: format!("fixture-worktree-{run}"),
        revision: "fixture-revision-1".into(),
        state: State::Running,
        summary: "Work started".into(),
        next_step: "Continue the scoped task".into(),
        next_actor: NextActor::Agent,
        evidence: vec!["fixture:initial-report".into()],
        evidence_status: EvidenceStatus::Current,
        supersedes: None,
    }
}

fn parallel_reports(path: &Path, reports: Vec<(WorkReport, i64)>) -> Vec<Receipt> {
    let barrier = Arc::new(Barrier::new(reports.len()));
    let workers: Vec<_> = reports
        .into_iter()
        .map(|(update, received)| {
            let barrier = Arc::clone(&barrier);
            let path = path.to_owned();
            thread::spawn(move || {
                let conn = db::open(&path).expect("open disposable connection");
                barrier.wait();
                report(&conn, &update, received).expect("store concurrent report")
            })
        })
        .collect();
    workers
        .into_iter()
        .map(|worker| worker.join().expect("writer finished"))
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("proof.sqlite");
    let conn = db::open(&path)?;
    let a = fixture(
        "Modern Printworks (sample)",
        "sample-import",
        "Artwork import",
        "a",
    );
    let b = fixture(
        "Modern Printworks (sample)",
        "sample-popup",
        "Discount popup",
        "b",
    );
    let c = fixture(
        "Scooda (sample)",
        "sample-scheduler",
        "Scheduler checks",
        "c",
    );
    parallel_reports(
        &path,
        vec![(a.clone(), 1000), (b.clone(), 1000), (c.clone(), 1000)],
    );
    let started = overview(&conn, 1000, 60)?;
    assert_eq!(
        started.tasks.len(),
        3,
        "concurrent tasks must not overwrite one another"
    );
    assert!(started.tasks.iter().all(|t| t.state == TaskState::Running));

    let mut implemented = a.clone();
    implemented.sequence = 2;
    implemented.observed_at = 1080;
    implemented.state = State::Implemented;
    implemented.summary = "Implementation ready for review".into();
    implemented.next_step = "Review the import changes".into();
    implemented.next_actor = NextActor::User;
    implemented.evidence = vec!["fixture:local-checks-passed".into()];
    let mut waiting = b.clone();
    waiting.sequence = 1;
    waiting.observed_at = 1080;
    waiting.state = State::Waiting;
    waiting.summary = "Waiting for a product decision".into();
    waiting.next_step = "Should the popup appear on support pages?".into();
    waiting.next_actor = NextActor::User;
    let receipts = parallel_reports(
        &path,
        vec![
            (implemented.clone(), 1080),
            (waiting, 1080),
            (c.clone(), 1080),
        ],
    );
    let replay = report(&conn, &implemented, 1090)?;
    assert_eq!(
        replay, receipts[0],
        "retry must return the original receipt"
    );
    let mut delayed = a.clone();
    delayed.sequence = 1;
    delayed.observed_at = 1050;
    delayed.summary = "Earlier progress report, delivered late".into();
    report(&conn, &delayed, 1090)?;
    let mut changed_duplicate = implemented.clone();
    changed_duplicate.summary = "Different claim under a reused report identity".into();
    assert!(report(&conn, &changed_duplicate, 1090).is_err());
    let settled = overview(&conn, 1100, 60)?;
    let task = |id: &str| settled.tasks.iter().find(|t| t.task == id).unwrap();
    assert_eq!(task("sample-import").state, TaskState::Implemented);
    assert_eq!(task("sample-popup").state, TaskState::NeedsUser);
    assert_eq!(task("sample-scheduler").state, TaskState::ReportingMissing);
    assert_eq!(history(&conn, "simulated-agent", "a")?.len(), 3);
    assert_eq!(history(&conn, "simulated-agent", "c")?.len(), 1);

    let mut resumed = c.clone();
    resumed.run_id = "c-successor".into();
    resumed.session_id = "session-c-successor".into();
    resumed.worktree = "fixture-worktree-c-successor".into();
    resumed.observed_at = 1110;
    resumed.summary = "Resumed in a new worktree".into();
    resumed.next_step = "Refresh the unavailable issue evidence".into();
    resumed.evidence_status = EvidenceStatus::Unavailable;
    resumed.supersedes = Some(RunKey {
        source: c.source.clone(),
        run_id: c.run_id.clone(),
    });
    report(&conn, &resumed, 1110)?;
    let mut old_session = c;
    old_session.sequence = 1;
    old_session.observed_at = 1105;
    old_session.state = State::Stopped;
    report(&conn, &old_session, 1111)?;
    let successor = overview(&conn, 1111, 60)?;
    let resumed_task = successor
        .tasks
        .iter()
        .find(|t| t.task == "sample-scheduler")
        .unwrap();
    assert_eq!(
        successor.tasks.len(),
        3,
        "resumption must not duplicate a task"
    );
    assert_eq!(resumed_task.state, TaskState::Running);
    assert!(resumed_task.source_unavailable);
    assert_eq!(
        resumed_task.runs.len(),
        2,
        "previous worktree history must survive"
    );
    assert_eq!(
        resumed_task.runs.iter().filter(|r| !r.superseded).count(),
        1
    );

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "simulation": true,
            "checks": ["three concurrent writers", "duplicate receipt replay", "delayed update preserves latest state", "conflicting duplicate rejected", "missing reporter remains visible", "successor worktree preserves history", "unavailable evidence remains visible"],
            "snapshots": [
                {"label":"Three sessions started", "overview":started},
                {"label":"Review, decision and missing update", "overview":settled},
                {"label":"Resumed in a new worktree", "overview":successor}
            ]
        }))?
    );
    drop(conn);
    temp.close()?;
    eprintln!(
        "PASS: 7 proof checks; 3 concurrent writers; temporary database removed; no services started."
    );
    Ok(())
}
