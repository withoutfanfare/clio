use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clio_core::cache::ClioCache;
use clio_core::embeddings::{EmbeddingBackend, semantic_search, store_embedding, suggest_links};
use clio_core::error::ClioError;
use clio_core::models::{Memory, RecallQuery, RememberInput, UpdateInput};
use clio_core::review::{ReviewInput, approve_review, get_review, queue_for_review};
use clio_core::{db, repository};
use rusqlite::{Connection, TransactionBehavior};
use tempfile::TempDir;

use clio_core::settings::Settings;

static UPSERT_BLOCKED: AtomicBool = AtomicBool::new(false);
static FIRST_APPROVAL_BLOCKED: AtomicBool = AtomicBool::new(false);
static SECOND_APPROVAL_BLOCKED: AtomicBool = AtomicBool::new(false);

fn upsert_busy_handler(_attempt: i32) -> bool {
    UPSERT_BLOCKED.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(1));
    true
}

fn first_approval_busy_handler(_attempt: i32) -> bool {
    FIRST_APPROVAL_BLOCKED.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(1));
    true
}

fn second_approval_busy_handler(_attempt: i32) -> bool {
    SECOND_APPROVAL_BLOCKED.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(1));
    true
}

fn wait_until_blocked<T: std::fmt::Debug>(
    flag: &AtomicBool,
    completed: &std::sync::mpsc::Receiver<T>,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !flag.load(Ordering::Acquire) {
        if let Ok(result) = completed.try_recv() {
            panic!("worker completed before reaching the contested SQLite write: {result:?}");
        }
        assert!(
            Instant::now() < deadline,
            "worker did not reach the contested SQLite write"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn file_db() -> (TempDir, Connection, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("clio.db");
    let first = db::open(&path).unwrap();
    let second = db::open(&path).unwrap();
    (directory, first, second)
}

fn upsert_input(content: &str) -> RememberInput {
    RememberInput {
        namespace: "project:multi-machine".into(),
        kind: "fact".into(),
        title: Some("Shared fact".into()),
        summary: None,
        content: content.into(),
        tags: vec!["shared".into()],
        source: Some("integration-test".into()),
        source_ref: Some("same-event".into()),
        confidence: Some(0.9),
        importance: 4,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: true,
    }
}

fn review_input() -> ReviewInput {
    ReviewInput {
        content: "A fact awaiting one review decision".into(),
        suggested_namespace: "project:multi-machine".into(),
        suggested_kind: "fact".into(),
        suggested_title: Some("Reviewed fact".into()),
        suggested_summary: None,
        suggested_tags: vec!["shared".into()],
        suggested_importance: 4,
        suggested_confidence: Some(0.7),
        source_route: Some("integration-test".into()),
        source_ref: None,
        metadata: serde_json::json!({}),
    }
}

fn plain_input(content: &str) -> RememberInput {
    RememberInput {
        source: None,
        source_ref: None,
        upsert: false,
        ..upsert_input(content)
    }
}

fn test_settings() -> Settings {
    Settings {
        auto_embed: false,
        ..Settings::default()
    }
}

struct TestEmbeddingBackend;

impl EmbeddingBackend for TestEmbeddingBackend {
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

fn remember_plain(conn: &Connection, content: &str) -> Memory {
    repository::remember(conn, &plain_input(content), &test_settings()).unwrap()
}

#[test]
fn simultaneous_upserts_with_the_same_source_reference_are_idempotent() {
    UPSERT_BLOCKED.store(false, Ordering::Release);
    let (_directory, mut first, second) = file_db();
    second
        .busy_handler(Some(upsert_busy_handler))
        .expect("failed to install busy handler");

    let transaction = first
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let (completed_tx, completed_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result =
            repository::remember(&second, &upsert_input("Identical event"), &test_settings());
        completed_tx.send(result).unwrap();
    });

    wait_until_blocked(&UPSERT_BLOCKED, &completed_rx);
    let first_result = repository::remember(
        &transaction,
        &upsert_input("Identical event"),
        &test_settings(),
    )
    .unwrap();
    transaction.commit().unwrap();

    let second_result = completed_rx
        .recv()
        .expect("upsert worker stopped without a result")
        .expect("the competing upsert should resolve to the existing memory");
    worker.join().expect("upsert worker panicked");

    assert_eq!(second_result.id, first_result.id);

    let count: u32 = first
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE source = ?1 AND source_ref = ?2",
            ["integration-test", "same-event"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn a_cache_observes_writes_from_another_connection_immediately() {
    let (_directory, first, second) = file_db();
    let first_cache = ClioCache::with_defaults();
    let second_cache = ClioCache::with_defaults();
    let original = upsert_input("Original content");

    let memory = first_cache
        .remember(&first, &original, &test_settings())
        .unwrap();
    assert_eq!(
        second_cache.get(&second, &memory.id).unwrap().content,
        "Original content"
    );

    let updated = UpdateInput {
        content: Some("Updated elsewhere".into()),
        ..UpdateInput::default()
    };
    repository::update(&first, &memory.id, &updated, &test_settings()).unwrap();
    assert_eq!(
        second_cache.get(&second, &memory.id).unwrap().content,
        "Updated elsewhere"
    );

    repository::archive(&first, &memory.id).unwrap();
    assert!(
        second_cache
            .get(&second, &memory.id)
            .unwrap()
            .archived_at
            .is_some()
    );

    repository::delete(&first, &memory.id).unwrap();
    assert!(matches!(
        second_cache.get(&second, &memory.id),
        Err(ClioError::NotFound(_))
    ));
}

#[test]
fn cached_recall_excludes_a_memory_archived_by_another_connection() {
    let (_directory, first, second) = file_db();
    let memory = remember_plain(&first, "crossprocessneedle");
    let second_cache = ClioCache::with_defaults();
    let query = RecallQuery {
        query: Some("crossprocessneedle".into()),
        namespace: Some("project:multi-machine".into()),
        ..RecallQuery::default()
    };

    let primed = second_cache.recall(&second, &query).unwrap();
    assert_eq!(primed.items.len(), 1);
    assert_eq!(primed.items[0].memory.id, memory.id);

    repository::archive(&first, &memory.id).unwrap();

    let refreshed = second_cache.recall(&second, &query).unwrap();
    assert!(refreshed.items.is_empty());
    assert_eq!(refreshed.total, 0);
}

#[test]
fn a_stale_patch_cannot_erase_another_connections_update() {
    let (_directory, first, second) = file_db();
    let memory = remember_plain(&first, "Original content");
    let first_snapshot = repository::get(&first, &memory.id).unwrap();
    let second_snapshot = repository::get(&second, &memory.id).unwrap();
    assert_eq!(first_snapshot.updated_at, second_snapshot.updated_at);

    let first_update = repository::update(
        &first,
        &memory.id,
        &UpdateInput {
            content: Some("First writer wins".into()),
            expected_updated_at: Some(first_snapshot.updated_at),
            ..UpdateInput::default()
        },
        &test_settings(),
    )
    .unwrap();
    assert_eq!(first_update.content, "First writer wins");

    let stale_result = repository::update(
        &second,
        &memory.id,
        &UpdateInput {
            title: Some(Some("Stale title".into())),
            expected_updated_at: Some(second_snapshot.updated_at),
            ..UpdateInput::default()
        },
        &test_settings(),
    );
    assert!(matches!(stale_result, Err(ClioError::Conflict(_))));

    let stored = repository::get(&first, &memory.id).unwrap();
    assert_eq!(stored.content, "First writer wins");
    assert_eq!(stored.title.as_deref(), Some("Shared fact"));
}

#[test]
fn bulk_add_tag_rolls_back_when_any_memory_is_missing() {
    let conn = db::open_in_memory().unwrap();
    let memory = remember_plain(&conn, "bulk tag rollback");
    let ids = vec![memory.id.clone(), "missing-memory".into()];

    let result = repository::add_tag_bulk(&conn, &ids, "bulk-only");

    assert!(result.is_err());
    assert!(
        conn.is_autocommit(),
        "failed bulk write left a savepoint open"
    );
    let stored = repository::get(&conn, &memory.id).unwrap();
    assert_eq!(stored.tags, vec!["shared"]);
    let tag_count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_tags WHERE memory_id = ?1 AND tag = ?2",
            [&memory.id, "bulk-only"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tag_count, 0);
}

#[test]
fn only_one_connection_can_approve_a_review_item() {
    FIRST_APPROVAL_BLOCKED.store(false, Ordering::Release);
    SECOND_APPROVAL_BLOCKED.store(false, Ordering::Release);

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("clio.db");
    let mut locker = db::open(&path).unwrap();
    let first = db::open(&path).unwrap();
    let second = db::open(&path).unwrap();
    first
        .busy_handler(Some(first_approval_busy_handler))
        .expect("failed to install first busy handler");
    second
        .busy_handler(Some(second_approval_busy_handler))
        .expect("failed to install second busy handler");

    let review = queue_for_review(&locker, &review_input()).unwrap();
    let review_id = review.id.clone();
    let lock = locker
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();

    let first_id = review_id.clone();
    let (first_tx, first_rx) = std::sync::mpsc::sync_channel(1);
    let first_worker = std::thread::spawn(move || {
        first_tx
            .send(approve_review(&first, &first_id, &test_settings()))
            .unwrap();
    });
    let second_id = review_id.clone();
    let (second_tx, second_rx) = std::sync::mpsc::sync_channel(1);
    let second_worker = std::thread::spawn(move || {
        second_tx
            .send(approve_review(&second, &second_id, &test_settings()))
            .unwrap();
    });

    wait_until_blocked(&FIRST_APPROVAL_BLOCKED, &first_rx);
    wait_until_blocked(&SECOND_APPROVAL_BLOCKED, &second_rx);
    lock.commit().unwrap();

    let outcomes = [
        first_rx.recv().expect("first approval worker lost"),
        second_rx.recv().expect("second approval worker lost"),
    ];
    first_worker.join().expect("first approval worker panicked");
    second_worker
        .join()
        .expect("second approval worker panicked");
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ClioError::Validation(_))))
            .count(),
        1
    );

    let review = get_review(&locker, &review_id).unwrap();
    assert_eq!(review.status, "approved");
    let memory_count: u32 = locker
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE namespace = ?1",
            ["project:multi-machine"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(memory_count, 1);
}

#[test]
fn link_suggestions_do_not_compare_different_embedding_spaces() {
    let conn = db::open_in_memory().unwrap();
    let target = remember_plain(&conn, "target");
    let compatible = remember_plain(&conn, "compatible");
    let wrong_model = remember_plain(&conn, "wrong model");
    let wrong_dimensions = remember_plain(&conn, "wrong dimensions");

    store_embedding(&conn, &target.id, "model-a", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &compatible.id, "model-a", 2, &[0.99, 0.01]).unwrap();
    store_embedding(&conn, &wrong_model.id, "model-b", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &wrong_dimensions.id, "model-a", 3, &[1.0, 0.0, 0.0]).unwrap();

    let suggestions = suggest_links(&conn, &target.id, &TestEmbeddingBackend, 0.9, 10).unwrap();
    let suggested_ids: Vec<&str> = suggestions
        .iter()
        .map(|(memory, _)| memory.id.as_str())
        .collect();

    assert_eq!(suggested_ids, vec![compatible.id.as_str()]);
}

#[test]
fn semantic_search_only_compares_the_active_embedding_space() {
    let conn = db::open_in_memory().unwrap();
    let compatible = remember_plain(&conn, "compatible");
    let wrong_model = remember_plain(&conn, "wrong model");
    let wrong_dimensions = remember_plain(&conn, "wrong dimensions");

    store_embedding(&conn, &compatible.id, "model-a", 2, &[0.99, 0.01]).unwrap();
    store_embedding(&conn, &wrong_model.id, "model-b", 2, &[1.0, 0.0]).unwrap();
    store_embedding(&conn, &wrong_dimensions.id, "model-a", 3, &[1.0, 0.0, 0.0]).unwrap();

    let results = semantic_search(&conn, &[1.0, 0.0], "model-a", None, false, false, 10).unwrap();
    let result_ids: Vec<&str> = results
        .iter()
        .map(|result| result.memory_id.as_str())
        .collect();

    assert_eq!(result_ids, vec![compatible.id.as_str()]);
}
