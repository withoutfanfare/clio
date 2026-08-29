//! In-memory caching layer for bounded Clio read operations.
//!
//! Only the namespace list is cached. Memory and recall reads go directly to
//! SQLite so separate Clio processes always observe committed writes.

use std::time::Duration;

use moka::sync::Cache;
use rusqlite::Connection;

use crate::error::Result;
use crate::models::*;
use crate::repository;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tuneable capacities and TTLs for each cache layer.
pub struct CacheConfig {
    /// Retained for configuration compatibility; individual memories are not cached.
    pub memory_capacity: u64,
    /// Retained for configuration compatibility; recall results are not cached.
    pub recall_capacity: u64,
    pub recall_ttl: Duration,
    /// Retained for configuration compatibility; embedding vectors are not cached.
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
    /// Always zero; retained for response compatibility.
    pub memory_cleared: u64,
    pub recall_cleared: u64,
    pub namespace_cleared: u64,
    /// Always zero; retained for response compatibility.
    pub embedding_cleared: u64,
}

/// Current entry counts for each cache layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheStats {
    /// Always zero; individual memories are not cached.
    pub memory_entries: u64,
    /// Always zero; recall results are not cached.
    pub recall_entries: u64,
    pub namespace_cached: bool,
    /// Always zero; embedding vectors are not cached.
    pub embedding_entries: u64,
}

// ---------------------------------------------------------------------------
// ClioCache
// ---------------------------------------------------------------------------

/// In-memory cache for the namespace list.
///
/// All caches are `Send + Sync` (moka guarantees this), so `ClioCache` can
/// live behind `Arc` or inside a `Mutex<AppState>` without issue.
pub struct ClioCache {
    namespace_list: Cache<String, CachedNamespaces>,
}

const NS_CACHE_KEY: &str = "__namespaces__";

#[derive(Clone)]
struct CachedNamespaces {
    generation: i64,
    namespaces: Vec<String>,
}

impl ClioCache {
    /// Build a cache with explicit configuration.
    pub fn new(config: &CacheConfig) -> Self {
        Self {
            namespace_list: Cache::builder()
                .max_capacity(1)
                .time_to_live(config.recall_ttl)
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

    /// Fetch a single memory by ID directly from SQLite.
    ///
    /// Record reads are intentionally not cached: independent MCP processes
    /// cannot invalidate one another's in-memory state.
    pub fn get(&self, conn: &Connection, id: &str) -> Result<Memory> {
        repository::get(conn, id)
    }

    /// Full-text/filter recall directly from SQLite.
    pub fn recall(&self, conn: &Connection, query: &RecallQuery) -> Result<RecallResult> {
        repository::recall(conn, query)
    }

    /// Scoped recall (project namespace first, then global fallback).
    pub fn recall_scoped(
        &self,
        conn: &Connection,
        query: &RecallQuery,
        namespace: &str,
    ) -> Result<RecallResult> {
        repository::recall_scoped(conn, query, namespace)
    }

    /// Recent memories (convenience wrapper around recall).
    pub fn recent(
        &self,
        conn: &Connection,
        namespace: Option<&str>,
        limit: u32,
    ) -> Result<RecallResult> {
        repository::recent(conn, namespace, limit)
    }

    /// List distinct namespaces (cache-through).
    pub fn list_namespaces(&self, conn: &Connection) -> Result<Vec<String>> {
        for _ in 0..2 {
            let generation = crate::repair::current_store_generation(conn)?;
            if let Some(cached) = self.namespace_list.get(NS_CACHE_KEY) {
                if cached.generation == generation {
                    return Ok(cached.namespaces);
                }
            }

            let namespaces = repository::list_namespaces(conn)?;
            if crate::repair::current_store_generation(conn)? == generation {
                self.namespace_list.insert(
                    NS_CACHE_KEY.to_string(),
                    CachedNamespaces {
                        generation,
                        namespaces: namespaces.clone(),
                    },
                );
                return Ok(namespaces);
            }
        }

        // A continuously mutating store is safer read directly than cached.
        repository::list_namespaces(conn)
    }

    /// Get links originating from a memory (not cached — low volume, and
    /// link results don't benefit from LRU since they change with writes).
    /// Exposed here for API symmetry so callers don't mix cache + repository.
    pub fn get_links(&self, conn: &Connection, memory_id: &str) -> Result<Vec<MemoryLink>> {
        repository::get_links(conn, memory_id)
    }

    // -----------------------------------------------------------------------
    // Writes (delegate to repository, then invalidate namespace cache)
    // -----------------------------------------------------------------------

    /// Store a new memory (or upsert), invalidating affected caches.
    pub fn remember(
        &self,
        conn: &Connection,
        input: &RememberInput,
        settings: &crate::settings::Settings,
    ) -> Result<Memory> {
        let memory = repository::remember(conn, input, settings)?;
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Update an existing memory by ID.
    pub fn update(
        &self,
        conn: &Connection,
        id: &str,
        input: &UpdateInput,
        settings: &crate::settings::Settings,
    ) -> Result<Memory> {
        let memory = repository::update(conn, id, input, settings)?;
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Archive a memory.
    pub fn archive(&self, conn: &Connection, id: &str) -> Result<Memory> {
        let memory = repository::archive(conn, id)?;
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Unarchive a memory.
    pub fn unarchive(&self, conn: &Connection, id: &str) -> Result<Memory> {
        let memory = repository::unarchive(conn, id)?;
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Permanently delete a memory.
    pub fn delete(&self, conn: &Connection, id: &str) -> Result<Memory> {
        let memory = repository::delete(conn, id)?;
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Move a single memory to a different namespace.
    pub fn move_namespace(&self, conn: &Connection, id: &str, namespace: &str) -> Result<Memory> {
        let memory = repository::move_namespace(conn, id, namespace)?;
        self.invalidate_namespaces();
        Ok(memory)
    }

    /// Move all memories from one namespace to another.
    pub fn move_namespace_bulk(&self, conn: &Connection, from: &str, to: &str) -> Result<usize> {
        let count = repository::move_namespace_bulk(conn, from, to)?;
        // Bulk move could affect many entries — flush everything.
        self.clear_all();
        Ok(count)
    }

    /// Create a link between two memories.
    pub fn link(&self, conn: &Connection, input: &LinkInput) -> Result<MemoryLink> {
        let link = repository::link(conn, input)?;
        Ok(link)
    }

    // -----------------------------------------------------------------------
    // Cache management
    // -----------------------------------------------------------------------

    /// Flush all caches, returning entry counts that were cleared.
    pub fn clear_all(&self) -> CacheClearResult {
        let result = CacheClearResult {
            memory_cleared: 0,
            recall_cleared: 0,
            namespace_cleared: self.namespace_list.entry_count(),
            embedding_cleared: 0,
        };
        self.namespace_list.invalidate_all();
        result
    }

    /// Current entry counts.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            memory_entries: 0,
            recall_entries: 0,
            namespace_cached: self.namespace_list.get(NS_CACHE_KEY).is_some(),
            embedding_entries: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Internal invalidation helpers
    // -----------------------------------------------------------------------

    fn invalidate_namespaces(&self) {
        self.namespace_list.invalidate_all();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_all_empties_caches() {
        let cache = ClioCache::with_defaults();
        cache.namespace_list.insert(
            NS_CACHE_KEY.into(),
            CachedNamespaces {
                generation: 0,
                namespaces: vec!["global".into()],
            },
        );

        // Verify entries are accessible before clearing.
        assert!(cache.namespace_list.get(NS_CACHE_KEY).is_some());

        cache.clear_all();

        // After invalidation, entries are no longer accessible.
        assert!(cache.namespace_list.get(NS_CACHE_KEY).is_none());
    }

    #[test]
    fn stats_reflects_entries() {
        let cache = ClioCache::with_defaults();
        let stats = cache.stats();
        assert_eq!(stats.memory_entries, 0);
        assert!(!stats.namespace_cached);
    }
}
