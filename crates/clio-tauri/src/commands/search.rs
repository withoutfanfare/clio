use tauri::{AppHandle, Manager, State};

use clio_core::models::{Memory, RecallResult};

use crate::{AppState, BackendState, CommandError};

/// Local search, as `memory_search` and `clio search` answer it: core fused
/// retrieval under the same `scoring` settings, with the keyword result alone
/// while the backend is absent or still loading. `namespace` is exact when
/// given and every namespace otherwise, the app's existing convention. With
/// `scoring.fusion` off it is the earlier vector-only search, which needs a
/// ready backend.
fn local_search(
    conn: &rusqlite::Connection,
    scoring: &clio_core::settings::ScoringConfig,
    backend: &BackendState,
    query: &str,
    namespace: Option<&str>,
    include_archived: bool,
    limit: u32,
) -> Result<RecallResult, CommandError> {
    let items = if scoring.fusion {
        let embedding =
            clio_core::fusion::query_embedding_inline(scoring, Some(query), |q| match backend {
                BackendState::Ready(b) => Ok((b.embed_one(q)?, b.model_name().to_string())),
                BackendState::Loading => Err(clio_core::error::ClioError::Config(
                    "embedding backend is still loading".into(),
                )),
                BackendState::Unavailable(reason) => {
                    Err(clio_core::error::ClioError::Config(reason.clone()))
                }
            });
        clio_core::fusion::search(
            conn,
            &clio_core::fusion::FusedQuery {
                query: query.to_string(),
                namespace: namespace.map(String::from),
                include_archived,
                exclude_expired: true,
                limit,
                scoring: Some(scoring.clone()),
                ..Default::default()
            },
            None,
            embedding.as_ref(),
        )?
        .0
    } else {
        let backend = match backend {
            BackendState::Ready(b) => b,
            BackendState::Loading => {
                return Err(CommandError::Config(
                    "Embedding backend is still loading. Please try again shortly.".into(),
                ));
            }
            BackendState::Unavailable(reason) => {
                return Err(CommandError::Config(format!(
                    "Embedding backend unavailable: {reason}"
                )));
            }
        };
        let query_embedding = backend.embed_one(query)?;
        clio_core::embeddings::semantic_recall(
            conn,
            query,
            &query_embedding,
            backend.model_name(),
            namespace,
            include_archived,
            true,
            Some(scoring),
            limit,
        )?
    };

    let count = items.len() as u32;
    Ok(RecallResult {
        archived_only: false,
        total: count,
        count,
        offset: 0,
        limit,
        items,
    })
}

/// The O(N) embedding scan runs on a blocking thread pool so it never freezes
/// the Tauri main thread. `AppHandle` is `Send + 'static`, so we resolve the
/// managed state inside the closure rather than borrowing it across the await.
#[tauri::command]
pub async fn cmd_search(
    app: AppHandle,
    query: String,
    namespace: Option<String>,
    include_archived: Option<bool>,
    limit: Option<u32>,
) -> Result<RecallResult, CommandError> {
    {
        let state = app.state::<AppState>();
        if let Some(remote) = state.remote() {
            let global = namespace.is_none();
            return remote
                .call_json(
                    "memory_search",
                    serde_json::json!({
                        "query": query,
                        "namespace": namespace,
                        "global": global,
                        "include_archived": include_archived.unwrap_or(false),
                        "limit": limit.unwrap_or(10),
                        "response_format": "json",
                    }),
                )
                .await;
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let app = state.local()?;
        local_search(
            &app.conn,
            &app.settings.scoring,
            &app.backend,
            &query,
            namespace.as_deref(),
            include_archived.unwrap_or(false),
            limit.unwrap_or(10),
        )
    })
    .await
    .map_err(|e| CommandError::Core(format!("Search task failed: {e}")))?
}

#[tauri::command]
pub async fn cmd_suggest_links(
    app: AppHandle,
    memory_id: String,
    threshold: Option<f64>,
    limit: Option<u32>,
) -> Result<Vec<SuggestionResult>, CommandError> {
    {
        let state = app.state::<AppState>();
        if let Some(remote) = state.remote() {
            return remote
                .call_json(
                    "memory_suggest_links",
                    serde_json::json!({
                        "memory_id": memory_id,
                        "threshold": threshold.unwrap_or(0.7),
                        "limit": limit.unwrap_or(5),
                        "response_format": "json",
                    }),
                )
                .await;
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let app = state.local()?;

        let backend = match &app.backend {
            BackendState::Ready(b) => b,
            BackendState::Loading => {
                return Err(CommandError::Config(
                    "Embedding backend is still loading. Please try again shortly.".into(),
                ));
            }
            BackendState::Unavailable(reason) => {
                return Err(CommandError::Config(format!(
                    "Embedding backend unavailable: {reason}"
                )));
            }
        };

        let suggestions = clio_core::embeddings::suggest_links(
            &app.conn,
            &memory_id,
            backend.as_ref(),
            threshold.unwrap_or(0.7),
            limit.unwrap_or(5),
        )?;

        Ok(suggestions
            .into_iter()
            .map(|(memory, similarity)| SuggestionResult { memory, similarity })
            .collect())
    })
    .await
    .map_err(|e| CommandError::Core(format!("Suggest-links task failed: {e}")))?
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SuggestionResult {
    pub memory: Memory,
    pub similarity: f64,
}

#[tauri::command]
pub async fn cmd_backend_status(state: State<'_, AppState>) -> Result<String, CommandError> {
    if let Some(remote) = state.remote() {
        let status = remote.status().await;
        return Ok(if status.connected {
            "ready".into()
        } else {
            format!("unavailable: {}", status.detail.unwrap_or_default())
        });
    }

    let app = state.local()?;
    Ok(match &app.backend {
        BackendState::Ready(_) => "ready".to_string(),
        BackendState::Loading => "loading".to_string(),
        BackendState::Unavailable(reason) => format!("unavailable: {reason}"),
    })
}

#[cfg(test)]
mod fused_search_tests {
    use super::*;

    /// Answers each fixture query with the vector the fixture gives it.
    struct FixtureBackend(Vec<(String, Vec<f32>)>);

    impl clio_core::embeddings::EmbeddingBackend for FixtureBackend {
        fn model_name(&self) -> &str {
            "fixture"
        }

        fn dimensions(&self) -> usize {
            2
        }

        fn embed_one(&self, text: &str) -> clio_core::error::Result<Vec<f32>> {
            self.0
                .iter()
                .find(|(query, _)| query == text)
                .map(|(_, vector)| vector.clone())
                .ok_or_else(|| clio_core::error::ClioError::Config("no fixture vector".into()))
        }

        fn embed_batch(&self, texts: &[String]) -> clio_core::error::Result<Vec<Vec<f32>>> {
            texts.iter().map(|text| self.embed_one(text)).collect()
        }
    }

    fn vector(value: &serde_json::Value) -> Vec<f32> {
        serde_json::from_value(value.clone()).unwrap()
    }

    #[test]
    fn desktop_local_fused_search_matrix() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../scripts/fixtures/fused-search.json"
        ))
        .unwrap();
        let namespace = fixture["namespace"].as_str().unwrap();
        let conn = clio_core::db::open_in_memory().unwrap();
        for memory in fixture["memories"].as_array().unwrap() {
            let input = serde_json::from_value(serde_json::json!({
                "namespace": namespace, "title": memory["title"], "content": memory["content"]
            }))
            .unwrap();
            let stored =
                clio_core::repository::remember(&conn, &input, &Default::default()).unwrap();
            let stored_vector = vector(&memory["vector"]);
            clio_core::embeddings::store_embedding(
                &conn,
                &stored.id,
                fixture["model"].as_str().unwrap(),
                stored_vector.len(),
                &stored_vector,
            )
            .unwrap();
        }
        let scoring = clio_core::settings::ScoringConfig::default();
        for case in fixture["cases"].as_array().unwrap() {
            let query = case["query"].as_str().unwrap();
            // No vector in the fixture means no usable backend: absent, or
            // still loading. Both must answer with keywords, not an error.
            let backends = if case["vector"].is_null() {
                vec![
                    BackendState::Unavailable("embeddings are disabled".into()),
                    BackendState::Loading,
                ]
            } else {
                vec![BackendState::Ready(Box::new(FixtureBackend(vec![(
                    query.to_string(),
                    vector(&case["vector"]),
                )])))]
            };
            for backend in backends {
                let result =
                    local_search(&conn, &scoring, &backend, query, Some(namespace), false, 10)
                        .unwrap();
                let mut titles: Vec<_> = result
                    .items
                    .iter()
                    .map(|i| i.memory.title.as_deref().unwrap())
                    .collect();
                titles.sort();
                assert_eq!(
                    serde_json::json!(titles),
                    case["titles"],
                    "{}",
                    case["name"]
                );
            }
        }
    }
}
