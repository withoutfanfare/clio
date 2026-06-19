# Clio Revival — Phase 0 + Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the in-flight work cleanly, then fix the confirmed data-integrity bugs in `clio-core` (merge tag-loss, FTS multi-term search, char-based validation, WAL-safe backup/restore, access-count inflation during dedup) — each with a regression test.

**Architecture:** All fixes live in `clio-core` (the only place business logic belongs). Phase 0 is git hygiene (no code authored). Phase 1 is test-driven: every bug gets a failing test in `crates/clio-core/tests/integration.rs` first, then the minimal fix, then a green run and a commit. No schema changes, so no new migrations.

**Tech Stack:** Rust 2024 edition (rustc 1.94, floor 1.85), `rusqlite` 0.34 with bundled SQLite + FTS5, `tempfile` 3 (dev-dependency, already present), Vue 3 + Vite for the UI side of Phase 0.

## Global Constraints

- British English in all comments, docs, and user-facing text.
- Conventional Commits. Stage with the `gitaddall` alias (direct `git add` is blocked by a hook); use `git restore --staged <path>` to narrow a commit.
- Commit messages must NOT mention "Claude", "Claude Code", or "AI" (user's global rule).
- Never edit an applied migration — Phase 1 introduces no schema changes.
- All business logic stays in `clio-core`; adapters remain thin.
- Run `cargo test -p clio-core` and `cargo clippy -p clio-core` green before each commit in Phase 1.
- `get_raw` is `pub(crate)` in `repository.rs` — usable from `deduplication.rs` (same crate) but not from outside.
- `repository::remember` normalises tags to lowercase and de-duplicates them.
- `crate::models::now_utc() -> String` returns an RFC3339 timestamp.

---

## Phase 0 — Clean the tree

No code authored here — these tasks land already-assessed in-flight work as separate logical commits so Phase 1 starts from a clean, current base. The design spec
(`docs/superpowers/specs/2026-06-19-clio-revival-design.md`) is already committed.

### Task 0a: Commit the documentation polish

**Files:**
- Modify (commit): `README.md`, `context/ARCHITECTURE.md`, `crates/clio-tauri/ROADMAP_LOG.md`, `docs/README.md`, `docs/cli-reference.md`, `docs/getting-started.md`, `docs/mcp-agent-setup.md`, `docs/reference/mcp-contract.md`, `docs/reference/schema.md`, `docs/tauri-app.md`, `CLAUDE.md`

These 11 files form one coherent docs-polish pass (diagrams, troubleshooting section, cross-linking). Assessed as complete and additive. Both files referenced by the new `docs/README.md` (`docs/performance-audit.md`, `docs/guides/moving-project.md`) were confirmed to exist — no dead links. `CLAUDE.md` carries the intentional build-command fixes from this session.

> Note: `docs/cli-reference.md` still contains the 6 inaccurate commands flagged in the audit. Those are corrected later in **Phase 4 (Documentation accuracy)** — committing the current cross-linking pass now does not block that; it just captures work already done.

- [ ] **Step 1: Stage everything, then unstage the two Vue files (they are a separate concern)**

```bash
gitaddall
git restore --staged ui/src/components/MemoryDrawer.vue ui/src/components/MemoryPage.vue
```

- [ ] **Step 2: Verify only docs + CLAUDE.md are staged**

Run: `git status --short`
Expected: the 11 doc files + `CLAUDE.md` show as staged (`M ` in the left column); `ui/src/components/MemoryDrawer.vue` and `ui/src/components/MemoryPage.vue` show as unstaged (` M`).

- [ ] **Step 3: Commit**

```bash
git commit -m "docs: add diagrams, troubleshooting, and cross-linking across docs"
```

### Task 0b: Commit the UI tweaks

**Files:**
- Modify (commit): `ui/src/components/MemoryDrawer.vue` (delete-confirmation bar), `ui/src/components/MemoryPage.vue` (`.mode-list` layout fix)

Assessed as coherent and self-contained; would compile. Different concern from the docs, so a separate commit.

- [ ] **Step 1: Stage the remaining changes**

```bash
gitaddall
```

- [ ] **Step 2: Verify only the two Vue files are staged**

Run: `git status --short`
Expected: only `ui/src/components/MemoryDrawer.vue` and `ui/src/components/MemoryPage.vue` staged. Working tree otherwise clean.

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(ui): explicit delete-confirmation bar and list layout fix"
```

### Task 0c: Integrate the feature branch

**Files:**
- Branch: `feature/virtualisation-namespace-colours-quickswitch` (virtual scrolling, namespace colours, quick-switch dropdown) — net-new, no duplication on `develop`.

- [ ] **Step 1: Rebase the branch onto the now-updated develop**

```bash
git switch feature/virtualisation-namespace-colours-quickswitch
git rebase develop
```
Resolve the one expected overlap in `ui/src/components/MemoryPage.vue` `<style>` block (the branch edits `.memory-page`; Task 0b touched `.mode-list` nearby). Keep both changes.

- [ ] **Step 2: Verify the UI builds**

Run: `cd ui && npm run build`
Expected: `vue-tsc --noEmit` passes, `vite build` succeeds. (Requires the local Verdaccio registry at `localhost:4873` for `@stuntrocket/ui`; if it is offline, bring it up first.)

- [ ] **Step 3: Manual scroll smoke-test**

Run the Tauri app (`./dev.sh`). Confirm: a large memory list scrolls smoothly, keyboard navigation reaches off-screen rows, and switching namespace via the quick-switch dropdown works. Watch for row-height drift (the virtual list hardcodes `LIST_CARD_HEIGHT=120` / `GRID_CARD_HEIGHT=180`).

- [ ] **Step 4: Merge back to develop**

```bash
git switch develop
git merge --no-ff feature/virtualisation-namespace-colours-quickswitch
```
Expected: clean merge commit; working tree clean; `git status` shows nothing to commit.

---

## Phase 1 — Data integrity (TDD)

All tasks add tests to `crates/clio-core/tests/integration.rs`. First, add the shared test helpers used across the new tests.

### Task 1.0: Add shared test helpers

**Files:**
- Modify: `crates/clio-core/tests/integration.rs` (append helpers near the existing `test_db` at the top, after line 10)

**Interfaces:**
- Produces: `base_input(content: &str) -> RememberInput`, `remember_simple(conn, content: &str) -> Memory`, `remember_with_tags(conn, content: &str, tags: &[&str]) -> Memory`, `remember_in(conn, namespace: &str, content: &str) -> Memory` — used by every later Phase 1 task.

- [ ] **Step 1: Add the helpers**

```rust
fn base_input(content: &str) -> RememberInput {
    RememberInput {
        namespace: "global".into(),
        kind: "note".into(),
        title: None,
        summary: None,
        content: content.into(),
        tags: vec![],
        source: None,
        source_ref: None,
        confidence: None,
        importance: 3,
        metadata: serde_json::json!({}),
        valid_from: None,
        valid_until: None,
        upsert: false,
    }
}

fn remember_simple(conn: &rusqlite::Connection, content: &str) -> Memory {
    repository::remember(conn, &base_input(content), &Settings::default()).unwrap()
}

fn remember_with_tags(conn: &rusqlite::Connection, content: &str, tags: &[&str]) -> Memory {
    let input = RememberInput {
        tags: tags.iter().map(|t| t.to_string()).collect(),
        ..base_input(content)
    };
    repository::remember(conn, &input, &Settings::default()).unwrap()
}

fn remember_in(conn: &rusqlite::Connection, namespace: &str, content: &str) -> Memory {
    let input = RememberInput { namespace: namespace.into(), ..base_input(content) };
    repository::remember(conn, &input, &Settings::default()).unwrap()
}
```

- [ ] **Step 2: Verify the crate still compiles (helpers may be unused until later tasks — that is fine for now)**

Run: `cargo test -p clio-core --test integration -- --list 2>&1 | tail -5`
Expected: compiles; existing tests listed. (Unused-helper warnings are acceptable at this step; later tasks consume them.)

- [ ] **Step 3: Commit**

```bash
gitaddall
git commit -m "test: add shared remember helpers for clio-core integration tests"
```

### Task 1.1: Fix merge tag-loss and make merge atomic

**Files:**
- Modify: `crates/clio-core/src/deduplication.rs:187-244` (the mutation section + return of `merge_memories`)
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Consumes: `remember_with_tags` (Task 1.0), `clio_core::deduplication::merge_memories(conn, keep_id: &str, merge_ids: &[String]) -> Result<Memory>`

**Root cause:** `memory_tags.created_at` is `NOT NULL` (migrations.rs:45), but the merge re-inserts tags with `INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)` — omitting `created_at`. `INSERT OR IGNORE` *silently skips* the NOT NULL violation, so after a merge the kept memory's `memory_tags` rows are deleted and never replaced (tags survive only in the denormalised `tags_text`). The whole mutation also runs without a savepoint.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn merge_retains_tags_in_memory_tags_table() {
    let conn = test_db();
    let keep = remember_with_tags(&conn, "Primary content about rust", &["alpha", "beta"]);
    let dup = remember_with_tags(&conn, "Duplicate content about rust", &["beta", "gamma"]);

    clio_core::deduplication::merge_memories(&conn, &keep.id, &[dup.id.clone()]).unwrap();

    // Regression: the normalised memory_tags rows for the kept memory were silently
    // dropped because the re-insert omitted the NOT NULL created_at column.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_tags WHERE memory_id = ?1",
            [&keep.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3, "kept memory should hold the union of tags (alpha, beta, gamma)");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p clio-core --test integration merge_retains_tags_in_memory_tags_table`
Expected: FAIL — `assertion failed: left == right`, left `0`, right `3` (tags silently dropped).

- [ ] **Step 3: Rewrite the mutation section of `merge_memories` to add `created_at` and a savepoint**

Replace lines 187-244 (from the `// Update the kept memory with merged metadata.` comment through the final `repository::get(conn, keep_id)`) with:

```rust
    // Update the kept memory with merged metadata.
    let tags_text = all_tags.join(" ");
    let now = crate::models::now_utc();

    // Wrap the whole mutation in a savepoint so a mid-merge failure rolls back
    // cleanly instead of leaving the kept memory half-mutated.
    conn.execute_batch("SAVEPOINT merge_memories")?;
    let result = (|| -> Result<()> {
        conn.execute(
            "UPDATE memories SET tags_text = ?1, confidence = ?2, importance = ?3, updated_at = ?4
             WHERE id = ?5",
            params![tags_text, best_confidence, best_importance, now, keep_id],
        )?;

        // Sync tags in the memory_tags table (created_at is NOT NULL).
        conn.execute("DELETE FROM memory_tags WHERE memory_id = ?1", params![keep_id])?;
        for tag in &all_tags {
            conn.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag, created_at) VALUES (?1, ?2, ?3)",
                params![keep_id, tag, now],
            )?;
        }

        // Transfer links from merged memories to the kept memory.
        for merge_id in merge_ids {
            conn.execute(
                "UPDATE OR IGNORE memory_links SET from_memory_id = ?1
                 WHERE from_memory_id = ?2 AND to_memory_id != ?1",
                params![keep_id, merge_id],
            )?;
            conn.execute(
                "UPDATE OR IGNORE memory_links SET to_memory_id = ?1
                 WHERE to_memory_id = ?2 AND from_memory_id != ?1",
                params![keep_id, merge_id],
            )?;
            conn.execute(
                "DELETE FROM memory_links WHERE from_memory_id = ?1 OR to_memory_id = ?1",
                params![merge_id],
            )?;
            conn.execute(
                "DELETE FROM memory_links WHERE from_memory_id = ?1 AND to_memory_id = ?1",
                params![keep_id],
            )?;

            // Archive the merged-away memory.
            repository::archive(conn, merge_id)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => conn.execute_batch("RELEASE merge_memories")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO merge_memories");
            let _ = conn.execute_batch("RELEASE merge_memories");
            return Err(e);
        }
    }

    // Return the updated kept memory without inflating its access_count.
    repository::get_raw(conn, keep_id)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p clio-core --test integration merge_retains_tags_in_memory_tags_table`
Expected: PASS.

- [ ] **Step 5: Run the full core test suite + clippy**

Run: `cargo test -p clio-core && cargo clippy -p clio-core`
Expected: all tests pass; no new clippy warnings.

- [ ] **Step 6: Commit**

```bash
gitaddall
git commit -m "fix(core): retain tags and ensure atomicity when merging memories"
```

### Task 1.2: Fix FTS multi-term search

**Files:**
- Modify: `crates/clio-core/src/repository.rs:530-536` (`sanitise_fts_query`)
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Consumes: `remember_simple` (Task 1.0), `repository::recall(conn, &RecallQuery) -> Result<RecallResult>`

**Root cause:** `sanitise_fts_query` wraps the *entire* query in one pair of quotes, forcing a single literal phrase match. So `"rust sqlite"` only matches documents where those words are adjacent, not documents containing both — silently degrading recall. The fix mirrors `deduplication::build_fts_query`: quote each term individually and join with spaces (FTS5 implicit AND).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn recall_multi_term_matches_documents_containing_all_terms() {
    let conn = test_db();
    remember_simple(&conn, "We use rust together with sqlite for storage");
    remember_simple(&conn, "Unrelated python notes about pandas");

    let q = RecallQuery {
        query: Some("rust sqlite".into()),
        ..Default::default()
    };
    let res = repository::recall(&conn, &q).unwrap();

    // Both terms appear in the first doc but are not adjacent; multi-term AND must match it.
    assert_eq!(res.count, 1, "multi-term query should match the doc containing both terms");
    assert!(res.items[0].memory.content.contains("rust"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p clio-core --test integration recall_multi_term_matches_documents_containing_all_terms`
Expected: FAIL — `res.count` is `0` (no adjacent phrase match), expected `1`.

- [ ] **Step 3: Rewrite `sanitise_fts_query`**

Replace lines 530-536 with:

```rust
/// Sanitise a user-supplied FTS query. Each whitespace-separated term is quoted
/// individually to neutralise FTS operators (`NOT`, `OR`, column filters) while
/// joining with spaces so multi-term queries match documents containing all terms
/// (FTS5 implicit AND), preserving BM25 ranking.
fn sanitise_fts_query(raw: &str) -> String {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|t| t.replace('"', " "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect();

    if terms.is_empty() {
        // Preserve previous behaviour for an empty query: a quoted empty phrase
        // (matches nothing rather than raising an FTS syntax error).
        return "\"\"".to_string();
    }
    terms.join(" ")
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p clio-core --test integration recall_multi_term_matches_documents_containing_all_terms`
Expected: PASS.

- [ ] **Step 5: Run the full core test suite + clippy**

Run: `cargo test -p clio-core && cargo clippy -p clio-core`
Expected: all pass. (Existing single-term recall tests still pass — a single term yields one quoted phrase, identical to before.)

- [ ] **Step 6: Commit**

```bash
gitaddall
git commit -m "fix(core): match all terms in multi-word FTS recall queries"
```

### Task 1.3: Validate character-defined limits by characters, not bytes

**Files:**
- Modify: `crates/clio-core/src/validate.rs:31,41,48,56,100` (namespace, kind, title, summary, tag length checks)
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Consumes: `base_input` (Task 1.0), `repository::remember`

**Root cause:** `validate::remember_input` uses `str::len()` (bytes) for limits the schema defines in characters (`CHECK (length(namespace) BETWEEN 1 AND 120)` etc.; SQLite `length()` counts characters). Multi-byte content can pass the DB CHECK but be wrongly rejected by app validation. Content (1 MiB) and metadata (64 KiB) limits stay byte-based — they are intentionally byte limits with no schema CHECK.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn validates_namespace_length_by_characters_not_bytes() {
    let conn = test_db();
    // 120 two-byte characters = 240 bytes, but 120 chars — valid by the schema's
    // character-based CHECK constraint.
    let namespace = "é".repeat(120);
    let input = RememberInput { namespace, ..base_input("multibyte namespace content") };

    let result = repository::remember(&conn, &input, &Settings::default());
    assert!(result.is_ok(), "a 120-character namespace must pass character-based validation");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p clio-core --test integration validates_namespace_length_by_characters_not_bytes`
Expected: FAIL — `remember` returns `Err(Validation("namespace must be at most 120 characters."))` because `len()` is 240 bytes.

- [ ] **Step 3: Switch the five character-defined checks to `chars().count()`**

In `crates/clio-core/src/validate.rs`, change each comparison:

Line 31: `if input.namespace.len() > 120 {` → `if input.namespace.chars().count() > 120 {`

Line 41: `if input.kind.len() > 50 {` → `if input.kind.chars().count() > 50 {`

Line 48: `if title.len() > 240 {` → `if title.chars().count() > 240 {`

Line 56: `if summary.len() > 1000 {` → `if summary.chars().count() > 1000 {`

Line 100: `if trimmed.is_empty() || trimmed.len() > 60 {` → `if trimmed.is_empty() || trimmed.chars().count() > 60 {`

(Leave the empty-string checks and the byte-based `MAX_CONTENT_BYTES` / `MAX_METADATA_BYTES` checks unchanged.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p clio-core --test integration validates_namespace_length_by_characters_not_bytes`
Expected: PASS.

- [ ] **Step 5: Run the full core test suite + clippy**

Run: `cargo test -p clio-core && cargo clippy -p clio-core`
Expected: all pass (existing ASCII-based validation tests unaffected — for ASCII, byte and char counts are equal).

- [ ] **Step 6: Commit**

```bash
gitaddall
git commit -m "fix(core): validate namespace/kind/title/summary/tag length by characters"
```

### Task 1.4: WAL-safe backup via VACUUM INTO + pre-restore safety snapshot

**Files:**
- Modify: `crates/clio-core/src/backup.rs:70-86` (`backup`) and `crates/clio-core/src/backup.rs:173-194` (`restore`)
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Consumes: `clio_core::db::open(path) -> Result<Connection>`, `base_input` (Task 1.0), `clio_core::backup::backup(db_path, dest_dir, max_backups) -> Result<BackupResult>`, `clio_core::backup::restore(db_path, backup_path) -> Result<RestoreResult>`
- Uses dev-dependency `tempfile` (already in `Cargo.toml`).

**Root cause:** `backup` does `std::fs::copy` of the `.db` then racily copies `-wal`/`-shm`. In WAL mode (default pragma), recent writes may live only in the `-wal`, so the copied `.db` can be torn/incomplete, and copying a WAL tied to a different DB header risks corruption. `VACUUM INTO` produces a transactionally-consistent standalone snapshot with no sidecar files. For `restore`, add a safety snapshot of the live DB before overwriting.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn backup_produces_standalone_snapshot_without_wal() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("memory.db");
    let conn = clio_core::db::open(&db_path).unwrap();
    repository::remember(&conn, &base_input("back me up"), &Settings::default()).unwrap();

    let dest = dir.path().join("backups");
    let res = clio_core::backup::backup(&db_path, Some(&dest), 5).unwrap();
    let backup_path = std::path::Path::new(&res.path);

    // The snapshot must be a complete, standalone DB needing no WAL sidecar.
    assert!(backup_path.exists(), "backup file should exist");
    assert!(
        !backup_path.with_extension("db-wal").exists(),
        "VACUUM INTO snapshot must not carry a -wal sidecar"
    );
    let bconn = rusqlite::Connection::open(backup_path).unwrap();
    let n: i64 = bconn
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "the snapshot must contain the row, even if it was still in the WAL");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p clio-core --test integration backup_produces_standalone_snapshot_without_wal`
Expected: FAIL — the current code copies the live `-wal` (so `db-wal` exists), tripping the `!...exists()` assertion (and the row may be missing from the bare `.db`).

- [ ] **Step 3: Replace the copy logic in `backup` with `VACUUM INTO`**

Replace lines 70-86 (from `// Copy the database file` through the WAL/SHM copy block) with:

```rust
    // Produce a transactionally-consistent, standalone snapshot. VACUUM INTO writes
    // a complete database with no -wal/-shm sidecars, avoiding torn-copy corruption
    // that plain file copies risk under WAL mode.
    let _ = std::fs::remove_file(&backup_path); // VACUUM INTO requires the target not exist
    let src = rusqlite::Connection::open(db_path).map_err(|e| {
        ClioError::Export(format!("could not open database for backup: {e}"))
    })?;
    src.execute(
        "VACUUM INTO ?1",
        rusqlite::params![backup_path.to_string_lossy()],
    )
    .map_err(|e| ClioError::Export(format!("backup VACUUM INTO failed: {e}")))?;
```

- [ ] **Step 4: Run the backup test to verify it passes**

Run: `cargo test -p clio-core --test integration backup_produces_standalone_snapshot_without_wal`
Expected: PASS.

- [ ] **Step 5: Write the restore safety test**

```rust
#[test]
fn restore_creates_pre_restore_safety_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("memory.db");
    let conn = clio_core::db::open(&db_path).unwrap();
    repository::remember(&conn, &base_input("original row"), &Settings::default()).unwrap();

    let dest = dir.path().join("backups");
    let res = clio_core::backup::backup(&db_path, Some(&dest), 5).unwrap();

    // Change the live DB after the backup, then restore.
    repository::remember(&conn, &base_input("added after backup"), &Settings::default()).unwrap();
    drop(conn);

    let r = clio_core::backup::restore(&db_path, std::path::Path::new(&res.path)).unwrap();
    assert!(r.integrity_ok);

    // A safety snapshot of the pre-restore live DB must be written.
    assert!(
        db_path.with_extension("db.pre-restore").exists(),
        "restore should snapshot the live DB before overwriting it"
    );

    // The restored DB reflects the backup (1 row) and leaves no stale WAL.
    assert!(!db_path.with_extension("db-wal").exists());
    let conn2 = clio_core::db::open(&db_path).unwrap();
    let n: i64 = conn2
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "restored DB should match the backup, not the post-backup state");
}
```

- [ ] **Step 6: Run the restore test to verify it fails**

Run: `cargo test -p clio-core --test integration restore_creates_pre_restore_safety_snapshot`
Expected: FAIL — no `db.pre-restore` file is created by the current `restore`.

- [ ] **Step 7: Add the safety snapshot to `restore`**

In `restore`, immediately before the `// Replace the current database` comment (currently line 173), insert:

```rust
    // Safety: snapshot the current live DB before overwriting, so a bad restore is
    // recoverable. Best-effort — failure here must not block a valid restore.
    if db_path.exists() {
        let safety = db_path.with_extension("db.pre-restore");
        let _ = std::fs::remove_file(&safety);
        if let Ok(live) = rusqlite::Connection::open(db_path) {
            let _ = live.execute(
                "VACUUM INTO ?1",
                rusqlite::params![safety.to_string_lossy()],
            );
        }
    }
```

(The existing else-branches at lines 186-193 already remove any stale live `-wal`/`-shm` when the backup has none — correct now that backups are WAL-free. Leave them.)

- [ ] **Step 8: Run both backup/restore tests to verify they pass**

Run: `cargo test -p clio-core --test integration restore_creates_pre_restore_safety_snapshot backup_produces_standalone_snapshot_without_wal`
Expected: both PASS.

- [ ] **Step 9: Run the full core test suite + clippy**

Run: `cargo test -p clio-core && cargo clippy -p clio-core`
Expected: all pass.

- [ ] **Step 10: Commit**

```bash
gitaddall
git commit -m "fix(core): take WAL-safe backups and snapshot before restore"
```

### Task 1.5: Stop dedup scans inflating access_count

**Files:**
- Modify: `crates/clio-core/src/deduplication.rs` lines 103, 109, 162, 169, 268, 417 (`repository::get` → `repository::get_raw`)
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Consumes: `remember_simple` (Task 1.0), `clio_core::deduplication::merge_memories`

**Root cause:** `preview_merge`, `merge_memories`, `find_exact_duplicates`, and `find_near_duplicates` load memories via `repository::get`, which fires access tracking (a write that bumps `access_count`). These are read-only maintenance scans; they should use `get_raw`. (Task 1.1 already switched the *return* of `merge_memories` to `get_raw`; this task fixes the remaining internal reads.)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn merge_does_not_inflate_access_count() {
    let conn = test_db();
    let keep = remember_simple(&conn, "keep this memory");
    let dup = remember_simple(&conn, "duplicate memory");

    clio_core::deduplication::merge_memories(&conn, &keep.id, &[dup.id.clone()]).unwrap();

    let access_count: i64 = conn
        .query_row(
            "SELECT access_count FROM memories WHERE id = ?1",
            [&keep.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(access_count, 0, "a merge is maintenance and must not bump access_count");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p clio-core --test integration merge_does_not_inflate_access_count`
Expected: FAIL — `access_count` is `1` (the `let keep = repository::get(...)` at line 162 bumped it), expected `0`.

- [ ] **Step 3: Switch the six internal reads to `get_raw`**

In `crates/clio-core/src/deduplication.rs`, replace `repository::get(conn, ...)` with `repository::get_raw(conn, ...)` at these call sites:

- Line 103: `let keep = repository::get(conn, keep_id)?;` (in `preview_merge`)
- Line 109: `let mem = repository::get(conn, merge_id)?;` (in `preview_merge`)
- Line 162: `let keep = repository::get(conn, keep_id)?;` (in `merge_memories`)
- Line 169: `let mem = repository::get(conn, merge_id)?;` (in `merge_memories`)
- Line 268: `if let Ok(mem) = repository::get(conn, id) {` (in `find_exact_duplicates`)
- Line 417: `if let Ok(mem) = repository::get(conn, id) {` (in `find_near_duplicates`)

Each becomes the same call with `get_raw`, e.g. `let keep = repository::get_raw(conn, keep_id)?;`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p clio-core --test integration merge_does_not_inflate_access_count`
Expected: PASS.

- [ ] **Step 5: Run the full core test suite + clippy**

Run: `cargo test -p clio-core && cargo clippy -p clio-core`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
gitaddall
git commit -m "fix(core): use get_raw in dedup scans to avoid inflating access_count"
```

### Task 1.6: Pin recall_scoped totals (characterisation) + document offset limitation

**Files:**
- Modify: `crates/clio-core/src/repository.rs:779-787` (doc comment on `recall_scoped`)
- Test: `crates/clio-core/tests/integration.rs`

**Interfaces:**
- Consumes: `remember_in` (Task 1.0), `repository::recall_scoped(conn, &RecallQuery, detected_namespace: &str) -> Result<RecallResult>`

**Note — not a red/green TDD task.** Audit finding #5 (recall_scoped "double-counts" the total) is a **false positive**: the scoped pass filters `namespace = detected_namespace` and the global pass filters `namespace = "global"`, which are disjoint, so `scoped.total + global.total` counts each memory once. This task adds a regression test that *passes on current code* to lock that in, and documents the genuine limitation (the global pass uses `offset: 0`, so paging with `offset > 0` is not meaningful across the combined set).

- [ ] **Step 1: Write the characterisation test (expected to pass immediately)**

```rust
#[test]
fn recall_scoped_total_counts_each_namespace_once() {
    let conn = test_db();
    // Two matches in the project namespace, one in global — all disjoint by namespace.
    remember_in(&conn, "proj", "alpha one note");
    remember_in(&conn, "proj", "alpha two note");
    remember_in(&conn, "global", "alpha three note");

    // limit high enough that the scoped pass does not satisfy it alone, exercising the merge.
    let q = RecallQuery {
        query: Some("alpha".into()),
        limit: 5,
        ..Default::default()
    };
    let res = repository::recall_scoped(&conn, &q, "proj").unwrap();

    assert_eq!(res.count, 3, "should merge 2 project + 1 global match");
    assert_eq!(res.total, 3, "disjoint namespaces — each counted once, no double count");
}
```

- [ ] **Step 2: Run the test (verify it already passes — confirms the audit finding was a false positive)**

Run: `cargo test -p clio-core --test integration recall_scoped_total_counts_each_namespace_once`
Expected: PASS. (If it fails, stop and investigate — that would mean a real double-count bug after all.)

- [ ] **Step 3: Document the offset limitation on `recall_scoped`**

Replace the existing doc comment / signature lead-in at lines 779-783 with a doc comment that records the limitation, keeping the signature identical:

```rust
/// Recall within the detected namespace, then fill any remaining slots from `global`.
///
/// The two passes query disjoint namespaces, so `total` is the sum of both passes
/// (each memory counted once). Note: the global fill always starts at `offset: 0`,
/// so this convenience path is intended for first-page recall — paging with
/// `offset > 0` does not page meaningfully across the combined result set.
pub fn recall_scoped(
    conn: &Connection,
    query: &RecallQuery,
    detected_namespace: &str,
) -> Result<RecallResult> {
```

- [ ] **Step 4: Run the full core test suite + clippy**

Run: `cargo test -p clio-core && cargo clippy -p clio-core`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
gitaddall
git commit -m "test(core): pin recall_scoped totals and document offset limitation"
```

---

## Phase 1 wrap-up

- [ ] **Final verification: full workspace build + tests**

Run: `cargo build && cargo test`
Expected: workspace builds; all tests pass. (Attempt the Tauri build too — `cargo build -p clio-tauri`; if it fails on system/display deps in this environment, record the error and defer Tauri-side verification to a manual run via `./dev.sh`.)

---

## Deferred to later phases / backlog (NOT in this plan)

Recorded so nothing is silently dropped:

- **migrate `duplicates` counter is always 0** (`migrate.rs:208`) — `store_entry` never returns `Conflict` on the upsert path; cosmetic stats only. Fix in a later docs/cleanup pass.
- **`auto_link` `has_embedding` N+1** (`embeddings.rs:822,876`) — background daemon batch op in feature-gated embedding code; needs a backend to test. Defer to a performance pass.
- **`archive`/`unarchive` bump `updated_at`** (`repository.rs:864,881`) — corrupts the activity feed / "updated" classification; the proper fix needs a new `archived_at`-aware ordering or schema migration. Deferred (Phase: schema).
- **Phases 2-6** — MCP/agent correctness (`review_id`, capture shape, Codex TOML), UI robustness, documentation accuracy (the cli-reference rewrite), dependency currency, and rollout across the five agents — each gets its own plan, per the design spec.
