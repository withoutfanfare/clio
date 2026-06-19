# Clio Revival — Design Spec

**Date:** 2026-06-19
**Status:** Approved design → ready for implementation planning
**Author:** Danny Harding (with Claude Code)

## Goal

Bring Clio from dormant (~3 months idle; last commit 2026-03-29) to a **trustworthy
daily-driver shared memory** used across all five MCP-capable AI coding agents in
Danny's daily rotation: **Claude Code, Codex, Gemini, Kimi, OpenCode**. They all
read and write **one shared `memory.db`** via a single `clio-mcp` server.

This is a *revive*, not a rebuild. The audit (2026-06-19) found the codebase is in
good structural shape: clean compile (0 warnings on the four non-Tauri crates),
healthy dependencies (patch/minor bumps only, no major migrations pending), all
five target agents already wired into `clio setup`, and the per-request DB-open
performance smell already resolved in MCP and Tauri.

## Strategy

Chosen sequencing: **clean tree → fix by severity → docs → rollout** (quality-first,
loses no in-flight work, protects the DB before centralising it). Data-integrity
bugs come first because the SQLite DB is the asset being centralised across five
agents; a corruption bug that bites during dogfooding is the worst outcome.

## Success criteria

- Data-integrity bugs fixed, each covered by a regression test that fails before
  the fix and passes after.
- MCP contract is honest: documented params/defaults/response shapes match the code.
- All five agents connect to one shared DB and complete a remember → recall →
  search → inbox round-trip.
- UI builds clean (`vue-tsc --noEmit`) and surfaces errors instead of swallowing them.
- Docs accurate: every documented command/flag exists in code.
- Dependencies current; full `cargo build` + `cargo test` + `npm run build` green.
- A small regression-test baseline + MCP smoke-test script exist so the project
  can't silently rot again.

## Verification approach

- Automated: `cargo test`, `cargo clippy`, `cargo build` (all crates including Tauri —
  attempt the full Tauri build and report any headless/system-dep limits), and
  `cd ui && npm run build`.
- Manual (Danny drives): Tauri app run + UI click-through at phase gates; live
  multi-agent dogfooding in Phase 6.

---

## Phase 0 — Clean the tree (no work lost)

Assessed in-flight work; all of it is valuable, nothing discarded.

- **Commit docs** — 11 modified doc files + the intentionally-edited `CLAUDE.md` as
  one coherent docs-polish commit (diagrams, troubleshooting, cross-linking).
  Both referenced files (`docs/performance-audit.md`, `docs/guides/moving-project.md`)
  confirmed to exist — no dead links.
- **Commit UI tweaks** — the 2 modified Vue components (`MemoryDrawer.vue`
  delete-confirmation bar, `MemoryPage.vue` layout fix) as a separate commit
  (different concern).
- **Merge feature branch** — `feature/virtualisation-namespace-colours-quickswitch`
  (virtual scrolling, namespace colours, quick-switch dropdown): net-new, no
  duplication on develop. Rebase onto develop after the UI commit, resolve the
  small `MemoryPage.vue` `<style>` overlap, run one scroll smoke-test (hardcoded
  row heights `LIST_CARD_HEIGHT=120`/`GRID_CARD_HEIGHT=180` are assumptions to
  verify), then `--no-ff` merge.

**Gate:** clean working tree; `cargo build` + `npm run build` green.

## Phase 1 — Data integrity (TDD: reproduce → fix → prove)

Each bug gets a failing test first, then the fix. No schema changes in this phase
(so no migrations).

- **`merge_memories` broken + non-atomic** (`deduplication.rs:206-211,157-244`) —
  CRITICAL. Tag re-insert omits `created_at` (NOT NULL) so any merge of a tagged
  memory fails mid-operation; no savepoint means the kept memory is left partially
  mutated. Fix: add `created_at` to the insert **and** wrap the whole function in a
  savepoint.
- **Backup/restore unsafe WAL copy** (`backup.rs:71-86,174-194`) — switch to
  `VACUUM INTO` / SQLite online-backup API (checkpoint first); never restore a
  foreign `-wal`/`-shm`; snapshot the current DB before overwriting on restore.
- **`recall_scoped` inflated total + broken paging** (`repository.rs:827`) — compute
  total from distinct ids; fix the global pass using `offset: 0` while scoped uses
  the caller's offset.
- **Validation byte-vs-char length** (`validate.rs:31,41,48,56,100`) — use
  `.chars().count()` to match schema `length()` CHECK semantics.
- **FTS over-quoting degrades multi-term search** (`repository.rs:533-536`) — quote
  each token individually (reuse `deduplication::build_fts_query`) instead of
  wrapping the whole query in one phrase.

Cheap correctness wins folded in:
- dedup/preview internals use `get_raw` not `get` (stop inflating `access_count`
  on read-only scans) (`deduplication.rs:103,109,162,169,268,417`).
- remove the always-zero migrate `duplicates` counter (`migrate.rs:208,319`).
- batch the `auto_link` `has_embedding` N+1 into one query (`embeddings.rs:820-826,876`).

**Gate:** new integration tests pass; `cargo test` + `cargo clippy` clean.

## Phase 2 — MCP & agent correctness

The five agents depend on this layer being honest.

- **Inbox param `id` → `review_id`** (`clio-mcp/src/main.rs:385,391,397`) — HIGH; the
  drift most likely to break an agent following the contract.
- **`memory_capture` response shape** (`clio-mcp/src/main.rs:1298-1334`) — document
  the two-variant (`Stored` / `Queued`) response in the contract.
- **Document the `global` flag** on `memory_recall`/`memory_search`
  (`clio-mcp/src/main.rs:103,277`) and **validate `sort_by`** instead of silently
  dropping invalid values (`:1048,1122`).
- **Harden `clio setup`** — escape Codex TOML via the `toml` crate
  (`clio-cli/src/main.rs:2838-2840`); add `~/.cargo/bin/clio-mcp` to the binary
  search (`:2307-2327`).
- **Daemon policy out of the adapter** — move hardcoded inbox namespace/importance
  into settings (`clio-daemon/src/watcher.rs`); log dropped file-watch events.

**Gate:** each of the five agents connects; a scripted MCP round-trip (remember →
recall → search → inbox) passes.

## Phase 3 — UI robustness

- Surface the clipboard error instead of silent fallback (`utils/memoryExport.ts:41`).
- Clean up untracked `setTimeout`s writing to unmounted components
  (`MemoryDrawer.vue:119`, `MemoryPage.vue:57`) and the `ContextBuilderView`
  search timer (`:130-153`).
- `await` and surface `store.loadRecent()` errors after archive/delete (4 sites).
- Reset `confirmingDelete` when the dropdown closes (`MemoryPage.vue:82-93`).
- Normalise `--colour-*` / `--color-*` token usage (`MemoryDrawer.vue:625-684`,
  `ContextBuilderView.vue:716,834` — add the missing `--colour-surface-hover` alias).
- Drop the dead `shiftKey` param on `toggleSelection` (`stores/memories.ts:175`).

**Gate:** `npm run build` (vue-tsc) clean; manual click-through.

## Phase 4 — Documentation accuracy

The biggest single debt.

- **Rewrite `docs/cli-reference.md`** against the actual clap source: fix the 6
  commands that fail verbatim (e.g. `clio embed --all` → `clio embed backfill`;
  `clio settings use-local-embeddings` → `clio settings use-local`; `search`/`capture`
  take positionals not `--query`/`--text`); add the 9 undocumented commands
  (`serve`, `setup`, `daemon`, `cache`, `inbox`, `schema`, `move`, `brief`, and
  nested subcommands).
- **MCP contract** — add `memory_cache_clear` (22 tools, not 21); apply the
  `review_id`, capture-shape, and `global` corrections from Phase 2.
- **Settings reference** — add the `auto_title` block (`enabled`, `api_key?`,
  `base_url?`, `model?`; falls back to `capture.*` then `OPENAI_API_KEY`).
- **CHANGELOG** — back-fill a `0.4.0` entry covering the ~25 post-0.3.0 commits
  (AI titles, Tauri desktop release, design tokens, namespace colours/quick-switch,
  perf optimisations, deduplication, context builder).
- **Reconcile the version** across `Cargo.toml`, `tauri.conf.json`, `package.json`
  (currently `0.1.0`) vs the changelog's `0.3.0`/`0.4.0` scheme.

**Gate:** spot-run a sample of documented commands/flags; each exists in code.

## Phase 5 — Dependency currency

- `cargo update` (patch/minor only — no majors pending).
- **Verify rmcp 0.17** is still current and protocol-compatible — pre-1.0 and
  fast-moving, the one real watch-item.
- Minor bumps: tauri (2.10→2.11), tokio, clap, fastembed, plugins.
- UI bumps need the local Verdaccio registry (`localhost:4873`) up for
  `@stuntrocket/ui`; otherwise UI deps are current-generation for mid-2026.

**Gate:** full `cargo build` + `cargo test` + `npm run build` green post-bump.

## Phase 6 — Roll out & dogfood

- `clio setup` for all five agents (Claude Code, Codex, Gemini, Kimi, OpenCode)
  against one shared `memory.db`.
- Live dogfooding across agents.
- Lock in a small regression-test baseline + an MCP smoke-test script so the
  project can't silently rot again.

**Gate:** all five agents read/write the shared DB in real use; smoke-test script
passes.

---

## Deferred backlog (explicitly NOT this pass)

Captured to avoid gold-plating; revisit later:

- **`archive`/`unarchive` bump `updated_at`** (`repository.rs:864,881,904`), corrupting
  the activity feed and "updated" classification — fix needs a new schema migration
  (`archived_at`), so deferred.
- **Union-find path compression** and **semantic ANN / `sqlite-vec`** — O(N)/O(N²)
  scaling limits in dedup and semantic search; fine at current scale.
- **Split the 1,360-line `repository.rs`** into `recall`/`links`/`namespace`/`bulk`
  submodules.
- **Scope the recall-cache invalidation** by namespace instead of clearing all on
  every write (`cache.rs:351-353`).

## Risks & watch-items

- **rmcp 0.17** pre-1.0 churn — verify before relying on it across five agents.
- **Tauri headless build** may hit system-dep/display limits in this environment —
  attempt and report; fall back to Danny driving the app for UI verification.
- **Verdaccio registry** must be up to build/refresh the UI (`@stuntrocket/ui` is
  private).
- **Feature-branch row-height assumptions** — verify virtual-list scrolling against
  real card heights during the Phase 0 smoke-test.
