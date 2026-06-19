# Clio

**Local-first shared memory for AI tooling.** One Rust core, one SQLite database, every AI agent you use.

```text
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│ Claude Code │    │   Codex     │    │   Cursor    │    │  Windsurf   │
└──────┬──────┘    └──────┬──────┘    └──────┬──────┘    └──────┬──────┘
       │                  │                  │                  │
       └──────────────────┴──────────────────┴──────────────────┘
                                  │
                          MCP (stdio)
                                  │
                          ┌───────▼───────┐
                          │   clio-mcp    │
                          │  MCP server   │
                          └───────┬───────┘
                                  │
                          ┌───────▼───────┐
                          │  clio-core    │
                          │ (Rust logic)  │
                          └───────┬───────┘
                                  │
                          ┌───────▼───────┐
                          │    SQLite     │
                          │  memory.db    │
                          └───────────────┘
```

## Why Clio?

AI coding assistants forget everything between sessions. You re-explain your project, preferences, and decisions every time. Clio fixes this by giving every agent you use access to the same persistent memory:

- **One memory, every agent** — Claude Code, Codex, Cursor, Windsurf, Gemini, Copilot, OpenCode, Kilo, Kimi all share the same knowledge
- **Zero cloud dependency** — SQLite on your machine, embeddings run locally by default
- **Automatic scoping** — memories are scoped to projects via directory detection, no manual namespace management
- **Semantic search** — find conceptually related context even when keywords don't match
- **Knowledge graph** — link related memories to build a navigable web of context

## Features

- **Full-text search** — FTS5 with BM25 ranking and temporal relevance scoring
- **Semantic search** — vector embeddings via local fastembed or OpenAI
- **Namespaces** — automatic project scoping from working directory
- **Knowledge graph** — typed directional links between memories with auto-link inference
- **Capture pipeline** — LLM-based classification of unstructured text
- **Always-on daemon** — inbox watcher, background linking, health checks
- **10+ agent integrations** — Claude Code, Codex, Cursor, Windsurf, Gemini, Copilot, OpenCode, Kilo, Kimi, and any MCP-compatible client
- **Desktop UI** — Tauri-based browse/edit/archive interface

## Quick Start

```sh
# Build and install
cargo install --path crates/clio-cli
cargo install --path crates/clio-mcp

# Initialise the database
clio init

# Connect your AI agent
clio setup claude-code   # or: codex, cursor, windsurf, gemini, copilot, opencode, kilo, kimi
```

## First Memory

```sh
# Store a decision
clio remember \
  --content "We use SQLite for persistence because it's zero-config and portable" \
  --title "SQLite decision" \
  --kind decision \
  --tags architecture,storage \
  --importance 4

# Recall it later
clio recall --query "storage"
```

## Documentation

| Document | Description |
|----------|-------------|
| [Getting Started](docs/getting-started.md) | Installation, setup, and first steps |
| [CLI Reference](docs/cli-reference.md) | All commands, flags, and examples |
| [MCP Agent Setup](docs/mcp-agent-setup.md) | Connecting AI agents to Clio |
| [Settings Reference](docs/reference/settings.md) | All configuration keys and defaults |
| [Schema Reference](docs/reference/schema.md) | SQLite tables, indexes, FTS, triggers |
| [MCP Contract](docs/reference/mcp-contract.md) | Full MCP tool and resource definitions |
| [Architecture](context/ARCHITECTURE.md) | System diagram, crate boundaries, tech stack |

**More documentation:** [docs/README.md](docs/README.md)

## Architecture

Rust core library (`clio-core`) owns all business logic. CLI, MCP server, daemon, and desktop UI are thin adapters over the core.

```text
crates/
├── clio-core/     # All business logic, SQLite access, embeddings
├── clio-cli/      # Thin CLI wrapper (clap)
├── clio-mcp/      # MCP server adapter (stdio)
├── clio-daemon/   # Background processes (inbox, auto-link)
└── clio-tauri/    # Desktop UI (Vue 3 + Tauri 2)
```

## Licence

See repository root.
