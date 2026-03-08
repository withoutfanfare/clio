# Clio

Local-first shared memory system for AI tooling. Rust workspace, SQLite storage, five crates (core, cli, mcp, daemon, tauri), Vue 3 desktop UI.

## Build & Dev

- **Build all:** `./build.sh` — builds all crates, restarts daemon
- **Build one:** `./build.sh <crate-name>` (e.g. `./build.sh clio-cli`)
- **Dev (Tauri):** `./dev.sh` — starts Vite + Tauri with hot reload
- **Rust only:** `cargo build` / `cargo test`
- **Frontend deps:** `cd ui && npm install`

## Architecture

- All business logic in `clio-core`. CLI, MCP, daemon, and Tauri are thin adapters.
- SQLite (WAL mode, FTS5) — single DB file, no external services.
- Tauri UI: Vue 3 + Pinia + Vue Router, built with Vite. Source in `ui/src/`.
- Five command modules in Tauri: memory, search, stats, namespaces, clipboard.

## Critical Rules

- Never put business logic outside `clio-core`.
- Archive means hidden, not deleted. Archived records excluded from default recall.
- Tags and FTS must stay in sync (triggers handle this).
- Upsert keyed on `source + source_ref` — must not create duplicates.
- Access tracking is fire-and-forget — never fail the parent operation.
- Auto-links use `auto:relates_to` prefix to distinguish from human links.
- Scoring with `decay_lambda = 0.0` must preserve original ranking (backwards-compatible).
- MCP defaults must match CLI/core semantics exactly.
- Every schema change ships as a new migration — never edit applied migrations.
- `archive/python-prototype/` is reference only — do not extend.

## Context (deep reference)

Read these before significant implementation work:

- `context/ARCHITECTURE.md` — crate boundaries, system diagram, module listings, tech stack
- `context/CRITICAL_WARNINGS.md` — invariants that produce bugs if violated
- `context/DOMAIN_RULES.md` — entities, namespaces, workflows, search semantics

## Contracts (source of truth)

- `docs/reference/schema.md` — SQLite schema, indexes, FTS, triggers
- `docs/reference/mcp-contract.md` — MCP tool/resource definitions
- `docs/reference/settings.md` — all config keys and defaults

## Rules

- British English in all documentation, comments, and user-facing text.
- Conventional commits. Stage with `gitaddall`.
