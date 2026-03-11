# Performance Audit

Date: 11 March 2026

## Scope

This is a static source audit of the Rust core, daemon, MCP server, Tauri bridge, and Vue UI.

No profiler, benchmark, build, or runtime memory tooling was used in this pass because the session was read-only. The findings below are therefore code-level observations and optimisation recommendations rather than measured benchmarks.

## Overall Verdict

- No obvious classic unbounded memory leak was found.
- The main risks are CPU churn, repeated allocation, broad locking, and several code paths that will scale poorly as the memory corpus grows.
- The highest-value work is in the UI refresh strategy, Tauri locking model, linked-memory expansion, semantic search, and auto-link batching.

## Highest-Impact Findings

### 1. UI polling churn

File: `ui/src/stores/memories.ts`

- `loadRecent()` fetches up to 50 records repeatedly.
- The store compares old and new lists using `JSON.stringify(result.items) !== JSON.stringify(items.value)`.
- This creates avoidable serialisation cost and garbage collection pressure, especially when records contain large `content` fields.

Why it matters:

- Polling plus full serialisation every few seconds will become visible as the dataset grows.
- The comparison work scales with payload size, not just item count.

Recommended improvement:

- Replace the `JSON.stringify` comparison with a cheaper structural check using stable fields such as `id` and `updated_at`.
- Better still, replace polling with event-driven refresh from Tauri when writes happen.

### 2. Polling never backs off

File: `ui/src/views/HomeView.vue`

- The home view starts polling every 3 seconds on mount.
- Polling only stops on unmount.
- It does not pause when the app window is hidden, unfocused, or idle.

Why it matters:

- This burns CPU and keeps the app active even when the user is not interacting.
- It also forces needless SQLite reads and UI reconciliation.

Recommended improvement:

- Pause polling when the window loses focus or becomes hidden.
- Prefer push-based refresh events from Tauri commands after create, update, archive, delete, and capture operations.

### 3. Global app lock held during expensive work

File: `crates/clio-tauri/src/commands/search.rs`

- `cmd_search()` locks `Mutex<AppState>`.
- It then performs embedding generation and semantic recall while still holding that lock.
- The same pattern appears in other command handlers that rely on the same shared state.

Why it matters:

- Expensive work under one global mutex serialises unrelated commands.
- Searches can block other actions and make the desktop UI feel stalled.

Recommended improvement:

- Split the app state so only the minimum shared fields are locked.
- Move long-running search work outside the global lock.
- Consider separate locking for settings, cache, backend, and database access.

### 4. Linked-memory expansion still performs N+1 queries

File: `crates/clio-core/src/repository.rs`

- `append_linked_memories()` claims to use batch fetching.
- It batch-fetches target memories, but still calls `get_links()` once for each result item.
- That means one extra query per recalled memory before the batch fetch even happens.

Why it matters:

- This becomes expensive when `include_links` is enabled on larger result sets.
- The query count grows with the number of base results.

Recommended improvement:

- Replace per-item `get_links()` calls with one query that fetches all links for the current result IDs using `IN (...)`.
- Build the source-to-target map from that one result set.

### 5. Semantic search scans and sorts the full embedding corpus

File: `crates/clio-core/src/embeddings.rs`

- `semantic_search()` selects every matching embedding blob.
- It decodes every blob into a `Vec<f32>`.
- It computes cosine similarity for every row.
- It sorts the entire result vector and then truncates to `limit`.

Why it matters:

- This is acceptable for small datasets but becomes the dominant bottleneck at scale.
- The cost grows with the total number of embeddings, not the requested `limit`.
- Full sorting adds more work than necessary when only the top few items are needed.

Recommended improvement:

- Maintain a fixed-size top-K heap instead of sorting the full result list.
- Avoid decoding all vectors if a more efficient storage or approximate index is introduced later.
- Longer term, consider ANN/vector index support if the dataset is expected to grow significantly.

## Medium-Priority Findings

### 6. Auto-linking compounds semantic-search cost

File: `crates/clio-core/src/embeddings.rs`

- `auto_link_batch()` checks `has_embedding()` per candidate.
- It then calls `suggest_links()` per memory.
- `suggest_links()` performs another scan across candidate embeddings.

Why it matters:

- This creates repeated full or near-full scans during background processing.
- The effective cost grows quickly with corpus size.

Recommended improvement:

- Fetch missing-embedding status in bulk.
- Reuse preloaded or cached candidate vectors during a batch.
- Avoid rescanning the entire candidate set for every source memory when possible.

### 7. Stats are recomputed from scratch each time

File: `crates/clio-core/src/stats.rs`

- `memory_stats()` runs several aggregate queries for counts, namespace breakdowns, kind breakdowns, weekly summaries, tags, and links.
- These are executed each time stats are requested.

Why it matters:

- This is probably fine today, but it will become slower as the database grows.
- The results are good candidates for caching because they are analytical rather than transactional.

Recommended improvement:

- Cache stats briefly in memory.
- Invalidate cached stats on write operations.
- Consider separate lightweight endpoints for views that only need a subset of the metrics.

### 8. Filesystem watcher callback can block

File: `crates/clio-daemon/src/watcher.rs`

- The notify callback uses `tx.blocking_send(path)`.
- If the bounded channel fills up, the callback thread can block.

Why it matters:

- Backpressure is good, but blocking in the filesystem event callback can delay or interfere with incoming notifications.

Recommended improvement:

- Use non-blocking send where practical and log/drop duplicate or burst traffic.
- Alternatively introduce a coalescing queue for bursty inbox drops.

### 9. Autosave cleanup could be made safer

File: `ui/src/composables/useAutoSave.ts`

- The composable clears timers when `cancel()` is called.
- It does not register its own teardown on component unmount.
- Current usage appears controlled, but the composable relies on callers always cleaning up correctly.

Why it matters:

- This is not a proven leak in the current code, but it is a fragile lifecycle pattern.

Recommended improvement:

- Register cleanup with `onUnmounted()` inside the composable.
- Keep timer ownership entirely local to the composable lifecycle.

## Memory-Leak Assessment

### What was checked

- Long-lived caches in the Rust core.
- Long-lived daemon loops and background tasks.
- Vue timers, polling, and event listeners.
- Search and embedding paths that allocate large temporary structures.

### Conclusion

- No obvious unbounded leak was found in the current static review.
- The main issue is retained work and repeated allocation, not leaked ownership.
- The current `moka` caches are bounded.
- Vue event listeners that were sampled appear to unregister correctly.
- The biggest memory pressure comes from repeatedly decoding embeddings and serialising large UI result sets.

## Recommended Priority Order

### Phase 1 — Best return for effort

1. Replace polling + `JSON.stringify` diffing in the UI.
2. Reduce global Tauri lock contention.
3. Batch linked-memory expansion properly.

### Phase 2 — Scale-focused improvements

4. Rework semantic search to use top-K selection instead of full sorting.
5. Optimise auto-link batch processing to avoid repeated scans.
6. Add short-lived caching for stats and other analytical reads.

### Phase 3 — Longer-term architecture

7. Move from polling to push-based UI refresh events.
8. Consider a more scalable vector-search approach if memory volume is expected to grow significantly.
9. Review whether a connection pool or a more concurrent read model would improve MCP and Tauri responsiveness.

## Suggested Implementation Plan

### Quick wins

- Compare lists using `id` and `updated_at` rather than serialising the full payload.
- Pause polling when the app is hidden or unfocused.
- Add `onUnmounted()` cleanup in `useAutoSave()`.

### Medium effort

- Refactor Tauri state access to avoid holding one mutex during embedding work.
- Replace per-item link lookups with a bulk query.
- Add a top-K heap for semantic search.

### Larger changes

- Introduce event-driven invalidation and refresh for the UI.
- Batch or cache vector decoding during auto-linking.
- Evaluate future vector indexing options.

## Final Summary

The app does not currently show a clear memory leak from static inspection, but it does have several hotspots that will limit responsiveness and scalability. The biggest practical wins will come from reducing unnecessary UI refresh work, narrowing lock scope in the Tauri layer, removing remaining N+1 access patterns, and making semantic/vector operations more incremental.
