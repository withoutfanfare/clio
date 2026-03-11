//! In-memory caching layer for Clio read operations.
//!
//! Wraps `repository` and `embeddings` calls with LRU caches backed by
//! [`moka`]. The cache sits alongside `&Connection`, not wrapping it, so
//! callers pass both `cache` and `conn` to each method.

use std::time::Duration;

use moka::sync::Cache;
use rusqlite::Connection;

use crate::embeddings;
use crate::error::Result;
use crate::models::*;
use crate::repository;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tuneable capacities and TTLs for each cache layer.
pub struct CacheConfig {
    pub memory_capacity: u64,
    pub recall_capacity: u64,
    pub recall_ttl: Duration,
    pub embedding_capacity: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            memory_capacity: 1_000,
            recall_capacity: 200,
            recall_ttl: Duration::from_secs(30),
            embedding_capacity: 10_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Stats / clear result types
// ---------------------------------------------------------------------------

/// Counts returned after clearing all caches.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheClearResult {
    pub memory_cleared: u64,
    pub recall_cleared: u64,
    pub namespace_cleared: u64,
    pub embedding_cleared: u64,
}

/// Current entry counts for each cache layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheStats {
    pub memory_entries: u64,
    pub recall_entries: u64,
    pub namespace_cached: bool,
    pub embedding_entries: u64,
}

// ---------------------------------------------------------------------------
// ClioCache
// ---------------------------------------------------------------------------

/// In-memory LRU cache for Clio read operations.
///
/// All caches are `Send + Sync` (moka guarantees this), so `ClioCache` can
/// live behind `Arc` or inside a `Mutex<AppState>` without issue.
pub struct ClioCache {
    memory_by_id: Cache<String, Memory>,
    recall_results: Cache<String, RecallResult>,
    namespace_list: Cache<String, Vec<String>>,
    embedding_vectors: Cache<String, Vec<f32>>,
}

const NS_CACHE_KEY: &str = "__namespaces__";

impl ClioCache {
    /// Build a cache with explicit configuration.
    pub fn new(config: &CacheConfig) -> Self {
        Self {
            memory_by_id: Cache::builder()
                .max_capacity(config.memory_capacity)
                .build(),
            recall_results: Cache::builder()
                .max_capacity(config.recall_capacity)
                .time_to_live(config.recall_ttl)
                .build(),
            namespace_list: Cache::builder()
                .max_capacity(1)
                .time_to_live(config.recall_ttl)
                .build(),
            embedding_vectors: Cache::builder()
                .max_capacity(config.embedding_capacity)
                .build(),
        }
    }

    /// Build a cache with sensible defaults.
    pub fn with_defaults() -> Self {
        Self::new(&CacheConfig::default())
    }

    // -----------------------------------------------------------------------
    // Cached reads
    // -----------------------------------------------------------------------

    /// Fetch a single memory by ID (cache-through).
    pub fn get(&self, conn: &Connection, id: &str) -> Result<Memory> {
        if let Some(cached) = self.memory_by_id.get(id) {
            return Ok(cached);
        }
        let memory = repository::get(conn, id)?;
        self.memory_by_id.insert(id.to_string(), memory.clone());
        Ok(memory)
    }

    /// Full-text/filter recall (cache-through). Cache key is the JSON
    /// serialisation of the query (scoring is `#[serde(skip)]` so server
    /// config doesn't fragment keys).
    pub fn recall(&self, conn: &Connection, query: &RecallQuery) -> Result<RecallResult> {
        let key = recall_cache_key(query, None);
        if let Some(cached) = self.recall_results.get(&key) {
            return Ok(cached);
        }
        let result = repository::recall(conn, query)?;
        self.recall_results.insert(key, result.clone());
        Ok(result)
    }

    /// Scoped recall (project namespace first, then global fallback).
    pub fn recall_scoped(
        &self,
        conn: &Connection,
        query: &RecallQuery,
        namespace: &str,
    ) -> Result<RecallResult> {
        let key = recall_cache_key(query, Some(namespace));
        if let Some(cached) = self.recall_results.get(&key) {
            return Ok(cached);
        }
        let result = repository::recall_scoped(conn, query, namespace)?;
        self.recall_results.insert(key, result.clone());
        Ok(result)
    }

    /// Recent memories (convenience wrapper around recall).
    pub fn recent(
        &self,
        conn: &Connection,
        namespace: Option<&str>,
        limit: u32,
    ) -> Result<RecallResult> {
        let query = RecallQuery {
            namespace: namespace.map(String::from),
            limit,
            ..Default::default()
        };
        let key = recall_cache_key(&query, None);
        if let Some(cached) = self.recall_results.get(&key) {
            return Ok(cached);
        }
        let result = repository::recent(conn, namespace, limit)?;
        self.recall_results.insert(key, result.clone());
        Ok(result)
    }

    /// List distinct namespaces (cache-through).
    pub fn list_namespaces(&self, conn: &Connection) -> Result<Vec<String>> {
        if let Some(cached) = self.namespace_list.get(NS_CACHE_KEY) {
            return Ok(cached);
        }
        let namespaces = repository::list_namespaces(conn)?;
        self.namespace_list
            .insert(NS_CACHE_KEY.to_string(), namespaces.clone());
        Ok(namespaces)
    }

    /// Retrieve a stored embedding vector (cache-through).
    pub fn get_embedding(&self, conn: &Connection, memory_id: &str) -> Result<Option<Vec<f32>>> {
        if let Some(cached) = self.embedding_vectors.get(memory_id) {
            return Ok(Some(cached));
        }
        let embedding = embeddings::get_stored_embedding(conn, memory_id)?;
        if let Some(ref vec) = embedding {
            self.embedding_vectors
                .insert(memory_id.to_string(), vec.clone());
        }
        Ok(embedding)
    }

    /// Semantic search using cached embedding vectors where possible.
    ///
    /// Falls back to the standard `semantic_search` for the actual query, but
    /// populates the embedding cache as a side-effect so subsequent calls for
    /// the same memory IDs hit the cache.
    pub fn semantic_search_cached(
        &self,
        conn: &Connection,
        query_embedding: &[f32],
        namespace: Option<&str>,
        include_archived: bool,
        limit: u32,
    ) -> Result<Vec<embeddings::SemanticResult>> {
        // Delegate to the standard semantic search (it needs to scan all
        // embeddings anyway for cosine similarity). We use this as an
        // opportunity to warm the embedding vector cache.
        let results =
            embeddings::semantic_search(conn, query_embedding, namespace, include_archived, limit)?;

        // Warm the embedding cache for returned IDs so that follow-up
        // get_embedding calls hit memory.
        for r in &results {
            if self.embedding_vectors.get(&r.memory_id).is_none() {
                if let Ok(Some(vec)) = embeddings::get_stored_embedding(conn, &r.memory_id) {
                    self.embedding_vectors.insert(r.memory_id.clone(), vec);
                }
            }
        }

        Ok(results)
    }

    /// Get links originating from a memory (not cached — low volume, and
    /// link results don't benefit from LRU since they change with writes).
    /// Exposed here for API symmetry so callers don't mix cache + repository.
    pub fn get_links(&self, conn: &Connection, memory_id: &str) -> Result<Vec<MemoryLink>> {
        repository::get_links(conn, memory_id)
    }

    // -----------------------------------------------------------------------
    // Cached writes (delegate to repository, then invalidate)
    // -----------------------------------------------------------------------

    /// Store a new memory (or upsert), invalidating affected caches.
    pub fn remember(&self, conn: &Connection, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
        let memory = repository::remember(conn, input, settings)?;
        self.memory_by_id
            .insert(memory.id.clone(), memory.clone());
        self.invalidate_recall();
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Update an existing memory by ID.
    pub fn update(&self, conn: &Connection, id: &str, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
        let memory = repository::update(conn, id, input, settings)?;
        self.memory_by_id
            .insert(memory.id.clone(), memory.clone());
        self.invalidate_recall();
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Archive a memory.
    pub fn archive(&self, conn: &Connection, id: &str) -> Result<Memory> {
        let memory = repository::archive(conn, id)?;
        self.memory_by_id.insert(id.to_string(), memory.clone());
        self.invalidate_recall();
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Unarchive a memory.
    pub fn unarchive(&self, conn: &Connection, id: &str) -> Result<Memory> {
        let memory = repository::unarchive(conn, id)?;
        self.memory_by_id.insert(id.to_string(), memory.clone());
        self.invalidate_recall();
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Permanently delete a memory.
    pub fn delete(&self, conn: &Connection, id: &str) -> Result<Memory> {
        let memory = repository::delete(conn, id)?;
        self.memory_by_id.invalidate(id);
        self.embedding_vectors.invalidate(id);
        self.invalidate_recall();
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Move a single memory to a different namespace.
    pub fn move_namespace(
        &self,
        conn: &Connection,
        id: &str,
        namespace: &str,
    ) -> Result<Memory> {
        let memory = repository::move_namespace(conn, id, namespace)?;
        self.memory_by_id.insert(id.to_string(), memory.clone());
        self.invalidate_recall();
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Move all memories from one namespace to another.
    pub fn move_namespace_bulk(
        &self,
        conn: &Connection,
        from: &str,
        to: &str,
    ) -> Result<usize> {
        let count = repository::move_namespace_bulk(conn, from, to)?;
        // Bulk move could affect many entries — flush everything.
        self.clear_all();
        Ok(count)
    }

    /// Create a link between two memories.
    pub fn link(&self, conn: &Connection, input: &LinkInput) -> Result<MemoryLink> {
        let link = repository::link(conn, input)?;
        self.invalidate_recall();
        Ok(link)
    }

    // -----------------------------------------------------------------------
    // Cache management
    // -----------------------------------------------------------------------

    /// Flush all caches, returning entry counts that were cleared.
    pub fn clear_all(&self) -> CacheClearResult {
        let result = CacheClearResult {
            memory_cleared: self.memory_by_id.entry_count(),
            recall_cleared: self.recall_results.entry_count(),
            namespace_cleared: self.namespace_list.entry_count(),
            embedding_cleared: self.embedding_vectors.entry_count(),
        };
        self.memory_by_id.invalidate_all();
        self.recall_results.invalidate_all();
        self.namespace_list.invalidate_all();
        self.embedding_vectors.invalidate_all();
        result
    }

    /// Current entry counts.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            memory_entries: self.memory_by_id.entry_count(),
            recall_entries: self.recall_results.entry_count(),
            namespace_cached: self.namespace_list.get(NS_CACHE_KEY).is_some(),
            embedding_entries: self.embedding_vectors.entry_count(),
        }
    }

    // -----------------------------------------------------------------------
    // Internal invalidation helpers
    // -----------------------------------------------------------------------

    fn invalidate_recall(&self) {
        self.recall_results.invalidate_all();
    }

    fn invalidate_namespaces(&self) {
        self.namespace_list.invalidate_all();
    }
}

// ---------------------------------------------------------------------------
// Cache key helpers
// ---------------------------------------------------------------------------

/// Build a deterministic cache key for a recall query. The `scoring` field is
/// `#[serde(skip)]` on `RecallQuery`, so server-side config changes don't
/// fragment keys.
fn recall_cache_key(query: &RecallQuery, scope_prefix: Option<&str>) -> String {
    let json = serde_json::to_string(query).unwrap_or_default();
    match scope_prefix {
        Some(prefix) => format!("scoped:{prefix}:{json}"),
        None => json,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_determinism() {
        let q1 = RecallQuery {
            query: Some("rust memory".into()),
            namespace: Some("global".into()),
            limit: 10,
            ..Default::default()
        };
        let q2 = q1.clone();
        assert_eq!(recall_cache_key(&q1, None), recall_cache_key(&q2, None));
    }

    #[test]
    fn cache_key_scoped_differs_from_unscoped() {
        let q = RecallQuery::default();
        let unscoped = recall_cache_key(&q, None);
        let scoped = recall_cache_key(&q, Some("my-project"));
        assert_ne!(unscoped, scoped);
    }

    #[test]
    fn clear_all_empties_caches() {
        let cache = ClioCache::with_defaults();

        // Populate some entries directly.
        let mem = Memory {
            id: "test-1".into(),
            namespace: "global".into(),
            kind: "note".into(),
            title: None,
            summary: None,
            content: "hello".into(),
            tags: vec![],
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
        cache.memory_by_id.insert("test-1".into(), mem);
        cache.embedding_vectors.insert("test-1".into(), vec![1.0, 2.0]);

        // Verify entries are accessible before clearing.
        assert!(cache.memory_by_id.get("test-1").is_some());
        assert!(cache.embedding_vectors.get("test-1").is_some());

        cache.clear_all();

        // After invalidation, entries are no longer accessible.
        assert!(cache.memory_by_id.get("test-1").is_none());
        assert!(cache.embedding_vectors.get("test-1").is_none());
    }

    #[test]
    fn stats_reflects_entries() {
        let cache = ClioCache::with_defaults();
        let stats = cache.stats();
        assert_eq!(stats.memory_entries, 0);
        assert!(!stats.namespace_cached);
    }
}
