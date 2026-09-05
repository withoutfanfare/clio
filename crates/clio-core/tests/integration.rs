use clio_core::db;
use clio_core::error::ClioError;
use clio_core::export;
use clio_core::models::*;
use clio_core::repository;
use clio_core::settings::Settings;

fn test_db() -> rusqlite::Connection {
    db::open_in_memory().expect("failed to open in-memory DB")
}

fn base_input(content: &str) -> RememberInput {
    RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: content.into(),
        tags: vec![],
        source: None,
        source_ref: None,
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    }
}

fn remember_simple(conn: &rusqlite::Connection, content: &str) -> Memory {
    repository::remember(conn, &base_input(content), &Settings::default()).unwrap()
}

fn remember_with_tags(conn: &rusqlite::Connection, content: &str, tags: &[&str]) -> Memory {
    let input = RememberInput {
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..base_input(content)
    };
    repository::remember(conn, &input, &Settings::default()).unwrap()
}

fn remember_in(conn: &rusqlite::Connection, namespace: &str, content: &str) -> Memory {
    let input = RememberInput {
        namespace: namespace.into(),
        ..base_input(content)
    };
    repository::remember(conn, &input, &Settings::default()).unwrap()
}

fn has_issue(report: &clio_core::integrity::IntegrityReport, kind: &str, id: &str) -> bool {
    report
        .issues
        .iter()
        .any(|issue| issue.kind == kind && issue.affected_ids.iter().any(|affected| affected == id))
}

#[test]
fn archived_only_recall_pages_exclude_active_and_preserve_total_past_last_page() {
    let conn = test_db();
    remember_in(&conn, "project:a", "active evidence");
    let mut archived = Vec::new();
    for _ in 0..3 {
        let memory = remember_in(&conn, "project:a", "archived evidence");
        repository::archive(&conn, &memory.id).unwrap();
        archived.push(memory.id);
    }
    archived.sort();
    let elsewhere = remember_in(&conn, "project:b", "archived elsewhere");
    repository::archive(&conn, &elsewhere.id).unwrap();
    conn.execute(
        "UPDATE memories SET updated_at = '2026-01-01T00:00:00Z'",
        [],
    )
    .unwrap();
    for query in [None, Some("evidence")] {
        let mut seen = Vec::new();
        for offset in 0..=3 {
            let q: RecallQuery = serde_json::from_value(serde_json::json!({
                "namespace": "project:a", "query": query, "archived_only": true,
                "limit": 1, "offset": offset, "sort_by": "updated_desc"
            }))
            .unwrap();
            let result = repository::recall(&conn, &q).unwrap();
            assert_eq!(result.total, 3);
            assert_eq!(
                serde_json::to_value(&result).unwrap()["archived_only"],
                true
            );
            assert!(
                result
                    .items
                    .iter()
                    .all(|item| item.memory.archived_at.is_some())
            );
            seen.extend(result.items.into_iter().map(|item| item.memory.id));
        }
        assert_eq!(seen, archived);
    }
}

// ---------------------------------------------------------------------------
// Migration bootstrap
// ---------------------------------------------------------------------------

#[test]
fn migration_bootstrap_creates_tables() {
    let conn = test_db();
    let versions = clio_core::migrations::applied_versions(&conn).unwrap();
    assert!(!versions.is_empty());
    assert!(versions.contains(&"001_initial".to_string()));
}

// ---------------------------------------------------------------------------
// Insert memory
// ---------------------------------------------------------------------------

#[test]
fn insert_basic_memory() {
    let conn = test_db();
    let input = RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: Some("Test note".into()),
        summary: None,
        content: "This is a test memory.".into(),
        tags: vec!["test".into(), "unit".into()],
        source: None,
        source_ref: None,
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let mem = repository::remember(&conn, &input, &Settings::default()).unwrap();
    assert_eq!(mem.namespace, "global");
    assert_eq!(mem.kind, "note");
    assert_eq!(mem.title, Some("Test note".into()));
    assert_eq!(mem.content, "This is a test memory.");
    assert_eq!(mem.tags, vec!["test", "unit"]);
    assert_eq!(mem.importance, 3);
    assert!(mem.archived_at.is_none());
    assert!(!mem.id.is_empty());
}

#[test]
fn remember_sorts_tags_text_for_integrity_check() {
    let conn = test_db();
    let memory = remember_with_tags(&conn, "tag order regression", &["rust", "async"]);

    let tags_text: String = conn
        .query_row(
            "SELECT tags_text FROM memories WHERE id = ?1",
            [&memory.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tags_text, "async rust");

    let clean = clio_core::integrity::check(&conn).unwrap();
    assert!(!has_issue(&clean, "tag_mismatch", &memory.id));

    conn.execute(
        "UPDATE memories SET tags_text = ?1 WHERE id = ?2",
        rusqlite::params!["rust", &memory.id],
    )
    .unwrap();
    let corrupt = clio_core::integrity::check(&conn).unwrap();
    assert!(has_issue(&corrupt, "tag_mismatch", &memory.id));
}

#[test]
fn insert_validates_empty_content() {
    let conn = test_db();
    let input = RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: "".into(),
        tags: vec![],
        source: None,
        source_ref: None,
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let err = repository::remember(&conn, &input, &Settings::default()).unwrap_err();
    assert!(matches!(err, ClioError::Validation(_)));
}

#[test]
fn insert_validates_importance_range() {
    let conn = test_db();
    let input = RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: "test".into(),
        tags: vec![],
        source: None,
        source_ref: None,
        confidence: None,
        importance: 6,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let err = repository::remember(&conn, &input, &Settings::default()).unwrap_err();
    assert!(matches!(err, ClioError::Validation(_)));
}

#[test]
fn insert_validates_confidence_range() {
    let conn = test_db();
    let input = RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: "test".into(),
        tags: vec![],
        source: None,
        source_ref: None,
        confidence: Some(1.5),
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let err = repository::remember(&conn, &input, &Settings::default()).unwrap_err();
    assert!(matches!(err, ClioError::Validation(_)));
}

#[test]
fn insert_validates_metadata_must_be_object() {
    let conn = test_db();
    let input = RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: "test".into(),
        tags: vec![],
        source: None,
        source_ref: None,
        confidence: None,
        importance: 3,
        metadata: serde_json::json!([1, 2, 3]),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let err = repository::remember(&conn, &input, &Settings::default()).unwrap_err();
    assert!(matches!(err, ClioError::Validation(_)));
}

#[test]
fn tags_are_normalised_and_deduplicated() {
    let conn = test_db();
    let input = RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: "tag test".into(),
        tags: vec!["Rust".into(), "  rust ".into(), "SQLite".into()],
        source: None,
        source_ref: None,
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let mem = repository::remember(&conn, &input, &Settings::default()).unwrap();
    assert_eq!(mem.tags, vec!["rust", "sqlite"]);
}

// ---------------------------------------------------------------------------
// Upsert
// ---------------------------------------------------------------------------

#[test]
fn upsert_updates_existing_by_source_ref() {
    let conn = test_db();
    let input1 = RememberInput {
        namespace: "project:ai".into(),
        kind: "decision".into(),
        title: Some("Original".into()),
        summary: None,
        content: "First version".into(),
        tags: vec!["v1".into()],
        source: Some("test".into()),
        source_ref: Some("ref-001".into()),
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    };

    let mem1 = repository::remember(&conn, &input1, &Settings::default()).unwrap();

    let input2 = RememberInput {
        namespace: "project:ai".into(),
        kind: "decision".into(),
        title: Some("Updated".into()),
        summary: Some("Now with summary".into()),
        content: "Second version".into(),
        tags: vec!["v2".into()],
        source: Some("test".into()),
        source_ref: Some("ref-001".into()),
        confidence: Some(0.9),
        importance: 4,
        metadata: serde_json::json!({"updated": true}),
        valid_from: None,
        valid_until: None,
        upsert: true,
    };

    let mem2 = repository::remember(&conn, &input2, &Settings::default()).unwrap();

    // Same id preserved.
    assert_eq!(mem2.id, mem1.id);
    // created_at preserved.
    assert_eq!(mem2.created_at, mem1.created_at);
    // Fields updated.
    assert_eq!(mem2.title, Some("Updated".into()));
    assert_eq!(mem2.content, "Second version");
    assert_eq!(mem2.tags, vec!["v2"]);
    assert_eq!(mem2.importance, 4);
    assert!(mem2.updated_at > mem1.updated_at);
}

#[test]
fn upsert_without_source_ref_inserts_new_row() {
    let conn = test_db();
    let input = RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: "upsert but no source_ref".into(),
        tags: vec![],
        source: Some("test".into()),
        source_ref: None,
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: true,
    };

    let mem1 = repository::remember(&conn, &input, &Settings::default()).unwrap();
    let mem2 = repository::remember(&conn, &input, &Settings::default()).unwrap();
    // Should create two distinct records.
    assert_ne!(mem1.id, mem2.id);
}

// ---------------------------------------------------------------------------
// Get
// ---------------------------------------------------------------------------

#[test]
fn get_returns_not_found_for_missing_id() {
    let conn = test_db();
    let err = repository::get(&conn, "nonexistent-id").unwrap_err();
    assert!(matches!(err, ClioError::NotFound(_)));
}

// ---------------------------------------------------------------------------
// FTS recall
// ---------------------------------------------------------------------------

#[test]
fn fts_recall_finds_by_content() {
    let conn = test_db();
    let _ = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Architecture decision".into()),
            summary: None,
            content: "We chose SQLite for the database engine.".into(),
            tags: vec!["sqlite".into(), "architecture".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 4,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("SQLite".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    assert!(result.items[0].rank.is_some());
    assert_eq!(
        result.items[0].memory.title,
        Some("Architecture decision".into())
    );
}

#[test]
fn fts_recall_finds_by_title() {
    let conn = test_db();
    let _ = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "decision".into(),
            title: Some("Rust memory backbone".into()),
            summary: None,
            content: "Implementing the core in Rust.".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("backbone".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.total, 1);
}

// ---------------------------------------------------------------------------
// Recent recall
// ---------------------------------------------------------------------------

#[test]
fn recent_recall_returns_by_updated_at_desc() {
    let conn = test_db();

    for i in 0..5 {
        let _ = repository::remember(
            &conn,
            &RememberInput {
                namespace: "global".into(),
                kind: "note".into(),
                title: Some(format!("Note {i}")),
                summary: None,
                content: format!("Content for note {i}"),
                tags: vec![],
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Settings::default(),
        )
        .unwrap();
    }

    let result = repository::recent(&conn, None, 3).unwrap();
    assert_eq!(result.count, 3);
    assert_eq!(result.total, 5);
    // Most recent first.
    assert!(result.items[0].memory.updated_at >= result.items[1].memory.updated_at);
}

// ---------------------------------------------------------------------------
// Namespace filtering
// ---------------------------------------------------------------------------

#[test]
fn recall_filters_by_namespace() {
    let conn = test_db();

    for ns in &["project:alpha", "project:beta", "global"] {
        let _ = repository::remember(
            &conn,
            &RememberInput {
                namespace: ns.to_string(),
                kind: "note".into(),
                title: None,
                summary: None,
                content: format!("Memory in {ns}"),
                tags: vec![],
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Settings::default(),
        )
        .unwrap();
    }

    let result = repository::recall(
        &conn,
        &RecallQuery {
            namespace: Some("project:alpha".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].memory.namespace, "project:alpha");
}

// ---------------------------------------------------------------------------
// Tag filtering (match-all and match-any)
// ---------------------------------------------------------------------------

#[test]
fn recall_filters_tags_match_all() {
    let conn = test_db();

    let _ = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Has both tags".into(),
            tags: vec!["rust".into(), "sqlite".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let _ = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Only rust tag".into(),
            tags: vec!["rust".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let result = repository::recall(
        &conn,
        &RecallQuery {
            tags: vec!["rust".into(), "sqlite".into()],
            match_all_tags: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].memory.content, "Has both tags");
}

#[test]
fn recall_filters_tags_match_any() {
    let conn = test_db();

    let _ = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Rust note".into(),
            tags: vec!["rust".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let _ = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Python note".into(),
            tags: vec!["python".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let result = repository::recall(
        &conn,
        &RecallQuery {
            tags: vec!["rust".into(), "python".into()],
            match_all_tags: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.total, 2);
}

// ---------------------------------------------------------------------------
// Archive hides by default
// ---------------------------------------------------------------------------

#[test]
fn archive_hides_from_default_recall() {
    let conn = test_db();

    let mem = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Will be archived".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let archived = repository::archive(&conn, &mem.id).unwrap();
    assert!(archived.archived_at.is_some());

    // Default recall should not include it.
    let result = repository::recall(&conn, &RecallQuery::default()).unwrap();
    assert_eq!(result.total, 0);

    // Explicit include_archived should find it.
    let result = repository::recall(
        &conn,
        &RecallQuery {
            include_archived: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.total, 1);
}

#[test]
fn archive_is_idempotent() {
    let conn = test_db();

    let mem = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Idempotent archive test".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let a1 = repository::archive(&conn, &mem.id).unwrap();
    let a2 = repository::archive(&conn, &mem.id).unwrap();
    // archived_at should be the same (COALESCE preserves original).
    assert_eq!(a1.archived_at, a2.archived_at);
}

#[test]
fn archive_by_source_reference_is_exact_and_missing_is_a_noop() {
    let conn = test_db();
    let projected = repository::remember(
        &conn,
        &RememberInput {
            source: Some("waypoint-worktree".into()),
            source_ref: Some("worktree:wt_demo:current".into()),
            upsert: true,
            ..base_input("Current worktree projection")
        },
        &Settings::default(),
    )
    .unwrap();
    let other = repository::remember(
        &conn,
        &RememberInput {
            source: Some("waypoint-worktree".into()),
            source_ref: Some("worktree:wt_other:current".into()),
            upsert: true,
            ..base_input("Other worktree projection")
        },
        &Settings::default(),
    )
    .unwrap();

    let archived =
        repository::archive_by_source_ref(&conn, "waypoint-worktree", "worktree:wt_demo:current")
            .unwrap()
            .unwrap();
    assert_eq!(archived.id, projected.id);
    assert!(archived.archived_at.is_some());
    let replayed =
        repository::archive_by_source_ref(&conn, "waypoint-worktree", "worktree:wt_demo:current")
            .unwrap()
            .unwrap();
    assert_eq!(
        replayed.updated_at, archived.updated_at,
        "retirement replay must not create fresh semantic activity"
    );
    assert!(
        repository::get(&conn, &other.id)
            .unwrap()
            .archived_at
            .is_none()
    );
    assert!(
        repository::archive_by_source_ref(&conn, "waypoint-worktree", "worktree:missing:current")
            .unwrap()
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// Unarchive
// ---------------------------------------------------------------------------

#[test]
fn unarchive_restores_memory() {
    let conn = test_db();

    let mem = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Will be unarchived".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    // Archive it.
    let archived = repository::archive(&conn, &mem.id).unwrap();
    assert!(archived.archived_at.is_some());

    // Default recall should not include it.
    let result = repository::recall(&conn, &RecallQuery::default()).unwrap();
    assert_eq!(result.total, 0);

    // Unarchive it.
    let restored = repository::unarchive(&conn, &mem.id).unwrap();
    assert!(restored.archived_at.is_none());

    // Default recall should include it again.
    let result = repository::recall(&conn, &RecallQuery::default()).unwrap();
    assert_eq!(result.total, 1);
}

#[test]
fn unarchive_is_idempotent() {
    let conn = test_db();

    let mem = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Idempotent unarchive test".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    // Unarchiving a non-archived memory should succeed without error.
    let result = repository::unarchive(&conn, &mem.id).unwrap();
    assert!(result.archived_at.is_none());
}

#[test]
fn unarchive_not_found() {
    let conn = test_db();
    let err = repository::unarchive(&conn, "nonexistent-id").unwrap_err();
    assert!(matches!(err, ClioError::NotFound(_)));
}

// ---------------------------------------------------------------------------
// List namespaces
// ---------------------------------------------------------------------------

#[test]
fn list_namespaces_returns_distinct_sorted() {
    let conn = test_db();

    for ns in &["project:beta", "global", "project:alpha", "global"] {
        let _ = repository::remember(
            &conn,
            &RememberInput {
                namespace: ns.to_string(),
                kind: "note".into(),
                title: None,
                summary: None,
                content: format!("Memory in {ns}"),
                tags: vec![],
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Settings::default(),
        )
        .unwrap();
    }

    let namespaces = repository::list_namespaces(&conn).unwrap();
    assert_eq!(namespaces, vec!["global", "project:alpha", "project:beta"]);
}

#[test]
fn list_namespaces_empty_db() {
    let conn = test_db();
    let namespaces = repository::list_namespaces(&conn).unwrap();
    assert!(namespaces.is_empty());
}

// ---------------------------------------------------------------------------
// Link creation
// ---------------------------------------------------------------------------

#[test]
fn link_creation_and_retrieval() {
    let conn = test_db();

    let mem1 = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "decision".into(),
            title: Some("Decision A".into()),
            summary: None,
            content: "First decision".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let mem2 = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "decision".into(),
            title: Some("Decision B".into()),
            summary: None,
            content: "Supports A".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let link = repository::link(
        &conn,
        &LinkInput {
            from_memory_id: mem1.id.clone(),
            to_memory_id: mem2.id.clone(),
            relationship: "supports".into(),
            metadata: serde_json::json!({"reason": "follow-up"}),
        },
    )
    .unwrap();

    assert_eq!(link.from_memory_id, mem1.id);
    assert_eq!(link.to_memory_id, mem2.id);
    assert_eq!(link.relationship, "supports");

    let links = repository::get_links(&conn, &mem1.id).unwrap();
    assert_eq!(links.len(), 1);
}

#[test]
fn link_to_nonexistent_memory_fails() {
    let conn = test_db();

    let mem = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "exists".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let err = repository::link(
        &conn,
        &LinkInput {
            from_memory_id: mem.id.clone(),
            to_memory_id: "nonexistent".into(),
            relationship: "relates_to".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap_err();

    assert!(matches!(err, ClioError::Validation(_)));
}

// ---------------------------------------------------------------------------
// JSONL export shape
// ---------------------------------------------------------------------------

#[test]
fn export_jsonl_shape() {
    let conn = test_db();

    let _ = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:ai".into(),
            kind: "decision".into(),
            title: Some("Use SQLite".into()),
            summary: Some("SQLite is the default store.".into()),
            content: "Shared memory uses SQLite with WAL mode.".into(),
            tags: vec!["sqlite".into(), "architecture".into()],
            source: Some("codex".into()),
            source_ref: Some("design-001".into()),
            confidence: Some(0.93),
            importance: 4,
            metadata: serde_json::json!({"origin": "planning"}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let mut buf = Vec::new();
    let count = export::export_jsonl(&conn, &mut buf, None, false).unwrap();
    assert_eq!(count, 1);

    let output = String::from_utf8(buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();

    assert_eq!(parsed["namespace"], "project:ai");
    assert_eq!(parsed["kind"], "decision");
    assert_eq!(parsed["title"], "Use SQLite");
    assert_eq!(
        parsed["tags"],
        serde_json::json!(["architecture", "sqlite"])
    );
    assert_eq!(parsed["source"], "codex");
    assert_eq!(parsed["source_ref"], "design-001");
    assert_eq!(parsed["confidence"], 0.93);
    assert_eq!(parsed["importance"], 4);
    assert!(parsed["id"].is_string());
    assert!(parsed["created_at"].is_string());
    assert!(parsed["updated_at"].is_string());
}

// ---------------------------------------------------------------------------
// Import JSONL round-trip
// ---------------------------------------------------------------------------

#[test]
fn import_jsonl_round_trip() {
    let conn = test_db();

    // Create some memories.
    for i in 0..3 {
        let _ = repository::remember(
            &conn,
            &RememberInput {
                namespace: "global".into(),
                kind: "note".into(),
                title: Some(format!("Note {i}")),
                summary: None,
                content: format!("Content {i}"),
                tags: vec![format!("tag{i}")],
                source: Some("test".into()),
                source_ref: Some(format!("ref-{i}")),
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Settings::default(),
        )
        .unwrap();
    }

    // Export.
    let mut export_buf = Vec::new();
    let exported = export::export_jsonl(&conn, &mut export_buf, None, false).unwrap();
    assert_eq!(exported, 3);

    // Import into a fresh DB.
    let conn2 = test_db();
    let mut reader = std::io::Cursor::new(export_buf);
    let result = export::import_jsonl(&conn2, &mut reader).unwrap();
    assert_eq!(result.imported, 3);
    assert_eq!(result.skipped, 0);

    let recent = repository::recent(&conn2, None, 10).unwrap();
    assert_eq!(recent.total, 3);
}

// ---------------------------------------------------------------------------
// Schema info
// ---------------------------------------------------------------------------

#[test]
fn schema_info_returns_summary() {
    let conn = test_db();
    let info = repository::schema_info(&conn).unwrap();
    assert!(info.contains("Clio Database Schema"));
    assert!(info.contains("001_initial"));
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

#[test]
fn stats_returns_counts() {
    let conn = test_db();

    // Insert some memories across namespaces and kinds.
    for (ns, kind) in &[
        ("global", "note"),
        ("global", "decision"),
        ("project:ai", "note"),
    ] {
        let _ = repository::remember(
            &conn,
            &RememberInput {
                namespace: ns.to_string(),
                kind: kind.to_string(),
                title: Some(format!("{ns}/{kind}")),
                summary: None,
                content: format!("Content for {ns}/{kind}"),
                tags: vec!["test".into()],
                source: None,
                source_ref: None,
                confidence: None,
                importance: 3,
                metadata: serde_json::json!({}),
                valid_from: None,
                valid_until: None,
                upsert: false,
            },
            &Settings::default(),
        )
        .unwrap();
    }

    let stats = clio_core::stats::memory_stats(&conn, None).unwrap();
    assert_eq!(stats.total_memories, 3);
    assert_eq!(stats.active_memories, 3);
    assert_eq!(stats.archived_memories, 0);
    assert!(stats.by_namespace.len() >= 2);
    assert!(stats.by_kind.len() >= 2);
    assert!(!stats.top_tags.is_empty());
}

// ---------------------------------------------------------------------------
// Recent activity
// ---------------------------------------------------------------------------

#[test]
fn activity_shows_recent_events() {
    let conn = test_db();

    let mem = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Activity test".into()),
            summary: None,
            content: "Testing activity feed".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let activity = clio_core::stats::recent_activity(&conn, None, 10).unwrap();
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].action, "created");
    assert_eq!(activity[0].memory_id, mem.id);

    // Archive it and check activity changes.
    repository::archive(&conn, &mem.id).unwrap();
    let activity = clio_core::stats::recent_activity(&conn, None, 10).unwrap();
    assert_eq!(activity[0].action, "archived");
}

// ---------------------------------------------------------------------------
// Graph neighbours
// ---------------------------------------------------------------------------

#[test]
fn get_neighbours_traverses_links() {
    let conn = test_db();

    let mem_a = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Node A".into()),
            summary: None,
            content: "Root node".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let mem_b = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Node B".into()),
            summary: None,
            content: "Linked to A".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let mem_c = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Node C".into()),
            summary: None,
            content: "Linked to B".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    // Create links: A -> B -> C
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: mem_a.id.clone(),
            to_memory_id: mem_b.id.clone(),
            relationship: "relates_to".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();

    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: mem_b.id.clone(),
            to_memory_id: mem_c.id.clone(),
            relationship: "relates_to".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();

    // Depth 1 from A: should find B only.
    let neighbours_1 = repository::get_neighbours(&conn, &mem_a.id, 1).unwrap();
    assert_eq!(neighbours_1.len(), 1);
    assert_eq!(neighbours_1[0].id, mem_b.id);

    // Depth 2 from A: should find B and C.
    let neighbours_2 = repository::get_neighbours(&conn, &mem_a.id, 2).unwrap();
    assert_eq!(neighbours_2.len(), 2);
    let ids: Vec<&str> = neighbours_2.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&mem_b.id.as_str()));
    assert!(ids.contains(&mem_c.id.as_str()));

    // Depth 1 from C: should find B (incoming link).
    let neighbours_c = repository::get_neighbours(&conn, &mem_c.id, 1).unwrap();
    assert_eq!(neighbours_c.len(), 1);
    assert_eq!(neighbours_c[0].id, mem_b.id);
}

#[test]
fn get_neighbours_no_links() {
    let conn = test_db();

    let mem = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "Isolated node".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let neighbours = repository::get_neighbours(&conn, &mem.id, 3).unwrap();
    assert!(neighbours.is_empty());
}

// ---------------------------------------------------------------------------
// Graph-aware recall (include_links)
// ---------------------------------------------------------------------------

#[test]
fn recall_with_include_links_appends_linked_memories() {
    let conn = test_db();

    let mem_a = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "decision".into(),
            title: Some("Use Rust".into()),
            summary: None,
            content: "We decided to use Rust for the core.".into(),
            tags: vec!["rust".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 4,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    let mem_b = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Rust performance".into()),
            summary: None,
            content: "Rust gives us memory safety without garbage collection.".into(),
            tags: vec!["rust".into(), "performance".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &Settings::default(),
    )
    .unwrap();

    // Link A -> B
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: mem_a.id.clone(),
            to_memory_id: mem_b.id.clone(),
            relationship: "supports".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();

    // Recall with FTS that only matches mem_a, but include_links should also bring in mem_b.
    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("decided".into()),
            include_links: true,
            ..Default::default()
        },
    )
    .unwrap();

    // Should have at least 2 items: the direct match + the linked memory.
    assert!(
        result.items.len() >= 2,
        "expected at least 2, got {}",
        result.items.len()
    );

    // The linked memory should have linked_from set.
    let linked_item = result.items.iter().find(|i| i.memory.id == mem_b.id);
    assert!(
        linked_item.is_some(),
        "linked memory B should be in results"
    );
    assert_eq!(
        linked_item.unwrap().linked_from.as_deref(),
        Some(mem_a.id.as_str())
    );

    // Without include_links, should only find the direct match.
    let result_no_links = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("decided".into()),
            include_links: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result_no_links.items.len(), 1);
    assert!(result_no_links.items[0].linked_from.is_none());
}

fn link_simple(conn: &rusqlite::Connection, from: &Memory, to: &Memory) {
    repository::link(
        conn,
        &LinkInput {
            from_memory_id: from.id.clone(),
            to_memory_id: to.id.clone(),
            relationship: "relates_to".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();
}

#[test]
fn recall_with_include_links_hides_archived_and_expired_targets() {
    let conn = test_db();

    let anchor = remember_simple(&conn, "anchor memory about quinces");
    let live = remember_simple(&conn, "linked live fact");
    let archived = remember_simple(&conn, "linked archived fact");
    repository::archive(&conn, &archived.id).unwrap();
    let expired = repository::remember(
        &conn,
        &RememberInput {
            valid_until: Some("2000-01-01T00:00:00Z".into()),
            ..base_input("linked expired fact")
        },
        &Settings::default(),
    )
    .unwrap();

    link_simple(&conn, &anchor, &live);
    link_simple(&conn, &anchor, &archived);
    link_simple(&conn, &anchor, &expired);

    // Default recall: an archived linked target must stay hidden.
    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("quinces".into()),
            include_links: true,
            ..Default::default()
        },
    )
    .unwrap();
    let ids: Vec<&str> = result.items.iter().map(|i| i.memory.id.as_str()).collect();
    assert!(
        ids.contains(&live.id.as_str()),
        "live linked target should appear"
    );
    assert!(
        !ids.contains(&archived.id.as_str()),
        "archived linked target must stay hidden in default recall"
    );
    assert_eq!(result.count as usize, result.items.len());

    // Expiry eligibility follows the parent query.
    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("quinces".into()),
            include_links: true,
            exclude_expired: true,
            ..Default::default()
        },
    )
    .unwrap();
    let ids: Vec<&str> = result.items.iter().map(|i| i.memory.id.as_str()).collect();
    assert!(ids.contains(&live.id.as_str()));
    assert!(
        !ids.contains(&expired.id.as_str()),
        "expired linked target must stay hidden when the parent recall excludes expired"
    );
    assert!(!ids.contains(&archived.id.as_str()));
    assert_eq!(result.count as usize, result.items.len());
}

#[test]
fn recall_with_include_archived_still_expands_archived_targets() {
    // Characterisation: an explicitly requested archived recall keeps working,
    // and linked expansion follows the parent query's eligibility.
    let conn = test_db();

    let anchor = remember_simple(&conn, "anchor memory about medlars");
    let archived = remember_simple(&conn, "linked archived companion");
    repository::archive(&conn, &archived.id).unwrap();
    link_simple(&conn, &anchor, &archived);

    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("medlars".into()),
            include_links: true,
            include_archived: true,
            ..Default::default()
        },
    )
    .unwrap();
    let archived_item = result.items.iter().find(|i| i.memory.id == archived.id);
    assert!(
        archived_item.is_some(),
        "archived linked target should appear when the parent recall includes archived"
    );
    assert_eq!(
        archived_item.unwrap().linked_from.as_deref(),
        Some(anchor.id.as_str())
    );
}

#[test]
fn bulk_link_expansion_returns_linked_memories() {
    let conn = test_db();
    let settings = Settings::default();

    // Create three memories.
    let a = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Memory A".into()),
            summary: None,
            content: "First memory about apples".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &settings,
    )
    .unwrap();

    let b = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Memory B".into()),
            summary: None,
            content: "Second memory about bananas".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &settings,
    )
    .unwrap();

    let c = repository::remember(
        &conn,
        &RememberInput {
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("Memory C".into()),
            summary: None,
            content: "Third memory about cherries".into(),
            tags: vec![],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &settings,
    )
    .unwrap();

    // Link A -> B and A -> C.
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: a.id.clone(),
            to_memory_id: b.id.clone(),
            relationship: "relates_to".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: a.id.clone(),
            to_memory_id: c.id.clone(),
            relationship: "relates_to".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();

    // Recall with include_links — should return A plus linked B and C.
    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("apples".into()),
            namespace: None,
            kind: None,
            tags: vec![],
            match_all_tags: true,
            include_archived: false,
            archived_only: false,
            include_links: true,
            exclude_expired: false,
            importance_min: None,
            importance_max: None,
            sort_by: None,
            offset: 0,
            limit: 50,
            scoring: None,
            skip_access_tracking: false,
            match_any_term: false,
        },
    )
    .unwrap();

    let ids: Vec<&str> = result.items.iter().map(|i| i.memory.id.as_str()).collect();
    assert!(
        ids.contains(&a.id.as_str()),
        "should contain source memory A"
    );
    assert!(
        ids.contains(&b.id.as_str()),
        "should contain linked memory B"
    );
    assert!(
        ids.contains(&c.id.as_str()),
        "should contain linked memory C"
    );

    // Verify linked_from is set on the linked items.
    let b_item = result.items.iter().find(|i| i.memory.id == b.id).unwrap();
    assert_eq!(b_item.linked_from.as_deref(), Some(a.id.as_str()));
}

// ---------------------------------------------------------------------------
// Deduplication: merge retains tags in memory_tags table
// ---------------------------------------------------------------------------

#[test]
fn merge_retains_tags_in_memory_tags_table() {
    let conn = test_db();
    let keep = remember_with_tags(&conn, "Primary content about rust", &["alpha", "beta"]);
    let dup = remember_with_tags(&conn, "Duplicate content about rust", &["beta", "gamma"]);

    clio_core::deduplication::merge_memories(&conn, &keep.id, std::slice::from_ref(&dup.id))
        .unwrap();

    // Regression: the normalised memory_tags rows for the kept memory were silently
    // dropped because the re-insert omitted the NOT NULL created_at column.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_tags WHERE memory_id = ?1",
            [&keep.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 3,
        "kept memory should hold the union of tags (alpha, beta, gamma)"
    );
}

// ---------------------------------------------------------------------------
// FTS multi-term search
// ---------------------------------------------------------------------------

#[test]
fn recall_multi_term_matches_documents_containing_all_terms() {
    let conn = test_db();
    remember_simple(&conn, "We use rust together with sqlite for storage");
    remember_simple(&conn, "Unrelated python notes about pandas");

    let q = RecallQuery {
        query: Some("rust sqlite".into()),
        ..Default::default()
    };
    let res = repository::recall(&conn, &q).unwrap();

    // Both terms appear in the first doc but are not adjacent; multi-term AND must match it.
    assert_eq!(
        res.count, 1,
        "multi-term query should match the doc containing both terms"
    );
    assert!(res.items[0].memory.content.contains("rust"));
}

// ---------------------------------------------------------------------------
// Backup: WAL-safe standalone snapshot
// ---------------------------------------------------------------------------

#[test]
fn backup_produces_standalone_snapshot_without_wal() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("memory.db");
    let conn = clio_core::db::open(&db_path).unwrap();
    repository::remember(&conn, &base_input("back me up"), &Settings::default()).unwrap();

    let dest = dir.path().join("backups");
    let res = clio_core::backup::backup(&db_path, Some(&dest), 5).unwrap();
    let backup_path = std::path::Path::new(&res.path);

    // The snapshot must be a complete, standalone DB needing no WAL sidecar.
    assert!(backup_path.exists(), "backup file should exist");
    assert!(
        !backup_path.with_extension("db-wal").exists(),
        "VACUUM INTO snapshot must not carry a -wal sidecar"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            std::fs::metadata(backup_path).unwrap().permissions().mode() & 0o777,
            0o600,
            "database backups contain private memory and must not be group/world-readable"
        );
    }
    let bconn = rusqlite::Connection::open(backup_path).unwrap();
    let n: i64 = bconn
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        n, 1,
        "the snapshot must contain the row, even if it was still in the WAL"
    );
}

// ---------------------------------------------------------------------------
// Restore: safety snapshot before overwrite
// ---------------------------------------------------------------------------

#[test]
fn restore_creates_pre_restore_safety_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("memory.db");
    let conn = clio_core::db::open(&db_path).unwrap();
    repository::remember(&conn, &base_input("original row"), &Settings::default()).unwrap();

    let dest = dir.path().join("backups");
    let res = clio_core::backup::backup(&db_path, Some(&dest), 5).unwrap();

    // Change the live DB after the backup, then restore.
    repository::remember(
        &conn,
        &base_input("added after backup"),
        &Settings::default(),
    )
    .unwrap();
    drop(conn);

    let r = clio_core::backup::restore(&db_path, std::path::Path::new(&res.path)).unwrap();
    assert!(r.integrity_ok);

    // A safety snapshot of the pre-restore live DB must be written.
    assert!(
        db_path.with_extension("db.pre-restore").exists(),
        "restore should snapshot the live DB before overwriting it"
    );

    // The restored DB reflects the backup (1 row) and leaves no stale WAL.
    assert!(!db_path.with_extension("db-wal").exists());
    let conn2 = clio_core::db::open(&db_path).unwrap();
    let n: i64 = conn2
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        n, 1,
        "restored DB should match the backup, not the post-backup state"
    );
}

// ---------------------------------------------------------------------------
// Character-based validation
// ---------------------------------------------------------------------------

#[test]
fn validates_namespace_length_by_characters_not_bytes() {
    let conn = test_db();
    // 120 two-byte characters = 240 bytes, but 120 chars — valid by the schema's
    // character-based CHECK constraint.
    let namespace = "é".repeat(120);
    let input = RememberInput {
        namespace,
        ..base_input("multibyte namespace content")
    };

    let result = repository::remember(&conn, &input, &Settings::default());
    assert!(
        result.is_ok(),
        "a 120-character namespace must pass character-based validation"
    );
}

// ---------------------------------------------------------------------------
// Deduplication — access_count invariant
// ---------------------------------------------------------------------------

#[test]
fn merge_does_not_inflate_access_count() {
    let conn = test_db();
    let keep = remember_simple(&conn, "keep this memory");
    let dup = remember_simple(&conn, "duplicate memory");

    clio_core::deduplication::merge_memories(&conn, &keep.id, std::slice::from_ref(&dup.id))
        .unwrap();

    let access_count: i64 = conn
        .query_row(
            "SELECT access_count FROM memories WHERE id = ?1",
            [&keep.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        access_count, 0,
        "a merge is maintenance and must not bump access_count"
    );
}

// ---------------------------------------------------------------------------
// recall_scoped — total counting (characterisation / regression)
// ---------------------------------------------------------------------------

#[test]
fn recall_scoped_total_counts_each_namespace_once() {
    let conn = test_db();
    // Two matches in the project namespace, one in global — all disjoint by namespace.
    remember_in(&conn, "proj", "alpha one note");
    remember_in(&conn, "proj", "alpha two note");
    remember_in(&conn, "global", "alpha three note");

    // limit high enough that the scoped pass does not satisfy it alone, exercising the merge.
    let q = RecallQuery {
        query: Some("alpha".into()),
        limit: 5,
        ..Default::default()
    };
    let res = repository::recall_scoped(&conn, &q, "proj").unwrap();

    assert_eq!(res.count, 3, "should merge 2 project + 1 global match");
    assert_eq!(
        res.total, 3,
        "disjoint namespaces — each counted once, no double count"
    );
}

// ---------------------------------------------------------------------------
// Expiry filtering (valid_until)
// ---------------------------------------------------------------------------

#[test]
fn exclude_expired_filters_only_past_valid_until() {
    let conn = test_db();

    let stale = RememberInput {
        valid_until: Some("2000-01-01T00:00:00Z".into()),
        ..base_input("stale fact")
    };
    repository::remember(&conn, &stale, &Settings::default()).unwrap();

    let future = RememberInput {
        valid_until: Some("2999-01-01T00:00:00Z".into()),
        ..base_input("future fact")
    };
    repository::remember(&conn, &future, &Settings::default()).unwrap();

    remember_simple(&conn, "live fact"); // valid_until = None

    // Default recall: all three visible (backwards-compatible).
    assert_eq!(
        repository::recall(&conn, &RecallQuery::default())
            .unwrap()
            .total,
        3
    );

    // Opt-in expiry filter: drops only the past-expired memory.
    let q = RecallQuery {
        exclude_expired: true,
        ..RecallQuery::default()
    };
    let res = repository::recall(&conn, &q).unwrap();
    assert_eq!(res.total, 2);
    assert!(
        res.items
            .iter()
            .all(|i| !i.memory.content.contains("stale"))
    );
}

// ---------------------------------------------------------------------------
// Write-path deduplication
// ---------------------------------------------------------------------------

#[test]
fn find_content_duplicate_matches_same_namespace_non_archived() {
    let conn = test_db();
    let m = remember_in(&conn, "proj:a", "the sky is blue");

    // Same namespace + identical content → the existing id.
    assert_eq!(
        repository::find_content_duplicate(&conn, "proj:a", "the sky is blue").unwrap(),
        Some(m.id.clone())
    );
    // Different namespace → no match.
    assert_eq!(
        repository::find_content_duplicate(&conn, "proj:b", "the sky is blue").unwrap(),
        None
    );
    // Different content → no match.
    assert_eq!(
        repository::find_content_duplicate(&conn, "proj:a", "the grass is green").unwrap(),
        None
    );

    // Archived memories are not matched — archive means hidden, so a re-capture
    // should create a fresh live memory rather than resurrect a hidden one.
    repository::archive(&conn, &m.id).unwrap();
    assert_eq!(
        repository::find_content_duplicate(&conn, "proj:a", "the sky is blue").unwrap(),
        None
    );
}

#[test]
fn capture_of_identical_content_does_not_duplicate() {
    use clio_core::capture::{CaptureResult, ClassificationResult, capture_with_classification};

    let conn = test_db();
    let classification = ClassificationResult {
        kind: "fact".into(),
        title: "Env config".into(),
        summary: "X is configured via env".into(),
        tags: vec![],
        namespace: "proj:a".into(),
        importance: 3,
        confidence: 0.9,
    };
    let body = "X is configured via env";

    let stored_id = |r: CaptureResult| match r {
        CaptureResult::Stored(m) => m.id,
        CaptureResult::Queued(_) => panic!("expected Stored, got Queued"),
    };

    let first = stored_id(
        capture_with_classification(
            &conn,
            body,
            &classification,
            None,
            None,
            &Settings::default(),
        )
        .unwrap(),
    );
    let second = stored_id(
        capture_with_classification(
            &conn,
            body,
            &classification,
            None,
            None,
            &Settings::default(),
        )
        .unwrap(),
    );

    assert_eq!(
        first, second,
        "re-capture should return the existing memory"
    );

    let res = repository::recall(
        &conn,
        &RecallQuery {
            namespace: Some("proj:a".into()),
            ..RecallQuery::default()
        },
    )
    .unwrap();
    assert_eq!(res.total, 1, "no duplicate row should be created");
}

#[test]
fn capture_and_distill_share_one_namespace_precedence() {
    use clio_core::capture::{CaptureResult, ClassificationResult, capture_with_classification};

    let conn = test_db();
    let classify_into = |ns: &str, title: &str| ClassificationResult {
        kind: "fact".into(),
        title: title.into(),
        summary: "s".into(),
        tags: vec![],
        namespace: ns.into(),
        importance: 3,
        confidence: 0.9,
    };
    let capture_into = |classification: &ClassificationResult,
                        override_ns: Option<&str>,
                        default_ns: Option<&str>|
     -> String {
        match capture_with_classification(
            &conn,
            &format!("body for {}", classification.title),
            classification,
            override_ns,
            default_ns,
            &Settings::default(),
        )
        .unwrap()
        {
            CaptureResult::Stored(m) => m.namespace,
            CaptureResult::Queued(_) => panic!("expected Stored, got Queued"),
        }
    };

    // The drift this guards against: capture once let the working directory
    // override a model's `global` promotion while distill honoured it, so the
    // same classification landed in different namespaces depending on which
    // command stored it. Both now resolve through capture::resolve_namespace.
    assert_eq!(
        capture_into(
            &classify_into("global", "applies everywhere"),
            None,
            Some("project:cwd")
        ),
        "global",
        "the model's global promotion must beat the working-directory default"
    );
    assert_eq!(
        capture_into(
            &classify_into("project:model-idea", "project fact"),
            None,
            Some("project:cwd")
        ),
        "project:cwd",
        "a non-global suggestion yields to the working directory"
    );
    assert_eq!(
        capture_into(
            &classify_into("global", "explicitly filed"),
            Some("project:explicit"),
            Some("project:cwd")
        ),
        "project:explicit",
        "an explicit override beats everything, including a global promotion"
    );
}

#[test]
fn approve_review_of_duplicate_content_does_not_create_second_memory() {
    use clio_core::review::{ReviewInput, approve_review, queue_for_review};

    let conn = test_db();
    let mk = || ReviewInput {
        content: "shared review content".into(),
        suggested_namespace: "proj:a".into(),
        suggested_kind: "note".into(),
        suggested_title: Some("t".into()),
        suggested_summary: None,
        suggested_tags: vec![],
        suggested_importance: 3,
        suggested_confidence: Some(0.4),
        source_route: Some("capture".into()),
        source_ref: None,
        metadata: serde_json::json!({}),
    };
    let r1 = queue_for_review(&conn, &mk()).unwrap();
    let r2 = queue_for_review(&conn, &mk()).unwrap();

    let m1 = approve_review(&conn, &r1.id, &Settings::default()).unwrap();
    let m2 = approve_review(&conn, &r2.id, &Settings::default()).unwrap();

    assert_eq!(
        m1.id, m2.id,
        "approving duplicate content should return the existing memory"
    );

    let res = repository::recall(
        &conn,
        &RecallQuery {
            namespace: Some("proj:a".into()),
            ..RecallQuery::default()
        },
    )
    .unwrap();
    assert_eq!(res.total, 1, "no duplicate row should be created");
}

#[test]
fn approve_review_preserves_source_reference_idempotency() {
    use clio_core::review::{ReviewInput, approve_review, queue_for_review};

    let conn = test_db();
    repository::remember(
        &conn,
        &RememberInput {
            namespace: "test".into(),
            ..base_input("shared review content")
        },
        &Settings::default(),
    )
    .unwrap();
    let input = ReviewInput {
        content: "shared review content".into(),
        suggested_namespace: "test".into(),
        suggested_kind: "fact".into(),
        suggested_title: Some("provenanced".into()),
        suggested_summary: None,
        suggested_tags: vec![],
        suggested_importance: 4,
        suggested_confidence: Some(0.9),
        source_route: Some("import".into()),
        source_ref: Some("record-1".into()),
        metadata: serde_json::json!({}),
    };
    let first = queue_for_review(&conn, &input).unwrap();
    let second = queue_for_review(&conn, &input).unwrap();

    let first_memory = approve_review(&conn, &first.id, &Settings::default()).unwrap();
    let second_memory = approve_review(&conn, &second.id, &Settings::default()).unwrap();

    assert_eq!(first_memory.id, second_memory.id);
    assert_eq!(second_memory.source.as_deref(), Some("import"));
    assert_eq!(second_memory.source_ref.as_deref(), Some("record-1"));
}

#[test]
fn empty_patch_does_not_advance_updated_at() {
    let conn = test_db();
    let memory = remember_simple(&conn, "unchanged");
    let patched = repository::update(
        &conn,
        &memory.id,
        &UpdateInput {
            expected_updated_at: Some(memory.updated_at.clone()),
            ..UpdateInput::default()
        },
        &Settings::default(),
    )
    .unwrap();

    assert_eq!(patched.updated_at, memory.updated_at);
}

// ---------------------------------------------------------------------------
// Semantic recall: composite scoring fusion + expiry
// ---------------------------------------------------------------------------

#[test]
fn semantic_search_returns_best_match_first() {
    use clio_core::embeddings::{semantic_search, store_embedding};

    let conn = test_db();
    let best = remember_simple(&conn, "best semantic match");
    let middle = remember_simple(&conn, "middle semantic match");
    let worst = remember_simple(&conn, "worst semantic match");

    store_embedding(&conn, &best.id, "test", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &middle.id, "test", 2, &[0.8, 0.6]).unwrap();
    store_embedding(&conn, &worst.id, "test", 2, &[0.0, 1.0]).unwrap();

    let query = [1.0_f32, 0.0];
    let results = semantic_search(&conn, &query, "test", None, false, false, 10).unwrap();

    assert_eq!(results[0].memory_id, best.id);
    assert_eq!(results[1].memory_id, middle.id);
    assert_eq!(results[2].memory_id, worst.id);
    assert!(results[0].similarity >= results[1].similarity);
    assert!(results[1].similarity >= results[2].similarity);
}

#[test]
fn semantic_recall_min_similarity_floor_drops_weak_matches() {
    use clio_core::embeddings::{semantic_recall, store_embedding};
    use clio_core::settings::ScoringConfig;

    let conn = test_db();
    let close = remember_simple(&conn, "close semantic match");
    let far = remember_simple(&conn, "far semantic match");
    store_embedding(&conn, &close.id, "test", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &far.id, "test", 2, &[0.0, 1.0]).unwrap();

    let query = [1.0_f32, 0.0];
    let scoring = ScoringConfig {
        decay_lambda: 0.0,
        access_boost_weight: 0.0,
        min_similarity: 0.5,
    };
    let results = semantic_recall(
        &conn,
        "zzqq",
        &query,
        "test",
        None,
        false,
        false,
        Some(&scoring),
        10,
    )
    .unwrap();
    assert_eq!(results.len(), 1, "orthogonal match falls below the floor");
    assert_eq!(results[0].memory.id, close.id);

    // Nothing above the floor: an empty result, not the least-bad guess.
    let none = semantic_recall(
        &conn,
        "zzqq",
        &[0.0_f32, -1.0],
        "test",
        None,
        false,
        false,
        Some(&scoring),
        10,
    )
    .unwrap();
    assert!(none.is_empty());
}

#[test]
fn semantic_recall_importance_lifts_weaker_match_when_scoring_enabled() {
    use clio_core::embeddings::{semantic_recall, store_embedding};
    use clio_core::settings::ScoringConfig;

    let conn = test_db();

    // A: perfect cosine match but lowest importance.
    let a = RememberInput {
        importance: 1,
        ..base_input("alpha content")
    };
    let a = repository::remember(&conn, &a, &Settings::default()).unwrap();
    store_embedding(&conn, &a.id, "test", 2, &[1.0, 0.0]).unwrap();

    // B: weaker cosine match but highest importance.
    let b = RememberInput {
        importance: 5,
        ..base_input("beta content")
    };
    let b = repository::remember(&conn, &b, &Settings::default()).unwrap();
    store_embedding(&conn, &b.id, "test", 2, &[0.9, 0.436]).unwrap();

    let query = [1.0_f32, 0.0];

    // Without scoring: pure cosine — the perfect match A ranks first.
    let plain =
        semantic_recall(&conn, "zzqq", &query, "test", None, false, false, None, 10).unwrap();
    assert_eq!(plain[0].memory.id, a.id, "pure cosine should rank A first");

    // With scoring: importance lifts B above A.
    let scoring = ScoringConfig {
        decay_lambda: 0.01,
        access_boost_weight: 0.1,
        min_similarity: 0.0,
    };
    let scored = semantic_recall(
        &conn,
        "zzqq",
        &query,
        "test",
        None,
        false,
        false,
        Some(&scoring),
        10,
    )
    .unwrap();
    assert_eq!(
        scored[0].memory.id, b.id,
        "composite scoring should lift high-importance B above A"
    );
}

#[test]
fn semantic_recall_keyword_boost_is_proportional() {
    use clio_core::embeddings::{semantic_recall, store_embedding};

    let conn = test_db();

    // STRONG_KW: short doc containing the phrase — high BM25.
    let strong = remember_simple(&conn, "borrow checker");
    store_embedding(&conn, &strong.id, "test", 2, &[0.8, 0.6]).unwrap();

    // WEAK_KW: same phrase but buried in a long doc — lower BM25. Same cosine.
    let padding = "padding filler text ".repeat(40);
    let weak = remember_simple(&conn, &format!("borrow checker {padding}"));
    store_embedding(&conn, &weak.id, "test", 2, &[0.8, 0.6]).unwrap();

    // SEM: perfect cosine but no keyword match at all.
    let sem = remember_simple(&conn, "quantum entanglement");
    store_embedding(&conn, &sem.id, "test", 2, &[1.0, 0.0]).unwrap();

    let query = [1.0_f32, 0.0];
    let items = semantic_recall(
        &conn,
        "borrow checker",
        &query,
        "test",
        None,
        false,
        false,
        None,
        10,
    )
    .unwrap();

    let pos = |id: &str| items.iter().position(|it| it.memory.id == id).unwrap();

    // Stronger keyword match earns more boost → outranks the weaker one at equal cosine.
    assert!(
        pos(&strong.id) < pos(&weak.id),
        "stronger BM25 match should rank above the weaker one"
    );
    // A pure strong-semantic hit still beats a weak keyword match (boost doesn't over-lift).
    assert!(
        pos(&sem.id) < pos(&weak.id),
        "strong semantic should outrank a weak keyword match"
    );
}

#[test]
fn semantic_recall_scoped_includes_global_memories() {
    use clio_core::embeddings::{semantic_recall, semantic_recall_scoped, store_embedding};

    let conn = test_db();
    let project = remember_in(&conn, "project:x", "project semantic fact");
    let global = remember_in(&conn, "global", "global semantic fact");
    let other = remember_in(&conn, "project:y", "other semantic fact");

    store_embedding(&conn, &project.id, "test", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &global.id, "test", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &other.id, "test", 2, &[1.0, 0.0]).unwrap();

    let query = [1.0_f32, 0.0];
    let scoped = semantic_recall_scoped(
        &conn,
        "zzqq",
        &query,
        "test",
        "project:x",
        false,
        false,
        None,
        10,
    )
    .unwrap();
    let scoped_ids: std::collections::HashSet<_> =
        scoped.iter().map(|item| item.memory.id.as_str()).collect();

    assert!(scoped_ids.contains(project.id.as_str()));
    assert!(scoped_ids.contains(global.id.as_str()));
    assert!(!scoped_ids.contains(other.id.as_str()));

    let exact = semantic_recall(
        &conn,
        "zzqq",
        &query,
        "test",
        Some("project:x"),
        false,
        false,
        None,
        10,
    )
    .unwrap();
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].memory.id, project.id);
}

#[test]
fn semantic_recall_excludes_expired_when_requested() {
    use clio_core::embeddings::{semantic_recall, store_embedding};

    let conn = test_db();

    let stale = RememberInput {
        valid_until: Some("2000-01-01T00:00:00Z".into()),
        ..base_input("stale embedded fact")
    };
    let stale = repository::remember(&conn, &stale, &Settings::default()).unwrap();
    store_embedding(&conn, &stale.id, "test", 2, &[1.0, 0.0]).unwrap();

    let live = RememberInput {
        ..base_input("live embedded fact")
    };
    let live = repository::remember(&conn, &live, &Settings::default()).unwrap();
    store_embedding(&conn, &live.id, "test", 2, &[1.0, 0.0]).unwrap();

    let query = [1.0_f32, 0.0];

    // Default: both returned.
    let all = semantic_recall(&conn, "zzqq", &query, "test", None, false, false, None, 10).unwrap();
    assert_eq!(all.len(), 2);

    // exclude_expired: the past-expired memory is dropped.
    let live_only =
        semantic_recall(&conn, "zzqq", &query, "test", None, false, true, None, 10).unwrap();
    assert_eq!(live_only.len(), 1);
    assert_eq!(live_only[0].memory.id, live.id);
}

#[test]
fn semantic_recall_scoped_prefers_project_then_global() {
    use clio_core::embeddings::{semantic_recall, semantic_recall_scoped, store_embedding};

    let conn = test_db();

    let project = remember_in(&conn, "project:x", "project memory");
    store_embedding(&conn, &project.id, "test", 2, &[0.7, 0.7]).unwrap();

    let global = remember_in(&conn, "global", "global memory");
    store_embedding(&conn, &global.id, "test", 2, &[1.0, 0.0]).unwrap();

    let other = remember_in(&conn, "project:y", "other project memory");
    store_embedding(&conn, &other.id, "test", 2, &[1.0, 0.0]).unwrap();

    let query = [1.0_f32, 0.0];

    let scoped = semantic_recall_scoped(
        &conn,
        "zzqq",
        &query,
        "test",
        "project:x",
        false,
        false,
        None,
        2,
    )
    .unwrap();
    assert_eq!(scoped.len(), 2);
    assert_eq!(scoped[0].memory.id, project.id);
    assert_eq!(scoped[1].memory.id, global.id);

    let explicit = semantic_recall(
        &conn,
        "zzqq",
        &query,
        "test",
        Some("project:x"),
        false,
        false,
        None,
        2,
    )
    .unwrap();
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].memory.id, project.id);

    let global_only = semantic_recall_scoped(
        &conn, "zzqq", &query, "test", "global", false, false, None, 10,
    )
    .unwrap();
    assert_eq!(global_only.len(), 1);
    assert_eq!(global_only[0].memory.id, global.id);
}

// ---------------------------------------------------------------------------
// Content-duplicate probes (capture dedup + archived-twin revival)
// ---------------------------------------------------------------------------

#[test]
fn content_duplicate_probes_split_live_and_archived() {
    let conn = test_db();
    let m = remember_in(&conn, "project:x", "a durable fact worth keeping");

    // Live: found by find_content_duplicate, not by the archived probe.
    assert_eq!(
        repository::find_content_duplicate(&conn, "project:x", "a durable fact worth keeping")
            .unwrap()
            .as_deref(),
        Some(m.id.as_str())
    );
    assert!(
        repository::find_archived_duplicate(&conn, "project:x", "a durable fact worth keeping")
            .unwrap()
            .is_none()
    );

    // Once archived, the roles swap: live probe misses it, archived probe finds it.
    repository::archive(&conn, &m.id).unwrap();
    assert!(
        repository::find_content_duplicate(&conn, "project:x", "a durable fact worth keeping")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        repository::find_archived_duplicate(&conn, "project:x", "a durable fact worth keeping")
            .unwrap()
            .as_deref(),
        Some(m.id.as_str())
    );
}

// ---------------------------------------------------------------------------
// Scoped recall paging (detected namespace first, global fill)
// ---------------------------------------------------------------------------

#[test]
fn recall_scoped_pages_across_namespaces() {
    let conn = test_db();

    // Three memories in the detected namespace, three in global (disjoint).
    for i in 0..3 {
        remember_in(&conn, "projectx", &format!("scoped fact {i}"));
    }
    for i in 0..3 {
        remember_in(&conn, "global", &format!("global fact {i}"));
    }

    let page = |offset: u32, limit: u32| {
        repository::recall_scoped(
            &conn,
            &RecallQuery {
                limit,
                offset,
                ..RecallQuery::default()
            },
            "projectx",
        )
        .unwrap()
    };

    // Page 1: scoped namespace takes priority; total counts both namespaces once.
    let p1 = page(0, 2);
    assert_eq!(p1.total, 6);
    assert_eq!(p1.count, 2);
    assert!(p1.items.iter().all(|it| it.memory.namespace == "projectx"));

    // Page 2 (offset 2): pages across the boundary — last scoped + first global.
    let p2 = page(2, 2);
    assert_eq!(p2.total, 6);
    assert_eq!(p2.count, 2);
    assert_eq!(p2.items[0].memory.namespace, "projectx");
    assert_eq!(p2.items[1].memory.namespace, "global");

    // No id appears on both pages.
    let ids1: std::collections::HashSet<_> = p1.items.iter().map(|i| &i.memory.id).collect();
    assert!(p2.items.iter().all(|i| !ids1.contains(&i.memory.id)));
}

#[test]
fn recall_scoped_global_fallback_is_global_only() {
    let conn = test_db();

    remember_in(&conn, "global", "shared fact");
    remember_in(&conn, "project:x", "project fact");

    let res = repository::recall_scoped(&conn, &RecallQuery::default(), "global").unwrap();

    assert_eq!(res.total, 1);
    assert_eq!(res.count, 1);
    assert_eq!(res.items[0].memory.namespace, "global");
}

// ---------------------------------------------------------------------------
// Attention lifecycle (public API)
// ---------------------------------------------------------------------------

#[test]
fn attention_lifecycle_preserves_content_and_history() {
    use clio_core::attention;

    let conn = test_db();
    let memory = remember_simple(&conn, "Follow up: verify the deployment");

    let item = attention::create_attention(
        &conn,
        &attention::AttentionInput {
            memory_id: memory.id.clone(),
            owner: Some("user".into()),
            due_at: Some("2026-07-01T00:00:00Z".into()),
            ..attention::AttentionInput::default()
        },
    )
    .unwrap();
    assert_eq!(item.status, "open");

    // Eligible with a machine-readable reason at a fixed time.
    let eligible = attention::eligible(
        &conn,
        &attention::EligibilityContext {
            namespace: None,
            scope: None,
            now: "2026-07-29T12:00:00Z".into(),
            dormant_days: 0,
            max_age_days: 0,
        },
    )
    .unwrap();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].reason.as_str(), "overdue");

    // Snooze, then resolve with evidence; the source memory is untouched.
    attention::snooze(&conn, &item.id, "2026-08-01T00:00:00Z", Some("user")).unwrap();
    let evidence = remember_simple(&conn, "Deployment verified via smoke test");
    let resolved =
        attention::complete(&conn, &item.id, Some(&evidence.id), None, Some("user")).unwrap();
    assert_eq!(resolved.status, "resolved");

    let unchanged = repository::get(&conn, &memory.id).unwrap();
    assert_eq!(unchanged.content, "Follow up: verify the deployment");

    let history = clio_core::events::list_events(&conn, &memory.id, 20).unwrap();
    let types: Vec<&str> = history.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(types, vec!["attention_opened", "snoozed", "resolved"]);
}

// ---------------------------------------------------------------------------
// Session-attention fixture runner (payload -> operational outcome)
// ---------------------------------------------------------------------------

#[test]
fn session_attention_cases_route_to_the_annotated_outcome() {
    use clio_core::attention;
    use clio_core::checkpoint::{self, CheckpointRequest};

    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/session_attention_cases.json")).unwrap();

    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let name = case["name"].as_str().unwrap();
        let conn = test_db();
        let settings = Settings::default();

        // A resolution case needs a pre-existing open loop to complete.
        let target_id = {
            let target = remember_simple(&conn, "Open loop: verify the deployment");
            attention::create_attention(
                &conn,
                &attention::AttentionInput {
                    memory_id: target.id.clone(),
                    ..attention::AttentionInput::default()
                },
            )
            .unwrap();
            target.id
        };

        let payload = serde_json::to_string(&serde_json::json!({
            "memories": [case["payload"]]
        }))
        .unwrap()
        .replace("__TARGET_MEMORY_ID__", &target_id);
        let memories = clio_core::capture::parse_distillation(&payload).unwrap();
        assert_eq!(memories.len(), 1, "case {name}: payload must parse");

        let result = checkpoint::store_checkpoint(
            &conn,
            &CheckpointRequest {
                source: "claude-session".into(),
                session_id: format!("fixture-{index}"),
                cursor: 1,
                recover_stale: false,
                namespace_override: None,
                default_namespace: Some("project:clio".into()),
                cwd: None,
                branch: Some("develop".into()),
                ticket: Some("CLIO-42".into()),
            },
            &memories,
            &settings,
        )
        .unwrap();

        let expect = &case["expect"];
        let stored_new: Vec<&String> = result
            .stored_memory_ids
            .iter()
            .filter(|id| **id != target_id)
            .collect();
        assert_eq!(
            !stored_new.is_empty(),
            expect["stored"].as_bool().unwrap(),
            "case {name}: stored expectation"
        );
        assert_eq!(
            result.queued_review_ids.len() == 1,
            expect["review_queued"].as_bool().unwrap(),
            "case {name}: review expectation"
        );

        if expect["attention_open"].as_bool().unwrap() {
            let memory_id = stored_new[0];
            let item = attention::get_by_memory(&conn, memory_id).unwrap().unwrap();
            assert_eq!(item.status, "open", "case {name}: attention open");
            // Deterministic ticket tagging survives into the stored memory.
            let stored = repository::get(&conn, memory_id).unwrap();
            assert!(
                stored.tags.iter().any(|t| t == "ticket:clio-42"),
                "case {name}: ticket tag applied, got {:?}",
                stored.tags
            );
        } else if expect["stored"].as_bool().unwrap() {
            let memory_id = stored_new[0];
            assert!(
                attention::get_by_memory(&conn, memory_id)
                    .unwrap()
                    .is_none(),
                "case {name}: no attention expected"
            );
        }

        // A queued suggestion must carry enough metadata for approval to
        // open attention atomically.
        if expect["review_queued"].as_bool().unwrap() {
            let review_id = &result.queued_review_ids[0];
            let item = clio_core::review::get_review(&conn, review_id).unwrap();
            assert!(
                item.metadata.get("attention").is_some(),
                "case {name}: queued item keeps attention metadata"
            );
            let memory = clio_core::review::approve_review(&conn, review_id, &settings).unwrap();
            let opened = attention::get_by_memory(&conn, &memory.id).unwrap();
            assert!(
                opened.is_some(),
                "case {name}: approval opens attention atomically"
            );
        }

        // Resolution claims are review candidates, never automatic
        // completions: the target ALWAYS stays open, and an explicit
        // stable-ID claim records a visible candidate event.
        let target_state = attention::get_by_memory(&conn, &target_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            target_state.status, "open",
            "case {name}: model output can never close real work"
        );
        let history = clio_core::events::list_events(&conn, &target_state.memory_id, 20).unwrap();
        let has_candidate = history
            .iter()
            .any(|e| e.event_type == "resolution_candidate");
        assert_eq!(
            has_candidate,
            expect["resolution_candidate"].as_bool().unwrap_or(false),
            "case {name}: resolution candidate expectation"
        );
    }
}

// ---------------------------------------------------------------------------
// Directional, typed graph recall (Task 8)
// ---------------------------------------------------------------------------

#[test]
fn recall_includes_incoming_links_with_direction_and_metadata() {
    let conn = test_db();
    let anchor = remember_simple(&conn, "anchor memory about loquats");
    let upstream = remember_simple(&conn, "upstream evidence memory");

    // upstream --evidence_for--> anchor (an INCOMING edge for the anchor).
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: upstream.id.clone(),
            to_memory_id: anchor.id.clone(),
            relationship: "evidence_for".into(),
            metadata: serde_json::json!({ "note": "smoke test output" }),
        },
    )
    .unwrap();

    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("loquats".into()),
            include_links: true,
            ..Default::default()
        },
    )
    .unwrap();

    let linked = result
        .items
        .iter()
        .find(|i| i.memory.id == upstream.id)
        .expect("incoming-linked memory should be included");
    assert_eq!(linked.linked_from.as_deref(), Some(anchor.id.as_str()));
    assert_eq!(linked.link_context.len(), 1);
    let ctx = &linked.link_context[0];
    assert_eq!(ctx.direction, "incoming");
    assert_eq!(ctx.relationship, "evidence_for");
    assert_eq!(ctx.from_memory_id, upstream.id);
    assert_eq!(ctx.to_memory_id, anchor.id);
    assert_eq!(ctx.metadata["note"], "smoke test output");
}

#[test]
fn recall_preserves_multiple_edges_to_one_target() {
    let conn = test_db();
    let anchor = remember_simple(&conn, "anchor memory about damsons");
    let target = remember_simple(&conn, "richly linked target");

    for relationship in ["supports", "follow_up_of"] {
        repository::link(
            &conn,
            &LinkInput {
                from_memory_id: anchor.id.clone(),
                to_memory_id: target.id.clone(),
                relationship: relationship.into(),
                metadata: serde_json::json!({}),
            },
        )
        .unwrap();
    }
    // And one incoming edge from the target back to the anchor.
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: target.id.clone(),
            to_memory_id: anchor.id.clone(),
            relationship: "contradicts".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();

    let result = repository::recall(
        &conn,
        &RecallQuery {
            query: Some("damsons".into()),
            include_links: true,
            ..Default::default()
        },
    )
    .unwrap();

    let appearances: Vec<_> = result
        .items
        .iter()
        .filter(|i| i.memory.id == target.id)
        .collect();
    assert_eq!(appearances.len(), 1, "target memory deduplicated");
    let contexts = &appearances[0].link_context;
    assert_eq!(contexts.len(), 3, "every edge context preserved");
    let rels: Vec<&str> = contexts.iter().map(|c| c.relationship.as_str()).collect();
    assert!(rels.contains(&"supports"));
    assert!(rels.contains(&"follow_up_of"));
    assert!(rels.contains(&"contradicts"));
    assert_eq!(
        contexts
            .iter()
            .filter(|c| c.direction == "incoming")
            .count(),
        1
    );
}

#[test]
fn link_contexts_expose_both_directions_for_one_memory() {
    let conn = test_db();
    let memory = remember_simple(&conn, "central memory");
    let out_target = remember_simple(&conn, "outgoing target");
    let in_source = remember_simple(&conn, "incoming source");

    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: memory.id.clone(),
            to_memory_id: out_target.id.clone(),
            relationship: "supersedes".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();
    repository::link(
        &conn,
        &LinkInput {
            from_memory_id: in_source.id.clone(),
            to_memory_id: memory.id.clone(),
            relationship: "resolved_by".into(),
            metadata: serde_json::json!({}),
        },
    )
    .unwrap();

    let contexts = repository::get_link_contexts(&conn, &memory.id).unwrap();
    assert_eq!(contexts.len(), 2);
    let outgoing = contexts.iter().find(|c| c.direction == "outgoing").unwrap();
    assert_eq!(outgoing.relationship, "supersedes");
    assert_eq!(outgoing.to_memory_id, out_target.id);
    let incoming = contexts.iter().find(|c| c.direction == "incoming").unwrap();
    assert_eq!(incoming.relationship, "resolved_by");
    assert_eq!(incoming.from_memory_id, in_source.id);
}

#[test]
fn suggest_links_stays_in_namespace_and_skips_hidden_candidates() {
    use clio_core::embeddings::{EmbeddingBackend, store_embedding, suggest_links};

    struct SameVectorBackend;
    impl EmbeddingBackend for SameVectorBackend {
        fn model_name(&self) -> &str {
            "model-a"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            Ok(vec![1.0, 0.0])
        }
        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }

    let conn = test_db();
    let backend = SameVectorBackend;

    let source = remember_in(&conn, "project:x", "source memory");
    let same_ns = remember_in(&conn, "project:x", "same namespace candidate");
    let other_ns = remember_in(&conn, "project:y", "other namespace candidate");
    let archived = remember_in(&conn, "project:x", "archived candidate");
    repository::archive(&conn, &archived.id).unwrap();
    let expired = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:x".into(),
            valid_until: Some("2000-01-01T00:00:00Z".into()),
            ..base_input("expired candidate")
        },
        &Settings::default(),
    )
    .unwrap();

    for memory in [&source, &same_ns, &other_ns, &archived, &expired] {
        store_embedding(&conn, &memory.id, "model-a", 2, &[1.0, 0.0]).unwrap();
    }

    let suggestions = suggest_links(&conn, &source.id, &backend, 0.5, 10).unwrap();
    let ids: Vec<&str> = suggestions.iter().map(|(m, _)| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![same_ns.id.as_str()],
        "only live, unexpired, same-namespace candidates may be suggested"
    );
}

/// Count auto-links touching a memory in either direction.
fn auto_link_degree(conn: &rusqlite::Connection, id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM memory_links
         WHERE (from_memory_id = ?1 OR to_memory_id = ?1) AND relationship = 'auto:relates_to'",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn auto_link_skips_multiple_excluded_kinds_as_source_and_target() {
    use clio_core::embeddings::{EmbeddingBackend, auto_link_batch, store_embedding};
    use clio_core::settings::AutoLinkConfig;

    struct SameVectorBackend;
    impl EmbeddingBackend for SameVectorBackend {
        fn model_name(&self) -> &str {
            "model-a"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            Ok(vec![1.0, 0.0])
        }
        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }

    let conn = test_db();
    let note_a = remember_in(&conn, "project:x", "connection pooling enabled for the api");
    let note_b = remember_in(
        &conn,
        "project:x",
        "pooling switched on for postgres connections",
    );
    let receipt = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:x".into(),
            kind: "receipt".into(),
            ..base_input("session receipt describing the pooling work")
        },
        &Settings::default(),
    )
    .unwrap();
    let summary = repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:x".into(),
            kind: "summary".into(),
            ..base_input("session summary describing the pooling work")
        },
        &Settings::default(),
    )
    .unwrap();

    for m in [&note_a, &note_b, &receipt, &summary] {
        store_embedding(&conn, &m.id, "model-a", 2, &[1.0, 0.0]).unwrap();
    }

    // Two excluded kinds, so the dynamically numbered placeholders (?5, ?6) are
    // both exercised — the case the hand-numbering could get wrong.
    let config = AutoLinkConfig {
        enabled: true,
        threshold: 0.5,
        max_links_per_memory: 10,
        exclude_kinds: vec!["receipt".into(), "summary".into()],
        ..Default::default()
    };
    let report = auto_link_batch(&conn, &SameVectorBackend, None, None, &config).unwrap();

    assert_eq!(
        report.memories_processed, 4,
        "excluded kinds are passed over but still count as processed"
    );
    assert_eq!(report.memories_skipped, 0);
    assert!(report.links_created > 0, "the two notes should link");
    assert!(auto_link_degree(&conn, &note_a.id) > 0);
    assert_eq!(
        auto_link_degree(&conn, &receipt.id),
        0,
        "an excluded kind must not link in either direction"
    );
    assert_eq!(
        auto_link_degree(&conn, &summary.id),
        0,
        "an excluded kind must not link in either direction"
    );
}

#[test]
fn auto_link_cap_bounds_total_degree_and_binds() {
    use clio_core::embeddings::{EmbeddingBackend, auto_link_batch, store_embedding};
    use clio_core::settings::AutoLinkConfig;

    struct SameVectorBackend;
    impl EmbeddingBackend for SameVectorBackend {
        fn model_name(&self) -> &str {
            "model-a"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            Ok(vec![1.0, 0.0])
        }
        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }

    let conn = test_db();
    // Seven mutually similar memories against a cap of five: an uncapped run
    // would give every memory degree six, so the cap must actually bind here —
    // unlike a cluster smaller than the cap, where these assertions pass with
    // the cap logic deleted.
    let cluster: Vec<_> = (0..7)
        .map(|i| remember_in(&conn, "project:x", &format!("pooling memory number {i}")))
        .collect();
    for m in &cluster {
        store_embedding(&conn, &m.id, "model-a", 2, &[1.0, 0.0]).unwrap();
    }

    let config = AutoLinkConfig {
        enabled: true,
        threshold: 0.5,
        max_links_per_memory: 5,
        ..Default::default()
    };
    auto_link_batch(&conn, &SameVectorBackend, None, None, &config).unwrap();

    let degrees: Vec<i64> = cluster
        .iter()
        .map(|m| auto_link_degree(&conn, &m.id))
        .collect();
    assert!(
        degrees.iter().all(|&d| d <= 5),
        "no memory may exceed the total-degree cap: {degrees:?}"
    );
    assert_eq!(
        degrees.iter().max(),
        Some(&5),
        "the cap must bind in this fixture, or the assertions prove nothing: {degrees:?}"
    );

    // A second run must add nothing: every pair is either linked or blocked by
    // the cap, and the cap is a cumulative total, not a per-run allowance.
    let second = auto_link_batch(&conn, &SameVectorBackend, None, None, &config).unwrap();
    assert_eq!(second.links_created, 0, "second run must be idempotent");
}

#[test]
fn auto_link_never_lifts_a_memory_already_over_its_cap() {
    use clio_core::embeddings::{EmbeddingBackend, auto_link_batch, store_embedding};
    use clio_core::models::LinkInput;
    use clio_core::settings::AutoLinkConfig;

    struct SameVectorBackend;
    impl EmbeddingBackend for SameVectorBackend {
        fn model_name(&self) -> &str {
            "model-a"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            Ok(vec![1.0, 0.0])
        }
        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }

    let conn = test_db();
    // A memory already over cap (degree 6 against a cap of 5): legacy state from
    // before the cumulative cap existed. It must neither gain links, panic the
    // budget arithmetic, nor be pushed further over as a target.
    let over = remember_in(&conn, "project:x", "memory already over its cap");
    let newcomer = remember_in(&conn, "project:x", "new memory similar to the over-cap one");
    store_embedding(&conn, &over.id, "model-a", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &newcomer.id, "model-a", 2, &[1.0, 0.0]).unwrap();
    for i in 0..6 {
        // Each dummy lives in its own namespace, so auto-link can never suggest
        // them — to each other or to `over` — and only the pre-made links exist.
        let dummy = remember_in(&conn, &format!("project:dummy-{i}"), &format!("dummy {i}"));
        store_embedding(&conn, &dummy.id, "model-a", 2, &[0.0, 1.0]).unwrap();
        repository::link(
            &conn,
            &LinkInput {
                from_memory_id: over.id.clone(),
                to_memory_id: dummy.id.clone(),
                relationship: "auto:relates_to".into(),
                metadata: serde_json::json!({}),
            },
        )
        .unwrap();
    }
    assert_eq!(auto_link_degree(&conn, &over.id), 6);

    let config = AutoLinkConfig {
        enabled: true,
        threshold: 0.5,
        max_links_per_memory: 5,
        ..Default::default()
    };
    let report = auto_link_batch(&conn, &SameVectorBackend, None, None, &config).unwrap();

    assert_eq!(
        report.links_created, 0,
        "the newcomer's only similar candidate is over cap and must be skipped as a target"
    );
    assert_eq!(
        auto_link_degree(&conn, &over.id),
        6,
        "an over-cap memory must not move: no new links out of it or into it"
    );
}

#[test]
fn auto_link_reports_unembeddable_memories_as_skipped_not_processed() {
    use clio_core::embeddings::{EmbeddingBackend, auto_link_batch};
    use clio_core::error::ClioError;
    use clio_core::settings::AutoLinkConfig;

    struct FailingBackend;
    impl EmbeddingBackend for FailingBackend {
        fn model_name(&self) -> &str {
            "model-a"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            Err(ClioError::Storage("backend down".into()))
        }
        fn embed_batch(&self, _texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Err(ClioError::Storage("backend down".into()))
        }
    }

    let conn = test_db();
    remember_in(&conn, "project:x", "first memory with no embedding");
    remember_in(&conn, "project:x", "second memory with no embedding");

    let config = AutoLinkConfig {
        enabled: true,
        threshold: 0.5,
        ..Default::default()
    };
    let report = auto_link_batch(&conn, &FailingBackend, None, None, &config).unwrap();

    // The driver distinguishes "corpus finished" (processed == 0 AND skipped == 0)
    // from "backend broken" (skipped > 0), and must be able to keep walking past a
    // broken batch — so the watermark still advances.
    assert_eq!(report.memories_processed, 0);
    assert_eq!(report.memories_skipped, 2);
    assert_eq!(report.links_created, 0);
    assert!(
        report.last_watermark.is_some(),
        "watermark advances past unembeddable memories"
    );
}

#[test]
fn auto_link_watermark_does_not_skip_timestamp_ties_at_a_batch_boundary() {
    use clio_core::embeddings::{EmbeddingBackend, auto_link_batch};
    use clio_core::settings::AutoLinkConfig;

    struct SameVectorBackend;
    impl EmbeddingBackend for SameVectorBackend {
        fn model_name(&self) -> &str {
            "model-a"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            Ok(vec![1.0, 0.0])
        }
        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }

    let conn = test_db();
    for i in 0..3 {
        remember_in(
            &conn,
            "project:x",
            &format!("memory sharing one update timestamp {i}"),
        );
    }
    conn.execute(
        "UPDATE memories SET updated_at = '2026-07-30T12:00:00Z'",
        [],
    )
    .unwrap();

    let config = AutoLinkConfig {
        enabled: true,
        threshold: 0.5,
        batch_size: 2,
        ..Default::default()
    };
    let first = auto_link_batch(&conn, &SameVectorBackend, None, None, &config).unwrap();
    assert_eq!(first.memories_processed, 2);

    let second = auto_link_batch(
        &conn,
        &SameVectorBackend,
        first.last_watermark.as_deref(),
        first.last_watermark_id.as_deref(),
        &config,
    )
    .unwrap();
    assert_eq!(
        second.memories_processed, 1,
        "the row after the timestamp tie must remain reachable"
    );
}

#[test]
fn auto_link_propagates_degree_query_failures() {
    use clio_core::embeddings::{EmbeddingBackend, auto_link_batch};
    use clio_core::settings::AutoLinkConfig;

    struct SameVectorBackend;
    impl EmbeddingBackend for SameVectorBackend {
        fn model_name(&self) -> &str {
            "model-a"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed_one(&self, _text: &str) -> clio_core::error::Result<Vec<f32>> {
            Ok(vec![1.0, 0.0])
        }
        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }

    let conn = test_db();
    remember_in(&conn, "project:x", "memory whose degree cannot be read");
    conn.execute("DROP TABLE memory_links", []).unwrap();

    let result = auto_link_batch(
        &conn,
        &SameVectorBackend,
        None,
        None,
        &AutoLinkConfig {
            enabled: true,
            threshold: 0.5,
            ..Default::default()
        },
    );

    assert!(
        result.is_err(),
        "a failed degree query must not be treated as zero links"
    );
}

// ---------------------------------------------------------------------------
// Occurrences through the checkpoint path (Task 9)
// ---------------------------------------------------------------------------

#[test]
fn repeated_evidence_across_sessions_keeps_one_memory_with_occurrences() {
    use clio_core::checkpoint::{CheckpointRequest, store_checkpoint};
    use clio_core::occurrences;

    let conn = test_db();
    let settings = Settings::default();
    let atom = || clio_core::capture::DistilledMemory {
        content: "The Atlas key rotates monthly.".into(),
        kind: "fact".into(),
        title: "Atlas key rotation".into(),
        summary: String::new(),
        tags: vec![],
        namespace: "project:occ".into(),
        importance: 3,
        confidence: 1.0,
        attention: None,
        resolves: None,
    };
    let request = |session: &str, cursor: i64| CheckpointRequest {
        source: "claude-session".into(),
        session_id: session.into(),
        cursor,
        recover_stale: false,
        namespace_override: None,
        default_namespace: Some("project:occ".into()),
        cwd: None,
        branch: None,
        ticket: None,
    };

    let first = store_checkpoint(&conn, &request("sess-a", 10), &[atom()], &settings).unwrap();
    let memory_id = first.stored_memory_ids[0].clone();

    // Same evidenced fact from a different session: no second memory row.
    let second = store_checkpoint(&conn, &request("sess-b", 5), &[atom()], &settings).unwrap();
    assert_eq!(second.stored_memory_ids, vec![memory_id.clone()]);
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE namespace = 'project:occ'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "one canonical memory");
    assert_eq!(
        occurrences::count_occurrences(&conn, &memory_id).unwrap(),
        2,
        "each session's sighting is provenance"
    );

    // Checkpoint replay of a delivered key adds no occurrence.
    let replay = store_checkpoint(&conn, &request("sess-a", 10), &[atom()], &settings).unwrap();
    assert!(replay.replayed);
    assert_eq!(
        occurrences::count_occurrences(&conn, &memory_id).unwrap(),
        2
    );
}

// ---------------------------------------------------------------------------
// Effectiveness reporting (Task 12)
// ---------------------------------------------------------------------------

#[test]
fn effectiveness_report_is_untracked_and_dedupes_surfaced_events() {
    use clio_core::attention::{self, AttentionInput};
    use clio_core::stats;

    let conn = test_db();
    let memory = remember_in(&conn, "project:fx", "An overdue follow-up");
    attention::create_attention(
        &conn,
        &AttentionInput {
            memory_id: memory.id.clone(),
            due_at: Some("2000-01-01T00:00:00Z".into()),
            ..AttentionInput::default()
        },
    )
    .unwrap();

    // The same surfaced key recorded twice counts once.
    let item = attention::resolve_attention(&conn, &memory.id).unwrap();
    attention::record_surfaced(&conn, &item, "session-a", "overdue", None);
    attention::record_surfaced(&conn, &item, "session-a", "overdue", None);
    attention::record_surfaced(&conn, &item, "session-b", "overdue", None);

    let before_access: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(access_count),0) FROM memories",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let before_events: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_events", [], |r| r.get(0))
        .unwrap();

    let disabled = stats::effectiveness(&conn, Some("project:fx"), 0).unwrap();
    assert_eq!(
        disabled.attention_stale, 0,
        "dormancy disabled reports zero stale items"
    );

    let report = stats::effectiveness(&conn, Some("project:fx"), 14).unwrap();
    assert_eq!(
        report.surfaced_unique, 2,
        "idempotent surfacing deduplicated"
    );
    assert_eq!(report.attention.get("open"), Some(&1));
    assert_eq!(report.corrupt_rows, 0);
    assert_eq!(report.deliberate_accesses, before_access);

    // Reporting mutates nothing: no access tracking, no new events.
    let after_access: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(access_count),0) FROM memories",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let after_events: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after_access, before_access);
    assert_eq!(after_events, before_events);
}

#[test]
fn edited_inbox_items_remain_visible_until_reviewed() {
    use clio_core::review;
    let conn = test_db();
    let input = serde_json::from_value(serde_json::json!({"content": "Review evidence"})).unwrap();
    let item = review::queue_for_review(&conn, &input).unwrap();
    let edits = serde_json::from_value(serde_json::json!({"title": "Revised title"})).unwrap();
    review::edit_review(&conn, &item.id, &edits).unwrap();
    assert_eq!(review::list_pending(&conn, 10).unwrap().len(), 1);
    assert_eq!(
        clio_core::attention::overview(&conn, None, None, 14, 14)
            .unwrap()
            .review_pending,
        1
    );
    review::approve_review(&conn, &item.id, &Settings::default()).unwrap();
    assert!(review::list_pending(&conn, 10).unwrap().is_empty());
}

#[test]
fn edit_review_rejects_out_of_range_importance() {
    use clio_core::review;
    let conn = test_db();
    let input = serde_json::from_value(serde_json::json!({"content": "Review evidence"})).unwrap();
    let item = review::queue_for_review(&conn, &input).unwrap();
    for importance in [0, 6] {
        let edits = serde_json::from_value(serde_json::json!({"importance": importance})).unwrap();
        let err = review::edit_review(&conn, &item.id, &edits).unwrap_err();
        assert!(
            matches!(err, clio_core::error::ClioError::Validation(_)),
            "{err}"
        );
    }
    assert_eq!(
        review::get_review(&conn, &item.id).unwrap().status,
        "pending"
    );
}
