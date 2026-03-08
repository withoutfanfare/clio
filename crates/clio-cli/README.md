# clio-cli

Thin CLI wrapper over `clio-core`. Handles argument parsing (clap), text/JSON rendering, and exit codes.

See [CLI Reference](../../docs/cli-reference.md) for the full command listing.

## Key Commands

- `clio remember` / `recall` / `search` / `show` / `recent` — memory CRUD
- `clio capture` — LLM classification pipeline
- `clio daemon` — daemon lifecycle management
- `clio setup` — generate MCP client configuration
- `clio stats` / `activity` / `suggest-links` — analytics and knowledge graph

Must NOT open ad hoc SQL queries or implement its own validation rules.
