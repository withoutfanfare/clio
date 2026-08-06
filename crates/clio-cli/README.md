# clio-cli

Thin CLI wrapper over `clio-core`. Handles argument parsing (clap), text/JSON rendering, and exit codes.

See [CLI Reference](../../docs/cli-reference.md) for the full command listing.

## Key Commands

- `clio remember` / `recall` / `search` / `show` / `recent` — memory CRUD
- `clio capture` — LLM classification pipeline
- `clio remote-mcp` — bounded stdio-to-SSH MCP bridge with local namespace detection
- `clio settings use-remote` — persist one remote route for CLI, hooks, MCP clients, and Tauri
- `clio daemon` — daemon lifecycle management
- `clio setup` — generate MCP client configuration
- `clio stats` / `activity` / `suggest-links` — analytics and knowledge graph

The remote bridge accepts newline-delimited JSON-RPC requests up to 2 MiB,
removes the client-only `cwd`, and forwards a locally detected namespace over
non-interactive SSH. It has no offline cache or later synchronisation path.

Must NOT open ad hoc SQL queries or implement its own validation rules.
