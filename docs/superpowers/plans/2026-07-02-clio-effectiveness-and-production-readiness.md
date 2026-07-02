# Clio Effectiveness & Production-Readiness Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Clio maximally effective as a shared memory system for AI agents and production-ready as a product: fix the confirmed data-loss and correctness bugs, sharpen the model-facing MCP surface, close the remaining retrieval-quality gaps, and harden the hook integration that feeds the system.

**Source:** Four-track review on 2026-07-02 (retrieval efficacy, MCP surface as experienced by models, hook integration, engine performance/reliability). All P0 findings verified against code and logs before inclusion.

**Architecture:** All engine changes live in `crates/clio-core`; MCP changes in `crates/clio-mcp` are adapter-surface only (descriptions, defaults, tool shape); hook changes live outside this repo at `~/Ai/Assets/Claude/Skills/clio-hooks/scripts/` (source of truth for the symlinked skill).

**Tech Stack:** Rust, `rusqlite` (SQLite WAL + FTS5), fastembed, Python hook scripts, `cargo test -p clio-core`.

---

## Assessment (the honest opinion)

**The concept is right and the engine is in good shape.** Local-first SQLite, one core crate with thin adapters, archive-not-delete, upsert keyed on `source + source_ref`, savepoint-wrapped migrations, no panics reachable from user input, no injection vectors — the foundations are production-grade. The recent retrieval/lifecycle work (composite scoring in semantic recall, write-path dedup, `valid_until`) landed correctly.

**The weakest layer is not the engine — it is the seam between Clio and the agents that use it.** Three things hold effectiveness back:

1. **The capture pipeline silently loses knowledge.** The session-stop hook has failed at least six times with a `UNIQUE constraint` collision (verified in `~/.claude/.clio-hook-log.txt`), every hook failure exits 0 with no user-visible signal, and nothing ever measures whether captured memories are subsequently recalled. A memory system whose write path fails silently and whose value is never measured cannot improve.
2. **Models are not taught how to use the tools.** The MCP server instructions are ~55 words for 22 tools; namespace/`cwd` precedence, `recall` vs `recent` vs `search`, embedding availability, and the six `memory_context` presets are all undocumented at the point of use. Responses default to markdown, costing roughly twice the tokens of JSON for a model that wants structured data.
3. **Hybrid retrieval is two halves, not a whole.** Keyword recall and semantic search are disjoint paths with non-comparable scores; the semantic path's keyword boost is a hard-coded flat `0.3` regardless of match strength; FTS forces implicit AND and has no stemming.

**Scale items (ANN index, MCP read concurrency, statement caching, shared embedding process) are real but not urgent** at the current corpus size — they are gated in Phase 4 with explicit triggers rather than built speculatively.

**Multi-tool claim vs reality:** every tool can reach Clio over MCP, but only Claude Code has lifecycle hooks (context injection + capture). That is the single biggest efficacy multiplier available: the same brief/distill pattern ported to one more tool doubles the knowledge inflow.

Public-readiness items (LICENSE, npm registry, CI scanning, SECURITY.md) are already tracked in `READINESS-TODO.md` and deliberately **not** duplicated here. Team sync is covered by `2026-06-27-clio-team-hub-sync.md`.

---

## Global Constraints

- Archive means hidden, not deleted. Archived records stay excluded from default recall.
- Tags and FTS must stay in sync (triggers handle this; preserve them).
- Upsert keyed on `source + source_ref` must not create duplicates.
- Access tracking is fire-and-forget — never fail the parent operation.
- `decay_lambda = 0.0` must preserve original ranking (backwards-compatible).
- MCP defaults must match CLI/core semantics exactly; any new `RecallQuery` field defaults to the backwards-compatible value (`#[serde(default)]`).
- Every schema change ships as a new migration — never edit an applied one; keep `docs/reference/schema.md` in step.
- British English; conventional commits; stage with `gitaddall`.
- Every task leaves `cargo test -p clio-core` green.

---

## Phase 0 — Confirmed bugs (data loss and correctness; do first)

### Task 0.1 — Stop-hook capture collision loses session knowledge (S)

**Where:** `~/Ai/Assets/Claude/Skills/clio-hooks/scripts/session_stop.py` (outside this repo); optionally `crates/clio-cli` distill command.

**Evidence:** 6 × `UNIQUE constraint failed: memories.source, memories.source_ref` in `~/.claude/.clio-hook-log.txt` (e.g. lines 6, 11). Two plausible causes — a re-fired Stop event for the same session slipping past the `.done` marker, and `clio distill` extracting **multiple** memories that all share `source="claude-code-session"`, `source_ref=<session_id>`, so the second insert collides with the first.

- [ ] Reproduce: run `clio distill` with a digest that yields ≥2 memories; confirm whether the second insert collides.
- [ ] Fix so each stored memory has a unique key: suffix `source_ref` per memory (`{session_id}#{n}` or `#{content-hash}`), or pass `upsert` semantics deliberately if replacement is the intent. Fix belongs in whichever layer assigns `source_ref` (CLI distill or hook script) — keep core's upsert invariant untouched.
- [ ] Make the hook surface the failure: on non-zero `distill` exit, write a visible warning line to stderr (Claude Code shows hook stderr) as well as the CSV.
- [ ] Verify: re-run a multi-memory distill twice; zero constraint errors; both runs logged.

### Task 0.2 — `recall_scoped` pagination and total are wrong (S)

**Where:** `crates/clio-core/src/repository.rs:859-916`.

**Evidence:** the global fill query hard-codes `offset: 0` (line 886), so `offset > 0` never pages across the merged set; `total = scoped.total + global.total` (line 907) double-counts memories present in both namespaces.

- [ ] Decide the paging contract (simplest honest option: document that scoped recall ignores `offset` in the global fill, and return `total` as scoped-total only; better option: fetch `offset + limit` from both and page in-process).
- [ ] Implement + unit test: scoped result spanning both namespaces with `offset > 0`; overlapping IDs counted once.
- [ ] Verify MCP/CLI behaviour unchanged for `offset = 0` (default path).

### Task 0.3 — Tauri UI freezes on long semantic searches (M)

**Where:** `crates/clio-tauri/src/lib.rs:23-29` (AppState), `crates/clio-tauri/src/commands/search.rs` and any command invoking `semantic_search`/`semantic_recall`.

**Evidence:** commands lock the single `Mutex<AppState>` and run the O(N) embedding scan synchronously; no `spawn_blocking` in command handlers.

- [ ] Wrap semantic search/recall command bodies in `tauri::async_runtime::spawn_blocking` so the scan doesn't block the main thread.
- [ ] Verify: trigger a search from the UI while clicking around; no freeze. `cd ui && npm run build` stays green.

### Task 0.4 — WAL checkpoint hygiene (S)

**Where:** `crates/clio-core/src/db.rs:43-52` (`apply_pragmas`).

- [ ] Add `PRAGMA wal_autocheckpoint = 1000;` alongside the existing pragmas.
- [ ] Daemon: issue `PRAGMA wal_checkpoint(PASSIVE);` on shutdown.
- [ ] Verify: `-wal` file stays bounded after a batch of writes from a long-lived process.

---

## Phase 1 — Model-facing effectiveness (MCP surface)

The cheapest, highest-leverage phase: nothing here touches core logic.

### Task 1.1 — Rewrite the MCP server instructions (S)

**Where:** `crates/clio-mcp/src/main.rs:1745-1749`.

Current instructions are ~55 words and omit the things models get wrong. Expand to ~150 words covering:

- [ ] Namespace resolution order: explicit `namespace` → auto-detect from `cwd` → `global`; recommend always passing `cwd`.
- [ ] Tool choice: `recall` = keyword/FTS, `search` = semantic (needs embeddings; may error if unconfigured), `recent` = no-query listing, `remember` = deliberate store, `capture` = LLM-classified store that may queue to the inbox.
- [ ] Archive is soft-delete; excluded from recall by default.
- [ ] `response_format: "json"` for structured processing; markdown for display.
- [ ] `memory_context` preset names listed inline.
- [ ] Verify: restart MCP, inspect instructions via a client; docs/reference/mcp-contract.md updated to match.

### Task 1.2 — Trim the tool count: merge `recall`+`recent`, unify the inbox (M)

**Where:** `crates/clio-mcp/src/main.rs` tool definitions; `docs/reference/mcp-contract.md`.

- [ ] Make `memory_recall.query` optional: absent → recent-style listing (core already supports both paths). Deprecate `memory_recent` (keep as alias for one release, then remove).
- [ ] Collapse `memory_inbox_list/approve/reject/edit` into `memory_inbox(action, review_id?, …overrides)`.
- [ ] Net effect: 22 → ~17 tools, ~20% schema-token saving per session, and no wrong-variant guessing.
- [ ] Verify: contract doc updated; MCP defaults still match CLI/core semantics exactly.

### Task 1.3 — Fail fast and teach failure modes (S)

**Where:** tool descriptions in `crates/clio-mcp/src/main.rs`.

- [ ] `memory_search` / `memory_suggest_links` descriptions state upfront: "requires a configured embedding backend; returns a configuration error otherwise".
- [ ] `memory_remember` description states: upsert requires **both** `source` and `source_ref`, otherwise a new record is inserted.
- [ ] `memory_suggest_links.threshold` description explains cosine similarity direction ("lower = more permissive; 0.9+ = near-identical").
- [ ] Optional: add `available: bool` to `memory_stats` output (or a field in instructions) so agents can detect embedding availability without a failed call.

### Task 1.4 — Cheaper responses for models (M)

**Where:** `crates/clio-mcp/src/main.rs` response formatting.

Markdown-by-default costs a model roughly 2× the tokens of JSON per recall. Changing the default breaks compatibility, so:

- [ ] Slim the markdown card: drop per-card fields models rarely need (rank, full timestamps) behind `response_format: "json"`.
- [ ] Recommend `json` in the server instructions (Task 1.1) rather than flipping the default.
- [ ] Verify: 10-result recall payload measured before/after; target ≥30% reduction in the markdown path.

---

## Phase 2 — Retrieval quality

### Task 2.1 — Make the keyword boost proportional, not flat (M)

**Where:** `crates/clio-core/src/embeddings.rs:580-671` (`semantic_recall`, `KEYWORD_BOOST = 0.3`).

A weak FTS match earns the same +0.3 as a strong one, flattening the signal; cosine+boost can also exceed 1.0.

- [ ] Scale the boost by normalised BM25 strength of the FTS hit (e.g. `boost × bm25_rank_percentile`), or switch the fusion to Reciprocal Rank Fusion (rank-based, scale-free — the more robust option).
- [ ] Preserve the invariant: `decay_lambda = 0.0` keeps pure-semantic ordering unchanged when there are no FTS hits.
- [ ] Add a fixture test: strong-FTS+weak-semantic vs weak-FTS+strong-semantic ordering is sane.

### Task 2.2 — FTS expressiveness: stemming and operators (S/M)

**Where:** `crates/clio-core/src/repository.rs:590-605` (sanitiser); FTS table definition via a new migration.

- [ ] Enable the porter tokeniser on the FTS table (new migration + reindex) so `test` matches `tests`. This is the bigger win for agent-generated queries.
- [ ] Optionally allow `OR` through the sanitiser (quote only literal terms). Lower priority — agents rarely emit boolean syntax.
- [ ] Document tokeniser behaviour in `docs/reference/schema.md`.

### Task 2.3 — Context assembly: token budget and de-duplication (M)

**Where:** `crates/clio-core/src/assembly.rs:142-202`.

- [ ] De-duplicate memories by ID across preset sections (a decision tagged as constraint currently appears twice).
- [ ] Add an optional `char_budget` to `ContextRequest`; truncate sections greedily when set. Session-start hooks should pass it so briefs never balloon.
- [ ] Verify: existing preset tests green; new test for budget truncation and cross-section dedup.

### Task 2.4 — Dedup hardening (S)

**Where:** `crates/clio-core/src/repository.rs:259-274` (`find_content_duplicate`).

- [ ] New migration: index to make the exact-match probe cheap at scale — a `content_hash` column (or expression index) beats indexing full content.
- [ ] Decide the archived-duplicate rule: currently a re-captured memory whose twin is archived creates a fresh row. Preferred: detect the archived twin and unarchive it instead.

---

## Phase 3 — Hook & capture pipeline robustness

All hook script changes happen in `~/Ai/Assets/Claude/Skills/clio-hooks/scripts/` (source of truth; symlinked into `~/.claude/skills/`).

### Task 3.1 — Kill silent failure (S)

- [ ] Un-hardcode `/Users/dannyharding/.cargo/bin/clio` in `session_start.py` and `session_stop.py`: use `$CLIO_BIN` env override, then `shutil.which("clio")`, then the cargo default.
- [ ] On any hook failure, emit one plain-English stderr line (visible in Claude Code) in addition to the CSV row.
- [ ] Add a `clio doctor`-style check to the report script: daemon up? DB reachable? last N hook outcomes?

### Task 3.2 — Stop-hook latency (M)

The distill LLM call blocks session end by 10–30 s.

- [ ] Detach the distill step: write the digest to a spool file and exit; the daemon (or a background `clio distill --spool` invocation via `nohup`) processes it. Session end becomes instant; capture becomes at-least-once with the spool as the retry queue.
- [ ] Verify: stop hook completes < 2 s; spooled digest appears as memories within a minute.

### Task 3.3 — Measure capture value (M)

Nothing currently tells you whether captured memories are ever used — the one metric that justifies the whole pipeline.

- [ ] Extend `clio_report.py`: for memories with `source = "claude-code-session"`, report the recall rate (`access_count > 0` share) by age bucket.
- [ ] Use the number to tune the distillation prompt and the inbox `review_threshold`; note current junk-rate in the report.
- [ ] Surface pending-inbox count in the session-start brief so the review queue stops silently accumulating.

### Task 3.4 — Wire lifecycle jobs into the daemon (M)

**Where:** `crates/clio-daemon`; `crates/clio-core/src/{cleanup.rs,consolidate.rs,backup.rs,integrity.rs}`.

Consolidation, cleanup, backup, and integrity checks all exist and all require manual invocation — functionality shipped but not running.

- [ ] Add configurable daemon intervals (settings keys, off by default where destructive): weekly backup, weekly `integrity::check` (log-only), consolidation `--if-due` per active namespace.
- [ ] Cleanup stays manual (it deletes namespaces) but the daemon should *report* stale-namespace candidates in `clio daemon status`.
- [ ] Update `docs/reference/settings.md` with the new keys.

### Task 3.5 — Smarter session-start recall (S)

**Where:** `session_start.py` branch recall.

- [ ] Replace the exact branch-name FTS query with `clio search` (semantic) using the branch name *and* recent commit subjects as the query text, falling back to FTS when embeddings are unavailable.

### Task 3.6 — Port hooks to a second tool (M, strategic)

The README's multi-tool story is MCP-only outside Claude Code. Pick the one other tool actually used daily (Codex per the machine setup) and port the pattern:

- [ ] Session-start context brief (its automation/profile mechanism) + session-stop digest→distill.
- [ ] Reuse the same CLI commands; scripts should share the digest/distill core with the Claude Code versions rather than forking.
- [ ] Until then, soften the README claim to "shared via MCP; lifecycle hooks currently Claude Code-only".

---

## Phase 4 — Scale (gated; do not build speculatively)

| Task | Trigger to start | Effort | Sketch |
|---|---|---|---|
| 4.1 MCP read concurrency | >1 agent regularly blocked (measure lock wait) | M | Keep the write connection + mutex; add 2–4 read-only connections (WAL supports concurrent readers) picked per read-tool call. |
| 4.2 Prepared-statement cache | profiling shows parse overhead | M | Swap hot-path `prepare` → `prepare_cached` in `repository.rs` / `embeddings.rs` (~20 sites); connections are long-lived now so it pays off. |
| 4.3 ANN for semantic search | >10–20k embeddings or p95 semantic query >50 ms | L | `sqlite-vec` virtual table beside `memory_embeddings`; new migration; brute-force fallback for small corpora. |
| 4.4 Shared embedding process | ≥3 concurrent MCP agents (RAM: ~50 MB × N) | L | Daemon owns the fastembed model; MCP requests embeddings over the local socket; also removes the MCP startup model-load tax. |

Add a `clio stats --perf` counter (embedding count, p95 semantic latency, lock-wait) so the triggers are observable rather than guessed — S effort, do this part now.

---

## Explicitly not in this plan

- **Public-readiness** (LICENSE, Verdaccio/npm, gitleaks, audits, SECURITY.md, privacy note) — tracked in `READINESS-TODO.md`.
- **Team hub sync** — `docs/superpowers/plans/2026-06-27-clio-team-hub-sync.md`.
- **PreCompact hook** — blocked on Claude Code exposing the event usefully; revisit later.
- **Batch `memory_remember` / metadata-field search** — no demonstrated need yet (YAGNI; revisit if an agent workflow hits it).

## Verification baseline

Before starting: `cargo test -p clio-core` green, `cargo clippy` clean, `cd ui && npm run build` green. Each task ends green and independently committable.
