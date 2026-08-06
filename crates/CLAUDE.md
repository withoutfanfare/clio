# crates — Rust workspace

Local contract for the Rust workspace. Inherits all repo-wide rules from the
root CLAUDE.md. This doc owns the crate boundaries and Rust build/test workflow.

## Purpose

Five crates make up Clio: one core library and four thin adapters over it.

| Crate | Role |
|-------|------|
| `clio-core` | All business logic. SQLite storage, scoring, migrations, embeddings. |
| `clio-cli` | Command-line adapter (single `main.rs`, ~25 subcommands). |
| `clio-mcp` | MCP server adapter (stdio JSON-RPC) for AI agents. |
| `clio-daemon` | Background watcher: auto-embed, auto-link, capture. |
| `clio-tauri` | Desktop backend; commands in `clio-tauri/src/commands/`. |

## Ownership

- This doc owns: the core/adapter boundary and Rust build/test conventions.
- `clio-core` owns its own invariants and migration rules — see its child doc.
- Adapters (`cli`, `mcp`, `daemon`, `tauri`) have no child doc; they follow this
  doc plus the root. They contain no business logic of their own.

## Work Guidance

- MCP defaults must match CLI/core semantics exactly.
- Follow existing crate and module boundaries.
- `clio-tauri` backend commands live under `src/commands/` (one module per
  domain: memory, search, stats, namespaces, clipboard, deduplication).

## Verification

Build/dev commands: see root CLAUDE.md.

## Child DOX Index

- [clio-core](clio-core/CLAUDE.md) — business-logic home; all invariants and migration rules.
