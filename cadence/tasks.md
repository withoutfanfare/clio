# Cadence Tasks

## TASK-1: Daemon 'stop'/'restart' leaves a stuck live process (run() never subscribes to shutdown broadcast)
status: completed
labels: agent:triaged

## Problem

`clio daemon stop` (and therefore `restart`) never terminates the daemon process — it leaves a stuck live process that can only be killed at the OS level.

The control socket's `stop` handler broadcasts on the shutdown channel (`crates/clio-daemon/src/control.rs:161`), and every worker task subscribes to it (control/watcher/maintenance/auto-linker — `crates/clio-daemon/src/main.rs:119,145,156,168`). But `run()` itself does **not** subscribe. It blocks solely on OS signals:

```rust
// crates/clio-daemon/src/main.rs:182
shutdown_signal().await;          // only returns on SIGINT / SIGTERM
```

`shutdown_signal()` (main.rs:224) awaits only `ctrl_c`/`SIGTERM`. So on a `stop` command: all worker tasks break out and exit, `control::serve` deletes the socket file — but `run()` stays parked forever. The cleanup block (main.rs:187-217: join handles, WAL checkpoint, PID-file removal) is never reached, so the PID file also survives.

**Cascade into `restart`:** `DaemonCommand::Restart` calls stop then `cmd_daemon_start` (main.rs:2676-2677). `cmd_daemon_start` sees the still-present PID file as live (main.rs:2793) and prints "Daemon is already running", refusing to spawn a fresh one. Net effect: `restart` leaves a half-dead daemon — workers dead, process alive but inert, no control socket, unrecoverable without a manual `kill`.

## Where it lives

- `crates/clio-daemon/src/main.rs:181-184` — `run()` awaits only `shutdown_signal()`.
- `crates/clio-daemon/src/control.rs:161-165` — `stop` only broadcasts.

Verified: `run()` at main.rs:182 does not `subscribe()` to `shutdown_tx`; the broadcast reaches workers but not the main task.

## Fix direction

Have `run()` `tokio::select!` on its own `shutdown_tx.subscribe()` alongside `shutdown_signal()`, so a control-socket `stop` reaches the cleanup path just as an OS signal does.

**Why it matters:** correctness/reliability — the documented graceful-stop path is completely broken; `stop` and `restart` do not do what they claim, and the daemon can only be recovered with an OS-level kill.

### Acceptance Criteria

- [ ] `clio daemon stop` causes the daemon process to actually exit (PID file removed, socket removed, WAL checkpointed on the way out).
- [ ] `run()` reaches its cleanup block (main.rs:187-217) on a control-socket `stop`, not only on SIGINT/SIGTERM.
- [ ] `clio daemon restart` reliably tears down the old process and starts a fresh one (no "already running" false positive from a stale PID file).
- [ ] No regression to SIGINT/SIGTERM shutdown behaviour.
- [ ] `cargo test`, `cargo clippy`, `cargo fmt --check` pass.
---

## Spec

### Approach

Make `run()` a party to the graceful-shutdown broadcast it already owns. Take a `shutdown_tx.subscribe()` receiver immediately after the broadcast channel is created and before `control::serve` is spawned, then `tokio::select!` on both the OS signal and that receiver. Whichever fires first, fall through to the existing send + cleanup block unchanged.

### Change

`crates/clio-daemon/src/main.rs` — create the main receiver before starting the control socket, then replace the single-await at line 181–182:

```rust
let mut shutdown_rx = shutdown_tx.subscribe();

// Start the control socket server.
// ...

// Wait for shutdown: OS signal (SIGTERM/SIGINT) or a control-socket `stop`.
tokio::select! {
    _ = shutdown_signal() => tracing::info!("shutdown signal received"),
    _ = shutdown_rx.recv() => tracing::info!("shutdown requested via control socket"),
}
let _ = shutdown_tx.send(());
```

Everything from line 186 onward (join handles, WAL checkpoint, PID/socket removal) is already correct and stays as-is. The extra `shutdown_tx.send(())` is a harmless no-op re-broadcast when the trigger was already the control socket — workers have their own receivers and idempotently exit; keep it so the OS-signal path is unchanged.

Subscribe before `control::serve` is spawned so no `stop` broadcast is missed before the final wait point (broadcast delivers to receivers that exist at send time).

### Files

- `crates/clio-daemon/src/main.rs:181-184` — the only edit.

### Test plan

- Existing `cargo test` / `cargo clippy` / `cargo fmt --check` must stay green.
- Manual/integration verification (daemon lifecycle is not unit-testable in-process): `clio daemon start` → `clio daemon stop` → assert process gone, PID file removed, socket removed, and a "WAL checkpointed on shutdown" log line present. Then `clio daemon restart` twice in a row and confirm no "already running" false positive and a fresh PID each time.
- If an integration harness for the daemon exists under `crates/clio-daemon/tests/`, add a test that sends `stop` over the control socket and asserts the process exits within a timeout; otherwise document the manual steps in the PR description.

### Out of scope

- No change to `shutdown_signal()`, the control handler, or worker subscription — they are already correct. Do not refactor the cleanup block.

PR: https://github.com/withoutfanfare/clio/pull/1

---

## Advance loop note (2026-07-05)

PR #1 (branch `task-1`) is **closed, not merged** (`mergedAt: null`). `gh pr diff 1` shows a zero-line diff against `origin/develop` — the fix already landed on `develop` via a separate commit (`d15994d fix(daemon,mcp): control-socket stop hangs, semantic recall global fallback`), confirmed by reading `crates/clio-daemon/src/main.rs` on `origin/develop`: `shutdown_rx` is subscribed before `control::serve` and the shutdown wait uses `tokio::select!` on both the OS signal and the control-socket receiver, matching this task's acceptance criteria.

This does not fit the decision core's model (open PR passing/failing a bar) — the PR was superseded rather than reviewed and merged through the normal gate. Escalating to `agent:needs-human` rather than granting `agent:pr-open` (misleading — no open PR exists) or `agent:revise` (nothing left to revise). Recommend closing this task as `status: completed` once you confirm the develop fix covers it.

## Resolution (2026-08-21)

Closed as `completed` on the strength of commit `d15994d` (`fix(daemon,mcp): control-socket stop hangs, semantic recall global fallback`), which is on `develop`. Verified by reading `crates/clio-daemon/src/main.rs` at `develop`: `shutdown_rx` is subscribed at line 111 (before `control::serve` is spawned) and the shutdown wait at lines 182-187 is a `tokio::select!` on `shutdown_signal()` and `shutdown_rx.recv()`, falling through to the existing cleanup block. PR #1 was superseded by this commit rather than merged. Live daemon stop/restart was not re-exercised for this closure — code-level verification only.

## TASK-2: Integrity check falsely flags every unsorted-tag memory as corrupt (tags_text not sorted on write)
status: completed
labels: agent:triaged

## Problem

The integrity check `find_tag_mismatches` falsely reports almost every multi-tag memory as corrupt, because it assumes `tags_text` is stored alphabetically sorted — but the write path does not sort.

The check compares stored `tags_text` against a **sorted** reconstruction:

```rust
// crates/clio-core/src/integrity.rs:231
WHERE m.tags_text != COALESCE(
  (SELECT GROUP_CONCAT(mt.tag, ' ')
   FROM (SELECT tag FROM memory_tags WHERE memory_id = m.id ORDER BY tag) mt), '')
```

But `remember`/`update_existing` write `tags_text = normalise_tags(tags).join(" ")`, and `normalise_tags` preserves insertion order (lowercases, trims, de-dupes — no sort):

```rust
// crates/clio-core/src/repository.rs:207
fn normalise_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    tags.iter().map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty() && seen.insert(t.clone())).collect()
}
```

**Failure scenario:** `remember` with `tags: ["rust", "async"]` → stored `tags_text = "rust async"`; the subquery yields `"async rust"`; `"rust async" != "async rust"` → flagged as a `tag_mismatch`, `auto_fixable: true`. `integrity::fix` (integrity.rs:148-164) then rewrites `tags_text` to sorted form, firing the `memories_au` FTS trigger for a non-issue.

There is also a latent storage-convention inconsistency worth resolving in the same pass: `add_tag_bulk`/`remove_tag_bulk` and `merge_memories` **do** write sorted `tags_text`, and `row_to_memory` sorts on read (repository.rs:402) — so the store is internally inconsistent about ordering.

## Where it lives

- `crates/clio-core/src/integrity.rs:226-240` — `find_tag_mismatches` (expects sorted).
- `crates/clio-core/src/repository.rs:207-213` — `normalise_tags` (does not sort).

Both verified by direct read.

## Fix direction

Make the two agree. Cleanest is to normalise `tags_text` to sorted order on the write path (so `tags_text`, `memory_tags`, and read-time ordering all match), which also matches what `add_tag_bulk`/`merge` already do. Then the integrity check's sorted comparison is correct. Alternatively, relax the check to be order-insensitive — but sorting the write path removes the inconsistency rather than papering over it. Note the tags/FTS-in-sync invariant is not actually being violated; this is a spurious-corruption report.

**Why it matters:** correctness — the integrity/maintenance job cries wolf on virtually every normal memory (2+ non-alphabetical tags is extremely common), inflating `issues_found` and triggering needless "repairs" plus FTS churn, which erodes trust in the integrity report.

### Acceptance Criteria

- [ ] A memory stored via `remember` with tags `["rust", "async"]` is NOT reported as a `tag_mismatch` by the integrity check.
- [ ] `tags_text` ordering is consistent across all write paths (`remember`, `update`, `add_tag_bulk`, `remove_tag_bulk`, `merge_memories`).
- [ ] Existing legitimate mismatch detection (genuine `tags_text` vs `memory_tags` divergence) still works.
- [ ] A regression test asserts a non-alphabetical-tag memory passes the integrity check.
- [ ] `cargo test -p clio-core`, `cargo clippy`, `cargo fmt --check` pass.
---

## Spec

### Approach

Resolve the disagreement by sorting on the **write path**, so `tags_text`, the `memory_tags` rows, and read-time ordering all agree — which also matches what `add_tag_bulk` / `remove_tag_bulk` / `merge_memories` already do. Then `find_tag_mismatches`' sorted comparison becomes correct, and no change to the integrity check is needed.

### Change

`crates/clio-core/src/repository.rs` — `normalise_tags` (line 207) currently lowercases, trims, de-dupes preserving insertion order. Add a final sort so output is alphabetical:

```rust
fn normalise_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty() && seen.insert(t.clone()))
        .collect();
    out.sort();
    out
}
```

Because both `tags_text = normalise_tags(tags).join(" ")` and the `memory_tags` inserts derive from the same `normalise_tags` result, sorting there fixes both in lock-step for `remember` and `update_existing`. `row_to_memory` already sorts on read (repository.rs:402), so read output is unchanged.

### Why write-path sort over relaxing the check

Sorting removes the store's internal ordering inconsistency (some write paths sorted, some didn't) rather than papering over it in one consumer. The tags/FTS-in-sync invariant is not violated either way — this was only a spurious-corruption report — but a single canonical ordering is the durable fix.

### Files

- `crates/clio-core/src/repository.rs:207-213` — the only production edit.
- `crates/clio-core/src/integrity.rs:226-240` — no change; verify it now passes.

### Test plan

- Add a regression test in `clio-core` (inline in `repository.rs` tests or `tests/integration.rs`): `remember` a memory with `tags: ["rust", "async"]`, then run the integrity check (`integrity::check` / `find_tag_mismatches`) and assert **zero** `tag_mismatch` findings for it. Assert the stored `tags_text` is `"async rust"`.
- Add/confirm a test that a genuine divergence (manually corrupt `tags_text` out of step with `memory_tags`) is still detected as a mismatch — so detection isn't neutered.
- `cargo test -p clio-core`, `cargo clippy`, `cargo fmt --check` pass.

### Out of scope

- No new migration and no bulk rewrite of existing rows. Existing unsorted `tags_text` rows will be flagged once and auto-fixed by the integrity job's normal repair path (that is the correct, one-time convergence). Note this in the PR description; do not add a data-migration unless review asks for it.

PR: https://github.com/withoutfanfare/clio/pull/2

## TASK-3: MCP memory_search silently drops global memories (default scope diverges from CLI and memory_recall)
status: completed
labels: agent:triaged

## Problem

MCP `memory_search` silently drops `global`-namespace memories, diverging from both the CLI `search` command and MCP `memory_recall` — a violation of the crate rule "MCP defaults must match CLI/core semantics exactly."

When called with a `cwd` (agents always pass one) and no explicit namespace/global flag, `memory_search` resolves a project namespace and filters to it with **no global fallback**:

```rust
// crates/clio-mcp/src/main.rs:1412
let ns_filter = if params.global { None } else {
    let resolved_ns = clio_core::context::resolve_namespace(
        params.namespace.as_deref(), cwd_path, settings.context.auto_detect);
    if params.namespace.is_some() || resolved_ns != "global" {
        Some(resolved_ns)   // scope to detected project ns, no global fallback
    } else { None }
};
```

Two divergences:

1. **vs CLI:** `cmd_search` ignores auto-detection and passes the raw namespace (`None` ⇒ all namespaces — `crates/clio-cli/src/main.rs:1585`). So from inside a project dir, CLI `clio search "X"` searches all namespaces while MCP `memory_search "X"` searches only the project namespace. (Aside: the CLI `SearchArgs.global` flag at main.rs:419 is never read by `cmd_search` — a dead flag.)

2. **vs MCP `memory_recall`:** for the same no-namespace/no-global case, `memory_recall` uses `recall_scoped` = project **plus** global fallback (main.rs:1128). `memory_search` gives project-only. Same logical query, different result sets across the two MCP tools — and the semantic path silently hides exactly the shared/global knowledge the store exists to surface.

**Failure scenario:** an agent stores a decision in `global`, then working in `project:my-app` calls `memory_search` with `cwd=/path/to/my-app`. `resolved_ns = "project:my-app"`, so the global decision is never returned — even though `memory_recall` with identical args would return it.

## Where it lives

- `crates/clio-mcp/src/main.rs:1412-1426` — `memory_search` namespace filter.
- Compare `crates/clio-mcp/src/main.rs:1114-1130` — `memory_recall` uses `recall_scoped` (project + global).

## Fix direction

Align `memory_search`'s default scoping with `memory_recall` — i.e. project namespace with a global fallback (scoped semantics), rather than a hard project-only filter. Confirm the intended default with the maintainer if ambiguous, but the internal MCP inconsistency (recall vs search) is the clearest signal of the correct target. Fixing the dead CLI `--global` flag is optional cleanup, not required.

**Why it matters:** correctness/consistency — the same query returns different results across adapters, and the semantic path silently omits global memories that the keyword path surfaces, causing agents to miss relevant shared knowledge.

### Acceptance Criteria

- [ ] MCP `memory_search` with a project `cwd` and no explicit namespace returns matching `global` memories as well as project ones (matching `memory_recall`).
- [ ] `memory_search` and `memory_recall` return consistent namespace scoping for equivalent arguments.
- [ ] Explicit `namespace` and `global: true` params still behave as documented.
- [ ] A test covers the project+global scoping for the semantic search path.
- [ ] `cargo test`, `cargo clippy`, `cargo fmt --check` pass.
---

## Spec

### Approach

Align `memory_search`'s default scoping with `memory_recall`: project namespace **plus** global fallback. `memory_recall` achieves this via `recall_scoped` (main.rs:1128); the semantic path has no scoped equivalent, so add one in core and call it from the MCP else-branch.

### Change

1. **Core — add scoped semantic recall** (`crates/clio-core/src/embeddings.rs`).
   `semantic_recall` / `semantic_search` today take a single `namespace: Option<&str>` and filter `AND m.namespace = ?` (embeddings.rs:525-527, 584). Add a scoped path that mirrors `repository::recall_scoped`: query the detected namespace first, then fill remaining slots from `global`. If `detected_ns == "global"`, search only `global`; only an explicit `global: true` call should be unscoped. Keep the existing single-namespace `semantic_recall` for explicit-namespace and all-namespace calls. All business logic stays in core (no scoping logic in the adapter).

2. **MCP — use it in the default branch** (`crates/clio-mcp/src/main.rs:1412-1447`).
   Replace the project-only `ns_filter` construction with the same three-way shape `memory_recall` uses:
   - `params.global` → unscoped (`semantic_recall` with `None`).
   - `params.namespace.is_some()` → `semantic_recall` scoped to that exact namespace.
   - neither → resolve `detected_ns` from `cwd`, then `semantic_recall_scoped(detected_ns)` (project + global fill; if `detected_ns` resolves to `global`, search `global` only).

### Files

- `crates/clio-core/src/embeddings.rs:500-599` — new scoped SQL branch / wrapper.
- `crates/clio-mcp/src/main.rs:1412-1447` — default branch calls the scoped path.
- Compare against `crates/clio-mcp/src/main.rs:1114-1130` (`memory_recall`) to keep the three-way branching identical.

### Test plan

- Core unit/integration test (`clio-core`): seed one `global` memory, one `project:x` memory, and one unrelated project memory that all match a query embedding; assert `semantic_recall_scoped(conn, ..., "project:x", ...)` returns the project result first and then global fill, `semantic_recall_scoped(conn, ..., "global", ...)` returns only global, and `semantic_recall(conn, ..., Some("project:x"), ...)` returns only the project one (explicit-namespace stays strict).
- Assert `global: true` still searches everything and explicit `namespace` still scopes exactly.
- `cargo test`, `cargo clippy`, `cargo fmt --check` pass.

### Decision / open question

The internal MCP inconsistency (recall vs search) is the authoritative signal that **project + global** is the intended default; build to that. The CLI `cmd_search` searching *all* namespaces (main.rs:1585) is a separate, looser behaviour and is **not** the target to match. The dead CLI `--global` flag (SearchArgs.global, main.rs:419, never read) is noted but **out of scope** — mention it in the PR, don't fix it here unless asked.

PR: https://github.com/withoutfanfare/clio/pull/3

## TASK-4: semantic_search returns results worst-match-first, contradicting its doc (stray .rev())
status: completed
labels: agent:triaged

## Problem

`semantic_search` returns its top-K results in **ascending** similarity order (worst match first), directly contradicting its own doc comment ("sorted by similarity descending").

```rust
// crates/clio-core/src/embeddings.rs:559
// Drain heap into a vec sorted by similarity descending.
let results: Vec<SemanticResult> = heap
    .into_sorted_vec()
    .into_iter()
    .rev()                    // <-- this flip is wrong
    .map(...)
```

`ScoredEntry`'s `Ord` is deliberately reversed for min-heap eviction (embeddings.rs:488-497: smaller similarity = `Greater`). `BinaryHeap::into_sorted_vec()` always sorts ascending by the type's `Ord`, so on this reverse-ordered heap it already yields **best-first** (`[0.9, 0.5, 0.1]`). The extra `.rev()` flips it to worst-first (`[0.1, 0.5, 0.9]`).

**Failure scenario:** similarities `0.9, 0.5, 0.1`, `limit >= 3` → returns `[{0.1}, {0.5}, {0.9}]`, presenting the worst match as the top hit.

## Scope / severity (important — read before building)

This is currently **latent, not live**. All three adapters (MCP main.rs:1437, `crates/clio-tauri/src/commands/search.rs:42`, CLI main.rs:1585) go through `semantic_recall`, which builds an order-independent similarity map and re-sorts descending by hybrid rank (embeddings.rs:654-660) before truncating — so the inverted order does not affect their output. The only verbatim consumer, `cache.rs::semantic_search_cached` (cache.rs:198), currently has no callers. So there is no user-facing symptom today.

It is still worth fixing: the function violates its documented contract and is a landmine the moment `semantic_search`/`semantic_search_cached` is wired to any surface directly. The fix is a single line (drop the `.rev()`). Do **not** over-scope this into a `semantic_recall` change — that path is already correct.

## Where it lives

- `crates/clio-core/src/embeddings.rs:559-569` — the erroneous `.rev()`.

Verified by direct read of the `Ord` impl and the drain.

**Why it matters:** correctness — a function whose returned order contradicts its doc comment; harmless today only by accident of every caller re-sorting, and a trap for the next caller.

### Acceptance Criteria

- [ ] `semantic_search` returns results in descending similarity order (best match first), matching its doc comment.
- [ ] A unit test asserts ordering: given embeddings with known similarities, the first result has the highest.
- [ ] `semantic_recall` behaviour is unchanged (its own re-sort still governs adapter output).
- [ ] `cargo test -p clio-core`, `cargo clippy`, `cargo fmt --check` pass.
---

## Spec

### Approach

One-line fix: drop the stray `.rev()`. `ScoredEntry`'s `Ord` is reversed for min-heap eviction (embeddings.rs:488-497), so `BinaryHeap::into_sorted_vec()` already yields best-similarity-first. The `.rev()` re-inverts it to worst-first, contradicting the doc comment.

### Change

`crates/clio-core/src/embeddings.rs:561-569` — remove line 564:

```rust
let results: Vec<SemanticResult> = heap
    .into_sorted_vec()
    .into_iter()
    .map(|e| SemanticResult {
        memory_id: e.memory_id,
        similarity: e.similarity,
    })
    .collect();
```

### Files

- `crates/clio-core/src/embeddings.rs:564` — delete the `.rev()`. No other edit.

### Test plan

- Add a unit test in `embeddings.rs`: build entries with known similarities (e.g. 0.9, 0.5, 0.1), push through the same heap logic (or call `semantic_search` against a small seeded set), and assert `results[0].similarity >= results[1].similarity >= results[2].similarity` — i.e. descending, best first.
- `cargo test -p clio-core`, `cargo clippy`, `cargo fmt --check` pass.

### Scope guard

Do **not** touch `semantic_recall` — its own descending re-sort by hybrid rank (embeddings.rs:654-660) already governs all three adapters' output, so this fix is latent-correctness only and must not change adapter results. The only current verbatim consumer, `cache.rs::semantic_search_cached`, has no callers; the fix simply makes the contract honest before the next caller is wired up.

PR: https://github.com/withoutfanfare/clio/pull/4
