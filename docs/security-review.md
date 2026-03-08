# Clio Security & Best Practices Review

**Date:** 2026-03-03
**Scope:** Full codebase — clio-core, clio-daemon, clio-mcp, clio-cli, clio-tauri, UI, dependencies, and build configuration
**Method:** Four-agent parallel review with findings consolidated by severity

## Executive Summary

| Severity      | Count |
|---------------|-------|
| High          | 6     |
| Medium        | 10    |
| Low           | 14    |
| Informational | 9     |
| **Total**     | **39** |

No `unsafe` code was found anywhere in the codebase. SQL queries consistently use parameterised bindings. No XSS vectors were found in the Vue frontend. These are strong foundations. The main areas of concern are: **input validation gaps** (FTS queries, MCP parameters, namespace values), **resource exhaustion vectors** (unbounded connections, file reads, query limits), **secrets handling** (API keys in plaintext settings), and **missing hardening** (socket permissions, CSP, mutex poisoning).

---

## High Severity

### H1 — FTS Query Injection via Unsanitised MATCH Input
**File:** `crates/clio-core/src/repository.rs:351–364` (`recall_fts()`)
**Description:** The FTS query string is passed directly into `memory_fts MATCH ?1`. While this uses a parameterised binding (safe from classic SQL injection), FTS5 MATCH syntax accepts operators (`NOT`, `OR`, `AND`, column filters like `title:word`, prefix wildcards). A caller supplying a malformed FTS expression (e.g. `"*"`, `"title:* NOT content:*"`) can cause query errors or unexpected data exposure. No sanitisation or validation of the FTS query string exists anywhere before it reaches the DB. The `RecallQuery.query` field has no length limit or character-set validation.
**Fix:** Validate or sanitise the FTS query before execution. Reject or escape special FTS operators, or wrap input in double-quotes to force literal phrase matching.

### H2 — Unix Socket Has No Authentication or Access Control
**File:** `crates/clio-daemon/src/control.rs:38–100`
**Description:** The Unix domain socket is created with no explicit permissions set after binding (permissions depend on umask). The `stop` command allows any local user who can connect to the socket to shut down the daemon. There is no authentication, no credential check (e.g. `SO_PEERCRED`), and no allowlist beyond hard-coded commands.
**Fix:** After `UnixListener::bind()`, call `std::fs::set_permissions(&socket_path, Permissions::from_mode(0o600))`. For the `stop` command, consider checking peer credentials via `stream.peer_cred()`.

### H3 — Unbounded Connection Acceptance on Control Socket
**File:** `crates/clio-daemon/src/control.rs:41–56`
**Description:** The `serve` loop spawns a new Tokio task per connection with no limit. A local process could open thousands of connections, exhausting memory and file descriptors. No connection limit, rate limiting, or idle timeout exists.
**Fix:** Add a `tokio::sync::Semaphore` (cap ~16 concurrent connections) and a per-connection read timeout via `tokio::time::timeout`.

### H4 — Unbounded Line Reads from Control Socket Clients
**File:** `crates/clio-daemon/src/control.rs:86`
**Description:** `lines.next_line().await` does not enforce a per-line maximum length. A malicious local process can send a multi-gigabyte line before a newline, causing the daemon to allocate a massive buffer.
**Fix:** Use `tokio_util::codec::LinesCodec` with `max_length` set, or enforce a per-message size cap (~64 KiB).

### H5 — API Key Stored in Plaintext Settings File
**Files:** `crates/clio-core/src/settings.rs:238–256`, `embeddings.rs:33–43`, `capture.rs:83–91`
**Description:** `CaptureConfig.api_key` and `EmbeddingConfig::OpenAi.api_key` are persisted to `clio-settings.json` in plaintext via `serde_json::to_string_pretty()`. No file permission restriction (e.g. 0o600) is applied when the file is written.
**Fix:** After writing the settings file, call `std::fs::set_permissions(&path, Permissions::from_mode(0o600))`. Support keychain/secret-store backends. Consider deprecating the in-settings `api_key` field in favour of environment variables.

### H6 — Tauri CSP Disabled (null)
**File:** `crates/clio-tauri/tauri.conf.json:23`
**Description:** `"csp": null` disables the Content Security Policy entirely in the Tauri webview. Even though IPC goes via `invoke()`, a CSP provides defence-in-depth against XSS if user-controlled content is ever rendered.
**Fix:** Set a restrictive CSP: `"default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'"`.

---

## Medium Severity

### M1 — Namespace Value from `.clio-namespace` Used Unvalidated
**File:** `crates/clio-core/src/context.rs:53–58` (`detect_namespace()`)
**Description:** Reads `.clio-namespace` file content with only an `is_empty()` check. A crafted file could inject arbitrarily long or structured namespace values into DB queries and log messages. No length cap or character-set validation is applied.
**Fix:** Apply the same validation as `validate::remember_input()` — max 120 chars, allowed slug pattern.

### M2 — Dynamic SQL Unbounded Batch Size in `touch_accessed`
**File:** `crates/clio-core/src/repository.rs:283–306`
**Description:** Builds an `IN ({placeholders})` clause where placeholder count is derived from `ids.len()` (caller-controlled). A very large slice generates a very large SQL statement with no cap.
**Fix:** Add a reasonable upper bound (e.g. 1000 IDs per batch).

### M3 — Graph Traversal Unbounded Depth in `get_neighbours`
**File:** `crates/clio-core/src/repository.rs:837–857`
**Description:** Depth is caller-controlled (`u32`); each hop doubles the frontier. `depth=10` on a dense graph generates enormous SQL and consumes significant memory.
**Fix:** Cap `depth` to a small maximum (e.g. 5) and cap frontier size per hop.

### M4 — Stale Socket File Removal is a TOCTOU Race
**File:** `crates/clio-daemon/src/control.rs:28–31`
**Description:** Checks `socket_path.exists()` then calls `remove_file()`. Between check and removal, another process could replace the socket with a symlink to a sensitive file.
**Fix:** Call `remove_file()` unconditionally, ignoring `ErrorKind::NotFound`.

### M5 — File Content Read Without Size Limit in Watcher
**File:** `crates/clio-daemon/src/watcher.rs:96`
**Description:** `std::fs::read_to_string(file_path)` reads the entire file into memory with no size cap. A 1 GB file dropped into inbox will be fully buffered.
**Fix:** Check `metadata().len()` before reading; reject files exceeding a configurable max (e.g. 10 MiB).

### M6 — Symlink Attack on Inbox Watcher
**File:** `crates/clio-daemon/src/watcher.rs:96–106`
**Description:** `should_process()` does not resolve symlinks. An attacker with inbox write access can create a symlink to a sensitive file (e.g. `~/.ssh/id_rsa`), causing its contents to be read, stored, and embedded.
**Fix:** Use `path.canonicalize()` and verify the canonical path is inside the configured inbox directory.

### M7 — No Size Limits on MCP Content/Tags/Metadata
**File:** `crates/clio-mcp/src/main.rs:29–89` (`RememberParams`)
**Description:** `content: String`, `tags: Vec<String>`, and `metadata: serde_json::Value` have no size bounds. An MCP client can supply arbitrarily large content, thousands of tags, or deeply nested metadata.
**Fix:** Validate: content max ~1 MiB, tags max 50 items / 100 chars each, metadata max depth/size.

### M8 — Mutex Poison Panic in MCP Settings Cache
**File:** `crates/clio-mcp/src/main.rs:455`
**Description:** `settings_cache.lock().unwrap()` will panic if the mutex is poisoned. A single panic in a settings-loading path renders the entire MCP server non-functional.
**Fix:** Replace `.unwrap()` with `.unwrap_or_else(|e| e.into_inner())` for poison recovery.

### M9 — Non-Atomic Settings File Write
**File:** `crates/clio-core/src/settings.rs:251`
**Description:** `std::fs::write()` is non-atomic. A crash mid-write leaves the settings file truncated or corrupted.
**Fix:** Write to a temporary file, then `std::fs::rename()` to the final path.

### M10 — `DefaultHasher` Used for Deduplication `source_ref`
**File:** `crates/clio-core/src/migrate.rs:70–75`
**Description:** `std::collections::hash_map::DefaultHasher` is explicitly not stable across Rust versions and provides no collision resistance. Two different inputs could collide, silently treating the wrong memory as a duplicate.
**Fix:** Use SHA-256 (via `sha2` crate) or a fixed-seed SipHash.

---

## Low Severity

### L1 — `expect()` in Production Paths
**Files:** `crates/clio-core/src/models.rs:209`, `embeddings.rs:220`
**Description:** `now_utc()` and the OpenAI runtime `OnceLock` initialiser use `expect()`, which panics on failure.
**Fix:** Convert to `Result`-returning functions.

### L2 — Capture Pipeline Creates Tokio Runtime Per Call
**File:** `crates/clio-core/src/capture.rs:93–98`
**Description:** `classify()` creates a fresh `Builder::new_current_thread()` runtime on every invocation, risking resource exhaustion under rapid calls.
**Fix:** Use the same `OnceLock` pattern as `embeddings.rs`.

### L3 — OpenAI Error Response Body Leaked in Error Messages
**Files:** `crates/clio-core/src/embeddings.rs:252–254`, `capture.rs:135–137`
**Description:** Full OpenAI error response bodies (which may contain account/rate-limit metadata) are included in error messages that propagate to callers.
**Fix:** Log full body at DEBUG level; surface only status code to callers.

### L4 — TOCTOU on PID File
**File:** `crates/clio-core/src/daemon.rs:123–153`
**Description:** Three separate operations (read, check liveness, write) with no file lock between them, allowing duplicate daemon starts.
**Fix:** Use `OpenOptions::new().create_new(true)` (O_CREAT|O_EXCL) for atomic create-or-fail.

### L5 — TOCTOU on Settings File Existence Check
**File:** `crates/clio-core/src/settings.rs:214`
**Description:** Checks `if !path.exists()` then reads. File could be removed between check and read.
**Fix:** Open directly and handle `NotFound` error.

### L6 — DB Path from Environment Variable Unvalidated
**File:** `crates/clio-core/src/config.rs:18–21`
**Description:** `CLIO_DB_PATH` accepted as-is and passed to `Connection::open()`.
**Fix:** Validate extension or log a warning for unusual paths.

### L7 — Fragile `unwrap()` After Guard Check
**File:** `crates/clio-core/src/repository.rs:380, 467`
**Description:** `q.scoring.as_ref().unwrap()` after `use_scoring` guard — fragile if guard logic changes.
**Fix:** Use `if let Some(scoring) = q.scoring.as_ref()` instead.

### L8 — Namespace Written Without Sanitisation
**File:** `crates/clio-core/src/context.rs:156–159`
**Description:** `init_namespace()` writes caller-supplied namespace to file with no validation for control characters.
**Fix:** Validate using the same rules as `validate::remember_input()`.

### L9 — Internal File Paths in Error Messages
**Files:** `crates/clio-core/src/db.rs:15–18`, `settings.rs:220–222`, `daemon.rs:128`
**Description:** Error messages include `path.display()` which exposes filesystem paths when returned to external callers.
**Fix:** Strip or abstract path info in externally-facing errors.

### L10 — Resource URI ID Not Validated in MCP
**File:** `crates/clio-mcp/src/main.rs:1520–1528`
**Description:** Extracted memory ID from `memory://item/{id}` is passed to the repository without format validation.
**Fix:** Validate against UUID format (`[0-9a-f-]{36}`) before passing to repository.

### L11 — Uncapped `limit` Parameter in MCP
**File:** `crates/clio-mcp/src/main.rs:123, 233`
**Description:** `limit: u32` defaults to 10 but has no upper bound. A caller can pass `limit: 1000000`.
**Fix:** Cap at a reasonable maximum (e.g. 500).

### L12 — Plist/XML Injection in LaunchAgent Generation
**File:** `crates/clio-cli/src/main.rs:2372–2401`
**Description:** `format!()` interpolates filesystem paths into XML without escaping. Paths containing `<`, `>`, or `&` produce malformed or attacker-controlled plist structure. `db_path` can be set via `--db-path` or `CLIO_DB_PATH`.
**Fix:** XML-escape all interpolated values, or use the `plist` crate for programmatic construction.

### L13 — API Key Leakage via Debug Output
**Files:** `crates/clio-cli/src/main.rs:1430, 1483–1490`
**Description:** `EmbeddingConfig` is printed via `{:?}` and serialised as JSON, potentially exposing the API key in terminal output.
**Fix:** Implement custom `Debug`/`Display` that redacts sensitive fields.

### L14 — Mutex Poisoning Panics in Tauri Commands
**File:** `crates/clio-tauri/src/commands/memory.rs:28` (all command handlers)
**Description:** All 12+ Tauri command handlers call `state.lock().unwrap()`. A single panic while holding the lock renders the entire app non-functional.
**Fix:** Replace `.unwrap()` with `.map_err(|_| CommandError::Config("state mutex poisoned".into()))?`.

---

## Informational

### I1 — No `unsafe` Code (Positive)
Confirmed: zero `unsafe` blocks across all crates. Excellent.

### I2 — SQL Parameterisation Sound (Positive)
All INSERT, UPDATE, DELETE, and SELECT queries use `rusqlite::params![]`. No string interpolation of user data into SQL was found.

### I3 — No `v-html` Usage in Vue Frontend (Positive)
All memory content rendered via `{{ }}` text interpolation with auto-escaping. No XSS risk from templates.

### I4 — API Keys Never Logged (Positive)
Settings load/save only log file paths at `debug` level, never content. API keys are not passed to tracing macros.

### I5 — Embedding BLOB Integrity
**File:** `crates/clio-core/src/embeddings.rs:380–384`
`decode_embedding()` uses `chunks_exact(4)` which silently ignores trailing bytes if blob length is not a multiple of 4, potentially producing truncated vectors.
**Fix:** Assert or return error if `blob.len() % 4 != 0`.

### I6 — Import JSONL Unbounded Memory
`import_jsonl` reads entire input via `BufReader::lines()`. Acceptable for a local tool.

### I7 — UTF-8 Boundary Panic in CLI Display
**Files:** `crates/clio-cli/src/main.rs:1832–1833, 1644–1645`
`&item.content[..300]` and `&preview[..120]` slice bytes without checking UTF-8 boundaries, risking a panic on multi-byte characters.
**Fix:** Use `char_indices()` for boundary-safe slicing, or the existing `truncate()` helper.

### I8 — PATH Fallback for Binary Resolution
**File:** `crates/clio-cli/src/main.rs:2049–2056, 2097–2104`
`find_mcp_binary()` and `find_daemon_binary()` shell out to `which` and trust the result. On a system with attacker-controlled PATH, this could execute a malicious binary. The adjacent-binary path is checked first, mitigating this.

### I9 — Semver Ranges in Dependencies (Mitigated by Cargo.lock)
All workspace dependencies use loose semver ranges. `Cargo.lock` is checked in, mitigating reproducibility concerns. Consider adding `cargo-audit` to CI.

---

## Recommended Priority Actions

1. **Set file permissions (0o600) on settings file and Unix socket** — Quick wins addressing H2, H5
2. **Enable Tauri CSP** — One-line config change addressing H6
3. **Add connection limits and line-length caps to control socket** — Addresses H3, H4
4. **Sanitise FTS queries** — Wrap in double-quotes or validate input, addressing H1
5. **Add size validation to MCP parameters** — Content, tags, metadata limits, addressing M7
6. **Validate namespace values** — Apply consistent rules from `validate` module, addressing M1
7. **Atomic settings write** — Write-then-rename pattern, addressing M9
8. **Resolve symlinks in inbox watcher** — `canonicalize()` check, addressing M6
9. **Cap batch sizes and graph depth** — Addresses M2, M3
10. **Replace `DefaultHasher` with stable hash** — Addresses M10
