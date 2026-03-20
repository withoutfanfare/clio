# AI Shared Memory

Local-first shared memory for AI tools. It provides:

- A single SQLite database that multiple tools can share.
- A CLI for shell scripts and non-MCP agents.
- An MCP server for any MCP-capable client.
- Full-text search, tags, namespaces, links, and source-aware upserts.

## Why this shape

SQLite is the right default here:

- easy to back up and inspect
- no daemon to keep running
- stable file format
- fast enough for local memory workloads
- future-safe export path through JSONL

The database is local and portable. MCP is just an access surface over the same store, so you are not trapped inside one client or one vendor.

## Features

- `uuid7`-style sortable IDs when available
- namespaced memories such as `global`, `project:my-app`, `person:danny`
- memory kinds such as `note`, `fact`, `decision`, `summary`, `task`
- tags and typed links between memories
- FTS5 search over title, summary, content, and tags
- source-aware upsert using `source + source_ref`
- soft archive instead of hard delete
- JSON metadata for future extensions

## Quick start

```bash
uv sync
uv run ai-memory init
uv run ai-memory remember \
  --namespace global \
  --kind decision \
  --title "Use SQLite as the default store" \
  --content "Shared local memory should default to SQLite with WAL mode." \
  --tags memory sqlite architecture

uv run ai-memory recall --query sqlite --json
```

## Database location

Default location:

- macOS: `~/Library/Application Support/ai-shared-memory/memory.db`
- Linux: `$XDG_DATA_HOME/ai-shared-memory/memory.db` or `~/.local/share/ai-shared-memory/memory.db`
- Windows: `%APPDATA%\\ai-shared-memory\\memory.db`

Override it for every tool with:

```bash
export AI_SHARED_MEMORY_DB_PATH="$HOME/.ai-memory/shared.db"
```

That env var is the main integration contract for cross-tool sharing.

## CLI usage

Initialise the database:

```bash
uv run ai-memory init
```

Create a memory:

```bash
uv run ai-memory remember \
  --namespace project:ai \
  --kind note \
  --title "MCP transport" \
  --content "Prefer stdio for local agents; add streamable HTTP later if needed." \
  --tags mcp transport local \
  --source codex \
  --source-ref design-001 \
  --upsert
```

Search:

```bash
uv run ai-memory recall --query "stdio" --namespace project:ai
```

Show one item:

```bash
uv run ai-memory show <memory-id>
```

Link two memories:

```bash
uv run ai-memory link <from-id> <to-id> --relationship relates_to
```

Archive one item:

```bash
uv run ai-memory archive <memory-id>
```

Export for backup or migration:

```bash
uv run ai-memory export --output ./memory-export.jsonl
```

## MCP usage

Run the server over stdio:

```bash
uv run ai-memory-mcp
```

Example Claude Desktop style config:

```json
{
  "mcpServers": {
    "ai_shared_memory": {
      "command": "uv",
      "args": [
        "--directory",
        "/Users/dannyharding/Databases/AI",
        "run",
        "ai-memory-mcp"
      ],
      "env": {
        "AI_SHARED_MEMORY_DB_PATH": "/Users/dannyharding/.ai-memory/shared.db"
      }
    }
  }
}
```

Exposed MCP tools:

- `memory_remember`
- `memory_recall`
- `memory_get`
- `memory_recent`
- `memory_link`
- `memory_archive`

Exposed MCP resources:

- `memory://schema`
- `memory://item/{memory_id}`
- `memory://recent/{namespace}`

## Future extensions

This build keeps the core small, but the schema is ready for:

- embeddings stored in a side table
- sync adapters for cloud backup
- per-tool ACLs if you later need them
- graph traversal over links
- temporal expiry and confidence-based pruning
- event streams for automatic summarisation

## Notes

- SQLite FTS5 is included in the macOS `sqlite3` build on this machine.
- Archived records are kept for auditability.
- `metadata_json` is intentionally unopinionated to keep client integrations simple.
