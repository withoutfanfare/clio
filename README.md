# Clio

Local-first shared memory system for AI tooling. One Rust core, one SQLite database, every AI agent you use.

## Features

- **Full-text search** — FTS5 with BM25 ranking and temporal relevance scoring
- **Semantic search** — vector embeddings via local fastembed or OpenAI
- **Namespaces** — automatic project scoping from working directory
- **Knowledge graph** — typed directional links between memories with auto-link inference
- **Capture pipeline** — LLM-based classification of unstructured text
- **Always-on daemon** — inbox watcher, background linking, health checks
- **10+ agent integrations** — Claude Code, Codex, Cursor, Windsurf, Gemini, Copilot, OpenCode, Kilo, Kimi, and any MCP-compatible client
- **Desktop UI** — Tauri-based browse/edit/archive interface (in progress)

## Quick Start

```sh
cargo install --path crates/clio-cli
cargo install --path crates/clio-mcp
clio init
clio setup claude-code   # or: codex, cursor, windsurf, gemini, copilot, generic
```

## Documentation

| Document | Description |
|----------|-------------|
| [Getting Started](docs/getting-started.md) | Installation, setup, and first steps |
| [CLI Reference](docs/cli-reference.md) | All commands, flags, and examples |
| [MCP Agent Setup](docs/mcp-agent-setup.md) | Connecting AI agents to Clio |
| [Resource Limits](docs/resource-limits.md) | Sizing constraints and thresholds |
| [Settings Reference](docs/reference/settings.md) | All configuration keys and defaults |
| [Schema Reference](docs/reference/schema.md) | SQLite tables, indexes, FTS, triggers |
| [MCP Contract](docs/reference/mcp-contract.md) | Full MCP tool and resource definitions |
| [Rationale](docs/rationale.md) | Why Clio exists |
| [Security Review](docs/security-review.md) | Codebase security audit |

## Architecture

Rust core library (`clio-core`) owns all business logic. CLI, MCP server, daemon, and desktop UI are thin adapters. See [Architecture](context/ARCHITECTURE.md) for the full system diagram.

## Licence

See repository root.
