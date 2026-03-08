//! Vector embedding support for semantic search.
//!
//! Provides a pluggable `EmbeddingBackend` trait with two implementations:
//! - **Local** (fastembed / ONNX): fully offline, no API keys, bundled model
//! - **OpenAI**: uses the OpenAI embeddings API for higher quality at the cost
//!   of network access and an API key
//!
//! The active backend is controlled by `EmbeddingConfig`. Embeddings are stored
//! as BLOBs in the `memory_embeddings` table and compared using cosine
//! similarity for semantic recall.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{ClioError, Result};
use crate::models::{Memory, RecallItem};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Which embedding backend to use.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum EmbeddingConfig {
    /// Fully local ONNX-based embeddings (default).
    Local {
        /// Model to use. Default: "all-MiniLM-L6-v2".
        #[serde(default = "default_local_model")]
        model: String,
    },
    /// OpenAI API embeddings.
    #[serde(rename = "openai")]
    OpenAi {
        /// API key. If absent, reads from OPENAI_API_KEY env var.
        #[serde(default)]
        api_key: Option<String>,
        /// Model to use. Default: "text-embedding-3-small".
        #[serde(default = "default_openai_model")]
        model: String,
        /// Optional base URL override (for proxies or compatible APIs).
        #[serde(default)]
        base_url: Option<String>,
    },
    /// Embeddings disabled.
    Disabled,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        EmbeddingConfig::Local {
            model: default_local_model(),
        }
    }
}

fn default_local_model() -> String {
    "all-MiniLM-L6-v2".into()
}

fn default_openai_model() -> String {
    "text-embedding-3-small".into()
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Trait for embedding providers. Implemented by local and OpenAI backends.
pub trait EmbeddingBackend: Send + Sync {
    /// The model identifier stored alongside each embedding.
    fn model_name(&self) -> &str;

    /// The dimensionality of the output vectors.
    fn dimensions(&self) -> usize;

    /// Generate an embedding for a single text passage.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>>;

    /// Generate embeddings for multiple texts.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

// ---------------------------------------------------------------------------
// Local backend (fastembed)
// ---------------------------------------------------------------------------

#[cfg(feature = "local-embeddings")]
pub struct LocalBackend {
    model: std::sync::Mutex<fastembed::TextEmbedding>,
    model_name: String,
    dimensions: usize,
}

#[cfg(feature = "local-embeddings")]
impl LocalBackend {
    pub fn new(model_name: &str) -> Result<Self> {
        let fastembed_model = match model_name {
            "all-MiniLM-L6-v2" => fastembed::EmbeddingModel::AllMiniLML6V2,
            "all-MiniLM-L12-v2" => fastembed::EmbeddingModel::AllMiniLML12V2,
            "bge-small-en-v1.5" => fastembed::EmbeddingModel::BGESmallENV15,
            other => {
                return Err(ClioError::Config(format!(
                    "unsupported local embedding model: {other}. \
                     Supported: all-MiniLM-L6-v2, all-MiniLM-L12-v2, bge-small-en-v1.5"
                )));
            }
        };

        // Resolve cache directory: use the Clio data dir so models are found
        // regardless of the process working directory (important for .app bundles).
        let cache_dir = crate::config::resolve_db_path(None)
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("models")))
            .unwrap_or_else(|| std::path::PathBuf::from(".fastembed_cache"));

        let init_opts = fastembed::InitOptions::new(fastembed_model)
            .with_cache_dir(cache_dir);

        let model = fastembed::TextEmbedding::try_new(init_opts)
            .map_err(|e| ClioError::Config(format!("failed to load embedding model: {e}")))?;

        // All supported models produce 384-dimension vectors.
        let dimensions = 384;

        Ok(Self {
            model: std::sync::Mutex::new(model),
            model_name: model_name.to_string(),
            dimensions,
        })
    }
}

#[cfg(feature = "local-embeddings")]
impl EmbeddingBackend for LocalBackend {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut model = self
            .model
            .lock()
            .map_err(|e| ClioError::Storage(format!("embedding model lock poisoned: {e}")))?;
        let results = model
            .embed(vec![text], None)
            .map_err(|e| ClioError::Storage(format!("embedding generation failed: {e}")))?;

        results
            .into_iter()
            .next()
            .ok_or_else(|| ClioError::Storage("embedding returned empty result".into()))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut model = self
            .model
            .lock()
            .map_err(|e| ClioError::Storage(format!("embedding model lock poisoned: {e}")))?;
        model
            .embed(texts, None)
            .map_err(|e| ClioError::Storage(format!("batch embedding failed: {e}")))
    }
}

// ---------------------------------------------------------------------------
// OpenAI backend
// ---------------------------------------------------------------------------

#[cfg(feature = "openai-embeddings")]
pub struct OpenAiBackend {
    api_key: String,
    model_name: String,
    base_url: String,
    client: reqwest::Client,
    dimensions: usize,
}

#[cfg(feature = "openai-embeddings")]
impl OpenAiBackend {
    pub fn new(api_key: Option<&str>, model: &str, base_url: Option<&str>) -> Result<Self> {
        let api_key = match api_key {
            Some(key) if !key.is_empty() => key.to_string(),
            _ => std::env::var("OPENAI_API_KEY").map_err(|_| {
                ClioError::Config(
                    "OpenAI API key required: set OPENAI_API_KEY or configure api_key in settings"
                        .into(),
                )
            })?,
        };

        let base_url = base_url
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_string();

        // text-embedding-3-small produces 1536 dims by default.
        // text-embedding-3-large produces 3072 dims.
        // text-embedding-ada-002 produces 1536 dims.
        let dimensions = match model {
            "text-embedding-3-small" => 1536,
            "text-embedding-3-large" => 3072,
            "text-embedding-ada-002" => 1536,
            _ => 1536, // Reasonable default
        };

        let client = reqwest::Client::new();

        Ok(Self {
            api_key,
            model_name: model.to_string(),
            base_url,
            client,
            dimensions,
        })
    }

    fn get_or_create_runtime() -> &'static tokio::runtime::Runtime {
        static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create OpenAI embedding runtime")
        })
    }

    fn call_api_sync(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Self::get_or_create_runtime().block_on(self.call_api(input))
    }

    async fn call_api(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.base_url);

        let body = serde_json::json!({
            "model": self.model_name,
            "input": input,
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ClioError::Storage(format!("OpenAI API request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            tracing::debug!("OpenAI embedding API error body: {body}");
            return Err(ClioError::Storage(format!(
                "OpenAI embedding API returned {status}"
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ClioError::Storage(format!("OpenAI API response parse error: {e}")))?;

        let data = json["data"]
            .as_array()
            .ok_or_else(|| ClioError::Storage("OpenAI API: missing 'data' array".into()))?;

        let mut embeddings = Vec::with_capacity(data.len());
        for item in data {
            let embedding = item["embedding"]
                .as_array()
                .ok_or_else(|| ClioError::Storage("OpenAI API: missing 'embedding' array".into()))?
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            embeddings.push(embedding);
        }

        Ok(embeddings)
    }
}

#[cfg(feature = "openai-embeddings")]
impl EmbeddingBackend for OpenAiBackend {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.call_api_sync(vec![text.to_string()])?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| ClioError::Storage("OpenAI returned empty result".into()))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        self.call_api_sync(texts.to_vec())
    }
}

// ---------------------------------------------------------------------------
// Backend factory
// ---------------------------------------------------------------------------

/// Create an embedding backend from configuration.
pub fn create_backend(config: &EmbeddingConfig) -> Result<Box<dyn EmbeddingBackend>> {
    match config {
        EmbeddingConfig::Disabled => Err(ClioError::Config("embeddings are disabled".into())),

        #[cfg(feature = "local-embeddings")]
        EmbeddingConfig::Local { model } => {
            let backend = LocalBackend::new(model)?;
            Ok(Box::new(backend))
        }
        #[cfg(not(feature = "local-embeddings"))]
        EmbeddingConfig::Local { .. } => Err(ClioError::Config(
            "local embeddings not available: compile with the 'local-embeddings' feature".into(),
        )),

        #[cfg(feature = "openai-embeddings")]
        EmbeddingConfig::OpenAi {
            api_key,
            model,
            base_url,
        } => {
            let backend =
                OpenAiBackend::new(api_key.as_deref(), model, base_url.as_deref())?;
            Ok(Box::new(backend))
        }
        #[cfg(not(feature = "openai-embeddings"))]
        EmbeddingConfig::OpenAi { .. } => Err(ClioError::Config(
            "OpenAI embeddings not available: compile with the 'openai-embeddings' feature".into(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Passage construction
// ---------------------------------------------------------------------------

/// Build the text passage to embed from a memory's fields.
pub fn build_passage(memory: &Memory) -> String {
    let mut parts = Vec::new();

    if let Some(ref title) = memory.title {
        parts.push(title.clone());
    }

    if let Some(ref summary) = memory.summary {
        parts.push(summary.clone());
    }

    if !memory.tags.is_empty() {
        parts.push(memory.tags.join(", "));
    }

    parts.push(memory.content.clone());

    parts.join(" — ")
}

// ---------------------------------------------------------------------------
// Storage (BLOB encoding of f32 vectors)
// ---------------------------------------------------------------------------

fn encode_embedding(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>> {
    if blob.len() % 4 != 0 {
        return Err(ClioError::Storage(format!(
            "embedding blob has invalid size {} (must be a multiple of 4)",
            blob.len()
        )));
    }
    Ok(blob
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

/// Store (or replace) the embedding for a memory.
pub fn store_embedding(
    conn: &Connection,
    memory_id: &str,
    model_name: &str,
    dimensions: usize,
    embedding: &[f32],
) -> Result<()> {
    let blob = encode_embedding(embedding);
    let now = crate::models::now_utc();

    conn.execute(
        "INSERT OR REPLACE INTO memory_embeddings (memory_id, model, dimensions, embedding, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![memory_id, model_name, dimensions as i32, blob, now],
    )?;

    Ok(())
}

/// Delete the embedding for a memory.
pub fn delete_embedding(conn: &Connection, memory_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM memory_embeddings WHERE memory_id = ?1",
        params![memory_id],
    )?;
    Ok(())
}

/// Check whether a memory has a stored embedding.
pub fn has_embedding(conn: &Connection, memory_id: &str) -> Result<bool> {
    let exists: Option<i32> = conn
        .query_row(
            "SELECT 1 FROM memory_embeddings WHERE memory_id = ?1",
            params![memory_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

// ---------------------------------------------------------------------------
// Semantic search
// ---------------------------------------------------------------------------

/// Result of a semantic similarity comparison.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticResult {
    pub memory_id: String,
    pub similarity: f64,
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;

    for (ai, bi) in a.iter().zip(b.iter()) {
        let ai = *ai as f64;
        let bi = *bi as f64;
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Search for semantically similar memories using the query embedding.
pub fn semantic_search(
    conn: &Connection,
    query_embedding: &[f32],
    namespace: Option<&str>,
    include_archived: bool,
    limit: u32,
) -> Result<Vec<SemanticResult>> {
    let mut sql = String::from(
        "SELECT e.memory_id, e.embedding
         FROM memory_embeddings e
         JOIN memories m ON m.id = e.memory_id
         WHERE 1=1",
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !include_archived {
        sql.push_str(" AND m.archived_at IS NULL");
    }

    if let Some(ns) = namespace {
        let idx = param_values.len() + 1;
        sql.push_str(&format!(" AND m.namespace = ?{idx}"));
        param_values.push(Box::new(ns.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let memory_id: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        Ok((memory_id, blob))
    })?;

    let mut results: Vec<SemanticResult> = Vec::new();
    for row_result in rows {
        let (memory_id, blob) = row_result?;
        let embedding = decode_embedding(&blob)?;
        let similarity = cosine_similarity(query_embedding, &embedding);
        results.push(SemanticResult {
            memory_id,
            similarity,
        });
    }

    results.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit as usize);

    Ok(results)
}

/// Perform semantic search and return full Memory objects with similarity scores.
///
/// Uses batch fetching to avoid N+1 queries. Applies a hybrid keyword boost:
/// memories that also match the query via FTS get a score uplift so that
/// direct keyword hits are not buried by purely semantic neighbours.
pub fn semantic_recall(
    conn: &Connection,
    query_text: &str,
    query_embedding: &[f32],
    namespace: Option<&str>,
    include_archived: bool,
    limit: u32,
) -> Result<Vec<RecallItem>> {
    // Over-fetch semantically so the keyword boost can re-rank within a wider pool.
    let fetch_limit = limit.saturating_mul(2).max(20);
    let results = semantic_search(conn, query_embedding, namespace, include_archived, fetch_limit)?;

    if results.is_empty() {
        return Ok(Vec::new());
    }

    // Build a similarity map from the search results.
    let mut similarity_map: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let ids: Vec<String> = results
        .iter()
        .map(|r| {
            similarity_map.insert(r.memory_id.clone(), r.similarity);
            r.memory_id.clone()
        })
        .collect();

    // Collect IDs of memories that also match the query via FTS.
    let fts_hits = fts_matching_ids(conn, query_text);

    // Batch-fetch all memories in one query.
    let memories = crate::repository::get_many_pub(conn, &ids)?;

    // Keyword boost: if a memory matches FTS, add a fixed boost to its
    // semantic score. The boost (0.3) is large enough to lift a direct keyword
    // match above unrelated semantic neighbours, but small enough that a
    // strong semantic hit still outranks a weak keyword match.
    const KEYWORD_BOOST: f64 = 0.3;

    let mut items: Vec<RecallItem> = memories
        .into_iter()
        .map(|memory| {
            let semantic = similarity_map.get(&memory.id).copied().unwrap_or(0.0);
            let boost = if fts_hits.contains(&memory.id) { KEYWORD_BOOST } else { 0.0 };
            RecallItem {
                memory,
                rank: Some(semantic + boost),
                linked_from: None,
            }
        })
        .collect();

    // Re-sort by hybrid score descending.
    items.sort_by(|a, b| {
        b.rank
            .partial_cmp(&a.rank)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items.truncate(limit as usize);

    // Fire-and-forget access tracking.
    let id_refs: Vec<&str> = items.iter().map(|i| i.memory.id.as_str()).collect();
    if let Err(e) = crate::repository::touch_accessed(conn, &id_refs) {
        tracing::warn!("access tracking failed in semantic_recall: {e}");
    }

    Ok(items)
}

/// Return the set of memory IDs that match a query via FTS.
/// Used for hybrid keyword boosting in semantic search.
/// Silently returns an empty set on any FTS error (e.g. empty query).
fn fts_matching_ids(conn: &Connection, query: &str) -> std::collections::HashSet<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return std::collections::HashSet::new();
    }
    // Wrap in quotes for literal matching, escaping any internal quotes.
    let safe = format!("\"{}\"", trimmed.replace('"', " "));
    let result: std::result::Result<Vec<String>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT m.id FROM memory_fts
             JOIN memories m ON m.rowid = memory_fts.rowid
             WHERE memory_fts MATCH ?1"
        )?;
        let rows = stmt.query_map([&safe], |row| row.get::<_, String>(0))?;
        rows.collect()
    })();
    result.unwrap_or_default().into_iter().collect()
}

// ---------------------------------------------------------------------------
// Similarity-based link suggestions
// ---------------------------------------------------------------------------

/// Suggest potential links for a memory based on embedding similarity.
///
/// Finds memories with cosine similarity above `threshold`, excluding
/// memories that are already linked to the given memory (in either
/// direction). Results are sorted by similarity descending.
pub fn suggest_links(
    conn: &Connection,
    memory_id: &str,
    backend: &dyn EmbeddingBackend,
    threshold: f64,
    limit: u32,
) -> Result<Vec<(Memory, f64)>> {
    // Get the embedding for the target memory (generate if needed).
    let target_embedding = match get_stored_embedding(conn, memory_id)? {
        Some(emb) => emb,
        None => {
            // Generate it on-the-fly.
            let memory = crate::repository::get(conn, memory_id)?;
            let passage = build_passage(&memory);
            let emb = backend.embed_one(&passage)?;
            store_embedding(conn, memory_id, backend.model_name(), backend.dimensions(), &emb)?;
            emb
        }
    };

    // Find similar memories, excluding the source memory and any already linked
    // (in either direction). Filtering at the SQL level avoids pulling linked
    // rows into memory and decoding their embeddings.
    let mut stmt = conn.prepare(
        "SELECT e.memory_id, e.embedding
         FROM memory_embeddings e
         JOIN memories m ON m.id = e.memory_id
         WHERE m.archived_at IS NULL
           AND e.memory_id != ?1
           AND e.memory_id NOT IN (
               SELECT to_memory_id FROM memory_links WHERE from_memory_id = ?1
               UNION
               SELECT from_memory_id FROM memory_links WHERE to_memory_id = ?1
           )",
    )?;
    let rows = stmt.query_map(params![memory_id], |row| {
        let mid: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        Ok((mid, blob))
    })?;

    let mut candidates: Vec<(String, f64)> = Vec::new();
    for row_result in rows {
        let (mid, blob) = row_result?;
        let embedding = decode_embedding(&blob)?;
        let similarity = cosine_similarity(&target_embedding, &embedding);
        if similarity >= threshold {
            candidates.push((mid, similarity));
        }
    }

    candidates.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(limit as usize);

    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    // Batch-fetch all candidate memories in one query.
    let sim_map: std::collections::HashMap<String, f64> = candidates
        .iter()
        .cloned()
        .collect();
    let ids: Vec<String> = candidates.into_iter().map(|(id, _)| id).collect();
    let memories = crate::repository::get_many_pub(conn, &ids)?;

    let mut results: Vec<(Memory, f64)> = memories
        .into_iter()
        .filter_map(|m| sim_map.get(&m.id).map(|&sim| (m, sim)))
        .collect();

    // Maintain similarity ordering.
    results.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results)
}

// ---------------------------------------------------------------------------
// Auto-link inference
// ---------------------------------------------------------------------------

/// Report from an auto-link inference pass.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoLinkReport {
    pub memories_processed: u32,
    pub links_created: u32,
    pub last_watermark: Option<String>,
}

/// Run a batch of automatic link inference.
///
/// Processes memories updated since `since` (or all if None), generates
/// embeddings if missing, finds similar memories above the threshold, and
/// creates links with the "auto:relates_to" relationship.
///
/// Unembedded memories are batch-fetched and batch-embedded in a single pass
/// to avoid N sequential round-trips to the embedding backend.
pub fn auto_link_batch(
    conn: &Connection,
    backend: &dyn EmbeddingBackend,
    since: Option<&str>,
    config: &crate::settings::AutoLinkConfig,
) -> Result<AutoLinkReport> {
    // Query memories updated since the watermark.
    let mut sql = String::from(
        "SELECT id, updated_at FROM memories WHERE archived_at IS NULL",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(watermark) = since {
        let idx = param_values.len() + 1;
        sql.push_str(&format!(" AND updated_at > ?{idx}"));
        param_values.push(Box::new(watermark.to_string()));
    }

    sql.push_str(" ORDER BY updated_at ASC");
    let idx = param_values.len() + 1;
    sql.push_str(&format!(" LIMIT ?{idx}"));
    param_values.push(Box::new(config.batch_size));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let id: String = row.get(0)?;
        let updated_at: String = row.get(1)?;
        Ok((id, updated_at))
    })?;

    let candidates: Vec<(String, String)> = rows
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // --- Batch embed unembedded candidates ---
    // Collect IDs that lack embeddings.
    let unembedded_ids: Vec<String> = candidates
        .iter()
        .filter_map(|(id, _)| match has_embedding(conn, id) {
            Ok(false) => Some(id.clone()),
            _ => None,
        })
        .collect();

    if !unembedded_ids.is_empty() {
        // Batch-fetch all unembedded memories in one query.
        let memories = crate::repository::get_many_pub(conn, &unembedded_ids)?;

        // Build passages in the same order as the fetched memories.
        let passages: Vec<String> = memories.iter().map(build_passage).collect();

        // Batch-embed all passages at once.
        match backend.embed_batch(&passages) {
            Ok(embeddings) => {
                // Store each embedding. If one store fails, log and continue.
                for (memory, embedding) in memories.iter().zip(embeddings.iter()) {
                    if let Err(e) = store_embedding(
                        conn,
                        &memory.id,
                        backend.model_name(),
                        backend.dimensions(),
                        embedding,
                    ) {
                        tracing::warn!(
                            memory_id = memory.id,
                            "auto-link: failed to store embedding: {e}"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!("auto-link: batch embedding failed, falling back to per-memory: {e}");
                // Fall back to per-memory embedding so we still make progress.
                for memory in &memories {
                    if let Err(e) = embed_and_store(conn, backend, memory) {
                        tracing::warn!(
                            memory_id = memory.id,
                            "auto-link: fallback embed failed: {e}"
                        );
                    }
                }
            }
        }
    }

    // --- Link inference ---
    let mut memories_processed: u32 = 0;
    let mut links_created: u32 = 0;
    let mut last_watermark: Option<String> = None;

    for (memory_id, updated_at) in &candidates {
        // Skip if embedding is still missing (e.g. both batch and fallback failed).
        if !has_embedding(conn, memory_id).unwrap_or(false) {
            tracing::warn!(memory_id, "auto-link: skipping — no embedding available");
            last_watermark = Some(updated_at.clone());
            continue;
        }

        // Find similar memories above threshold.
        match suggest_links(conn, memory_id, backend, config.threshold, config.max_links_per_memory) {
            Ok(suggestions) => {
                for (target_memory, _similarity) in suggestions {
                    let link_input = crate::models::LinkInput {
                        from_memory_id: memory_id.clone(),
                        to_memory_id: target_memory.id.clone(),
                        relationship: "auto:relates_to".to_string(),
                        metadata: serde_json::json!({}),
                    };
                    match crate::repository::link(conn, &link_input) {
                        Ok(_) => links_created += 1,
                        Err(e) => {
                            tracing::debug!(
                                from = memory_id,
                                to = target_memory.id,
                                "auto-link: skipped ({})", e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(memory_id, "auto-link: suggest_links failed: {e}");
            }
        }

        memories_processed += 1;
        last_watermark = Some(updated_at.clone());
    }

    tracing::info!(
        memories_processed,
        links_created,
        "auto-link batch complete"
    );

    Ok(AutoLinkReport {
        memories_processed,
        links_created,
        last_watermark,
    })
}

/// Retrieve the stored embedding for a memory, if it exists.
pub(crate) fn get_stored_embedding(conn: &Connection, memory_id: &str) -> Result<Option<Vec<f32>>> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT embedding FROM memory_embeddings WHERE memory_id = ?1",
            params![memory_id],
            |row| row.get(0),
        )
        .optional()?;

    match blob {
        Some(b) => Ok(Some(decode_embedding(&b)?)),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Convenience: embed and store for a memory
// ---------------------------------------------------------------------------

/// Generate and store an embedding for a memory using the given backend.
pub fn embed_and_store(
    conn: &Connection,
    backend: &dyn EmbeddingBackend,
    memory: &Memory,
) -> Result<()> {
    let passage = build_passage(memory);
    let embedding = backend.embed_one(&passage)?;
    store_embedding(
        conn,
        &memory.id,
        backend.model_name(),
        backend.dimensions(),
        &embedding,
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Bulk embedding management
// ---------------------------------------------------------------------------

/// Count how many memories are missing embeddings.
pub fn count_unembedded(conn: &Connection) -> Result<u32> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM memories m
         WHERE NOT EXISTS (SELECT 1 FROM memory_embeddings e WHERE e.memory_id = m.id)",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Return IDs of memories that don't have embeddings yet.
pub fn list_unembedded(conn: &Connection, limit: u32) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT m.id FROM memories m
         WHERE NOT EXISTS (SELECT 1 FROM memory_embeddings e WHERE e.memory_id = m.id)
         ORDER BY m.updated_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let original = vec![1.0f32, -0.5, 0.0, 3.14160];
        let encoded = encode_embedding(&original);
        let decoded = decode_embedding(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn decode_rejects_invalid_blob_size() {
        let blob = vec![0u8, 1, 2]; // 3 bytes, not a multiple of 4
        assert!(decode_embedding(&blob).is_err());
    }

    #[test]
    fn cosine_similarity_identical_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let similarity = cosine_similarity(&a, &a);
        assert!((similarity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let similarity = cosine_similarity(&a, &b);
        assert!(similarity.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_opposite_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let similarity = cosine_similarity(&a, &b);
        assert!((similarity + 1.0).abs() < 1e-6);
    }

    #[test]
    fn default_config_is_local() {
        let config = EmbeddingConfig::default();
        assert!(matches!(config, EmbeddingConfig::Local { .. }));
    }

    #[test]
    fn config_deserialises_from_json() {
        let json = r#"{"provider": "openai", "model": "text-embedding-3-small"}"#;
        let config: EmbeddingConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, EmbeddingConfig::OpenAi { .. }));
    }

    #[test]
    fn config_disabled_deserialises() {
        let json = r#"{"provider": "disabled"}"#;
        let config: EmbeddingConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, EmbeddingConfig::Disabled));
    }

    #[test]
    fn build_passage_concatenates_fields() {
        let memory = Memory {
            id: "test".into(),
            namespace: "global".into(),
            kind: "note".into(),
            title: Some("My Title".into()),
            summary: Some("A summary.".into()),
            content: "Full content here.".into(),
            tags: vec!["rust".into(), "memory".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            archived_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            last_accessed_at: None,
            access_count: 0,
        };

        let passage = build_passage(&memory);
        assert!(passage.contains("My Title"));
        assert!(passage.contains("A summary."));
        assert!(passage.contains("rust, memory"));
        assert!(passage.contains("Full content here."));
    }
}
