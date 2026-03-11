# Clio Agent Guide

This file applies to the entire repository.

## Project Summary

Clio is a local-first shared memory system for AI tooling. The repository is a Rust workspace with five main crates (`clio-core`, `clio-cli`, `clio-mcp`, `clio-daemon`, `clio-tauri`) plus a Vue 3 UI in `ui/`.

## Core Architecture

- Keep all business logic in `crates/clio-core`.
- Treat CLI, MCP, daemon, and Tauri as thin adapters over the core.
- SQLite is the system of record; preserve existing schema, trigger, and FTS behaviour.
- `archive/python-prototype/` is reference-only and must not be extended.

## Critical Invariants

- Archive means hidden, not deleted.
- Archived records must stay excluded from default recall paths.
- Tags and FTS data must stay in sync.
- Upserts keyed by `source + source_ref` must not create duplicates.
- Access tracking is fire-and-forget and must never fail the parent operation.
- Auto-links must keep the `auto:relates_to` prefix distinction.
- With `decay_lambda = 0.0`, scoring must preserve original ranking.
- MCP defaults must match CLI and core semantics exactly.
- Never edit an applied migration; add a new migration instead.

## Working Conventions

- Use British English in documentation, comments, and user-facing text.
- Keep changes minimal and focused.
- Follow existing crate and module boundaries.
- Update documentation when behaviour, commands, or contracts change.

## Build And Test

- Build all crates with `./build.sh`.
- Build one crate with `./build.sh <crate-name>`.
- Use `cargo build` and `cargo test` for Rust-only work.
- Start Tauri development with `./dev.sh`.
- Front-end dependencies live in `ui/`; install with `cd ui && npm install`.

## Key References

Read these before significant implementation work:

- `context/ARCHITECTURE.md`
- `context/CRITICAL_WARNINGS.md`
- `context/DOMAIN_RULES.md`
- `docs/reference/schema.md`
- `docs/reference/mcp-contract.md`
- `docs/reference/settings.md`
