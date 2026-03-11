# Performance Optimisations Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the highest-value performance improvements from the static audit: cheaper poll diffing, visibility-aware polling, autosave cleanup, batch link expansion, and top-K semantic search.

**Architecture:** Five independent changes across the Vue UI (stores, composables, views) and Rust core (repository, embeddings). All changes are backwards-compatible — no API contract changes, no schema migrations.

**Tech Stack:** Vue 3 / TypeScript (UI), Rust / rusqlite (core)

**Spec:** `docs/superpowers/specs/2026-03-11-performance-optimisations-design.md`

---

## Chunk 1: UI Performance (Tasks 1–3)

### Task 1: Replace JSON.stringify comparison with fingerprint check

**Files:**
- Modify: `ui/src/stores/memories.ts:168-197` (loadRecent function)

- [ ] **Step 1: Build a fingerprint helper**

Add a helper function at the top of the store (inside `defineStore`) that builds a lightweight fingerprint from an array of `RecallItem`:

```typescript
function fingerprint(list: RecallItem[]): string {
  return list.map((i) => `${i.id}:${i.updated_at}`).join("|");
}
```

- [ ] **Step 2: Replace JSON.stringify comparison**

In `loadRecent()`, replace:

```typescript
if (JSON.stringify(result.items) !== JSON.stringify(items.value)) {
  items.value = result.items;
}
```

with:

```typescript
if (fingerprint(result.items) !== fingerprint(items.value)) {
  items.value = result.items;
}
```

- [ ] **Step 3: Verify in dev mode**

Run: `./dev.sh`

Open the app, confirm the memory list loads. Create a new memory via compose — confirm it appears. Verify no console errors.

- [ ] **Step 4: Commit**

```bash
gitaddall && git commit -m "perf: replace JSON.stringify polling diff with id+updated_at fingerprint"
```

---

### Task 2: Pause polling when window is hidden or unfocused

**Files:**
- Modify: `ui/src/stores/memories.ts:301-313` (startPolling/stopPolling)
- Modify: `ui/src/views/HomeView.vue:81-89` (onMounted/onUnmounted)

**Note:** Only `visibilitychange` is implemented. `blur`/`focus` listeners are intentionally omitted — they fire on every focus switch even when the app remains visible, causing jarring pauses during normal multitasking.

- [ ] **Step 1: Add pausePolling and resumePolling to the store**

In `ui/src/stores/memories.ts`, add two new functions and a paused flag, and export them:

```typescript
let pollPaused = false;

function pausePolling() {
  pollPaused = true;
  stopPolling();
}

function resumePolling(intervalMs = 3000) {
  if (pollPaused) {
    pollPaused = false;
    loadRecent(true);
    startPolling(intervalMs);
  }
}
```

Add `pausePolling` and `resumePolling` to the return object.

- [ ] **Step 2: Register visibility listeners in HomeView.vue**

In `ui/src/views/HomeView.vue`, update the `<script setup>` block. Add a visibility handler and register/unregister it:

```typescript
function onVisibilityChange() {
  if (document.hidden) {
    store.pausePolling();
  } else {
    store.resumePolling(3000);
  }
}

onMounted(() => {
  store.loadRecent();
  store.loadStats();
  store.startPolling(3000);
  document.addEventListener("visibilitychange", onVisibilityChange);
});

onUnmounted(() => {
  store.stopPolling();
  document.removeEventListener("visibilitychange", onVisibilityChange);
});
```

- [ ] **Step 3: Verify in dev mode**

Run: `./dev.sh`

Open the app, switch to another window, wait 10+ seconds, switch back. Confirm:
- Polling pauses when hidden (no network requests in devtools while hidden)
- Polling resumes and data refreshes immediately on return

- [ ] **Step 4: Commit**

```bash
gitaddall && git commit -m "perf: pause UI polling when window is hidden"
```

---

### Task 3: Add onUnmounted cleanup to useAutoSave

**Files:**
- Modify: `ui/src/composables/useAutoSave.ts`

- [ ] **Step 1: Import onUnmounted and register cleanup**

In `ui/src/composables/useAutoSave.ts`, update the import and add the cleanup hook:

Change the import:

```typescript
import { ref, onUnmounted } from "vue";
```

Add the cleanup registration after the `cancel` function definition (before the `return`):

```typescript
onUnmounted(() => {
  cancel();
});
```

- [ ] **Step 2: Verify the composable still works**

Run: `./dev.sh`

Open a memory in the editor, make a change, confirm autosave still triggers (the "Saved" indicator appears). Navigate away — confirm no console errors.

- [ ] **Step 3: Commit**

```bash
gitaddall && git commit -m "fix: register onUnmounted cleanup in useAutoSave composable"
```

---

## Chunk 2: Rust Core Performance (Tasks 4–5)

### Task 4: Batch linked-memory expansion (fix N+1)

**Files:**
- Modify: `crates/clio-core/src/repository.rs:431-482` (append_linked_memories function)
- Test: `crates/clio-core/tests/integration.rs`

- [ ] **Step 1: Add a bulk link fetch helper**

Add a new function `get_links_bulk` in `crates/clio-core/src/repository.rs`, immediately above the `append_linked_memories` function (around line 428) so it sits next to its only caller:

```rust
/// Fetch all outgoing links for multiple memory IDs in a single query.
fn get_links_bulk(conn: &Connection, memory_ids: &[String]) -> Result<Vec<MemoryLink>> {
    if memory_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: String = (1..=memory_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT from_memory_id, to_memory_id, relationship, metadata_json, created_at
         FROM memory_links WHERE from_memory_id IN ({placeholders}) ORDER BY created_at"
    );
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = memory_ids
        .iter()
        .map(|id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id.clone()) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(MemoryLinkRow {
            from_memory_id: row.get(0)?,
            to_memory_id: row.get(1)?,
            relationship: row.get(2)?,
            metadata_json: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;

    let mut links = Vec::new();
    for row in rows {
        let r = row?;
        let metadata: serde_json::Value = serde_json::from_str(&r.metadata_json)?;
        links.push(MemoryLink {
            from_memory_id: r.from_memory_id,
            to_memory_id: r.to_memory_id,
            relationship: r.relationship,
            metadata,
            created_at: r.created_at,
        });
    }
    Ok(links)
}
```

- [ ] **Step 2: Rewrite append_linked_memories to use bulk fetch**

Replace the body of `append_linked_memories` (lines ~431–482):

```rust
fn append_linked_memories(conn: &Connection, result: &mut RecallResult) -> Result<()> {
    use std::collections::HashSet;

    let existing_ids: HashSet<String> = result.items.iter().map(|i| i.memory.id.clone()).collect();

    // Fetch all outgoing links for current result items in one query.
    let source_ids: Vec<String> = result.items.iter().map(|i| i.memory.id.clone()).collect();
    let all_links = get_links_bulk(conn, &source_ids)?;

    // Collect target IDs and their link sources, deduplicating.
    let mut target_to_source: Vec<(String, String)> = Vec::new();
    let mut added_ids: HashSet<String> = HashSet::new();

    for link in all_links {
        let target_id = link.to_memory_id;
        if !existing_ids.contains(&target_id) && added_ids.insert(target_id.clone()) {
            target_to_source.push((target_id, link.from_memory_id));
        }
    }

    if target_to_source.is_empty() {
        return Ok(());
    }

    // Batch-fetch all linked memories in one query.
    let ids_to_fetch: Vec<String> = target_to_source.iter().map(|(id, _)| id.clone()).collect();
    let fetched = get_many(conn, &ids_to_fetch)?;

    // Build a map of id -> source for linking.
    let source_map: std::collections::HashMap<String, String> = target_to_source
        .into_iter()
        .collect();

    let mut linked_items: Vec<RecallItem> = Vec::new();
    for memory in fetched {
        if let Some(linked_from) = source_map.get(&memory.id) {
            linked_items.push(RecallItem {
                memory,
                rank: None,
                linked_from: Some(linked_from.clone()),
            });
        }
    }

    if !linked_items.is_empty() {
        let added = linked_items.len() as u32;
        result.items.extend(linked_items);
        result.count += added;
        result.total += added;
    }

    Ok(())
}
```

- [ ] **Step 3: Write a targeted test for bulk link fetching**

Add the following test to `crates/clio-core/tests/integration.rs`:

```rust
#[test]
fn bulk_link_expansion_uses_single_query() {
    let conn = test_db();
    let settings = Settings::default();

    // Create three memories.
    let a = repository::remember(&conn, &RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: Some("Memory A".into()),
        summary: None,
        content: "First memory".into(),
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
        content: "Second memory".into(),
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
        content: "Third memory".into(),
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
        query: Some("First memory".into()),
        namespace: None,
        kind: None,
        tags: None,
        importance_min: None,
        importance_max: None,
        include_archived: false,
        include_links: true,
        sort_by: None,
        offset: 0,
        limit: 50,
    }, &settings).unwrap();

    let ids: Vec<&str> = result.items.iter().map(|i| i.memory.id.as_str()).collect();
    assert!(ids.contains(&a.id.as_str()), "should contain source memory A");
    assert!(ids.contains(&b.id.as_str()), "should contain linked memory B");
    assert!(ids.contains(&c.id.as_str()), "should contain linked memory C");

    // Verify linked_from is set on the linked items.
    let b_item = result.items.iter().find(|i| i.memory.id == b.id).unwrap();
    assert_eq!(b_item.linked_from.as_deref(), Some(a.id.as_str()));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p clio-core`

Expected: All tests pass, including the new `bulk_link_expansion_uses_single_query` test.

- [ ] **Step 5: Commit**

```bash
gitaddall && git commit -m "perf: batch linked-memory expansion into single query"
```

---

### Task 5: Top-K heap for semantic search

**Files:**
- Modify: `crates/clio-core/src/embeddings.rs:478-533` (semantic_search function)

- [ ] **Step 1: Add a ScoredEntry helper for the min-heap**

Add this just above the `semantic_search` function (around line 477):

```rust
/// Wrapper for BinaryHeap that orders by similarity ascending (min-heap).
/// The smallest similarity sits at the top so it can be ejected when a
/// better candidate arrives.
#[derive(Debug, PartialEq)]
struct ScoredEntry {
    memory_id: String,
    similarity: f64,
}

impl Eq for ScoredEntry {}

impl PartialOrd for ScoredEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering: smaller similarity = Greater, so BinaryHeap
        // keeps the minimum at the top (min-heap behaviour).
        other
            .similarity
            .partial_cmp(&self.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}
```

- [ ] **Step 2: Rewrite semantic_search to use a fixed-size heap**

Replace the results collection and sorting (lines ~514–533) in `semantic_search`:

```rust
    let capacity = limit as usize;
    let mut heap: std::collections::BinaryHeap<ScoredEntry> =
        std::collections::BinaryHeap::with_capacity(capacity + 1);

    for row_result in rows {
        let (memory_id, blob) = row_result?;
        let embedding = decode_embedding(&blob)?;
        let similarity = cosine_similarity(query_embedding, &embedding);

        heap.push(ScoredEntry {
            memory_id,
            similarity,
        });
        if heap.len() > capacity {
            heap.pop(); // Eject the lowest-scoring entry.
        }
    }

    // Drain heap into a vec sorted by similarity descending.
    // The heap already contains at most `capacity` items, so no truncation needed.
    let results: Vec<SemanticResult> = heap
        .into_sorted_vec()
        .into_iter()
        .rev()
        .map(|e| SemanticResult {
            memory_id: e.memory_id,
            similarity: e.similarity,
        })
        .collect();

    Ok(results)
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p clio-core`

Expected: All tests pass. The existing cosine similarity and embedding tests validate correctness.

- [ ] **Step 4: Run a full build**

Run: `cargo build`

Expected: Clean compilation with no warnings related to the changed code.

- [ ] **Step 5: Commit**

```bash
gitaddall && git commit -m "perf: use top-K min-heap in semantic search instead of full sort"
```

---

## Task 6: Final build and verification

**Files:** None (build-only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`

Expected: All tests pass across all crates.

- [ ] **Step 2: Build release**

Run: `./build.sh`

Expected: All crates build successfully, daemon restarts.

- [ ] **Step 3: Verify UI in dev mode**

Run: `./dev.sh`

Verify:
- Memory list loads
- Polling pauses when window hidden
- Polling resumes on focus
- Search returns results
- Memory editing with autosave works
