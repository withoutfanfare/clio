# Clio Implementation Plan

## Vision

Build Clio, a local-first memory backbone for AI tooling on this machine. The system should give every AI client, shell script, and future desktop UI a shared place to store and recall durable context without binding that memory to any one vendor, chat application, or agent runtime.

This is infrastructure, not a demo. The first release should already feel like a dependable local system component:

- one shared database
- one Rust core
- one CLI for scripts and humans
- one MCP server for AI clients
- one always-on local daemon for capture and lifecycle management
- one future Tauri app for inspection and management

The success condition is not a polished UI. The success condition is that multiple AI tools can safely use the same memory layer every day.

## Problem Statement

Most AI tools keep context in fragmented places:

- chat history inside a single app
- local notes in ad hoc Markdown files
- tool-specific caches
- project-specific state with no shared retrieval path

That causes repeated context loss, duplicated notes, and vendor lock-in. Clio solves that by separating:

- storage from interface
- durable memory from transient conversation state
- machine access from human inspection

## Product Goals

### Phase-one goals

- Rust-first implementation
- SQLite as the system of record
- stable schema and migrations
- CLI for storing and retrieving memory
- MCP server exposing the same operations
- full-text search
- namespacing across tools and projects
- idempotent upsert from external tools
- export path for backup and migration
- clear error handling and predictable outputs

### Phase-two goals

- always-on local daemon/agent over the same Rust core
- start/stop/status/install flows for the daemon
- dependable local capture surfaces beyond direct CLI use
- Tauri desktop app over the same Rust core
- richer browsing and editing workflows
- diagnostics and maintenance commands
- better import/export and backup UX

### Phase-three goals (Open Brain alignment)

These features close the gap between Clio as reliable local storage and the full "Open Brain" vision described in the project rationale — a system where every AI tool shares one agent-readable brain with semantic understanding.

- **Semantic search via vector embeddings** — search by meaning, not just keywords. Core infrastructure already exists (`embeddings.rs`, migration `002_embeddings`, fastembed + OpenAI backends, settings with `auto_embed`). Remaining work: wire auto-embedding into the CRUD write path, expose `semantic_recall` in CLI and MCP, add bulk re-embed command for existing memories.
- **Capture pipeline / automated ingestion** — accept unstructured input and automatically extract metadata (kind, tags, people, topics, action items) using an LLM before storage. This is the "type a thought in Slack and five seconds later it's embedded, classified, searchable" workflow. Requires a lightweight classification step on the ingest path.
- **Cross-tool memory migration** — import memories from other AI tools' memory formats (Claude memory, ChatGPT memory exports, conversation history). Seed the brain from existing context rather than starting from zero.
- **Stats and analytics** — a `memory_stats` tool/command that surfaces thinking patterns over time: memories by namespace, by kind, by week, tag frequency, connection density. Supports the "weekly review" workflow.
- **Knowledge graph traversal** — automatic relationship suggestions based on content similarity, graph-based recall that surfaces connected memories, path queries between concepts.
- **Dashboard and visualisation** — richer than browse/edit/archive. Thinking pattern visualisation, topic clustering, timeline views. Extends the Tauri phase.

### Explicit non-goals for now

- cloud sync
- multi-user auth
- remote service mode
- internet-exposed daemon or multi-machine listener
- autonomous summarisation pipelines
- permissions model beyond local filesystem ownership

## Product Principles

### 1. Local-first

The memory store must work without network access or cloud dependencies. An always-on local daemon is allowed as an optional convenience layer, but the database and core workflows must remain usable without it.

### 2. Interface-agnostic

CLI, MCP, and Tauri are access surfaces over the same core. None may invent separate business logic or storage rules.

### 3. Inspectable

The data must remain understandable via SQLite tools, exported JSONL, and straightforward docs.

### 4. Stable contract first

The schema and interface contracts are more important than implementation speed. Other agents should be able to build from the docs without guessing hidden rules.

### 5. Incremental sophistication

Start with reliable relational storage and FTS. Leave room for embeddings, graph workflows, and sync later without contaminating phase one.

### 6. One write contract

Every capture surface must resolve into the same ingest pipeline. Raycast, inbox watchers, CLI, voice agents, and MCP clients may differ in transport, but they must not invent different classification, deduplication, or storage semantics.

## Users And Use Cases

### Primary users

- local AI coding agents
- shell scripts and automations
- the human operator inspecting or curating memory

### Example use cases

- Codex stores a project decision and Claude later recalls it via MCP.
- A shell script records a deployment note from CI output.
- A future Tauri app shows recent memories for `project:scooda`.
- An agent stores a summary with `source=codex` and `source_ref=issue-123`; later runs update that memory instead of duplicating it.
- A Raycast command captures a thought into the local daemon and receives a confirmation immediately.
- An inbox folder watcher converts dropped text or Markdown files into reviewed memories.
- A voice capture route transcribes speech locally or via configured provider, then sends the transcript through the same capture pipeline.

## System Overview

```text
                    +----------------------+
                    |   Tauri Desktop UI   |
                    |     later phase      |
                    +----------+-----------+
                               |
                               v
  +----------------+   +----------------------+   +------------------+
  | Shell scripts  |-->| Rust core library    |<--| MCP AI clients   |
  | Cron jobs      |   | + domain logic       |   | Codex/Claude/etc |
  +--------+-------+   | + SQLite access      |   +------------------+
           |           | + migrations         |
           |           +-----+-----------+----+
           |                 ^           ^
           v                 |           |
       clio CLI              |           |
           |                 |           |
           v                 |           |
   +----------------+        |           |
   | clio daemon    |--------+           |
   | always-on      |                    |
   | capture/router |---- Raycast / inbox / voice / local webhook
   +--------+-------+
            |
            v
        SQLite database
```

## Architecture Decisions

## Decision 1: SQLite is the source of truth

Why:

- zero daemon administration
- robust local concurrency with WAL mode
- portable single-file storage
- excellent tooling
- enough performance for local memory workloads

Rejected alternatives:

- PostgreSQL: overkill for phase one
- flat files: weak querying and concurrency
- embedded KV stores: poor inspectability and migration ergonomics

## Decision 2: Rust owns the core

Why:

- strong type safety for a long-lived local system
- easy path to a single compiled binary
- reusable across CLI, MCP, and Tauri
- reduces drift between interfaces

## Decision 3: MCP is an interface, not the system

Why:

- any AI client with MCP can use the memory store
- the memory remains available even when MCP clients change
- CLI and Tauri are not second-class paths

## Decision 4: Namespaces are first-class

Why:

- multiple tools will share the same DB
- projects need isolation without separate files
- recall becomes more reliable when scoped

## Decision 5: Archive instead of delete

Why:

- memory systems should preserve history by default
- accidental deletion is expensive
- archived data can be hidden from default recall without being lost

## Decision 6: The daemon is optional infrastructure, not the source of truth

Why:

- the always-on experience needs a stable local process
- inbox watchers, hotkeys, and voice routes are awkward when everything depends on ephemeral stdio sessions
- MCP stdio remains the right interface for AI clients, but it is not enough for ambient capture

Constraints:

- the daemon must call the Rust core rather than becoming its own storage layer
- the daemon must remain local-only by default
- all core write paths must still work without the daemon

## Repository Strategy

Target repo layout:

```text
.
├── Cargo.toml
├── Cargo.lock
├── crates
│   ├── clio-core
│   ├── clio-cli
│   ├── clio-mcp
│   └── clio-tauri
├── docs
│   └── plan
│       ├── clio-implementation-plan.md
│       ├── schema.md
│       └── mcp-contract.md
├── migrations
└── archive
    └── python-prototype
```

### Agent rule

The active implementation lives under the Rust workspace. The Python archive is reference material only and must not be extended unless explicitly requested.

## Crate Boundaries

## `clio-core`

This crate owns all durable business logic.

Responsibilities:

- configuration and DB path resolution
- connection setup and pragmas
- migration runner
- domain types
- validation rules not handled at the interface layer
- repository/query functions
- search ranking behaviour
- archive/upsert/link semantics
- import/export

Must not depend on:

- Tauri UI code
- MCP-specific types unless hidden behind an adapter feature
- CLI formatting concerns

Likely modules:

```text
config.rs
db.rs
migrations.rs
error.rs
models.rs
repository.rs
search.rs
export.rs
lib.rs
```

## `clio-cli`

Thin binary wrapper over `clio-core`.

Responsibilities:

- argument parsing via `clap`
- plain-text and JSON rendering
- exit code handling
- mapping CLI flags to core commands

Must not:

- open ad hoc SQL queries not represented in the core
- implement its own validation rules differently from MCP

## `clio-mcp`

Thin MCP adapter over `clio-core`.

Responsibilities:

- expose tools/resources defined in [mcp-contract.md](../reference/mcp-contract.md)
- map MCP payloads to core input types
- format errors for agent usability

Must not:

- duplicate persistence logic
- invent alternate search or ranking semantics

## `clio-daemon`

Always-on local process for lifecycle management and capture routing.

Responsibilities:

- own long-running local integrations
- watch configured inbox folders
- host optional local-only capture transports such as Unix socket or loopback HTTP
- provide health/status information for capture backends
- route all inbound content through `clio-core`

Must not:

- become the only way to use Clio
- expose network listeners outside localhost by default
- implement storage semantics outside the core

## `clio-tauri`

Deferred UI crate.

Responsibilities later:

- browse memories
- edit and archive memories
- inspect namespaces and recent history
- perform maintenance/export actions

Must be a consumer of the core, not a second implementation.

## Domain Model

The initial domain model is intentionally modest:

- `Memory`
- `Tag`
- `MemoryLink`
- `RecallQuery`
- `RecallResult`
- `ArchiveAction`

The system should be biased toward durable note-like memory, not ephemeral conversation replay.

### Supported memory kinds

The implementation should not hardcode a closed enum yet, but the docs should guide expected usage:

- `note`
- `fact`
- `decision`
- `summary`
- `task`
- `observation`
- `constraint`

The system may validate length and format but should not require a centrally managed taxonomy in phase one.

## Namespace Strategy

Namespaces should be treated as explicit scope, not decoration.

Recommended conventions:

- `global`
- `project:<slug>`
- `tool:<slug>`
- `person:<slug>`
- `topic:<slug>`

Guidelines:

- default to `global` when no scope is supplied
- prefer `project:<slug>` for codebase-specific memory
- prefer `tool:<slug>` for tool-local operational state
- avoid creating many near-duplicate namespaces for the same concept

## Capture Surface Strategy

Clio should treat capture surfaces as adapters over one ingest contract, not as product-specific one-offs.

### Supported routes

- direct CLI capture: `clio capture`, stdin, shell pipes
- MCP capture: `memory_capture`
- Raycast script command
- inbox folder watcher
- verbal capture via a CLI agent or speech-to-text helper
- optional local webhook or Unix socket for automation tools
- future browser/share-sheet adapters

### Prioritised implementation order

1. daemon lifecycle and inbox watcher
2. Raycast command
3. local voice/transcript route
4. local webhook / Unix socket route
5. browser and OS share-sheet adapters

### Ingest contract

Every route should resolve into the same pipeline:

1. accept raw text and route metadata
2. resolve namespace from explicit input, cwd, route defaults, or daemon config
3. classify and normalise through the capture pipeline when enabled
4. deduplicate or upsert when route metadata provides a stable reference
5. store memory
6. embed when enabled
7. optionally place the result into a review queue if confidence is low or metadata is incomplete

## Configuration Strategy

### Required environment variable

- `CLIO_DB_PATH`

### Resolution order

1. explicit CLI flag or runtime config
2. `CLIO_DB_PATH`
3. platform default application-data location

### Platform defaults

- macOS: `~/Library/Application Support/clio/memory.db`
- Linux: `$XDG_DATA_HOME/clio/memory.db` or `~/.local/share/clio/memory.db`
- Windows: `%APPDATA%\\clio\\memory.db`

## CLI Surface

Planned initial commands:

```text
clio init
clio remember
clio recall
clio show <id>
clio recent
clio archive <id>
clio link <from> <to>
clio export --output <file>
clio import --input <file>
clio schema
```

### CLI expectations

- every command supports `--db-path` override
- read-oriented commands support `--json`
- failures return non-zero exit codes
- output is stable enough for scripts

## MCP Surface

Phase-one tools and resources are defined in [mcp-contract.md](../reference/mcp-contract.md).

High-level rule:

- tools for actions and structured retrieval
- resources for inspectable context snapshots

## Storage Model

The schema contract lives in [schema.md](../reference/schema.md). The implementation plan assumes:

- `memories` as canonical records
- `memory_tags` for normalised tags
- `memory_links` for graph edges
- `schema_migrations` for DB versioning
- FTS5 virtual table linked to `memories`

## Rust Technology Choices

### Recommended libraries

- `rusqlite`
- `serde`
- `serde_json`
- `clap`
- `uuid`
- `time`
- `thiserror`
- `tracing`
- `tracing-subscriber`

### Why `rusqlite`

For phase one, synchronous SQLite access is a feature, not a limitation:

- simple control flow
- fewer moving parts
- easier Tauri integration
- easier unit/integration testing

If future workloads justify async or connection pooling, that can be revisited later.

## Error Strategy

The project should define typed errors in the core and map them outward:

- core returns typed domain/storage errors
- CLI maps them to readable stderr plus exit code
- MCP maps them to agent-usable validation or not-found errors

Example error categories:

- configuration
- migration
- validation
- not found
- conflict
- storage
- export/import

## Logging And Observability

Phase-one observability should remain small but intentional.

Implement:

- structured logs via `tracing`
- info-level lifecycle logs for init/start
- warn-level logs for recoverable issues
- debug-level SQL-adjacent diagnostics only when explicitly enabled
- daemon status and last-seen timestamps for active watchers/transports
- health checks for database, embeddings, and capture providers

Do not implement:

- metrics backend
- telemetry service
- analytics

## Security And Privacy Posture

This is a local system handling potentially sensitive operator notes. Even without auth, there should be basic discipline:

- never log full memory contents at info level
- do not expose network listeners by default
- use filesystem paths under the current user
- keep export/import explicit
- avoid surprise sync behaviour
- require explicit opt-in before watching folders or opening localhost listeners

## Daemon And Lifecycle Plan

The current `clio serve` behaviour is appropriate for stdio MCP launch, but it is not enough for an always-on Clio experience. The daemon should be a separate local process, not a replacement for `clio-mcp`.

### Process model

- `clio-mcp` stays stdio-first for AI clients
- `clio daemon run` starts the long-running process in the foreground
- `clio daemon start` installs or launches the background process for the current user
- `clio daemon stop` stops the background process cleanly
- `clio daemon restart` performs a stop/start cycle
- `clio daemon status` reports PID, uptime, enabled routes, DB path, and health
- `clio daemon logs` tails recent daemon logs
- `clio daemon install` / `uninstall` manage LaunchAgent or systemd user units

### State and control

- use a PID file or OS-native service manager metadata
- use a Unix domain socket for local control where supported
- persist daemon config in `clio-settings.json`
- reject duplicate daemon instances cleanly

### First managed services

- inbox folder watcher
- optional loopback HTTP or Unix socket capture endpoint
- provider checks for embeddings and capture backends
- route registry and enable/disable state

## Review And Context Assembly Plan

Two gaps should be treated as first-class roadmap items: a review loop for low-confidence captures and a higher-level context assembly tool for agents.

### Review queue

Deliverables:

- `clio inbox list`
- `clio inbox approve <id>`
- `clio inbox reject <id>`
- `clio inbox edit <id>`
- review state in the data model or an adjacent queue table

Use cases:

- low-confidence captures from voice transcripts
- messy inbox-folder imports
- classification outputs missing title, namespace, or useful tags

### Context assembly

Deliverables:

- `clio context build`
- `memory_context` MCP tool
- context presets such as `project-brief`, `person-brief`, `decision-history`, `active-constraints`

Behaviour:

- combine recent, important, linked, and semantically relevant memories
- compress output to fit agent consumption
- prefer scoped context first, then global context
- optionally include unresolved tasks and recent changes

## Delivery Plan

## Phase 0: Contract lock

Deliverables:

- this implementation plan
- [schema.md](../reference/schema.md)
- [mcp-contract.md](../reference/mcp-contract.md)
- agreed crate layout

Exit criteria:

- another agent can start the Rust workspace without inventing storage or MCP behaviour

## Phase 1: Workspace and core

Deliverables:

- Cargo workspace
- `clio-core`
- migration runner
- DB bootstrap
- repository and model tests

Tasks:

1. create workspace and crates
2. add shared lint/test config
3. implement path resolution and DB creation
4. implement migrations from `docs/plan/schema.md`
5. implement core models and repository methods
6. add integration tests using temp DB files

Exit criteria:

- core can create, update, recall, archive, and link memories in tests

## Phase 2: CLI

Deliverables:

- `clio` binary
- JSON output mode
- human-readable output mode
- export/import support

Tasks:

1. design command grammar to match the contract
2. implement flag parsing with stable names
3. map commands to core operations
4. add snapshot-style output tests where useful

Exit criteria:

- shell scripts can use the system without MCP

## Phase 3: MCP

Deliverables:

- `clio-mcp` binary
- stdio transport
- stable phase-one tool/resource surface
- example config snippets

Tasks:

1. select Rust MCP library or minimal compatible implementation
2. implement tool handlers as thin adapters
3. implement resources
4. add protocol-level smoke tests if the library supports them

Exit criteria:

- at least one local MCP client can store and recall memory

## Phase 4: Semantic search integration

The embedding infrastructure already exists in `clio-core` (`embeddings.rs`, migration `002_embeddings`, `settings.rs` with `auto_embed`). This phase wires it into the live system.

Deliverables:

- auto-embedding on memory write when `auto_embed` is enabled
- `clio embed` and `clio semantic-search` CLI commands
- `memory_semantic_search` MCP tool
- bulk `clio reembed` command for backfilling existing memories
- embedding model selection via settings

Tasks:

1. wire `embed_and_store` into the repository insert/upsert path
2. add CLI commands for semantic search and re-embedding
3. add MCP tool for semantic recall with similarity scores
4. add integration tests for the full write-embed-search cycle
5. document embedding configuration in MCP contract

Exit criteria:

- storing a memory automatically generates an embedding (when enabled)
- semantic search returns relevant results even when query terms don't appear in the content
- existing memories can be bulk-embedded after enabling the feature

## Phase 5: Capture pipeline

This phase addresses the "type a thought and the system classifies it" workflow from the project rationale.

Deliverables:

- `clio capture` command that accepts unstructured text
- LLM-powered metadata extraction (kind, tags, title, summary)
- configurable LLM backend for classification (local or API)
- optional auto-capture webhook or stdin listener

Tasks:

1. define a classification prompt contract (input: raw text, output: structured memory fields)
2. implement classification as a `clio-core` module with pluggable LLM backend
3. add `clio capture` CLI command
4. add `memory_capture` MCP tool
5. add integration tests with fixture-based classification outputs

Exit criteria:

- a user can send unstructured text and get a properly classified, tagged, embedded memory
- classification is optional and configurable

## Phase 6: Cross-tool memory migration

Deliverables:

- importers for Claude memory export, ChatGPT memory export
- `clio migrate-from` CLI command
- deduplication against existing memories during import

Tasks:

1. document known export formats from Claude and ChatGPT
2. implement format-specific parsers
3. map external fields to Clio memory structure
4. add deduplication heuristics (content similarity or source_ref matching)

Exit criteria:

- a user can import their existing AI memories into Clio without manual reformatting

## Phase 7: Stats, analytics, and knowledge graph

Deliverables:

- `clio stats` command and `memory_stats` MCP tool
- memory counts by namespace, kind, time period, tag frequency
- automatic link suggestions based on embedding similarity
- graph-aware recall (surface linked memories alongside primary results)

Tasks:

1. implement stats queries in the repository layer
2. add CLI and MCP surfaces
3. implement similarity-based link suggestion
4. extend recall to optionally include graph neighbours

Exit criteria:

- a user can review their memory patterns and see connections between ideas

## Phase 8: Always-on daemon and lifecycle

This phase turns Clio from a set of callable tools into an always-available local system component.

Deliverables:

- `clio-daemon` runtime or `clio daemon` subcommand with foreground and background modes
- LaunchAgent (macOS) and systemd user unit templates
- lifecycle commands: start, stop, restart, status, logs, install, uninstall
- health and doctor commands for DB, embeddings, capture, and active routes
- local-only control channel (Unix socket preferred; loopback HTTP optional)

Tasks:

1. define daemon config in `clio-settings.json`
2. implement singleton locking and clean shutdown handling
3. implement lifecycle CLI commands and service templates
4. add health probes for database access, embeddings, and capture backend
5. document daemon operations and failure recovery
6. add integration tests for lifecycle and health flows

Exit criteria:

- Clio can run as an always-on user service
- a user can start, stop, and inspect the daemon without manual process hunting
- daemon failure states are visible through `status` and `doctor`

## Phase 9: Capture surfaces, review queue, and context assembly

This phase turns the daemon into a practical capture backbone and improves agent ergonomics.

Deliverables:

- inbox folder watcher
- Raycast capture command
- voice/transcript capture route via CLI agent
- optional local webhook or Unix socket capture endpoint
- capture review queue with approve/edit/reject workflows
- `memory_context` MCP tool and `clio context build`

Tasks:

1. define route abstraction shared by watcher, Raycast, voice, and webhook adapters
2. implement inbox watcher with file acknowledgement/archive behaviour
3. add Raycast command templates and setup docs
4. implement transcript ingestion path for voice capture
5. add review queue storage and CLI workflows
6. implement context assembly queries and output formats
7. add end-to-end tests for route -> review -> recall flows

Exit criteria:

- at least three non-MCP capture routes can write through the same ingest path
- low-confidence captures can be reviewed before becoming durable context
- agents can request a scoped context brief instead of piecing it together manually

## Phase 10: Tauri desktop shell

Deliverables:

- basic search UI (FTS and semantic)
- memory detail view
- create/edit/archive actions
- namespace browsing
- thinking pattern visualisation and timeline views
- stats dashboard

Exit criteria:

- a human can inspect and manage stored memory without CLI knowledge
- a human can inspect daemon status, review queue items, and capture route health from the desktop UI

## Work Breakdown For Coding Agents

If another agent picks this up, the recommended order is:

1. read this file
2. read [schema.md](../reference/schema.md)
3. read [mcp-contract.md](../reference/mcp-contract.md)
4. scaffold the Cargo workspace
5. implement the core before touching CLI or MCP
6. keep all business logic in `clio-core`
7. implement the daemon before expanding capture adapters or Tauri

### Rule of engagement

If a needed behaviour is not specified:

- prefer the simpler option
- avoid adding new concepts casually
- document the gap before extending the contract

## Acceptance Criteria For First Usable Release

- one Rust `clio` binary can initialise the store
- one Rust `clio` binary can write memories
- one Rust `clio` binary can recall memories via FTS and filters
- one Rust `clio-mcp` server can expose the same operations
- one always-on daemon can manage local capture routes
- multiple tools can safely point at the same SQLite file
- archived memories are hidden by default but still retrievable
- export exists for backup or migration

## Acceptance Criteria For Open Brain Parity

These criteria mark the point where Clio fulfils the full "Open Brain" vision:

- storing a memory automatically generates a vector embedding
- semantic search finds memories by meaning, not just keyword match
- unstructured text can be captured and automatically classified with kind, tags, title
- multiple capture routes can feed the same ingest path while the daemon stays running
- existing memories from Claude and ChatGPT can be imported
- a stats command surfaces thinking patterns across time and namespace
- related memories are discoverable through graph traversal or similarity
- agents can request a scoped context brief in one call
- a desktop UI allows visual exploration of the knowledge graph

## Risks And Mitigations

### Risk: Rust MCP library immaturity

Mitigation:

- keep MCP isolated in its own crate
- keep core logic protocol-agnostic
- if needed, temporarily bridge with a shim while preserving the contract

### Risk: schema drift between interfaces

Mitigation:

- implement all business logic in the core only
- treat docs as the source of truth until code exists
- review interface changes against schema and MCP docs together

### Risk: overbuilding too early

Mitigation:

- keep phases 1–3 limited to reliable local memory with FTS
- phase 4 integrates existing embedding infrastructure rather than building from scratch
- defer sync and internet-facing network services
- capture pipeline (phase 5) is opt-in and configurable

### Risk: daemon complexity and drift from the CLI/MCP path

Mitigation:

- keep the daemon as a transport/router layer over `clio-core`
- force all writes through the same ingest contract
- keep `clio-mcp` stdio-based and independent from daemon availability
- add end-to-end tests comparing daemon capture with CLI capture outputs

### Risk: noisy or low-quality captures reduce recall quality

Mitigation:

- add confidence thresholds and route-specific review rules
- store route provenance in metadata
- introduce a review queue before broadening capture routes
- keep inbox watcher and voice capture opt-in per route

### Risk: embedding model size and performance

Mitigation:

- default to `all-MiniLM-L6-v2` (fast, small, 384 dimensions)
- auto-embed is a settings toggle, not forced
- OpenAI backend available for users who prefer API over local compute
- bulk re-embed runs offline, not blocking writes

## Open Questions

These are intentionally deferred decisions and should not block phase one:

- whether to store memory edit history in `memory_events`
- whether import should support merge strategies beyond id-based and source-ref-based upsert
- whether Tauri should edit records directly or call the CLI/core through commands
- whether future sync should operate on JSONL snapshots or row-level change tracking
- which LLM to use for the capture pipeline classification step (local vs API, model selection)
- whether the capture pipeline should support batch ingestion (e.g. importing a week of notes at once)
- whether link suggestions should run eagerly on write or lazily on query
- what export formats to support for cross-tool migration beyond Claude and ChatGPT
- whether the daemon should be its own binary or a `clio daemon` subcommand wrapper over shared runtime code
- whether local control should use Unix sockets only or also loopback HTTP
- how inbox folder processing should acknowledge, archive, or retry failed files
- whether voice capture should require local transcription first or allow API-first transcription
- what threshold should route captures into review by default

## Progress

### Phase 0: Contract lock — COMPLETE

- Implementation plan, schema.md, mcp-contract.md written
- Crate layout agreed

### Phase 1: Workspace and core — COMPLETE

- Cargo workspace scaffolded with `clio-core`, `clio-cli`, `clio-mcp` crates
- `clio-core` implemented: config, db, migrations (001_initial + 002_embeddings), models, error types, validation, repository (CRUD, upsert, archive, link, FTS recall), export/import (JSONL)
- 23 integration tests passing (CRUD, FTS, upsert, archive, links, export, tag normalisation, namespace filtering)
- 8 unit tests passing (embeddings: encode/decode, cosine similarity, config deserialisation, passage building)

### Phase 2: CLI — COMPLETE

- `clio` binary with 22 commands: init, context, remember, recall, show, recent, archive, unarchive, namespaces, link, export, import, schema, search, embed, capture, stats, activity, suggest-links, migrate, settings, serve, setup
- Human-readable and `--json` output modes
- Stdin support for content and import
- `--db-path` global override
- `clio import` auto-embeds imported records when `auto_embed` is enabled
- `clio serve` — locates `clio-mcp` binary (adjacent to executable or on PATH), verifies database, execs with inherited stdio
- `clio setup <client>` — generates ready-to-paste MCP configuration for `claude-code`, `cursor`, `windsurf`, or `generic`; resolves binary path and database path automatically

### Phase 3: MCP — COMPLETE

- `clio-mcp` binary with stdio transport via `rmcp` v0.1
- 11 tools: memory_remember, memory_recall, memory_get, memory_recent, memory_link, memory_archive, memory_unarchive, memory_namespaces, memory_get_links, memory_capture, memory_search
- 3 resources: memory://schema, memory://item/{id}, memory://recent/{namespace}
- Markdown and JSON response format support

### Phase 4: Semantic search — COMPLETE

- Embedding infrastructure: `embeddings.rs` with pluggable `EmbeddingBackend` trait
- Two backends: local (fastembed/ONNX, all-MiniLM-L6-v2, 384 dims) and OpenAI API (text-embedding-3-small, 1536 dims)
- Migration `002_embeddings` for `memory_embeddings` table (BLOB storage with f32 little-endian encoding)
- Settings system (`clio-settings.json`) for embedding backend selection and auto-embed toggle
- Auto-embedding on write: both CLI `remember` and MCP `memory_remember` auto-embed when enabled
- `clio search <query>` and `memory_search` MCP tool for semantic recall with cosine similarity
- `clio embed status` for embedding coverage reporting
- `clio embed backfill` for bulk embedding of existing memories
- `clio settings show/use-local/use-openai/disable` for provider management

### Phase 5: Capture pipeline — COMPLETE

- `capture.rs` module in `clio-core` with `classify()`, `parse_classification()`, `capture()`, `capture_with_classification()`
- OpenAI-compatible chat completions API for classification (temperature 0.1, structured JSON output)
- Robust `parse_classification()`: strips markdown fences, unknown kinds fall back to `note`, out-of-range values clamped, tags normalised
- `memory_capture` MCP tool: classify → store → auto-embed; returns stored memory record
- `clio capture <text>` CLI command with `--dry-run`, `--namespace`, and stdin support
- `clio settings use-capture --api-key <key> [--model <model>] [--base-url <url>]` and `disable-capture` subcommands
- `capture` feature flag in `clio-core` gating reqwest + tokio classification dependencies
- Captured memories tagged with `source = "capture"`

### Phase 6: Cross-tool memory migration — COMPLETE

- `migrate.rs` module in `clio-core` with `migrate_claude()` and `migrate_chatgpt()` functions
- Claude format handling: line-delimited text, JSON string arrays, JSON object arrays (extracts `content`/`text`/`memory` fields)
- ChatGPT format handling: JSON object arrays, nested `memories`/`model_spec_memories`/`data` keys, JSON string arrays, line-delimited fallback
- Deterministic 16-hex `source_ref` hash from `(source, content)` — re-imports are fully idempotent via `upsert: true`
- Claude defaults: `source="claude"`, `namespace="tool:claude"`, `kind="note"`
- ChatGPT defaults: `source="chatgpt"`, `namespace="tool:chatgpt"`, `kind="fact"`
- `clio migrate claude <file>` and `clio migrate chatgpt <file>` CLI commands
- All commands support: `--namespace` override, `--classify` (LLM classification via capture pipeline), `--dry-run`, stdin via `-`
- Auto-embedding on each stored entry when `auto_embed` is enabled
- No MCP tool — migration is CLI-only (human-initiated)
- `MigrationResult` with `imported`, `skipped`, `duplicates`, `errors`, and dry-run `preview` fields

### Namespace auto-scoping (cross-cutting) — COMPLETE

- `context.rs` module in `clio-core`: `detect_namespace()` walks directory tree for `.clio-namespace`, `.git`, `Cargo.toml`, `package.json`; `resolve_namespace()` applies explicit → detected → global precedence
- `recall_scoped()` in `repository.rs`: searches detected namespace first, fills remaining slots from `global`, merges results with project items ranked first
- All write operations (`remember`, `capture`) auto-detect namespace from cwd when none supplied
- `clio context` command — shows detected namespace, detection source, and marker path for cwd
- `clio init --namespace project:foo` — creates `.clio-namespace` file in current directory
- `clio recall`, `clio search`, `clio remember`, `clio capture` — all auto-detect namespace from cwd; `--namespace` always overrides
- MCP tools `memory_remember`, `memory_recall`, `memory_capture`, `memory_search` gain optional `cwd` parameter for namespace auto-detection
- `context.auto_detect: bool` setting (default `true`) to toggle auto-detection globally

### Phase 7: Stats, analytics, and knowledge graph — COMPLETE

- `stats.rs` module in `clio-core`: `memory_stats()` (total/active/archived counts, embedding coverage %, namespace/kind breakdowns, 52-week ISO timeline, top 20 tags, link density), `tag_frequency()`, `timeline()`, `recent_activity()` (create/update/archive event classification from timestamp fields)
- `get_neighbours(conn, memory_id, depth)` in `repository.rs`: breadth-first bidirectional traversal of the link graph with cycle prevention via visited set
- `suggest_links(conn, memory_id, backend, threshold, limit)` in `embeddings.rs`: on-the-fly embedding generation if missing, bidirectional already-linked exclusion set, cosine similarity scan of all non-archived embedded memories, sorted by similarity descending
- `include_links` on `RecallQuery`: `append_linked_memories()` helper fetches outgoing links for each recall result and appends linked memories with `linked_from` provenance field, deduplicated
- MCP tools: `memory_stats` (namespace-scoped counts + analytics), `memory_activity` (recent event feed), `memory_suggest_links` (similarity-based link discovery, requires embeddings)
- CLI commands: `clio stats [--namespace]`, `clio activity [--namespace] [--limit]`, `clio suggest-links <memory_id> [--threshold] [--limit]`
- All three MCP tools support both `markdown` and `json` response formats

### Phase 8: Always-on daemon and lifecycle — COMPLETE

- `daemon.rs` module in `clio-core`: `DaemonConfig`, `DaemonStatus`, `DaemonHealth`, `HealthCheck`, `HealthStatus` types; `PidFile` struct with stale PID detection via `kill -0`; platform path functions (`default_socket_path()`, `default_pid_path()`, `default_log_dir()`); health check functions for database, embeddings, and capture
- `daemon: DaemonConfig` added to `Settings` with `enabled`, `inbox_paths`, `socket_path`, `log_dir`, `http_port` fields
- `clio-daemon` crate: tokio-based always-on local process with PID file singleton locking, Unix domain socket control channel (`status`/`stop`/`health` JSON commands), inbox folder watcher via `notify` crate, dual tracing (stderr + daily rolling log files), graceful SIGTERM/SIGINT shutdown
- Inbox watcher: watches configured directories for new files, routes through capture pipeline (if enabled) or stores as plain notes, moves processed files to `_processed/` subdirectory, skips hidden files
- CLI subcommands: `clio daemon run` (foreground), `start` (background), `stop`, `restart`, `status`, `logs`, `install` (macOS LaunchAgent), `uninstall`, `doctor` (health checks)
- `clio daemon doctor` runs health checks without requiring the daemon to be running
- All daemon storage operations route through `clio-core` — no direct SQL

### Phase 9: Capture surfaces, review queue, and context assembly — COMPLETE

- Migration `003_review_queue`: `review_queue` table with status CHECK constraint (`pending`, `approved`, `rejected`, `edited`) and index
- `review.rs` module in `clio-core`: `ReviewItem`, `ReviewInput`, `ReviewEdits`, `ReviewStats` types; `queue_for_review()`, `list_pending()`, `get_review()`, `approve_review()` (converts to memory via `repository::remember()`), `reject_review()`, `edit_review()`, `review_stats()` functions
- Capture pipeline integration: `CaptureResult` enum (`Stored(Memory)` | `Queued(ReviewItem)`); captures below `review_threshold` in settings route to review queue instead of direct storage
- `review_threshold: Option<f64>` added to `CaptureConfig` (default `None` = disabled)
- CLI commands: `clio inbox list`, `clio inbox approve <id>`, `clio inbox reject <id>`, `clio inbox edit <id>` (with `--title`, `--namespace`, `--kind`, `--tags`, `--summary`, `--importance`), `clio inbox stats`
- MCP tools: `memory_inbox_list`, `memory_inbox_approve`, `memory_inbox_reject`, `memory_inbox_edit`
- `assembly.rs` module in `clio-core`: `ContextPreset` enum (6 variants: `project-brief`, `person-brief`, `decision-history`, `active-constraints`, `recent-activity`, `custom`), `ContextRequest`, `ContextSection`, `ContextBrief` types; `build_context()` combines kind-filtered and recent memories into sectioned briefs
- CLI command: `clio brief` with `--namespace`, `--preset`, `--query`, `--max-items`, `--include-links`
- MCP tool: `memory_context` with namespace auto-detection from `cwd`, preset selection, markdown/JSON output
- 7 unit tests for assembly module (all presets, round-tripping, empty DB, max_items budgeting)

### Immediate Next Steps

1. Begin Tauri desktop shell (Phase 10)
2. Add integration tests for daemon lifecycle and review queue flows
3. Add Raycast capture command templates and voice/transcript capture route
