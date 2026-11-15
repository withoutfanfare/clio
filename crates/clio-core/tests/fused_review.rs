//! Regressions from the review of fused retrieval's fixes.

use clio_core::assembly::{build_resume_brief, ResumeRequest};
use clio_core::embeddings::store_embedding;
use clio_core::fusion::QueryEmbedding;
use clio_core::models::RememberInput;
use clio_core::settings::{ScoringConfig, Settings};
use clio_core::{db, repository};

const NS: &str = "project:x";

fn put(conn: &rusqlite::Connection, title: &str, content: &str, kind: &str, importance: u32) -> String {
    let input: RememberInput = serde_json::from_value(serde_json::json!({
        "namespace": NS, "content": content, "title": title, "kind": kind, "importance": importance
    }))
    .unwrap();
    repository::remember(conn, &input, &Settings::default()).unwrap().id
}

fn embedding() -> QueryEmbedding {
    QueryEmbedding {
        vector: vec![1.0, 0.0],
        model: "test".into(),
        deadline: None,
    }
}

fn brief_ids(conn: &rusqlite::Connection, request: &ResumeRequest) -> Vec<String> {
    build_resume_brief(conn, request)
        .unwrap()
        .sections
        .iter()
        .flat_map(|s| s.items.iter().map(|i| i.memory_id.clone()))
        .collect()
}

#[test]
fn matching_constraint_takes_the_reserved_slot() {
    let conn = db::open_in_memory().unwrap();
    put(&conn, "Unrelated", "Always keep backups", "constraint", 5);
    let matching = put(&conn, "Matching", "zebra safeguard", "constraint", 1);
    let request = ResumeRequest {
        namespace: Some(NS.into()),
        query: Some("zebra".into()),
        scoring: Some(ScoringConfig::default()),
        max_items: 2,
        ..Default::default()
    };
    assert!(brief_ids(&conn, &request).contains(&matching));
}

#[test]
fn semantic_only_matches_refill_past_the_first_window() {
    let conn = db::open_in_memory().unwrap();
    for n in 0..30 {
        let id = put(&conn, &format!("Copy {n}"), "duplicate payload", "fact", 3);
        store_embedding(&conn, &id, "test", 2, &[1.0, 0.0]).unwrap();
    }
    let distinct = put(&conn, "Distinct", "different payload", "fact", 3);
    store_embedding(&conn, &distinct, "test", 2, &[0.8, 0.6]).unwrap();
    let request = ResumeRequest {
        namespace: Some(NS.into()),
        query: Some("zebra".into()),
        query_embedding: Some(embedding()),
        scoring: Some(ScoringConfig::default()),
        max_items: 20,
        ..Default::default()
    };
    assert!(brief_ids(&conn, &request).contains(&distinct));
}
