# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

**Clearer desktop scope and retrieval (2026-09-05)**
- Browsing loads further pages, preserves depth on refresh and resolves pinned
  memories independently. Workspace search, shortcuts, dates and search excerpts
  make collection context clearer. Context briefs persist across sessions.
- Statistics consistently use the selected namespace and echo that scope.
  Attention projects eligible evidence titles without recording access.

**Distillation output hard-capped at 6 memories (2026-08-07)**
- One distillation call now stores at most 6 memories: 5 knowledge atoms plus
  the single session receipt. The prompt states the limit and a stricter
  leave-it-out bar; `parse_distillation` enforces it deterministically, ranking
  the receipt first, then open loops, then importance with confidence as the
  tiebreak, and preserving original order so provenance refs stay stable across
  a retry. Motivated by live volume — 892 distilled memories stored on 6 August
  alone — where model-written text at output-token prices was the second-largest
  cost component and the largest source of recall noise.

**One namespace precedence for capture and distill (2026-07-30)**
- `capture` now resolves namespaces through the same rule as `distill`: explicit
  override → the model's `global` promotion → the working directory → the model's
  suggestion. Previously the working directory silently overrode a model's `global`
  promotion for `capture` only, so the same classification could land in different
  namespaces depending on which command stored it. `capture --dry-run` reports the
  model's raw `suggested_namespace` alongside the resolved one.

**Auto-link excludes boilerplate kinds (2026-07-30)**
- New `daemon.auto_link.exclude_kinds`, default `["receipt"]`: excluded kinds are
  skipped as both link source and link target. Measured on live data at threshold
  0.6, receipts averaged 4.86 links each against 2.03 for `fact` — the most
  substantive kind was the least connected, and 807 receipts consumed roughly a
  third of all link mass, because session write-ups share phrasing and so attract
  each other on similarity while carrying little conceptual content.
- `suggest_links` is unchanged for explicit callers; auto-linking uses a new
  `suggest_links_excluding_kinds`, which filters in SQL so excluded candidates
  cannot consume slots from the per-memory limit.
- `max_links_per_memory` now bounds **total degree** — inferred links in both
  directions — rather than outgoing links only. Recall traverses edges both ways,
  so an inbound link costs recall exactly what an outbound one does, and the old
  outgoing-only count let heavily-pointed-at memories grow without bound (observed
  at total degree 23 against a configured cap of 5). A link is now created only
  while both of its endpoints are below the cap; memories already over it gain
  nothing further. Existing over-cap links are left in place.

### Added

**Desktop Archive and capture review (2026-09-05)**
- Explicit Archive/Restore browsing with `archived_only` recall/recent filtering
  and recent offsets across core, CLI, MCP and desktop adapters.
- A capture inbox for reviewing, editing, approving and rejecting unresolved
  captures, with recoverable suggestions and metadata-only local queue diagnostics.
- MCP inbox JSON lists optionally return `items` and `includes_edited` when
  `include_status_scope` is true. Existing callers retain array responses; the
  desktop requires this confirmation before accepting a remote inbox.
- [Implementation evidence and remaining release checks](docs/reviews/2026-09-04-app-improvements.md)
  record core/UI/adapter checks, native restart/export/remote workflows and the disposition of three follow-up reviews.

**Per-checkpoint token accounting and `clio usage` (2026-08-07)**
- Migration `014_checkpoint_usage` adds model and token columns to
  `session_checkpoints`; the checkpoint path (core and MCP) stamps them
  best-effort after commit — fire-and-forget, like access tracking, so
  recording can never fail a capture. `clio usage [--days N]` aggregates
  calls, input (with cached portion), output and reasoning tokens per UTC
  day, with pre-recording checkpoints surfaced as `unrecorded` rather than
  counted as zero. Replaces character-count estimates for cost questions.

**Reproducible capture-model benchmark (2026-08-07)**
- `scripts/bench/capture-model/` holds the five distillation cases as
  checked-in fixtures with an automated judge and runner, so the comparison in
  `docs/operations/capture-model-benchmark.md` is one command against a
  throwaway DB instead of archaeology. The 7 August rerun with cheap
  candidates is recorded there.

**Scheduled auto-linking (2026-07-30)**
- New `clio auto-link` runs a single auto-link inference pass, so link inference can
  be driven by a systemd timer or cron instead of requiring the daemon. Intended for
  hosts that hold the shared database but run no daemon: the release process already
  updates the `clio` binary, so a timer cannot drift out of step the way an
  separately-installed daemon would.
- Defaults to scanning every memory, because the daemon's in-memory watermark has no
  equivalent in a one-shot run. This is affordable since a memory already at
  `max_links_per_memory` is skipped before any similarity search; `--since` narrows
  the scan if a full pass ever becomes slow. `--threshold` overrides the configured
  value for one run.

### Fixed

**Desktop draft protection and queue visibility (2026-09-05)**
- Editor close waits for saves; failed or conflicting writes retain recoverable
  drafts. Creation uses the selected workspace and preserves explicit destinations.
- Edited captures remain in the unresolved inbox until confirmed approval or
  rejection. Stale requests cannot replace newer workspace results or reopen old evidence.
- Modal keyboard handling, palette result scrolling, labels, text contrast and
  reduced-motion support improve desktop accessibility.
- Unreadable recovery records and failed discards block replacement; same-memory
  fetch races preserve confirmed edits. Palette navigation respects IME composition.
- Attention titles respect namespace scope. Workspace purge rejects `global`
  before backup, stored briefs validate rendering fields, and future spool timestamps
  no longer hide bucket counts. The disposable fixture disables CORS explicitly.

**Dead-letter recovery and SSH session isolation (2026-08-07)**
- Long-lived `clio remote-mcp` bridges now use dedicated SSH connections instead
  of occupying channels on the multiplexed connection used by short-lived CLI
  and hook commands. This prevents persistent MCP clients from exhausting the
  server's per-connection session limit and causing capture commands to fail
  with `Session open refused by peer`.
- `clio checkpoint --recover-stale` lets an operator restore a retained older
  session delta after later cursors have committed. Normal CLI and MCP capture
  still reject stale cursors, while the recovered checkpoint keeps the same
  exact-key replay protection as every other checkpoint.
- Remote CLI forwarding now honours `CLIO_CONTEXT_CWD`, preserving the original
  session path in recovered jobs even when that worktree no longer exists.

**30 July review fixes (2026-07-30)**
- `clio-healthcheck` no longer records an alert as sent when the Slack delivery
  failed — a fault during a webhook outage was previously never reported at all.
  It now also requires auto-link's success line at the end of the log instead of
  grepping a five-line tail for failure words, which missed a dead binary and a
  fully skipped batch. A self-test (`clio-healthcheck-selftest.sh`) drives the
  alert state machine against a local fake webhook. Alerts point to the local log
  without copying provider errors or other raw log content into Slack.
- `clio auto-link` honours `daemon.auto_link.enabled` (with `--force` to
  override), exits non-zero when memories were skipped because no embedding could
  be produced instead of reporting a clean empty run, keeps walking the corpus
  past a wholly skipped batch, and uses the same advisory lock as the daemon so
  automatic and manual passes cannot interleave. Its cursor carries both timestamp
  and ID so a batch boundary cannot skip memories sharing one timestamp, and degree
  query errors fail the pass instead of being treated as zero links. Link-write
  failures log at warn rather than debug, so contention no longer looks quiet.
- Provider keys from settings and the environment are trimmed before use, and blank
  configured values fall through to environment resolution. Keys can no longer
  appear in `Debug` output.
- `dial-in.sh` asserts its relink the way it asserts its link-clear, copies
  WAL-mode databases with `.backup` instead of `cp`, aborts a trial when evaluation
  fails, and reports the captured error. `recall-eval.py` scores its two arms from
  identical snapshots, reports failed query pairs accurately, and refuses live
  paths resolved from platform defaults or environment configuration. The recorded
  threshold/cap sweep is annotated with its measured noise floor; the settings
  chosen from it are marked plausible, not confirmed.

**Release drains MCP sessions (2026-07-30)**
- `atlas-release.sh deploy` and `rollback` now send SIGTERM to lingering `clio-mcp`
  processes so clients reconnect against the release just activated. Swapping the
  `current` symlink does not affect a running process — it keeps the executable it
  already loaded — so a long-lived session previously served superseded code
  indefinitely, with the mismatch invisible from the client. Set
  `CLIO_KEEP_MCP_SESSIONS=1` to opt out. The match is exact on the process name and
  scoped to the invoking user. Note SIGTERM here is an immediate termination —
  `clio-mcp` installs no signal handler. The database stays consistent (WAL rollback,
  and the OS releases the advisory lock), but a request in flight at that instant is
  lost; its client sees a transport error and reconnects into the new release.

**Auto-link cap and dry-run namespaces (2026-07-30)**
- `daemon.auto_link.max_links_per_memory` is now a total per memory rather than a
  fresh allowance on every timed run. The auto-linker requested the full quota each
  pass, so inferred links accumulated without bound — memories were observed holding
  17 against a configured 3. The budget is now computed against existing inferred
  links, and a memory already at its cap is skipped.
- `capture --dry-run` and `distill --dry-run` report the namespace a memory would
  actually be stored under, instead of the model's raw suggestion which storage
  overrides. `distill`'s text output now shows the namespace, which it never did.
  `resolve_distill_namespace` is public so the preview reuses the storage rule
  rather than reimplementing it.
- Added `AUTO_LINK_RELATIONSHIP` so the `auto:relates_to` marker is defined once.

**Prompt cache visibility (2026-07-30)**
- Capture usage now records `cached_input_tokens` from the provider's
  `prompt_tokens_details.cached_tokens`, surfaced by `--metrics` and included in the
  JSON metrics output. The distillation system prompt is a ~1,200-token stable prefix
  on every call, so whether it is being served from the provider's prompt cache is the
  largest single influence on input cost — and was previously invisible. Providers that
  do not report the field leave it at zero.

**Attributable provider keys (2026-07-30)**
- `OPENAI_API_KEY_CLIO` is now preferred over the shared `OPENAI_API_KEY` wherever a
  provider key falls back to the environment (capture/distillation, OpenAI embeddings,
  auto-title). Resolution order is `api_key` in settings, then `OPENAI_API_KEY_CLIO`,
  then `OPENAI_API_KEY`.
- Falling back to the shared key logs a warning naming the caller, so a key reused
  across tools — which makes per-application billing attribution impossible — is
  visible rather than silent. Existing installs continue to work unchanged.
- A variable exported as an empty string is treated as absent, so a blank export falls
  through instead of sending an unauthenticated request.

**Dependable Workflow Memory (2026-07-29)**
- Exact-once session checkpoints (`clio checkpoint`, MCP `memory_session_checkpoint`, migration `009_session_checkpoints`): a session delta commits atomically under `source + session + cursor`; retries replay the stored result instead of duplicating atoms, and an empty extraction is a successful checkpoint.
- Follow-up attention lifecycle (`clio action`, MCP `memory_action`, migration `010_attention_and_events`): open/snoozed/resolved/cancelled items with due/reminder/trigger/waiting context, machine-readable eligibility reasons, evidence-backed completion via `resolved_by` links, and an append-only `memory_events` ledger with idempotency keys.
- Open-loop extraction in checkpoints: explicit user commitments open attention automatically; assistant suggestions always queue for review (approval creates memory + attention in one transaction); explicit stable-ID resolutions complete their target with the storing memory as evidence; deterministic `ticket:<id>` tags and branch metadata.
- Evidence-backed resume briefs (`clio resume`, MCP `memory_resume`): open work first with the reason each item surfaces now, blocked items, constraints (with a modest global prior), recent decisions, prompt-relevant knowledge and recent activity — untracked reads, once-per-session surfacing, budget with critical-section reservations.
- Directional, typed graph recall: linked recall follows edges in both directions and preserves every edge's relationship and metadata (`link_context`); `memory_get_links` gained a `direction` parameter; link suggestions stay in the source memory's namespace and skip archived/expired candidates.
- Source occurrences and cited consolidation (migration `011_occurrences_and_namespace_state`): repeated exact evidence strengthens one canonical memory with append-only sightings; consolidation output is structured and must cite current source IDs (invalid candidates rejected), watermarked by a per-namespace mutation generation so staleness is provable.
- Verified external delivery outbox (core, migration `012_delivery_outbox`): stable delivery keys, attempt history, read-back-proved confirmation, retryable failure and stable-ID completion mirroring. Live Things/Linear adapters remain gated on proving their create + read-back contracts.
- Desktop "Needs attention" view (`/attention`): eligible follow-ups with reasons, Complete/Snooze/Cancel, review depth, client capture-queue health and consolidation freshness; link rows show direction, relationship and titles.
- Event-backed effectiveness reporting (`clio effectiveness`, `stats::effectiveness`): capture, attention, surfacing, contradiction, delivery and corruption measures from untracked reads only.
- Hook package (clio-hooks skill): durable redacted capture queue (`capture_queue.py`) with atomic 0600/0700 spool, serial per-session cursors, backoff and dead-letter state; queue-backed Claude/Codex Stop hooks (ordered non-overlapping deltas, non-git sessions captured, late Codex rollouts retried); resume-led SessionStart and first-substantive-prompt recall (`prompt_recall.py`) with per-session/topic dedup.

### Fixed

- Linked recall (`include_links`) reapplies the parent query's archive/expiry eligibility, so hidden memories can no longer re-enter results through graph expansion.
- Chat output limits per model family: GPT-5-family title requests get reasoning-token headroom (previously truncated to empty), and unknown OpenAI-compatible models get a bounded `max_tokens` instead of no limit.
- Automatic context injection no longer trains recall ranking: resume reads, attention creation and internal lookups are untracked; deliberate recall still counts.

- **Remote MCP bridge** - `clio remote-mcp` connects MCP clients to a private Clio database over SSH while detecting project namespaces on the client computer.

**Handoff Briefs & Receipts**
- New `handoff` context preset (`clio brief --preset handoff --query <ticket-id>`, and via MCP `memory_context`): assembles a ticket-pickup brief with three sections — Directly Relevant (FTS on the query), Active Constraints, and Recent Receipts — sized to the usual `--char-budget`. The query is required; relevance takes budget priority (at `max_items ≤ 12` the other sections are deliberately empty).
- New `receipt` memory kind: a short per-session record of what was done, what was left undone, and why the session stopped. Distillation emits at most one per session (importance 2, tagged `receipt`) when substantive work happened, and receipts are exempt from the session-noise title filter so they cannot be silently dropped.
- Ticket tag convention: memories stored while working a tracked issue carry `ticket:<issue-id>` (lowercase). Tags are FTS-indexed, so a handoff query for the ticket id finds them even when the content never mentions it. Documented in `context/DOMAIN_RULES.md` and the MCP server instructions.
- Codex session capture: a new `codex_stop.py` hook (in the clio-hooks skill, registered via `~/.codex/hooks.json`) digests Codex rollout transcripts and reuses the shared distillation pipeline with `source: codex-session`; `distill_to_clio` gained a `source` parameter (default unchanged for Claude Code).

**Knowledge Distillation**
- `distill` / `distill_and_store` in `clio-core::capture`: send a long body of text (e.g. a session transcript) to the LLM and extract **zero or more** self-contained, durable memories (decisions, facts, constraints, insights). Routine input yields nothing, so noise is filtered by design.
- `DistilledMemory` struct and `parse_distillation` (tolerant of bare arrays or `{"memories": […]}`, drops empty-content items).
- `is_session_noise` deterministic backstop in `parse_distillation`: drops memories whose title narrates the working session or commit mechanics (e.g. "Session Summary", "Commit Summary", "Exploratory session", "Recent commits on branch") for the cases where the LLM ignores the prompt's instruction not to.
- `distill_and_store` now takes a `default_namespace`: each memory falls back to the working directory's namespace (resolved by the CLI via `context::detect_namespace`) instead of the model's unreliable per-project guess. An explicit `--namespace` still wins, and the model may still promote a genuinely cross-project fact to `global`. Stops session memories landing in the wrong drawer (e.g. project work filed under `project:notes`).
- Distillation/classification prompts now carry a strict 1–5 importance rubric so the score actually discriminates (most memories are 3; 4–5 reserved for invariants and consequential decisions) instead of clustering at 4.
- `clio distill` CLI command (stdin via `-`, `--dry-run`, `--source`, `--source-ref`, `--namespace`).
- Distilled memories from one session get a per-index `source_ref` suffix (`<ref>-<n>`) so the `UNIQUE(source, source_ref)` index is respected while keeping a shared session prefix for provenance.
- Reuses the existing capture pipeline per memory (review-queue routing below `review_threshold`, auto-embed) via a shared `store_or_queue` helper.

**Namespace Cleanup**
- New `clio-core::cleanup` module: `find_candidates` flags stale namespaces by age, all-archived state, or a missing project folder (the "folder gone" heuristic, which prunes the disk scan at project roots); `execute_cleanup` purges them after taking a database backup.
- `CleanupConfig` settings: `stale_months` (default 6), `dev_roots`, `record_cwd`.
- CLI: `clio cleanup` (dry-run by default; `--stale-months`, `--archived`, `--folder-gone`, `--execute`) and `clio delete <id>` (previously the CLI had no delete).
- Desktop app: a "Find stale" panel in the Namespaces view lists candidates with reasons and purges the selected ones (backup taken first). Backed by `cmd_find_cleanup_candidates` / `cmd_run_cleanup`.

**Memory Consolidation**
- New `clio-core::consolidate` module: rolls a namespace's atomic memories into a single AI-curated "consolidated memory" document. It is a *derived cache* — each run reconciles fully from the current memories (no iterative self-edit, so no drift) and leaves the atomic memories untouched.
- Stored as a singleton per namespace (`kind = summary`, `source = clio-consolidate`, `source_ref = <namespace>` for per-namespace uniqueness), upserted in place.
- The `project-brief` context now leads with the consolidated memory when one exists, so sessions open with the curated project summary.
- CLI: `clio consolidate [--namespace]`.
- Shared the OpenAI-compatible chat call across classify/distill/consolidate (`capture::chat`).
- `new_since_last_consolidation` helper counts memories added since the last run.
- Triggers: `clio consolidate --all` (every namespace) and `--if-due` (only namespaces past `consolidate.auto_threshold` new memories). The Stop hook runs `--if-due` after each productive session; a launchd plist can schedule `--all --if-due` (documented in the CLI reference).
- `ConsolidateConfig` setting `auto_threshold` (default 10).
- Desktop app: a per-namespace "Consolidate" button in the Namespaces view (`cmd_consolidate_namespace`).

**Retrieval & deduplication**
- `RecallQuery.exclude_expired` (default false): an opt-in filter that drops memories whose `valid_until` is in the past, applied across keyword, recent, and semantic recall. Previously `valid_until` was stored but never consulted, so known-stale facts ranked as current.
- Write-path deduplication: capturing or approving content identical to an existing non-archived memory in the same namespace now returns that memory instead of creating a duplicate row (`repository::find_content_duplicate`), so a known fact never duplicates or clogs the review inbox.

**Context assembly**
- `ContextRequest.char_budget` (CLI `clio brief --char-budget`, MCP `memory_context` `char_budget`): greedily truncates a context brief once the summed content length is reached, so briefs never balloon.

**Daemon maintenance**
- `daemon.maintenance` settings (`backup_interval_secs`, `backup_max_backups`, `integrity_interval_secs`; all off by default) and a scheduler task that runs local database backups and log-only integrity checks on their configured intervals. Both are pure-local (no LLM); consolidation stays on the session-stop hook.

**Deduplication**
- Migration `007_content_dedup_index`: a `(namespace, length(content))` index that prunes the exact-content duplicate probe cheaply at scale.
- `repository::find_archived_duplicate`: the capture path now revives an archived duplicate instead of creating a fresh live row.

### Changed

**MCP surface**
- Rewrote the server instructions (~55 → ~180 words): namespace resolution order, tool-choice guidance (`recall` vs `search` vs `recent` vs `remember` vs `capture`), `memory_context` presets, archive-is-soft-delete, and the JSON response hint.
- Merged `memory_inbox_list`/`memory_inbox_approve`/`memory_inbox_reject`/`memory_inbox_edit` into a single `memory_inbox(action, …)` tool. Deprecated `memory_recent` in favour of `memory_recall` with no `query` (retained as an alias for one release).
- Fail-fast tool descriptions: `memory_search`/`memory_suggest_links` state they need a configured embedding backend; `memory_remember` states upsert needs both `source` and `source_ref`; `memory_suggest_links.threshold` explains cosine direction.
- Slimmed the markdown recall card (one-line metadata; dropped `rank` and full timestamps) for ~30% fewer tokens; the full fields remain available via `response_format:"json"`.

**Retrieval**
- Semantic search now applies the same composite relevance scoring as keyword recall — time decay × access frequency × importance — on top of the hybrid semantic+keyword score, so the two retrieval paths rank consistently. Extracted into `clio-core::scoring::composite_multiplier`; neutral when `decay_lambda = 0.0` (preserves the backwards-compatibility invariant).
- The semantic keyword boost is now proportional to normalised BM25 match strength instead of a flat `0.3`, so a weak FTS hit no longer earns the same lift as a strong one. Pure-semantic ordering is preserved when there are no FTS hits.
- Context briefs de-duplicate memories across preset sections (a decision tagged as a constraint no longer appears twice).

**Desktop app**
- Memory cards now show importance with the same accent-fill dots used in the compose and drawer editors, replacing an inconsistent multi-colour scale.
- Archive, delete and namespace-purge actions report success and failure via toast notifications; archiving offers an inline **Undo**.

### Fixed

**Core**
- Classification and distillation calls now set OpenAI JSON mode (`response_format: json_object`), so a session digest containing its own output-format instructions (common in code-review prompts, e.g. "no preamble… end with VERDICT: CLEAN") can no longer hijack the model into returning plain text and failing the JSON parse. The distillation prompt now asks for a `{"memories": […]}` object (already accepted by the parser) and tells the model the digest is source material, not instructions. Consolidation still returns markdown and opts out.
- `recall_scoped` now pages correctly across the detected and `global` namespaces — the global fill no longer hard-codes `offset: 0`, so `offset > 0` pages across the merged result — and reports an honest `total`.
- `PRAGMA wal_autocheckpoint = 1000` plus a daemon WAL checkpoint (`PASSIVE`) on shutdown keep the `-wal` file bounded on long-lived processes.

**Desktop app**
- Semantic search and link suggestions now run on a blocking thread pool (`spawn_blocking`), so a large embedding scan no longer freezes the UI main thread.
- Compose "Add details" now persists the title and tags entered — previously only the body text and namespace were saved, so those fields were silently discarded.
- Keyboard navigation (`j`/`k`) now highlights the correct card when memories are pinned or grouped; focus order follows the rendered order rather than the raw recall order.
- Shift-click range selection now selects the correct cards when memories are pinned or grouped — like keyboard nav, it follows the rendered order rather than the raw recall order (previously bulk actions could act on the wrong memories whenever a group-by or pinning was active).
- Context Builder placeholders now show an ellipsis (…) instead of a literal `\u2026` escape sequence.

**MCP**
- The `memory_inbox` tool accepts the `review_id` parameter documented in the MCP contract; the previous `id` name is still accepted as an alias, so existing callers keep working.

## [0.3.0] - 2026-03-03

### Added

#### Auto-Intelligence (Phase 10.5)

**Access Tracking**
- Migration `004_access_tracking`: `last_accessed_at` and `access_count` columns on `memories` table with partial index
- `last_accessed_at` and `access_count` fields added to `Memory` struct
- `touch_accessed()` function records when memories are read, with 60-second throttle to prevent write amplification
- Fire-and-forget access tracking in `get()`, `recall()`, and `semantic_recall()` — failures log a warning but never fail the operation

**Temporal Relevance Scoring**
- `ScoringConfig` in settings: `decay_lambda` (default 0.01) and `access_boost_weight` (default 0.1)
- Composite scoring in `recall_fts()`: BM25 relevance x time decay x access frequency boost x importance factor
- Composite scoring in `recall_recent()`: time decay x access boost x importance (no BM25 component)
- Backwards-compatible: `decay_lambda = 0.0` preserves original `rank ASC, updated_at DESC` ordering
- `scoring` field added to `RecallQuery` with `#[serde(skip)]` — set by callers, not exposed via MCP parameters
- CLI `recall`, CLI `recent`, MCP `memory_recall`, MCP `memory_recent`, and Tauri commands all pass scoring config

**Auto-Link Inference**
- `AutoLinkConfig` in daemon settings: `enabled`, `threshold` (0.80), `interval_secs` (3600), `max_links_per_memory` (3), `batch_size` (50)
- `auto_link_batch()` in `embeddings.rs`: processes recently updated memories, generates embeddings if missing, creates `auto:relates_to` links above threshold
- `AutoLinkReport` struct tracks memories processed, links created, and watermark position
- New `auto_linker.rs` daemon module: async background task with interval loop, watermark tracking, and graceful shutdown
- Daemon `main.rs` creates shared `Arc<dyn EmbeddingBackend>` and spawns auto-linker when enabled
- `auto_linker` added to daemon status enabled routes

### Test coverage

- 54 unit tests, 33 integration tests — all passing (87 total)

## [0.2.0] - 2026-03-03

### Added

#### Workspace

- New crate: `clio-daemon` — always-on local daemon for lifecycle management and ambient capture

#### Always-on Daemon (Phase 8)

**Core types (`clio-core/daemon.rs`)**
- `DaemonConfig` — daemon settings (`enabled`, `inbox_paths`, `socket_path`, `log_dir`, `http_port`)
- `DaemonStatus`, `DaemonHealth`, `HealthCheck`, `HealthStatus` — status and health reporting types
- `PidFile` — singleton locking with stale PID detection via `kill -0`
- Platform path defaults: `default_socket_path()`, `default_pid_path()`, `default_log_dir()` (macOS + Linux)
- Health check functions: `check_database_health()`, `check_embeddings_health()`, `check_capture_health()`, `run_health_checks()`
- `daemon` field added to `Settings` struct

**Daemon binary (`clio-daemon`)**
- Tokio-based long-running local process
- Unix domain socket control channel accepting JSON commands: `status`, `stop`, `health`
- Inbox folder watcher via `notify` crate — watches configured directories, processes files through capture pipeline or stores as plain notes, moves processed files to `_processed/` subdirectory
- PID file singleton locking — rejects duplicate daemon instances
- Dual tracing: stderr + daily rolling log files via `tracing-appender`
- Graceful SIGTERM/SIGINT shutdown with PID file and socket cleanup

**CLI commands**
- `clio daemon run` — start daemon in foreground
- `clio daemon start` — start daemon in background
- `clio daemon stop` — stop running daemon via control socket
- `clio daemon restart` — stop then start
- `clio daemon status` — query daemon status (supports `--json`)
- `clio daemon logs` — tail recent daemon log file
- `clio daemon install` — generate and install macOS LaunchAgent plist
- `clio daemon uninstall` — remove LaunchAgent plist
- `clio daemon doctor` — run health checks without requiring daemon to be running (supports `--json`)

#### Review Queue (Phase 9)

**Database**
- Migration `003_review_queue`: `review_queue` table with status CHECK constraint and `idx_review_queue_status` index

**Core module (`clio-core/review.rs`)**
- `ReviewItem`, `ReviewInput`, `ReviewEdits`, `ReviewStats` types
- `queue_for_review()` — insert a capture into the review queue
- `list_pending()` — list pending review items
- `get_review()` — get a single review item by ID
- `approve_review()` — convert a review item to a stored memory via `repository::remember()`
- `reject_review()` — mark a review item as rejected
- `edit_review()` — update suggested fields before approval
- `review_stats()` — count items by status

**Capture pipeline integration**
- `CaptureResult` enum: `Stored(Memory)` | `Queued(ReviewItem)`
- Captures below `review_threshold` in settings route to review queue instead of direct storage
- `review_threshold: Option<f64>` added to `CaptureConfig` (default `None` = disabled)

**CLI commands**
- `clio inbox list` — list pending review items
- `clio inbox approve <id>` — approve and convert to memory
- `clio inbox reject <id>` — reject a review item
- `clio inbox edit <id>` — update suggested fields (`--title`, `--namespace`, `--kind`, `--tags`, `--summary`, `--importance`)
- `clio inbox stats` — pending/approved/rejected/edited counts

**MCP tools**
- `memory_inbox_list` — list pending review items
- `memory_inbox_approve` — approve by ID
- `memory_inbox_reject` — reject by ID
- `memory_inbox_edit` — edit suggested fields by ID

#### Context Assembly (Phase 9)

**Core module (`clio-core/assembly.rs`)**
- `ContextPreset` enum with 6 variants: `project-brief`, `person-brief`, `decision-history`, `active-constraints`, `recent-activity`, `custom`
- `ContextRequest`, `ContextSection`, `ContextBrief` types
- `build_context()` — combines kind-filtered and recent memories into sectioned briefs for agent consumption
- 7 unit tests covering all presets, round-tripping, empty DB, and max_items budgeting

**CLI command**
- `clio brief` — build a context brief with `--namespace`, `--preset`, `--query`, `--max-items`, `--include-links`

**MCP tool**
- `memory_context` — build scoped context briefs with namespace auto-detection from `cwd`, preset selection, markdown/JSON output

### Test coverage

- 54 unit tests, 33 integration tests — all passing (87 total)

## [0.1.0] - 2026-03-02

Initial release of Clio — a local-first shared memory system for AI tooling, written in Rust.

### Added

#### Workspace

- Cargo workspace containing three crates: `clio-core`, `clio-cli`, and `clio-mcp`

#### Core Library (`clio-core`)

**Database**
- SQLite storage with WAL mode, foreign keys, `busy_timeout`, and production-grade pragmas
- Migration system with two versioned migrations:
  - `001_initial`: `memories`, `memory_tags`, `memory_links`, and `schema_migrations` tables; FTS5 virtual table with triggers
  - `002_embeddings`: `memory_embeddings` table for vector storage
- Database path resolution: explicit argument, then `CLIO_DB_PATH` environment variable, then platform default (`~/Library/Application Support/clio/memory.db` on macOS)

**Domain Model**
- Core types: `Memory`, `RememberInput`, `RecallQuery`, `RecallItem`, `RecallResult`, `LinkInput`, `MemoryLink`
- UUIDv7 for time-sortable memory identifiers

**Repository Operations**
- `remember` — insert or upsert; keyed on `source` + `source_ref` pair, preserving original ID and `created_at`
- `get` — retrieve a single memory by ID
- `recall` — full-text search with BM25 ranking (weights: title 4.0, summary 2.0, content 1.0, tags 0.5)
- `recent` — paginated list of recent memories
- `archive` — soft-delete with `archived_at` timestamp; idempotent via `COALESCE`; archived memories are hidden, not deleted
- `link` — typed directional edges between memories

**Filtering and Normalisation**
- Namespace, kind, and tag filtering on recall and recent queries
- Match-all and match-any tag modes
- Tag normalisation: lowercase, trim, deduplication

**Input Validation**
- Content must not be empty
- Importance: 1–5
- Confidence: 0.0–1.0
- Metadata must be a JSON object
- Length limits enforced on namespace, kind, title, summary, and tags

**Data Portability**
- JSONL export and import with round-trip fidelity

**Error Handling**
- Typed error system with categories: `Config`, `Migration`, `Validation`, `NotFound`, `Conflict`, `Storage`, `Export`, `Import`

**Test Coverage**
- 23 integration tests and 8 unit tests, all passing

#### Vector Embeddings and Semantic Search

- Pluggable `EmbeddingBackend` trait
- Local backend: fastembed (ONNX-based), all-MiniLM-L6-v2 model, 384 dimensions, fully offline
- OpenAI backend: `text-embedding-3-small` (1536 dims), `text-embedding-3-large` (3072 dims), `text-embedding-ada-002` (1536 dims)
- Embedding storage as BLOB in `memory_embeddings` (f32 little-endian encoded)
- Cosine similarity for semantic search
- Passage construction: concatenates title, summary, tags, and content
- Auto-embedding on write (configurable)
- Bulk embedding utilities: `count_unembedded`, `list_unembedded` for backfill operations

#### Settings System

- JSON settings file (`clio-settings.json`) stored alongside the database
- Configurable embedding provider: `local`, `openai`, or `disabled`
- Auto-embed toggle (default: on)
- Sensible defaults when no settings file exists

#### CLI (`clio-cli`)

- 13 commands: `init`, `remember`, `recall`, `show`, `recent`, `archive`, `link`, `export`, `import`, `schema`, `search`, `embed`, `settings`
- Global flags: `--db-path` (override database location), `--json` (JSON output mode)
- Human-readable output with Unicode box-drawing for memory cards
- Compact list format for recall and recent results, including rank scores
- Stdin support for `--content -` and `--input -`
- `search` — semantic (meaning-based) search
- `embed status` — shows embedding coverage and provider information
- `embed backfill` — generates embeddings for all un-embedded memories
- `settings show | use-local | use-openai | disable` — manage the active embedding provider
- Auto-embedding on `remember` when enabled
- Status messages to stderr; data to stdout

#### MCP Server (`clio-mcp`)

- 7 tools: `memory_remember`, `memory_recall`, `memory_get`, `memory_recent`, `memory_link`, `memory_archive`, `memory_search`
- 3 resources: `memory://schema`, `memory://item/{id}`, `memory://recent/{namespace}`
- stdio transport via `rmcp` v0.1
- Markdown and JSON response format support
- Auto-embedding on `memory_remember` when enabled
- Actionable error messages formatted for agent usability
- Connection-per-request pattern with `spawn_blocking` for database calls

[0.1.0]: https://github.com/dannyharding/clio/releases/tag/v0.1.0
