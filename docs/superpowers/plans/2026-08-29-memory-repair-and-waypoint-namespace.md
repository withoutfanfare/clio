# Memory Repair and Waypoint Namespace Implementation Plan

> **Status:** local implementation, verification and private Atlas rehearsal are
> complete. Atlas schema or data mutation remains behind a separate explicit
> apply gate.

**Goal:** Add a fail-closed, atomic and reversible Clio repair path for the
audited namespace/archive corrections, then stop Waypoint from recreating
repository-slug namespaces or leaving retired worktree projections active.

**Architecture:** `clio-core` owns manifest construction, compare-and-swap
validation, transactional mutation and rollback journalling. The CLI owns
private-file I/O, backups and operator confirmations. Waypoint receives a
canonical namespace from project context; its repository slug remains a tag.

**Constraints:** archive rather than delete; preserve semantic timestamps and
all non-target fields; never remove human-authored links; journal the rollback
state in the same transaction as the mutation; never let a stale or incomplete
manifest partially apply; do not touch Atlas during implementation.

---

## Task 1: Add the immutable repair schema and store generation

**Files:**

- Modify: `crates/clio-core/src/migrations.rs`
- Modify: `docs/reference/schema.md`
- Test: `crates/clio-core/tests/repair.rs`

1. Add a failing migration contract test for repair transaction/journal tables,
   immutable update/delete triggers and the singleton database generation row.
2. Run `cargo test -p clio-core --test repair migration` and record the expected
   failure.
3. Add migration `015_memory_repair_journal` without changing applied
   migrations. Store a repair transaction header plus ordered mutation rows.
4. Add triggers that advance one database-wide monotonic generation for memory,
   link and attention mutations.
5. Re-run the focused test and update the schema reference, including the
   previously undocumented migration 014.

## Task 2: Build deterministic, complete repair manifests

**Files:**

- Create: `crates/clio-core/src/repair.rs`
- Modify: `crates/clio-core/src/lib.rs`
- Modify: `crates/clio-core/Cargo.toml`
- Modify: `Cargo.toml`
- Test: `crates/clio-core/tests/repair.rs`

1. Add failing tests for deterministic ordering/digests, exact memory snapshots,
   attention snapshots and the complete set of links touching every target.
2. Add tests proving only cross-namespace `auto:relates_to` links are proposed
   for removal and human-authored links are retained.
3. Run the focused test binary and confirm the red state is due to the missing
   repair API.
4. Implement serialisable repair intents, manifests and state records. Validate
   unique targets, valid namespaces, active-only archive targets and non-empty
   bounded evidence.
5. Run `PRAGMA quick_check`, snapshot exact before/after states, sort all
   collections and calculate SHA-256 identifiers from canonical JSON.
6. Re-run the focused tests.

## Task 3: Apply a manifest atomically and idempotently

**Files:**

- Modify: `crates/clio-core/src/repair.rs`
- Test: `crates/clio-core/tests/repair.rs`

1. Add failing tests proving a successful repair preserves IDs, content, tags,
   embeddings, occurrences, creation time and semantic `updated_at`, while
   moving associated attention namespaces and removing only planned auto-links.
2. Add conflict tests for changed memory state, changed attention state, changed
   link state and a newly added link touching a target. Each must leave the
   complete database and journal unchanged.
3. Add replay tests: the same digest returns the existing transaction; a reused
   transaction identifier with different content fails closed.
4. Implement `BEGIN IMMEDIATE`, whole-manifest compare-and-swap validation,
   mutation and journal insertion in one transaction. Roll back on any error.
5. Re-run the focused tests and `git diff --check`.

## Task 4: Roll back conditionally and journal the inverse

**Files:**

- Modify: `crates/clio-core/src/repair.rs`
- Test: `crates/clio-core/tests/repair.rs`

1. Add failing round-trip tests for move, archive, attention and removed
   auto-link restoration.
2. Add a post-repair drift test proving rollback aborts completely when any
   affected entity no longer matches its recorded after-state.
3. Add idempotent rollback replay and immutable-journal tests.
4. Implement conditional inverse application from the committed journal and
   record a linked rollback transaction in the same transaction.
5. Re-run the focused tests.

## Task 5: Make namespace-list caching cross-process safe

**Files:**

- Modify: `crates/clio-core/src/cache.rs`
- Test: `crates/clio-core/tests/multi_connection.rs`

1. Add a failing two-connection test: prime a namespace cache, commit a repair
   through another connection, then require the first cache to see the new list.
2. Cache the namespace list together with the singleton store generation and
   require the generation to match before serving a cached list.
3. Re-run the focused test and existing cache tests.

## Task 6: Add the private operator CLI

**Files:**

- Modify: `crates/clio-cli/src/main.rs`
- Modify: `docs/reference/cli.md` (or the existing CLI command reference)
- Test: `crates/clio-cli/tests/repair_cli.rs`

1. Add failing CLI tests for `repair manifest`, `repair apply`, `repair export`
   and `repair rollback`, including missing/wrong confirmation and conflict
   failures.
2. Implement JSON intent/manifest/journal file handling with restrictive file
   permissions and atomic rename. Do not print target contents or evidence.
3. `manifest` must take and validate an online SQLite backup, then build against
   that immutable snapshot. `apply` and `rollback` must take a fresh validated
   backup before opening the mutation transaction.
4. Require the manifest digest as apply confirmation and the forward transaction
   ID as rollback confirmation. Export a rollback file from the committed
   journal after apply; make re-export safe if file export fails after commit.
5. Keep repair commands CLI-only; do not expose the private manifest through
   MCP.
6. Re-run CLI tests and update the command reference.

## Task 7: Correct Waypoint namespace and retirement behaviour

**Repository:** the adjacent Waypoint repository

**Files:**

- Modify: `src-tauri/src/services/worktree_projection_service.rs`
- Modify: `src-tauri/src/services/clio_service.rs`
- Test: tests in those modules using only the existing Clio stub seam
- Modify: Clio contract test `crates/clio-core/tests/waypoint_worktree_projection.rs`

1. Add failing tests that the worktree projection accepts an explicit canonical
   namespace, retains the repository slug only in `repo:` provenance, and never
   falls back to `project:<repository_slug>`.
2. Add failing tests that an archived current view calls `clio archive` for its
   stable source provenance, while active and parked views continue to upsert.
3. Resolve the namespace from explicit project context (`.clio-namespace` or an
   existing operator override) and return a pending marker when it is absent or
   invalid. Never infer it from the repository slug.
4. Extend the Clio CLI seam with a stub-testable archive call. Refresh must
   archive rather than upsert when lifecycle is archived.
5. Run the focused Waypoint tests and the Clio projection contract test. If the
   sandbox denies the adjacent repository edit, preserve the tested Clio work,
   report the exact blocked files and do not bypass the restriction.

## Task 8: Verify, document and prepare the private Atlas proposal

**Files:**

- Modify: `docs/operations/roadmap.md`
- Modify: `docs/operations/audits/2026-08-29-memory-and-namespace-audit.md`
- Create privately outside Git: intent, manifest, backup and rollback paths

1. Run focused tests after each task, then fresh verification:
   `cargo test -p clio-core`, `cargo test -p clio-cli`, `cargo fmt --check`,
   `cargo clippy -p clio-core -p clio-cli --all-targets -- -D warnings`, and
   `git diff --check`.
2. Update the operational roadmap and audit with only bounded evidence. Refresh
   the corresponding Clio project memory because the roadmap changed.
3. Re-read Atlas state and rebuild the candidate set. Take an online backup and
   generate a fresh private manifest without applying it. Verify its digest,
   counts, complete touching-link snapshot and `PRAGMA quick_check` result.
4. Stop and present the exact mutation summary, backup evidence, digest and
   rollback readiness for explicit approval. Do not deploy a migration or apply
   any Atlas repair in this phase.
