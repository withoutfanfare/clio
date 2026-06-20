# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

### Changed

**Desktop app**
- Memory cards now show importance with the same accent-fill dots used in the compose and drawer editors, replacing an inconsistent multi-colour scale.
- Archive, delete and namespace-purge actions report success and failure via toast notifications; archiving offers an inline **Undo**.

### Fixed

**Desktop app**
- Compose "Add details" now persists the title and tags entered — previously only the body text and namespace were saved, so those fields were silently discarded.
- Keyboard navigation (`j`/`k`) now highlights the correct card when memories are pinned or grouped; focus order follows the rendered order rather than the raw recall order.

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
