//! Contract tests for Waypoint's worktree projection.
//!
//! Waypoint publishes exactly one memory per git worktree, keyed on
//! `source = "waypoint-worktree"` plus `source_ref = "worktree:<id>:current"`,
//! and republishes it after every ledger event. These tests pin the behaviour
//! Waypoint depends on from Clio's side, so a change here fails in Clio's own
//! suite rather than silently in Waypoint's.
//!
//! The full contract is documented in Waypoint at
//! `docs/contracts/clio-worktree-projection-v2.md`.
//!
//! Projection writes use the generic upsert and retirement uses the generic
//! source + source-ref archive seam. Clio must never grow a worktrees table:
//! the ledger's Markdown events are canonical.
//!
//! These use an in-memory database, like the rest of the suite. Do not rewrite
//! them to shell out to the `clio` binary: a shared Atlas route can be
//! configured on a developer machine, and the CLI would then reach a live
//! remote store even with `--db-path` naming a temporary file.

use clio_core::assembly::{self, ResumeRequest};
use clio_core::db;
use clio_core::models::*;
use clio_core::repository;
use clio_core::settings::Settings;

const SOURCE: &str = "waypoint-worktree";
const SOURCE_REF: &str = "worktree:wt_019887c4:current";
const NAMESPACE: &str = "project:scooda";

fn test_db() -> rusqlite::Connection {
    db::open_in_memory().expect("failed to open in-memory DB")
}

/// The exact payload Waypoint sends for one worktree's current view.
fn projection(content: &str, importance: i32) -> RememberInput {
    RememberInput {
        namespace: NAMESPACE.into(),
        kind: "summary".into(),
        title: Some("Worktree scooda-1784 — bemanza-scooda".into()),
        summary: None,
        content: content.into(),
        tags: vec![
            "worktree:wt_019887c4".into(),
            "workstream:ws_2fa_enrolment".into(),
            "repo:bemanza-scooda".into(),
            "branch:scooda-1784".into(),
            "machine:machine_019887c4".into(),
            "ticket:scooda-1784".into(),
        ],
        source: Some(SOURCE.into()),
        source_ref: Some(SOURCE_REF.into()),
        confidence: None,
        importance,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: true,
    }
}

fn recall_projections(conn: &rusqlite::Connection) -> RecallResult {
    repository::recall(
        conn,
        &RecallQuery {
            namespace: Some(NAMESPACE.into()),
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn republishing_a_worktree_view_upserts_one_memory_rather_than_duplicating() {
    let conn = test_db();

    let first = repository::remember(
        &conn,
        &projection(
            "Branch scooda-1784. Next action: recover the 2FA change.",
            4,
        ),
        &Settings::default(),
    )
    .unwrap();
    let second = repository::remember(
        &conn,
        &projection("Branch scooda-1784. Next action: run focused UAT.", 4),
        &Settings::default(),
    )
    .unwrap();

    assert_eq!(
        first.id, second.id,
        "a stable source + source_ref must update in place, never insert"
    );

    let recalled = recall_projections(&conn);
    assert_eq!(
        recalled.total, 1,
        "there is exactly one projection per worktree, however many checkpoints"
    );
    assert!(recalled.items[0].memory.content.contains("run focused UAT"));
}

#[test]
fn every_projection_tag_survives_the_round_trip() {
    let conn = test_db();
    repository::remember(
        &conn,
        &projection("Branch scooda-1784.", 4),
        &Settings::default(),
    )
    .unwrap();

    let recalled = recall_projections(&conn);
    let tags = &recalled.items[0].memory.tags;

    for expected in [
        "worktree:wt_019887c4",
        "workstream:ws_2fa_enrolment",
        "repo:bemanza-scooda",
        "branch:scooda-1784",
        "machine:machine_019887c4",
        "ticket:scooda-1784",
    ] {
        assert!(
            tags.iter().any(|tag| tag == expected),
            "missing {expected} in {tags:?}"
        );
    }
}

#[test]
fn a_projection_is_retrievable_by_its_worktree_tag() {
    let conn = test_db();
    repository::remember(
        &conn,
        &projection("Branch scooda-1784.", 4),
        &Settings::default(),
    )
    .unwrap();

    // How an agent asks "what was I doing in this worktree?".
    let recalled = repository::recall(
        &conn,
        &RecallQuery {
            tags: vec!["worktree:wt_019887c4".into()],
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(recalled.total, 1);
    assert_eq!(recalled.items[0].memory.source.as_deref(), Some(SOURCE));
    assert_eq!(
        recalled.items[0].memory.source_ref.as_deref(),
        Some(SOURCE_REF)
    );
}

#[test]
fn two_worktrees_in_one_repository_keep_separate_projections() {
    let conn = test_db();
    repository::remember(
        &conn,
        &projection("First worktree.", 4),
        &Settings::default(),
    )
    .unwrap();

    let other = RememberInput {
        source_ref: Some("worktree:wt_019887c5:current".into()),
        tags: vec!["worktree:wt_019887c5".into(), "repo:bemanza-scooda".into()],
        ..projection("Second worktree.", 3)
    };
    repository::remember(&conn, &other, &Settings::default()).unwrap();

    assert_eq!(
        recall_projections(&conn).total,
        2,
        "the upsert key is per worktree, not per repository"
    );
}

#[test]
fn an_updated_projection_can_change_importance_as_the_risk_changes() {
    let conn = test_db();
    repository::remember(
        &conn,
        &projection("Clean and checkpointed.", 3),
        &Settings::default(),
    )
    .unwrap();

    // The worktree becomes dirty, so Waypoint republishes at importance 4.
    repository::remember(
        &conn,
        &projection("Dirty: 2 modified, 1 untracked.", 4),
        &Settings::default(),
    )
    .unwrap();

    let recalled = recall_projections(&conn);
    assert_eq!(recalled.total, 1);
    assert_eq!(recalled.items[0].memory.importance, 4);
}

#[test]
fn archiving_a_projection_hides_it_from_default_recall_without_deleting_it() {
    let conn = test_db();
    let memory = repository::remember(
        &conn,
        &projection("Branch scooda-1784.", 4),
        &Settings::default(),
    )
    .unwrap();

    // How an archived worktree leaves the active set (specification: archived
    // worktrees are archived in Clio, never deleted).
    let archived = repository::archive_by_source_ref(&conn, SOURCE, SOURCE_REF)
        .unwrap()
        .expect("the stable projection identity should match");
    assert_eq!(archived.id, memory.id);

    assert_eq!(recall_projections(&conn).total, 0);

    let including_archived = repository::recall(
        &conn,
        &RecallQuery {
            namespace: Some(NAMESPACE.into()),
            include_archived: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        including_archived.total, 1,
        "the history must remain searchable"
    );
}

#[test]
fn a_projection_surfaces_through_the_generic_resume_brief() {
    let conn = test_db();
    repository::remember(
        &conn,
        &projection(
            "Branch scooda-1784. Next action: run focused UAT on the 2FA enrolment path.",
            4,
        ),
        &Settings::default(),
    )
    .unwrap();

    // Waypoint deliberately uses Clio's existing generic handoff path rather
    // than a second, worktree-specific "resume" API. This proves a projection
    // is reachable through it.
    let brief = assembly::build_resume_brief(
        &conn,
        &ResumeRequest {
            namespace: Some(NAMESPACE.into()),
            query: Some("scooda-1784".into()),
            ..ResumeRequest::default()
        },
    )
    .unwrap();

    let knowledge = brief
        .sections
        .iter()
        .find(|section| section.heading == "Relevant knowledge")
        .expect("a summary must be reachable from the knowledge section");

    let item = knowledge
        .items
        .iter()
        .find(|item| item.source.as_deref() == Some(SOURCE))
        .unwrap_or_else(|| panic!("projection missing from brief: {:?}", knowledge.items));
    assert!(item.content.contains("run focused UAT"));
    assert!(!item.reason.is_empty(), "every surfaced item explains why");
}
