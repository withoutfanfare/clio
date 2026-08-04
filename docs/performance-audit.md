# Performance Audit

Original audit: 11 March 2026
Current-source verification: 1 August 2026

## Scope

This is a static source audit of the Rust core, daemon, MCP server, Tauri bridge, and Vue UI.

The original pass was static. The 1 August refresh rechecked every finding
against the current source and adds focused MCP concurrency tests; it is still
not a whole-application profiler or benchmark report.

## Overall Verdict

- No obvious classic unbounded memory leak was found.
- Several original findings are now fixed: polling comparisons are structural,
  hidden-window polling pauses, autosave cleans itself up, linked recall batches
  edge queries, and semantic ranking retains only top-K results.
- The main remaining risks are full-corpus vector decoding, repeated auto-link
  candidate scans, aggregate-stat recomputation, Tauri's broad local-state lock,
  and daemon watcher backpressure.

## Highest-Impact Findings

### 1. UI polling comparison — resolved

File: `ui/src/stores/memories.ts`

- `loadRecent()` still fetches up to 50 records, but compares a compact
  `id:updated_at` fingerprint rather than serialising complete records.
- Event-driven refresh remains a possible future replacement for polling, not a
  current correctness issue.

### 2. Hidden-window polling — resolved

File: `ui/src/views/HomeView.vue`

- `HomeView.vue` pauses the three-second poll on `visibilitychange` when the
  document is hidden and resumes with an immediate refresh when visible.
- Polling while visible remains deliberate current behaviour.

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

### 3a. MCP provider lock contention — resolved 1 August 2026

File: `crates/clio-mcp/src/main.rs`

- The MCP process already reused one SQLite connection, settings cache and
  embedding backend, but semantic query embedding, capture classification,
  checkpoint distillation and post-write embedding held the connection mutex
  during provider work.
- These provider phases now run before acquiring, or after releasing, the
  SQLite mutex. Focused concurrency tests block the provider deliberately and
  prove an unrelated namespace read still completes.
- Post-write vectors use an `updated_at` check before storage, preventing an
  embedding generated from an older record version from overwriting a newer
  edit.

### 3b. Remaining MCP provider critical sections

Files: `crates/clio-mcp/src/main.rs`, `crates/clio-core/src/embeddings.rs`,
`crates/clio-core/src/review.rs`

- `memory_suggest_links` still holds the MCP SQLite mutex if the source memory
  has no stored vector and must be embedded on demand.
- Inbox approval still creates and invokes an embedding backend while the MCP
  mutex is held when automatic embedding is enabled.
- Both are lower-frequency paths than search, capture, checkpoints and direct
  writes, but should use the same provider/database phase split if profiling or
  contention reports justify the extra core API surface.

### 4. Linked-memory expansion — resolved

File: `crates/clio-core/src/repository.rs`

- `append_linked_memories()` now calls `get_links_bulk()` and
  `get_links_bulk_incoming()` once each for all anchors, then batch-fetches
  eligible targets. The original N+1 query path no longer exists.

### 5. Semantic search scans the embedding corpus but retains bounded top-K

File: `crates/clio-core/src/embeddings.rs`

- `semantic_search()` selects every matching embedding blob.
- It decodes every blob into a `Vec<f32>`.
- It computes cosine similarity for every row.
- It retains only `limit` candidates in a `BinaryHeap` and sorts that bounded
  heap for output.

Why it matters:

- This is acceptable for small datasets but becomes the dominant bottleneck at scale.
- The cost grows with the total number of embeddings, not the requested `limit`.
- Full-corpus decoding and cosine work remain O(N); ranking memory is now O(K).

Recommended improvement:

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

### 9. Autosave lifecycle — resolved

File: `ui/src/composables/useAutoSave.ts`

- `useAutoSave()` registers `onUnmounted(cancel)`, so its timers and pending
  state are owned by the composable lifecycle.

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

### Phase 1 — Current best return for effort

1. Reduce global Tauri lock contention.
2. Coalesce or otherwise drain bursty daemon watcher events without blocking
   the notify callback or dropping unacknowledged files.
3. Measure and reduce repeated vector scans in auto-link batches.

### Phase 2 — Scale-focused improvements

4. Add short-lived caching for stats and other analytical reads.
5. Reduce full-corpus vector decoding or introduce an indexed search path when
   measured corpus size justifies it.
6. Move visible-window polling to event-driven refresh if profiling shows the
   remaining reads matter.

### Phase 3 — Longer-term architecture

7. Consider a small MCP connection pool only if profiling shows SQLite phases,
   rather than provider waits, still limit responsiveness.
8. Consider a more scalable vector-search approach if memory volume is expected to grow significantly.

## Suggested Implementation Plan

### Quick wins

- Add watcher queue-depth and processing-latency instrumentation before changing
  backpressure behaviour.
- Measure stats latency at current and projected corpus sizes before adding a
  cache.

### Medium effort

- Refactor Tauri state access to avoid holding one mutex during embedding work.
- Reuse candidate vectors across an auto-link batch where measurement supports
  the additional memory cost.

### Larger changes

- Introduce event-driven invalidation and refresh for the UI if required.
- Evaluate future vector indexing options.

## Final Summary

The current source does not show a clear unbounded leak. Earlier UI lifecycle,
linked-query and top-K findings have been fixed, and the audited MCP semantic
query, capture, checkpoint and post-write embedding waits no longer serialise
unrelated database reads. The remaining performance work is measurement-led:
narrow the Tauri local-state lock, address daemon watcher backpressure safely,
and reduce repeated/full-corpus vector work as the corpus grows.
