# Clio Retrieval & Lifecycle Improvements — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the three highest-leverage quality gaps in the Clio engine: (1) make semantic search rank like keyword recall, (2) stop the write path creating duplicate memories, and (3) stop expired memories being recalled as current.

**Architecture:** All three changes live in `clio-core`; adapters are untouched except where a new query flag must be threaded through. Each task is an independently shippable slice with its own tests and commit. They share no code beyond a new scoring helper introduced in Task 1.

**Tech Stack:** Rust, `rusqlite` (SQLite + FTS5), `chrono` for time, `cargo test -p clio-core`.

## Global Constraints

- `decay_lambda = 0.0` MUST preserve original ranking (backwards-compatible). Any new scoring helper returns a neutral `1.0` multiplier when `decay_lambda <= 0.0`. (`crates/clio-core/CLAUDE.md`)
- Archive means hidden, not deleted. New filters must not delete or hard-exclude archived rows beyond existing behaviour.
- Upsert is keyed on `source + source_ref` and must not create duplicates. Dedup work (Task 2) must not break or bypass this.
- Access tracking is fire-and-forget — never fail the parent operation.
- MCP defaults must match CLI/core semantics exactly. Any new `RecallQuery` field defaults to the backwards-compatible value (`#[serde(default)]`).
- Every schema change ships as a NEW migration — never edit an applied one. (Tasks here need no schema change; if Task 2's embedding-cosine slice is taken, it still adds no columns.)
- British English in comments and user-facing text.
- Conventional commits. Stage with `gitaddall`.

**Verification baseline (run before starting):** `cargo test -p clio-core` is green.

---

## File Structure

| File | Responsibility | Touched by |
|------|----------------|-----------|
| `crates/clio-core/src/scoring.rs` *(new)* | Single home for the composite temporal multiplier shared by FTS recall and semantic recall. | Task 1 |
| `crates/clio-core/src/embeddings.rs` | `semantic_recall` fusion; gains scoring + namespace awareness (Task 1) and an expiry clause in `semantic_search` (Task 3). | Tasks 1, 3 |
| `crates/clio-core/src/repository.rs` | `recall_fts`/`recall_recent` composite SQL (Task 1 cross-check); `append_filters` expiry clause (Task 3); new `find_content_duplicate` (Task 2). | Tasks 1, 2, 3 |
| `crates/clio-core/src/models.rs` | `RecallQuery` gains `exclude_expired` (Task 3). | Task 3 |
| `crates/clio-core/src/capture.rs` | `store_or_queue` consults dedup before insert (Task 2). | Task 2 |
| `crates/clio-core/src/review.rs` | `approve_review` consults dedup before insert (Task 2). | Task 2 |
| `crates/clio-core/src/lib.rs` | Register `mod scoring;`. | Task 1 |

Each task ends green and committable on its own. Recommended order: **Task 3 → Task 2 → Task 1** (cheapest/lowest-risk first), but they are independent.

---

## Task 1: Unify semantic recall with the composite scorer

**Why:** `semantic_recall` (`embeddings.rs:574-651`) ranks on `cosine + flat 0.3 keyword boost` only — it ignores decay, importance, and access that FTS recall applies inline in SQL (`repository.rs:611-621`). The two retrieval paths return inconsistently ordered results. This task extracts the temporal formula into a shared Rust helper and applies it in semantic recall.

**Files:**
- Create: `crates/clio-core/src/scoring.rs`
- Modify: `crates/clio-core/src/lib.rs` (add `mod scoring;`)
- Modify: `crates/clio-core/src/embeddings.rs:574-651` (`semantic_recall` signature + fusion)
- Modify call sites: `crates/clio-mcp/src/main.rs:~1438`, `crates/clio-cli/src/main.rs:~1578`, `crates/clio-tauri/src/commands/search.rs:~37`
- Test: inline `#[cfg(test)]` in `scoring.rs`; ordering test in `embeddings.rs` tests or `tests/integration.rs`

**Interfaces:**
- Produces: `pub fn composite_multiplier(memory: &Memory, scoring: &ScoringConfig, now: chrono::DateTime<chrono::Utc>) -> f64`
- Consumes: existing `Memory` (`models.rs`), `ScoringConfig` (`settings.rs`), `RecallItem` (`models.rs`).

**Decision to confirm before coding:** the FTS SQL decays off `COALESCE(last_accessed_at, updated_at)`, which the retrieval investigation flagged as self-reinforcing (every recall resets the decay clock — `embeddings.rs:644-648`, `repository.rs:404-407`). Two options:
- **(a) Match FTS exactly** — decay off `COALESCE(last_accessed_at, updated_at)`. Lowest risk; keeps the two paths identical. **Recommended for this task.**
- **(b) Fix the clock** — decay off `updated_at` only, leave access as the separate boost term. Better signal, but changes FTS ranking too and needs its own task. Defer to a follow-up.

This task takes **(a)** so the helper provably matches current FTS output.

- [ ] **Step 1: Write the failing test for the helper**

In `crates/clio-core/src/scoring.rs` (new file), add a test asserting the multiplier matches the FTS SQL formula on a known input and that `decay_lambda = 0.0` yields exactly `1.0`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ScoringConfig;
    use chrono::{Duration, Utc};

    fn mem(importance: i32, access_count: i64, age_days: i64) -> crate::models::Memory {
        let mut m = crate::models::Memory::default();
        m.importance = importance;
        m.access_count = access_count;
        m.updated_at = Utc::now() - Duration::days(age_days);
        m.last_accessed_at = None;
        m
    }

    #[test]
    fn decay_lambda_zero_is_neutral() {
        let s = ScoringConfig { decay_lambda: 0.0, access_boost_weight: 0.1 };
        assert_eq!(composite_multiplier(&mem(5, 10, 30), &s, Utc::now()), 1.0);
    }

    #[test]
    fn importance_scales_one_third() {
        // With no decay (age 0) and no access, multiplier == importance/3.
        let s = ScoringConfig { decay_lambda: 0.01, access_boost_weight: 0.0 };
        let now = Utc::now();
        let mut m = mem(3, 0, 0);
        m.updated_at = now;
        assert!((composite_multiplier(&m, &s, now) - 1.0).abs() < 1e-6);
    }
}
```

> If `Memory`/`ScoringConfig` lack `Default` or these exact fields, adjust the constructors to the real shapes (read `models.rs` / `settings.rs` first). Keep the two asserted behaviours.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p clio-core scoring::`
Expected: FAIL — `composite_multiplier` not found.

- [ ] **Step 3: Implement the helper**

```rust
//! Shared composite relevance multiplier.
//!
//! Mirrors the FTS recall scoring SQL in `repository.rs` so that semantic and
//! keyword recall rank by the same temporal/importance signal. Returns a
//! neutral 1.0 when decay is disabled, preserving the `decay_lambda = 0.0`
//! backwards-compatibility invariant.

use crate::models::Memory;
use crate::settings::ScoringConfig;
use chrono::{DateTime, Utc};

pub fn composite_multiplier(memory: &Memory, scoring: &ScoringConfig, now: DateTime<Utc>) -> f64 {
    if scoring.decay_lambda <= 0.0 {
        return 1.0;
    }
    let reference = memory.last_accessed_at.unwrap_or(memory.updated_at);
    let age_days = (now - reference).num_seconds() as f64 / 86_400.0;
    let decay = (-scoring.decay_lambda * age_days).exp();
    let access =
        1.0 + 0.5_f64.min(scoring.access_boost_weight * (1.0 + memory.access_count as f64).ln());
    let importance = memory.importance as f64 / 3.0;
    decay * access * importance
}
```

Register the module in `lib.rs`: `mod scoring;` (or `pub mod scoring;` if call sites outside the crate need it — they don't; keep it crate-private and re-export the fn via `pub(crate)` if needed).

- [ ] **Step 4: Run the helper tests — expect PASS**

Run: `cargo test -p clio-core scoring::`
Expected: PASS.

- [ ] **Step 5: Write the failing fusion test**

Add an integration-style test (in `embeddings.rs` `#[cfg(test)]` or `tests/integration.rs`) that stores two memories with embeddings — one a strong semantic match but importance 1 and old, one a weaker match but importance 5 and fresh — and asserts the importance-5 memory now ranks above where pure cosine would place it. (Pin the assertion to the *relative order changing* vs the pre-change baseline, not an absolute score.)

- [ ] **Step 6: Run it and watch it fail**

Run: `cargo test -p clio-core semantic_recall_applies_importance`
Expected: FAIL — current fusion ignores importance.

- [ ] **Step 7: Apply the multiplier in `semantic_recall`**

Thread `scoring: Option<&ScoringConfig>` into `semantic_recall` (new param) and fold the multiplier into the rank. Replace the flat-boost block (`embeddings.rs:617-634`) so the hybrid score becomes:

```rust
let now = chrono::Utc::now();
let mut items: Vec<RecallItem> = memories
    .into_iter()
    .map(|memory| {
        let semantic = similarity_map.get(&memory.id).copied().unwrap_or(0.0);
        let keyword = if fts_hits.contains(&memory.id) { KEYWORD_BOOST } else { 0.0 };
        let base = semantic + keyword;
        let rank = match scoring {
            Some(s) => base * crate::scoring::composite_multiplier(&memory, s, now),
            None => base,
        };
        RecallItem { memory, rank: Some(rank), linked_from: None }
    })
    .collect();
```

Update the three call sites to pass the caller's `ScoringConfig` (the same one they already build for FTS recall — grep each adapter for where it constructs `RecallQuery.scoring`).

- [ ] **Step 8: Run the fusion test + full core suite**

Run: `cargo test -p clio-core`
Expected: PASS (new fusion test green; no regressions). If any existing semantic-ordering test breaks, confirm the new order is *correct* (importance/decay-aware) before updating its expectation.

- [ ] **Step 9: Commit**

```bash
gitaddall
git commit -m "feat(core): apply composite scoring to semantic recall

Extract the FTS temporal/importance multiplier into scoring::composite_multiplier
and fold it into semantic_recall so keyword and semantic recall rank consistently.
Neutral when decay_lambda = 0.0 (preserves backwards-compat invariant)."
```

**Follow-up (out of scope, note in commit body or an issue):** RRF-based fusion instead of the flat `KEYWORD_BOOST`, and the decay-clock fix (option (b) above). Namespace-scoped two-pass fallback for semantic recall to match `recall_scoped`.

---

## Task 2: Deduplicate on the write path

**Why:** `store_or_queue` (`capture.rs:603-622`) and `approve_review` (`review.rs:186-203`) both insert with `upsert: false` and `source_ref: None`, so every capture of an already-known fact creates a new row. Dedup machinery exists (`deduplication.rs`) but is a whole-DB scan, never consulted on write. This task adds a cheap exact-content check scoped to the target namespace and skips/links instead of duplicating.

**Scope decision:** first slice is **exact normalised-content match within the same namespace** (SQL, fast, deterministic). Embedding-cosine near-dup detection on write is a noted follow-up — it needs an embedding at write time and a threshold decision, and is higher risk.

**Files:**
- Modify: `crates/clio-core/src/repository.rs` (new `find_content_duplicate`)
- Modify: `crates/clio-core/src/capture.rs:603-622` (consult before insert)
- Modify: `crates/clio-core/src/review.rs:186-203` (consult before insert)
- Test: `tests/integration.rs` (capture-then-recapture returns the same id)

**Interfaces:**
- Produces: `pub fn find_content_duplicate(conn: &Connection, namespace: &str, content: &str) -> Result<Option<String>>` — returns the id of a non-archived memory in `namespace` whose `content` matches exactly, if any.
- Consumes: existing `repository::remember`, `CaptureResult`.

- [ ] **Step 1: Write the failing test for `find_content_duplicate`**

```rust
#[test]
fn find_content_duplicate_matches_same_namespace_only() {
    let conn = test_db(); // existing helper
    let m = remember_simple(&conn, "proj:a", "the sky is blue");
    assert_eq!(
        find_content_duplicate(&conn, "proj:a", "the sky is blue").unwrap(),
        Some(m.id.clone())
    );
    assert_eq!(find_content_duplicate(&conn, "proj:b", "the sky is blue").unwrap(), None);
    assert_eq!(find_content_duplicate(&conn, "proj:a", "different").unwrap(), None);
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p clio-core find_content_duplicate`
Expected: FAIL — function not found.

- [ ] **Step 3: Implement `find_content_duplicate`**

```rust
/// Return the id of a non-archived memory in `namespace` with identical
/// content, if one exists. Used to suppress duplicate writes on the capture
/// and review-approval paths.
pub fn find_content_duplicate(
    conn: &Connection,
    namespace: &str,
    content: &str,
) -> Result<Option<String>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM memories
             WHERE namespace = ?1 AND content = ?2 AND archived_at IS NULL
             ORDER BY created_at ASC LIMIT 1",
            rusqlite::params![namespace, content],
            |row| row.get(0),
        )
        .optional()?;
    Ok(id)
}
```

- [ ] **Step 4: Run the test — expect PASS**

Run: `cargo test -p clio-core find_content_duplicate`
Expected: PASS.

- [ ] **Step 5: Write the failing capture-dedup test**

```rust
#[test]
fn recapture_of_identical_content_does_not_duplicate() {
    let conn = test_db();
    let first = store_or_queue_test(&conn, "proj:a", "X is configured via env");
    let second = store_or_queue_test(&conn, "proj:a", "X is configured via env");
    assert_eq!(first_id(&first), second_id(&second)); // same memory id, no new row
    assert_eq!(count_memories(&conn, "proj:a"), 1);
}
```

- [ ] **Step 6: Run it and watch it fail**

Run: `cargo test -p clio-core recapture_of_identical_content`
Expected: FAIL — two rows created.

- [ ] **Step 7: Consult dedup in `store_or_queue` before insert**

In `capture.rs`, just before building `RememberInput` (`capture.rs:603`):

```rust
if let Some(existing_id) = crate::repository::find_content_duplicate(conn, namespace, content)? {
    let memory = crate::repository::get(conn, &existing_id)?;
    return Ok(CaptureResult::Stored(memory)); // or a new CaptureResult::Duplicate(memory) variant
}
```

> If signalling "this was a duplicate" matters to callers, add a `CaptureResult::Duplicate(Memory)` variant and handle it in the MCP/CLI formatters. Otherwise returning `Stored` is the minimal change. Confirm `repository::get` exists with this signature; adjust to the real getter.

- [ ] **Step 8: Apply the same guard in `approve_review`**

In `review.rs`, before building `RememberInput` (`review.rs:186`), check `find_content_duplicate(conn, &item.suggested_namespace, &item.content)`; if found, mark the review row approved and return the existing memory instead of inserting.

- [ ] **Step 9: Run the full suite — expect PASS**

Run: `cargo test -p clio-core`
Expected: PASS, including the new dedup tests; existing capture/review tests still green.

- [ ] **Step 10: Commit**

```bash
gitaddall
git commit -m "feat(core): suppress duplicate writes on capture and review-approval

Add repository::find_content_duplicate (exact content, same namespace, non-archived)
and consult it in store_or_queue and approve_review before inserting, so re-capturing
a known fact returns the existing memory instead of creating a row."
```

**Follow-up (note in commit/issue):** embedding-cosine near-dup detection (≥ ~0.9) on write; namespace-scoped `suggest_links`; an `auto:supersedes` lifecycle for contradicting facts.

---

## Task 3: Enforce `valid_until` in recall

**Why:** `valid_until` is stored and round-tripped (`repository.rs:591`) but consulted in no recall/scoring path — `append_filters` (`repository.rs:755-821`) has no temporal clause. Memories explicitly marked expired rank as current. This task adds an opt-in expiry filter so callers can exclude expired memories without changing default behaviour.

**Files:**
- Modify: `crates/clio-core/src/models.rs:117-147` (`RecallQuery` gains `exclude_expired`)
- Modify: `crates/clio-core/src/repository.rs:755-821` (`append_filters` expiry clause)
- Modify: `crates/clio-core/src/embeddings.rs` (`semantic_search` SQL — it does not use `append_filters`)
- Test: `tests/integration.rs` (expired memory excluded when flag set, present when not)

**Interfaces:**
- Produces: `RecallQuery.exclude_expired: bool` (`#[serde(default)]` → `false`, backwards-compatible).
- Consumes: existing `append_filters`, `semantic_search`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn exclude_expired_filters_past_valid_until() {
    let conn = test_db();
    remember_with_valid_until(&conn, "proj:a", "stale fact", Some("2000-01-01T00:00:00Z"));
    remember_with_valid_until(&conn, "proj:a", "live fact", None);

    let mut q = RecallQuery { namespace: Some("proj:a".into()), ..Default::default() };
    q.exclude_expired = false;
    assert_eq!(recall(&conn, &q).unwrap().total, 2); // default: both visible

    q.exclude_expired = true;
    let titles = recall(&conn, &q).unwrap().items;
    assert_eq!(titles.len(), 1);
    assert!(titles[0].memory.content.contains("live"));
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p clio-core exclude_expired`
Expected: FAIL — no `exclude_expired` field.

- [ ] **Step 3: Add the field to `RecallQuery`**

In `models.rs`, add after `include_archived`:

```rust
    /// When true, exclude memories whose `valid_until` is in the past.
    /// Defaults to false for backwards-compatible recall.
    #[serde(default)]
    pub exclude_expired: bool,
```

Add `exclude_expired: false` to the `Default` impl (`models.rs:157+`).

- [ ] **Step 4: Add the clause to `append_filters`**

In `repository.rs`, inside `append_filters` (after the archived clause, `repository.rs:762`):

```rust
    if q.exclude_expired {
        sql.push_str(" AND (m.valid_until IS NULL OR m.valid_until > datetime('now'))");
    }
```

(No bound param needed — `datetime('now')` is evaluated by SQLite. Confirm `valid_until` is stored as ISO-8601 text comparable with `datetime('now')`; the schema stores timestamps as text, so lexical/`datetime` comparison holds.)

- [ ] **Step 5: Add the matching clause to `semantic_search`**

`semantic_search` builds its own SQL and does not call `append_filters`. Add the same `valid_until` predicate to its WHERE when the caller requests expiry filtering. Thread an `exclude_expired: bool` argument through `semantic_search`/`semantic_recall` (default-false at call sites that don't care).

- [ ] **Step 6: Run the test — expect PASS**

Run: `cargo test -p clio-core exclude_expired`
Expected: PASS.

- [ ] **Step 7: Run the full suite**

Run: `cargo test -p clio-core`
Expected: PASS, no regressions (default path unchanged — flag is opt-in).

- [ ] **Step 8: Commit**

```bash
gitaddall
git commit -m "feat(core): optional valid_until expiry filter in recall

Add RecallQuery.exclude_expired (default false) and apply a valid_until predicate
in append_filters and semantic_search so callers can exclude expired memories
without changing default recall behaviour."
```

**Follow-up (note in commit/issue):** a daemon sweep that auto-archives long-expired memories; surfacing the flag through MCP/CLI once defaults are decided (keep MCP/CLI/core in lockstep per the invariant).

---

## Self-Review Notes

- **Spec coverage:** All three highest-leverage items from the investigation are covered — ranker unification (Task 1), write-path dedup (Task 2), `valid_until` enforcement (Task 3). The investigation's larger bets (RRF fusion, decay-clock fix, `supersedes` lifecycle, sqlite-vec ANN) are explicitly deferred as follow-ups, not silently dropped.
- **Invariant guards:** Task 1's helper returns `1.0` when `decay_lambda <= 0.0` (preserves original ranking); Task 2 routes through existing `remember`/upsert rather than around it; Task 3 is opt-in and default-false.
- **Open decisions flagged inline:** the decay-clock basis (Task 1 option a/b), whether to add a `CaptureResult::Duplicate` variant (Task 2 Step 7), and timestamp comparability for `valid_until` (Task 3 Step 4). Each is called out where the implementer hits it.
- **Type-consistency caveat:** the test helper constructors (`Memory::default()`, `test_db()`, `remember_simple`, etc.) are illustrative — confirm against the real `models.rs`/test harness in `tests/integration.rs` before writing, and adjust field names to match. The production code sketches use verified signatures from the current source.
