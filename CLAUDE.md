# Clio

Local-first shared memory system for AI tooling. Rust core, SQLite storage, multiple access surfaces (CLI, MCP, Tauri).

## Context

Read these before implementation work:

- `context/ARCHITECTURE.md` — system diagram, crate boundaries, tech stack, config
- `context/CRITICAL_WARNINGS.md` — invariants that produce bugs if violated
- `context/DOMAIN_RULES.md` — entities, namespaces, workflows, search

Canonical contracts (source of truth for implementation):

- `docs/reference/schema.md` — SQLite schema, indexes, FTS, triggers
- `docs/reference/mcp-contract.md` — MCP tool/resource definitions
- `docs/plan/implementation-plan.md` — full delivery plan

## Rules

- All business logic lives in `clio-core`. CLI and MCP are thin adapters.
- British English in all documentation, comments, and user-facing text.
- Conventional commits. Stage with `gitaddall`.
- The `archive/python-prototype/` is reference only — do not extend.
