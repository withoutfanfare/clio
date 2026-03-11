# Performance Optimisations — Design Spec

Date: 11 March 2026
Source: `docs/performance-audit.md`

## Scope

Implement the highest-value performance improvements identified in the static audit. Focuses on Phase 1 (best return for effort) plus quick wins. Defers Phase 2/3 items (stats caching, push-based events, vector indexing) as speculative or architectural.

## Changes

### 1. Replace JSON.stringify polling comparison

**File:** `ui/src/stores/memories.ts`

**Problem:** `loadRecent()` compares old and new lists using `JSON.stringify(result.items) !== JSON.stringify(items.value)`. This serialises every field of every memory (including large `content`) on every 3-second poll tick.

**Fix:** Compare using a fingerprint built from stable, lightweight fields: `id` and `updated_at`. Build a fingerprint string by mapping items to `${id}:${updated_at}` and joining. Compare the fingerprint strings instead of full serialisations.

**Why this works:** Any meaningful change to a memory updates `updated_at`. New or removed items change the ID set. This catches all real changes at a fraction of the cost.

### 2. Pause polling when window is hidden or unfocused

**File:** `ui/src/stores/memories.ts` and `ui/src/views/HomeView.vue`

**Problem:** Polling runs every 3 seconds unconditionally, even when the app is not visible.

**Fix:** Add `pausePolling()` and `resumePolling()` to the store. In `HomeView.vue`, register `visibilitychange` and `blur`/`focus` event listeners that pause/resume polling. Clean up listeners in `onUnmounted`.

**Behaviour:**
- `document.hidden === true` → pause
- `document.hidden === false` → resume and immediately refresh
- Window blur → pause (optional, can be skipped if too aggressive)
- Window focus → resume and immediately refresh

### 3. Add onUnmounted cleanup to useAutoSave

**File:** `ui/src/composables/useAutoSave.ts`

**Problem:** The composable relies on callers to call `cancel()`. If a caller forgets, timers leak.

**Fix:** Import `onUnmounted` from Vue and register `cancel()` as a cleanup hook inside the composable. Keep the manual `cancel()` export for explicit use.

### 4. Batch linked-memory expansion (fix N+1)

**File:** `crates/clio-core/src/repository.rs`

**Problem:** `append_linked_memories()` calls `get_links(conn, &item.memory.id)` once per result item. This is an N+1 pattern — one query per recalled memory.

**Fix:** Replace the per-item `get_links()` loop with a single SQL query that fetches all outgoing links for the current result IDs using `WHERE from_memory_id IN (...)`. Build the source-to-target map from that single result set.

**SQL:**
```sql
SELECT from_memory_id, to_memory_id, relationship, metadata_json, created_at
FROM memory_links
WHERE from_memory_id IN (?1, ?2, ...)
```

Use the same dynamic placeholder pattern already used in `get_many()`.

### 5. Top-K heap for semantic search

**File:** `crates/clio-core/src/embeddings.rs`

**Problem:** `semantic_search()` collects all results into a `Vec`, sorts the entire vector, then truncates to `limit`. The cost grows with the total number of embeddings.

**Fix:** Replace the collect-sort-truncate pattern with a fixed-size min-heap (using `std::collections::BinaryHeap` with a reverse-ordered wrapper). The heap maintains only the top `limit` items, rejecting lower-scoring candidates immediately.

**Approach:**
- Define a `ScoredEntry` wrapper that implements `Ord` in reverse (min-heap behaviour so the smallest score is at the top and can be ejected).
- As each row is processed, push onto the heap. If heap size exceeds `limit`, pop the minimum.
- After processing all rows, drain the heap into a sorted vec.

This caps memory at O(limit) instead of O(total embeddings) and avoids sorting more than `limit` items.

### 6. Tauri lock scope (assessment: no change needed)

**File:** `crates/clio-tauri/src/commands/search.rs`

The audit flagged the global `Mutex<AppState>` as holding the lock during embedding work. However, reviewing the actual code:

- `AppState` holds `conn`, `settings`, `backend`, and `cache` — all of which are needed by `cmd_search`.
- `embed_one()` for the local backend is CPU-bound and typically completes in <10ms.
- Splitting the state would require `Arc` wrappers and careful lifetime management for `Connection` (which is `!Send`).
- The current single-mutex pattern is appropriate for a desktop app with one active user.

**Decision:** No change. The lock contention is theoretical at current scale. If it becomes measurable, the fix is to wrap `backend` in its own `Arc<Mutex<>>` or use `RwLock`, but that's premature now.

## Out of Scope

These Phase 2/3 items are deferred:

- **Auto-link batch optimisation (#6 in audit):** The code already batch-embeds unembedded candidates. The per-memory `suggest_links` call is inherent to the algorithm. Caching decoded vectors would add complexity for a background process.
- **Stats caching (#7):** Analytical queries on SQLite are fast. Caching adds invalidation complexity. Defer until measured.
- **Filesystem watcher blocking_send (#8):** Backpressure is intentional. The bounded channel prevents unbounded queue growth. No change.
- **Push-based UI refresh:** Requires Tauri event emission from every write path. Larger architectural change. Defer.
- **Connection pooling / vector indexing:** Architecture-level changes. Defer.

## Testing

- **Items 1-2:** Manual verification in the Tauri app. Check that polling pauses when window is hidden and resumes on focus.
- **Item 3:** Verify `onUnmounted` cleanup by checking that no timers fire after component unmount.
- **Item 4:** Existing `cargo test` covers recall paths. Add a targeted test for batch link fetching.
- **Item 5:** Existing embedding tests cover correctness. Verify that `semantic_search` returns the same top-K results as before.
