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
    let input = RememberInput { namespace: namespace.into(), ..base_input(content) };
    repository::remember(conn, &input, &Settings::default()).unwrap()
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
    assert_eq!(result.items[0].memory.title, Some("Architecture decision".into()));
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
    assert_eq!(parsed["tags"], serde_json::json!(["architecture", "sqlite"]));
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
    for (ns, kind) in &[("global", "note"), ("global", "decision"), ("project:ai", "note")] {
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
    assert!(result.items.len() >= 2, "expected at least 2, got {}", result.items.len());

    // The linked memory should have linked_from set.
    let linked_item = result.items.iter().find(|i| i.memory.id == mem_b.id);
    assert!(linked_item.is_some(), "linked memory B should be in results");
    assert_eq!(linked_item.unwrap().linked_from.as_deref(), Some(mem_a.id.as_str()));

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

#[test]
fn bulk_link_expansion_returns_linked_memories() {
    let conn = test_db();
    let settings = Settings::default();

    // Create three memories.
    let a = repository::remember(&conn, &RememberInput {
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
    }, &settings).unwrap();

    let b = repository::remember(&conn, &RememberInput {
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
    }, &settings).unwrap();

    let c = repository::remember(&conn, &RememberInput {
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
    }, &settings).unwrap();

    // Link A -> B and A -> C.
    repository::link(&conn, &LinkInput {
        from_memory_id: a.id.clone(),
        to_memory_id: b.id.clone(),
        relationship: "relates_to".into(),
        metadata: serde_json::json!({}),
    }).unwrap();
    repository::link(&conn, &LinkInput {
        from_memory_id: a.id.clone(),
        to_memory_id: c.id.clone(),
        relationship: "relates_to".into(),
        metadata: serde_json::json!({}),
    }).unwrap();

    // Recall with include_links — should return A plus linked B and C.
    let result = repository::recall(&conn, &RecallQuery {
        query: Some("apples".into()),
        namespace: None,
        kind: None,
        tags: vec![],
        match_all_tags: true,
        include_archived: false,
        include_links: true,
        importance_min: None,
        importance_max: None,
        sort_by: None,
        offset: 0,
        limit: 50,
        scoring: None,
    }).unwrap();

    let ids: Vec<&str> = result.items.iter().map(|i| i.memory.id.as_str()).collect();
    assert!(ids.contains(&a.id.as_str()), "should contain source memory A");
    assert!(ids.contains(&b.id.as_str()), "should contain linked memory B");
    assert!(ids.contains(&c.id.as_str()), "should contain linked memory C");

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

    clio_core::deduplication::merge_memories(&conn, &keep.id, &[dup.id.clone()]).unwrap();

    // Regression: the normalised memory_tags rows for the kept memory were silently
    // dropped because the re-insert omitted the NOT NULL created_at column.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_tags WHERE memory_id = ?1",
            [&keep.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3, "kept memory should hold the union of tags (alpha, beta, gamma)");
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
    assert_eq!(res.count, 1, "multi-term query should match the doc containing both terms");
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
    let bconn = rusqlite::Connection::open(backup_path).unwrap();
    let n: i64 = bconn
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "the snapshot must contain the row, even if it was still in the WAL");
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
    repository::remember(&conn, &base_input("added after backup"), &Settings::default()).unwrap();
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
    assert_eq!(n, 1, "restored DB should match the backup, not the post-backup state");
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
    let input = RememberInput { namespace, ..base_input("multibyte namespace content") };

    let result = repository::remember(&conn, &input, &Settings::default());
    assert!(result.is_ok(), "a 120-character namespace must pass character-based validation");
}

// ---------------------------------------------------------------------------
// Deduplication — access_count invariant
// ---------------------------------------------------------------------------

#[test]
fn merge_does_not_inflate_access_count() {
    let conn = test_db();
    let keep = remember_simple(&conn, "keep this memory");
    let dup = remember_simple(&conn, "duplicate memory");

    clio_core::deduplication::merge_memories(&conn, &keep.id, &[dup.id.clone()]).unwrap();

    let access_count: i64 = conn
        .query_row(
            "SELECT access_count FROM memories WHERE id = ?1",
            [&keep.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(access_count, 0, "a merge is maintenance and must not bump access_count");
}
