# Clio

Local-first shared memory system for AI tooling. Rust workspace, SQLite storage, five crates (core, cli, mcp, daemon, tauri), Vue 3 desktop UI.

## Build & Dev

- **Build all:** `./build.sh` — installs all binaries, restarts daemon (macOS/launchctl; daemon logs in `~/Library/Logs/clio/`)
- **Build one:** `./build.sh {cli|mcp|daemon|tauri|restart}` (short names only — not `clio-cli`)
- **Dev (Tauri):** `./dev.sh` — starts Vite + Tauri with hot reload
- **Rust only:** `cargo build` / `cargo test`
- **Single test:** `cargo test -p clio-core <name>` (most tests live in `clio-core`, inline + `tests/integration.rs`)
- **Lint/format:** `cargo fmt` / `cargo clippy` (no custom config — workspace defaults)
- **Frontend deps:** `cd ui && npm install`
- **Frontend typecheck/build:** `cd ui && npm run build` (runs `vue-tsc --noEmit` then `vite build`)

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

# DOX framework

- DOX is highly performant CLAUDE.md hierarchy installed here
- Agent must follow DOX instructions across any edits

## Core Contract

- CLAUDE.md files are binding work contracts for their subtrees
- Work products, source materials, instructions, records, assets, and durable docs must stay understandable from the nearest applicable CLAUDE.md plus every parent CLAUDE.md above it

## Read Before Editing

1. Read the root CLAUDE.md
2. Identify every file or folder you expect to touch
3. Walk from the repository root to each target path
4. Read every CLAUDE.md found along each route
5. If a parent CLAUDE.md lists a child CLAUDE.md whose scope contains the path, read that child and continue from there
6. Use the nearest CLAUDE.md as the local contract and parent docs for repo-wide rules
7. If docs conflict, the closer doc controls local work details, but no child doc may weaken DOX

Every meaningful change requires a DOX pass before the task is done.

Update the closest owning CLAUDE.md when a change affects:

- purpose, scope, ownership, or responsibilities
- required inputs, outputs, permissions, constraints, side effects, or artifacts
- user preferences about behavior, communication, process, organization, or quality
- CLAUDE.md creation, deletion, move, rename, or index contents

Update parent docs when parent-level structure, ownership, workflow, or child index changes. Update child docs when parent changes alter local rules. Remove stale or contradictory text immediately. Small edits that do not change behavior or contracts may leave docs unchanged, but the DOX pass still must happen.
## Hierarchy

- Root CLAUDE.md is the DOX rail: project-wide instructions, global preferences, durable workflow rules, and the top-level Child DOX Index
- Child CLAUDE.md files own domain-specific instructions and their own Child DOX Index
- Each parent explains what its direct children cover and what stays owned by the parent
- The closer a doc is to the work, the more specific and practical it must be
## Child Doc Shape

- Create a child CLAUDE.md when a folder becomes a durable boundary with its own purpose, rules, responsibilities, workflow, materials, or quality standards
- Work Guidance must reflect the current standards of the project or user instructions; if there are no specific standards or instructions yet, leave it empty
- Verification must reflect an existing check; if no verification framework exists yet, leave it empty and update it when one exists
## User Preferences

When the user requests a durable behavior change, record it here or in the relevant child CLAUDE.md

## Child DOX Index

- [crates](crates/CLAUDE.md) — Rust workspace: core/adapter boundary, build/test. Indexes `clio-core`.
- [ui](ui/CLAUDE.md) — Vue 3 desktop frontend (npm/Vite/TypeScript).
- [docs](docs/CLAUDE.md) — user/contributor docs and the contract source-of-truth files.
- [context](context/CLAUDE.md) — deep reference: architecture, critical warnings, domain rules.
- [archive](archive/CLAUDE.md) — frozen, reference-only material (do not extend).
