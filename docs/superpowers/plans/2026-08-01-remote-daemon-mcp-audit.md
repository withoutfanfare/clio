# Remote, Daemon and MCP Reliability Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove verified data-loss, misleading-status and avoidable blocking behaviour from Clio's remote bridge, daemon and MCP service, then make every related contract document describe the resulting implementation accurately.

**Architecture:** Keep storage and classification semantics in `clio-core`; change only adapter lifecycle, framing, locking and acknowledgement behaviour in the CLI, daemon and MCP crates. Preserve stdio MCP over SSH, the daemon's local-only boundary, SQLite as the system of record and the existing public tool payloads.

**Tech Stack:** Rust 2024, Tokio, rusqlite, rmcp, notify, SSH stdio bridge, Markdown documentation.

## Global Constraints

- Keep changes minimal and focused on the remote bridge, daemon, MCP service and their documentation.
- Archive remains hidden rather than deleted; do not alter storage, FTS, tag, upsert or auto-link semantics.
- The daemon remains local-only and exposes no HTTP or non-local network listener.
- Provider calls must stay on blocking workers and must not hold the shared SQLite mutex while waiting on the provider.
- An inbox file may move to `_processed/` only after a durable capture, queued review, durable fallback note, or a deliberate size/empty-file rejection.
- Use British English in documentation, comments and user-facing text.
- Do not push or merge to `main`.

---

### Task 1: Bound remote MCP request framing

**Files:**
- Modify: `crates/clio-cli/src/remote_mcp.rs`
- Test: `crates/clio-cli/src/remote_mcp.rs`
- Modify: `docs/resource-limits.md`

**Interfaces:**
- Consumes: newline-delimited MCP JSON-RPC from local stdin.
- Produces: unchanged or namespace-rewritten JSON-RPC lines on SSH stdin; rejects a line larger than the documented bridge maximum with `io::ErrorKind::InvalidData`.

- [x] **Step 1: Write the failing oversized-line test**

```rust
#[test]
fn rejects_oversized_mcp_request_lines() {
    let input = vec![b'x'; MAX_MCP_MESSAGE_BYTES + 1];
    let mut output = Vec::new();

    let error = forward_requests(std::io::Cursor::new(input), &mut output).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(output.is_empty());
}
```

- [x] **Step 2: Run the focused test and verify RED**

Run: `CARGO_HOME=<writable-temp> cargo test -p clio-cli --no-default-features remote_mcp::tests::rejects_oversized_mcp_request_lines`

Expected: compilation failure because `MAX_MCP_MESSAGE_BYTES` does not exist, proving the framing bound is absent.

- [x] **Step 3: Add the bounded read**

```rust
const MAX_MCP_MESSAGE_BYTES: usize = 2 * 1024 * 1024;

let bytes_read = std::io::Read::by_ref(&mut reader)
    .take((MAX_MCP_MESSAGE_BYTES + 1) as u64)
    .read_until(b'\n', &mut line)?;
if bytes_read > MAX_MCP_MESSAGE_BYTES {
    return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "MCP request exceeds the 2 MiB bridge limit",
    ));
}
```

- [x] **Step 4: Run the remote bridge tests and verify GREEN**

Run: `CARGO_HOME=<writable-temp> cargo test -p clio-cli --no-default-features remote_mcp::tests`

Expected: all namespace, quoting and framing tests pass.

### Task 2: Preserve failed inbox files and report real daemon routes

**Files:**
- Modify: `crates/clio-daemon/src/watcher.rs`
- Modify: `crates/clio-daemon/src/control.rs`
- Test: `crates/clio-daemon/src/watcher.rs`
- Test: `crates/clio-daemon/src/control.rs`

**Interfaces:**
- Consumes: a file selected by the inbox watcher and the current daemon settings.
- Produces: `Result<(), ClioError>` from fallback storage; source acknowledgement only on success; status routes for control socket, watcher, capture, auto-link and enabled maintenance jobs only.

- [x] **Step 1: Write the failing failed-storage acknowledgement test**

```rust
#[tokio::test]
async fn failed_note_storage_keeps_the_inbox_file_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("note.md");
    std::fs::write(&file, "keep me").unwrap();
    let db = directory.path().join("memory.db");
    let conn = clio_core::db::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_inbox_insert BEFORE INSERT ON memories
         BEGIN SELECT RAISE(FAIL, 'forced test failure'); END;",
    )
    .unwrap();
    drop(conn);

    process_file(&file, &db, &clio_core::settings::Settings::default(), &None).await;

    assert!(file.exists());
    assert!(!directory.path().join("_processed/note.md").exists());
}
```

- [x] **Step 2: Run the focused watcher test and verify RED**

Run: `cargo test -p clio-daemon watcher::tests::failed_note_storage_keeps_the_inbox_file_for_retry`

Expected: failure because the current fallback logs the write error and still moves the file to `_processed/`.

- [x] **Step 3: Return storage success and gate acknowledgement**

```rust
fn store_as_note(...) -> clio_core::error::Result<()> {
    let memory = clio_core::repository::remember(conn, &input, settings)?;
    // Auto-embedding stays best-effort.
    Ok(())
}

if stored_successfully {
    move_to_processed(file_path);
}
```

- [x] **Step 4: Write route-reporting tests**

```rust
#[test]
fn enabled_routes_exclude_unimplemented_http_and_include_maintenance() {
    let mut settings = clio_core::settings::Settings::default();
    settings.daemon.http_port = Some(8080);
    settings.daemon.maintenance.backup_interval_secs = 3600;
    settings.daemon.maintenance.integrity_interval_secs = 7200;

    assert_eq!(
        build_enabled_routes(&settings),
        vec!["control_socket", "backup_scheduler", "integrity_scheduler"]
    );
}
```

- [x] **Step 5: Run daemon tests and verify GREEN**

Run: `cargo test -p clio-daemon`

Expected: failed storage is retryable and route status reflects only implemented subsystems.

### Task 3: Release the MCP database lock during provider work

**Files:**
- Modify: `crates/clio-mcp/src/main.rs`
- Test: `crates/clio-mcp/src/main.rs`

**Interfaces:**
- Consumes: cached settings/backend plus the existing shared SQLite connection.
- Produces: unchanged MCP responses, with classification, distillation and query embedding generated before acquiring the SQLite mutex; record embeddings are generated outside the mutex and stored only if the record version is still current.

- [x] **Step 1: Add a blocking fake backend and write the failing concurrency test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn semantic_embedding_does_not_block_unrelated_database_reads() {
    // The fake backend signals when embed_one starts and waits for release.
    // Start memory_search, wait for that signal, then require memory_namespaces
    // to finish before releasing the provider call.
    assert!(tokio::time::timeout(
        Duration::from_millis(250),
        server.memory_namespaces(),
    ).await.is_ok());
}
```

- [x] **Step 2: Run the concurrency test and verify RED**

Run: `CARGO_HOME=<writable-temp> cargo test -p clio-mcp --no-default-features semantic_embedding_does_not_block_unrelated_database_reads`

Expected: the namespace read times out because `memory_search` currently holds the SQLite mutex while `embed_one` waits.

- [x] **Step 3: Split provider and SQLite phases**

```rust
let query_embedding = be.embed_one(&params.query).map_err(format_clio_error)?;
let conn = conn.lock().map_err(|e| format!("lock error: {e}"))?;
let items = clio_core::embeddings::semantic_recall(...)?;
```

Apply the same phase separation to `memory_capture`, `memory_session_checkpoint`, and best-effort auto-embedding after `memory_remember`/`memory_update`. Before storing a generated record embedding, re-read `updated_at`; skip it if the record changed or disappeared while provider work was in flight.

- [x] **Step 4: Run MCP tests and verify GREEN**

Run: `CARGO_HOME=<writable-temp> cargo test -p clio-mcp --no-default-features`

Expected: all MCP unit/contract tests pass and the concurrency test completes before the fake backend is released.

### Task 4: Synchronise the remote, daemon, MCP and performance documentation

**Files:**
- Modify: `crates/clio-cli/README.md`
- Modify: `crates/clio-mcp/README.md`
- Modify: `crates/clio-daemon/README.md`
- Modify: `context/ARCHITECTURE.md`
- Modify: `docs/mcp-agent-setup.md`
- Modify: `docs/reference/mcp-contract.md`
- Modify: `docs/reference/settings.md`
- Modify: `docs/resource-limits.md`
- Modify: `docs/performance-audit.md`
- Modify if operational status changes: `docs/operations/roadmap.md`

**Interfaces:**
- Consumes: the final code, generated MCP tool schemas and focused test evidence.
- Produces: one consistent description of the 23 MCP tools, SSH bridge framing/namespace behaviour, one-connection MCP lifecycle, daemon acknowledgement and route semantics, reserved `http_port`, and current performance debt.

- [x] **Step 1: Reconcile tool inventories and namespace-capable tools**

Replace obsolete split inbox tools with `memory_inbox`; add `memory_update`, `memory_move`, `memory_session_checkpoint`, `memory_resume`, `memory_action` and `memory_cache_clear`; mark `memory_recent` deprecated; document all eight `cwd`-aware tools.

- [x] **Step 2: Reconcile daemon and remote contracts**

Document the 2 MiB remote bridge request limit, SSH timeout/keepalive behaviour, local-only daemon boundary, retryable failed inbox files, maintenance schedulers, and the fact that `http_port` is a reserved compatibility field with no listener.

- [x] **Step 3: Refresh stale performance findings**

Record that semantic search already uses a bounded top-K heap, MCP already reuses one connection/settings/backend, and this change removes provider waits from the shared SQLite critical section. Retain only verified remaining debt, including full embedding scans and the watcher callback's bounded-channel backpressure.

- [x] **Step 4: Review the canonical roadmap**

Change `docs/operations/roadmap.md` only if this implementation progresses an existing operational item; otherwise leave it deliberately unchanged and state why.

### Task 5: Verify the complete change and install sccache where permitted

**Files:**
- No repository file required for the package installation.

**Interfaces:**
- Consumes: the final workspace and the host package-manager permissions.
- Produces: formatted/linted/tested Rust changes; `sccache --version` when host installation is writable, or an explicit operator command when sandbox permissions block it.

- [x] **Step 1: Format and run focused verification**

Run: `cargo fmt --all -- --check`

Run: `CARGO_HOME=<writable-temp> cargo test -p clio-cli -p clio-mcp --no-default-features`

Run daemon tests through the repository's supported ONNX Runtime setup; if the native library is unavailable, report that exact environmental blocker and still run any source-only checks available.

- [x] **Step 2: Run strict linting**

Run: `CARGO_HOME=<writable-temp> cargo clippy -p clio-cli -p clio-mcp --no-default-features --all-targets -- -D warnings`

Run the equivalent daemon lint when ONNX Runtime is available.

- [ ] **Step 3: Install and verify sccache** — blocked by the workspace sandbox;
      run `brew install sccache && sccache --version` in a normal terminal.

Run: `brew install sccache && sccache --version`

If Homebrew paths are outside the writable sandbox, do not change their ownership. Report the blocked installation and provide the same command for the user to run in a normal terminal.

- [x] **Step 4: Inspect the final diff and worktree**

Run: `git diff --check && git status --short && git diff --stat`

Expected: only requested-scope Rust, test, plan and documentation files changed; no generated or Cargo-cache files are present.

## Self-Review

- Spec coverage: remote bridge, daemon, MCP, documentation and sccache are each owned by a task.
- Placeholder scan: no `TBD`, `TODO`, deferred implementation placeholder or unspecified test step remains.
- Type consistency: the plan retains `rusqlite::Connection`, `EmbeddingBackend`, `Settings`, `CheckpointRequest` and existing MCP parameter/response types; no new public storage contract is invented.
