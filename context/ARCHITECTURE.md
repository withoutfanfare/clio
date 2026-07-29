# Architecture

## Vision

Clio is a local-first memory backbone for AI tooling. One Rust core, multiple access surfaces (CLI, MCP, Tauri, daemon), one SQLite database.

## System Overview

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                              AI Coding Agents                                │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐ │
│  │  Claude   │  │   Codex   │  │  Cursor   │  │ Windsurf  │  │  Gemini   │ │
│  │   Code    │  │    CLI    │  │           │  │           │  │    CLI    │ │
│  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘ │
│        │              │              │              │              │       │
└────────┼──────────────┼──────────────┼──────────────┼──────────────┼───────┘
         │              │              │              │              │
         └──────────────┴──────────────┼──────────────┴──────────────┘
                                       │ MCP (stdio JSON-RPC)
                                       │
                         ┌─────────────▼─────────────┐
                         │        clio-mcp           │
                         │  MCP Server (thin adapter)│
                         └─────────────┬─────────────┘
                                       │
┌──────────────────────────────────────┼──────────────────────────────────────┐
│                          Rust Core Layer                                   │
│  ┌─────────────┐  ┌─────────────┐  ┌─▼───────────┐  ┌─────────────┐       │
│  │  clio-cli   │  │clio-tauri   │  │  clio-core  │  │clio-daemon  │       │
│  │ (CLI parser)│  │ (Desktop UI)│  │(All logic)  │  │(Background) │       │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘       │
│         │                │                │                │              │
│         └────────────────┴────────────────┴────────────────┘              │
│                                          │                                 │
└──────────────────────────────────────────┼─────────────────────────────────┘
                                           │
                         ┌─────────────────▼─────────────────┐
                         │           SQLite                   │
                         │  ┌───────────┐  ┌───────────────┐ │
                         │  │ memories  │  │ memory_tags   │ │
                         │  ├───────────┤  ├───────────────┤ │
                         │  │ embeddings│  │ memory_links  │ │
                         │  ├───────────┤  ├───────────────┤ │
                         │  │ FTS index │  │ review_queue  │ │
                         │  └───────────┘  └───────────────┘ │
                         └───────────────────────────────────┘
```

### Optional Remote MCP Topology

```text
┌──────────────────┐   stdio   ┌─────────────────┐   SSH   ┌──────────┐   rusqlite   ┌───────────┐
│ AI client or     │ ◄───────► │ clio remote-mcp │ ◄─────► │ clio-mcp │ ◄──────────► │ SQLite DB │
│ Tauri desktop    │           │ client computer │         │ server   │              │ server    │
└──────────────────┘           └─────────────────┘         └──────────┘              └───────────┘
```

The AI client launches `clio remote-mcp` as its stdio MCP command. The local
bridge detects the project namespace from each tool call's `cwd`, then forwards
the request over SSH. Explicit namespaces and `global: true` still take
precedence.

The server owns `clio-mcp`, its settings, and the single SQLite database. SSH
uses non-interactive key authentication, so Clio does not expose a network
listener or database port.

This topology requires a live SSH connection. A persisted remote route forwards
normal CLI commands and session hooks to the server, supplies bridge
configuration to MCP clients, and lets the desktop app connect when opened from
Finder. The desktop app's bulk, import/export, database maintenance,
deduplication, and namespace administration operations remain local only. The
daemon is local-only and should stay disabled on shared-memory clients.
Embeddings and capture run on the server. There is no offline cache or
synchronisation.

For a headless server, both binaries can be built with `--no-default-features`.
This omits local fastembed support while retaining storage, keyword recall,
capture, and OpenAI embeddings.

## Tech Stack

- **Language:** Rust
- **Storage:** SQLite (WAL mode, FTS5, foreign keys)
- **Libraries:** rusqlite, serde, serde_json, clap, uuid, time, thiserror, tracing, fastembed (optional), reqwest (optional, OpenAI backend)
- **Transport:** stdio (MCP), SSH bridge (remote MCP), direct binary (CLI), Unix domain socket (daemon control)
- **Daemon:** `notify` (filesystem watching), `tracing-appender` (rolling log files), `libc` (PID management)

## Directory Structure

```text
.
├── Cargo.toml              # workspace root
├── Cargo.lock
├── crates
│   ├── clio-core           # all business logic lives here
│   ├── clio-cli            # thin CLI wrapper
│   ├── clio-mcp            # thin MCP adapter
│   ├── clio-daemon         # always-on local daemon
│   └── clio-tauri          # desktop UI crate
├── docs
│   ├── getting-started.md          # setup and usage guide
│   ├── cli-reference.md            # CLI commands and flags
│   ├── mcp-agent-setup.md          # agent connection and workflows
│   ├── resource-limits.md          # sizing numbers and constraints
│   ├── rationale.md                # project rationale
│   ├── security-review.md          # security audit findings
│   ├── reference/
│   │   ├── schema.md               # SQLite schema contract
│   │   ├── mcp-contract.md         # MCP tool/resource definitions
│   │   └── settings.md             # all config keys + defaults
│   └── plan/
│       └── implementation-plan.md  # full delivery plan
├── context                         # priming docs for coding agents
└── archive
    └── python-prototype            # reference only, do not extend
```

## Crate Boundaries

### `clio-core`

Owns all durable business logic. Every other crate is a thin consumer.

Modules:
- `config.rs` — path resolution, DB location
- `db.rs` — connection setup, pragmas
- `migrations.rs` — migration runner (001_initial, 002_embeddings, 003_review_queue, 004_access_tracking)
- `error.rs` — typed domain errors
- `models.rs` — Memory, Tag, MemoryLink, RecallQuery, RecallResult
- `repository.rs` — CRUD, upsert, archive, unarchive, link, get_links, list_namespaces, recall_scoped, touch_accessed operations; composite temporal scoring (BM25 × recency × access × importance)
- `search.rs` — FTS5 recall, BM25 ranking
- `export.rs` — JSONL import/export
- `embeddings.rs` — pluggable embedding backends (local fastembed, OpenAI), cosine similarity, semantic recall, `auto_link_batch`
- `settings.rs` — load/save `clio-settings.json` for embedding backend, auto-embed toggle, capture config (incl. `review_threshold`), context detection config, daemon config, shared SSH route, `ScoringConfig`, and `AutoLinkConfig`
- `capture.rs` — LLM-based capture pipeline: `classify()`, `parse_classification()`, `capture()`; gated behind the `capture` feature flag
- `checkpoint.rs` — exact-once session checkpoints keyed by `(source, session_id, cursor)`: preflight lookup, in-transaction recheck, atomic atom + review + checkpoint storage, stored-result replay; model extraction and embedding stay outside the write transaction
- `attention.rs` — narrow follow-up lifecycle (`open`/`snoozed`/`resolved`/`cancelled`): idempotent creation, guarded transitions with audit events, evidence-backed completion via `resolved_by` links, pure-read eligibility with machine-readable reasons and once-per-scope surfacing
- `events.rs` — append-only `memory_events` ledger with a narrow validated vocabulary and idempotency keys; state-changing events share the parent transaction, observational events are fire-and-forget
- `migrate.rs` — cross-tool memory importers for Claude and ChatGPT exports; deterministic content-hash `source_ref` for idempotent re-import; optional `--classify` path via capture pipeline
- `context.rs` — automatic namespace detection from cwd: walks up the directory tree checking `.clio-namespace` file → `.git` → `Cargo.toml`/`package.json`; `detect_namespace()`, `resolve_namespace()`, `resolve_namespace_with_context()`, `init_namespace()`
- `stats.rs` — analytics queries: `memory_stats()` (counts, namespace/kind breakdown, weekly timeline, tag frequency, link density, embedding coverage), `tag_frequency()`, `timeline()`, `recent_activity()` (create/update/archive event feed)
- `daemon.rs` — daemon configuration, lifecycle, and health types: `DaemonConfig`, `AutoLinkConfig`, `DaemonStatus`, `DaemonHealth`, `HealthCheck`, `HealthStatus`, `PidFile`; platform path defaults; health check functions for database, embeddings, and capture
- `review.rs` — review queue for low-confidence captures: `ReviewItem`, `ReviewInput`, `ReviewEdits`, `ReviewStats`; `queue_for_review()`, `list_pending()`, `get_review()`, `approve_review()`, `reject_review()`, `edit_review()`, `review_stats()`
- `assembly.rs` — context assembly for agent consumption: `ContextPreset` (6 variants), `ContextRequest`, `ContextSection`, `ContextBrief`; `build_context()` combines kind-filtered and recent memories into sectioned briefs
- `validate.rs` — input validation helpers (private to core)

Must NOT depend on: Tauri UI code, MCP-specific types, CLI formatting.

### `clio-cli`

Thin binary wrapper. Argument parsing (clap), text/JSON rendering, exit codes.

Notable commands beyond CRUD: `clio serve` (locates `clio-mcp` binary adjacent to itself or on PATH, verifies the database is initialised, then execs it with stdio inherited and `CLIO_DB_PATH` set); `clio remote-mcp` (proxies stdio MCP over SSH while resolving namespaces on the client); `clio settings use-remote` (persists an Atlas route used by the CLI, hooks, MCP setup and Tauri); `clio setup <client>` (installs local or remote MCP client configuration); `clio daemon` subcommand group (`run`, `start`, `stop`, `restart`, `status`, `logs`, `install`, `uninstall`, `doctor`); `clio inbox` subcommand group (`list`, `approve`, `reject`, `edit`, `stats`); `clio brief` (context assembly with `--preset`, `--namespace`, `--query`).

Must NOT: open ad hoc SQL queries, implement its own validation rules.

### `clio-mcp`

Thin MCP adapter. Maps MCP payloads to core input types.

Tools: `memory_remember`, `memory_update`, `memory_recall`, `memory_get`, `memory_recent`, `memory_link`, `memory_archive`, `memory_unarchive`, `memory_delete`, `memory_move`, `memory_namespaces`, `memory_get_links`, `memory_capture`, `memory_session_checkpoint`, `memory_action`, `memory_search`, `memory_stats`, `memory_activity`, `memory_suggest_links`, `memory_context`, `memory_inbox`, `memory_cache_clear`

Must NOT: duplicate persistence logic, invent alternate search semantics.

### `clio-daemon`

Always-on local process for lifecycle management and capture routing.

Responsibilities:
- PID file singleton locking (rejects duplicate instances)
- Unix domain socket control channel (`status`, `stop`, `health` commands)
- Inbox folder watcher (via `notify` crate) — processes new files through capture pipeline or stores as plain notes
- Dual tracing: stderr + daily rolling log files
- Graceful SIGTERM/SIGINT shutdown with PID file and socket cleanup
- Health checks for database, embeddings, and capture backends
- Auto-link inference background task — periodically scans recent memories and creates `auto:relates_to` links between semantically similar memories above a configurable threshold

Must NOT: become the only way to use Clio, expose network listeners outside localhost, implement storage semantics outside the core.

### `clio-tauri`

Desktop UI crate. Vue 3 frontend with Tauri 2 backend for browse/edit/archive/inspect workflows. It opens `clio-core` directly in local mode or uses the existing SSH/MCP bridge when a remote route is persisted or `CLIO_REMOTE_HOST` is set. Environment variables override persisted settings. Remote misconfiguration is surfaced as disconnected and never falls back to local storage.

**Backend commands** (in `src/commands/`):
- `memory.rs` — CRUD, archive, unarchive, recall, recent, update
- `search.rs` — semantic search, embedding
- `stats.rs` — memory statistics and analytics
- `namespaces.rs` — namespace listing
- `clipboard.rs` — native clipboard copy (osascript with pbcopy fallback)
- `settings.rs` — non-secret capture preferences and model changes

**Frontend** (`ui/src/`):
- Vue 3 + Pinia (state) + Vue Router, built with Vite
- Components: AppBar, MemoryPage, MemoryDrawer, ComposeArea, CommandPalette, SidePanel, DateGroup, TagInput, LinkList, KindSelector
- Composables: useAutoSave, useDebounce, useGroupedMemories, useKeyboard
- Store: `stores/memories.ts` — filtering, sorting, grouping with localStorage persistence
- Views: HomeView (memory list/grid), StatsView, SettingsView

## Storage Engine

SQLite with these connection pragmas:

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
```

## Configuration

Resolution order:
1. Explicit CLI flag or runtime config
2. `CLIO_DB_PATH` environment variable
3. Platform default:
   - macOS: `~/Library/Application Support/clio/memory.db`
   - Linux: `$XDG_DATA_HOME/clio/memory.db` or `~/.local/share/clio/memory.db`
   - Windows: `%APPDATA%\clio\memory.db`

## Key Decisions

| Decision | Rationale |
|---|---|
| SQLite as source of truth | Zero daemon, robust WAL concurrency, portable, excellent tooling |
| Rust owns the core | Type safety, single binary, reusable across all interfaces |
| MCP is an interface, not the system | Memory remains available when MCP clients change |
| Namespaces are first-class | Multiple tools share one DB; scoping improves recall |
| Archive instead of delete | Memory systems preserve history; accidental deletion is expensive |
| Synchronous rusqlite | Simple control flow, fewer moving parts, easier testing |

## Delivery Status

All planned phases (0–10.5) are complete: core, CLI, MCP, semantic search, capture pipeline, migration, stats/analytics/knowledge graph, daemon, review queue, context assembly, auto-intelligence, and namespace auto-scoping. The Tauri desktop UI is actively developed.
