# Getting Started with Clio

Clio is a local-first shared memory system for AI tooling. It stores structured memories in a SQLite database on your machine, exposes them through a CLI and an MCP server, and uses semantic search to surface the right context when you need it.

This guide takes you from installation to a working setup in ten minutes.

---

## 1. Quick Start

### Install

Build and install both binaries from source. You need a working Rust toolchain.

```sh
cargo install --path crates/clio-cli
cargo install --path crates/clio-mcp
```

### Initialise the database

```sh
clio init
```

This creates the SQLite database at the platform default location:

- macOS: `~/Library/Application Support/clio/memory.db`
- Linux: `$XDG_DATA_HOME/clio/memory.db` (falls back to `~/.local/share/clio/memory.db`)
- Windows: `%APPDATA%\clio\memory.db`

To also pin a project namespace to the current directory, pass `--namespace`:

```sh
clio init --namespace project:my-project
```

This creates a `.clio-namespace` file in the current directory. Any future command run from this directory (or a subdirectory) will automatically scope to `project:my-project`.

### Store your first memory

```sh
clio remember \
  --content "SQLite is the default store" \
  --title "Storage decision" \
  --kind decision \
  --tags sqlite,architecture
```

### Recall it

```sh
clio recall --query "storage"
```

### Show the full detail of a memory

```sh
clio show <id>
```

Replace `<id>` with the UUID printed when you stored the memory.

---

## 2. Namespace Auto-Detection

Namespaces scope your memories by project, tool, or topic. Clio detects the right namespace automatically so you rarely need to pass `--namespace` explicitly.

### How detection works

When you run a command, Clio walks up the directory tree from your current working directory, looking for the first matching marker:

1. `.clio-namespace` file — reads the file content as the full namespace string (e.g. `project:clio`)
2. `.git` directory — derives `project:<repo-name>` from the directory name
3. `Cargo.toml` or `package.json` — derives `project:<dir-name>` from the directory name
4. Falls back to `global` if no marker is found

### The `.clio-namespace` file

This file takes the highest priority and contains exactly one line: the namespace string.

```text
project:clio
```

Create one with:

```sh
clio init --namespace project:my-project
```

### Check the detected namespace

```sh
clio context
```

This prints the namespace Clio would use for the current directory — useful for confirming detection before you write memories.

### Scoped recall

When you run `clio recall` inside a project directory, Clio applies a two-pass strategy:

1. Search within the detected project namespace first
2. Fill any remaining result slots from `global`

Project-scoped results appear before global ones. Pass an explicit `--namespace` flag to disable this fallback and search a single namespace only.

---

## 3. Storing Memories

`clio remember` is the primary write command. The only required flag is `--content`.

### Full options

| Flag | Description | Default |
|---|---|---|
| `--content` | Memory content (required; use `-` to read from stdin) | — |
| `--namespace` | Override the auto-detected namespace | auto-detected |
| `--kind` | Memory type | `note` |
| `--title` | Short label | none |
| `--summary` | Concise preview | none |
| `--tags` | Comma-separated tags | none |
| `--source` | Identifier for the writing system | none |
| `--source-ref` | External idempotency key | none |
| `--confidence` | Certainty score (0.0–1.0) | none |
| `--importance` | Relative significance (1–5) | `3` |
| `--metadata` | Arbitrary JSON object string | `{}` |
| `--upsert` | Update in place if `source` + `source-ref` match | false |

### Memory kinds

The `--kind` flag is guidance rather than a strict enum. Common values:

- `note` — general purpose
- `fact` — verified information
- `decision` — architectural or product decision
- `snippet` — code or configuration fragment
- `preference` — stated preference or constraint
- `process` — a defined workflow or procedure

### Examples

Store a decision with tags and an importance score:

```sh
clio remember \
  --content "We use WAL mode for SQLite to support concurrent readers." \
  --title "SQLite WAL decision" \
  --kind decision \
  --tags sqlite,architecture,performance \
  --importance 4
```

Read content from stdin — useful for piping output from other tools:

```sh
echo "Use fastembed for local embeddings by default." | clio remember --content -
```

Upsert — safe to run repeatedly without creating duplicates:

```sh
clio remember \
  --content "Auth service moved to a separate repository." \
  --source notes \
  --source-ref auth-migration-001 \
  --upsert
```

### Auto-embedding

Memories are automatically embedded for semantic search after each write. This behaviour is controlled by the `auto_embed` setting, which defaults to `true`. See [Section 11. Settings](#11-settings) to change it.

---

## 4. Capture Pipeline

The capture pipeline sends unstructured text to an LLM, which classifies it into a structured memory — assigning kind, title, summary, tags, namespace, importance, and confidence automatically.

### Enable capture

```sh
clio settings use-capture --api-key sk-...
```

This uses `gpt-4o-mini` by default. Specify a different model or a compatible API endpoint:

```sh
clio settings use-capture \
  --api-key sk-... \
  --model gpt-4o \
  --base-url https://api.openai.com/v1
```

### Dry run first

Preview what the LLM would classify without storing anything:

```sh
clio capture --text "We decided to use Redis for caching" --dry-run
```

### Store a captured memory

```sh
clio capture --text "We decided to use Redis for caching"
```

The stored memory will have `source: "capture"` set automatically. The original text is always preserved as the `content` field unchanged.

---

## 5. Retrieving Memories

Clio offers two search modes: full-text search and semantic search. They serve different purposes and can be used together.

### Full-text search

```sh
clio recall --query "redis caching"
```

Uses SQLite FTS5 with BM25 ranking. Matches keywords in title, summary, content, and tags. Fast, works on all memories, and requires no embedding setup.

**Filters:**

```sh
# Scope to a namespace
clio recall --query "redis" --namespace project:my-project

# Filter by kind
clio recall --query "redis" --kind decision

# Filter by tags (matches ALL tags by default)
clio recall --query "redis" --tags caching,backend

# Match ANY of the specified tags instead
clio recall --query "redis" --tags caching,backend --match-any

# Include archived memories
clio recall --query "redis" --include-archived

# Paginate
clio recall --query "redis" --limit 20 --offset 40
```

**JSON output:**

```sh
clio recall --query "redis" --json
```

### Semantic search

```sh
clio search --query "database performance optimisation"
```

Finds conceptually related memories using vector embeddings and cosine similarity — even when the query words do not appear in the content. Requires embeddings to be enabled (they are by default).

Uses local fastembed (`all-MiniLM-L6-v2`, 384 dimensions) by default, or OpenAI embeddings if configured.

**Filters:**

```sh
clio search --query "database performance" --namespace project:my-project --limit 5
clio search --query "database performance" --include-archived
```

### When to use which

| Use case | Command |
|---|---|
| You know the words that appear in the memory | `clio recall` |
| You want conceptually related content | `clio search` |
| You want the most recent memories | `clio recent` |
| You know the memory ID | `clio show <id>` |

### Other retrieval commands

```sh
# Most recently updated memories
clio recent

# Scoped to a namespace, limited to 5
clio recent --namespace project:my-project --limit 5

# Full detail of a single memory
clio show 01954d70-cf20-7d42-bb3b-ff2f0f0de123
```

---

## 6. Knowledge Graph

Memories can be linked to form a typed knowledge graph.

### Create a link

```sh
clio link <from-id> <to-id> --relationship supports
```

Links are directional. The default relationship is `relates_to` if `--relationship` is omitted.

**Available relationship types:**

- `relates_to` — general association (default)
- `supports` — one memory supports or corroborates another
- `contradicts` — conflicting content
- `supersedes` — one decision replaces another
- `derived_from` — one memory derived from another
- Any custom string up to 60 characters

### Discover link suggestions

```sh
clio suggest-links --memory-id <id>
```

Uses embedding similarity to suggest memories that are related but not yet linked. Adjust sensitivity with `--threshold` (default `0.7`) and `--limit` (default `5`):

```sh
clio suggest-links --memory-id <id> --threshold 0.8 --limit 10
```

---

## 7. Archiving

Clio never hard-deletes memories. Archive is a soft operation — the record is preserved and still searchable when you ask for it explicitly.

```sh
# Soft-archive a memory
clio archive <id>

# Restore it
clio unarchive <id>

# Include archived memories in search results
clio recall --query "redis" --include-archived
```

---

## 8. Migration

Import your existing memory from Claude or ChatGPT exports.

### Import from Claude

```sh
clio migrate --source claude --file conversations.json
```

### Import from ChatGPT

```sh
clio migrate --source chatgpt --file export.json
```

### Options

| Flag | Description |
|---|---|
| `--namespace` | Override the default namespace for imported memories |
| `--classify` | Route each entry through the capture pipeline for LLM classification |
| `--dry-run` | Preview what would be imported without writing anything |

### Idempotent by design

Migration uses content-hash deduplication. Running the same import file multiple times updates existing records in place rather than creating duplicates. It is safe to re-run.

---

## 9. MCP Server Setup

Clio exposes an MCP (Model Context Protocol) server so AI clients such as Claude Code, Cursor, and Windsurf can read and write memories directly during a session.

### Start the MCP server

```sh
clio serve
```

This starts `clio-mcp` on stdio, with `CLIO_DB_PATH` set automatically.

### Generate client configuration

```sh
clio setup claude-code   # Claude Code
clio setup cursor        # Cursor
clio setup windsurf      # Windsurf
clio setup generic       # Generic MCP JSON
```

Each command prints ready-to-paste configuration for that client. The generic output looks like this:

```json
{
  "mcpServers": {
    "clio": {
      "command": "/path/to/clio-mcp",
      "env": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      }
    }
  }
}
```

### Available MCP tools

Once connected, AI clients have access to 21 tools. For full connection instructions covering 10+ agents, see [MCP Agent Setup](mcp-agent-setup.md). The core tools:

| Tool | Purpose |
|---|---|
| `memory_remember` | Store or upsert a memory |
| `memory_recall` | Full-text search |
| `memory_get` | Fetch one memory by ID |
| `memory_recent` | List recent memories |
| `memory_search` | Semantic search |
| `memory_capture` | LLM-classify and store unstructured text |
| `memory_link` | Create a typed link between two memories |
| `memory_get_links` | Get all links from a memory |
| `memory_suggest_links` | Find semantically similar unlinked memories |
| `memory_archive` | Soft-archive a memory |
| `memory_unarchive` | Restore an archived memory |
| `memory_namespaces` | List all namespaces in the database |
| `memory_stats` | Aggregate statistics |
| `memory_activity` | Recent activity feed |

MCP tools that accept a `cwd` parameter (`memory_remember`, `memory_recall`, `memory_capture`, `memory_search`) will auto-detect the project namespace from the working directory, giving AI clients the same scoped recall behaviour as the CLI.

---

## 10. Stats and Analytics

### Overall statistics

```sh
clio stats
```

Prints total, active, and archived memory counts; breakdown by namespace and kind; a weekly creation timeline; top tags; embedding coverage; and link density.

### Scoped to one namespace

```sh
clio stats --namespace project:clio
```

### Activity feed

```sh
clio activity
```

Shows a feed of recent creates, updates, and archives across all memories.

```sh
clio activity --namespace project:clio --limit 20
```

---

## 11. Settings

Settings are stored in `clio-settings.json` alongside the database.

### View current settings

```sh
clio settings show
```

### Embedding providers

**Local (default)** — uses fastembed, `all-MiniLM-L6-v2`, 384 dimensions. No API key required.

```sh
clio settings use-local-embeddings
```

**OpenAI** — uses `text-embedding-3-small`, 1536 dimensions.

```sh
clio settings use-openai-embeddings --api-key sk-...
```

### Auto-embed toggle

```sh
clio settings auto-embed --enable
clio settings auto-embed --disable
```

When disabled, memories are stored without embeddings. Run `clio embed --all` later to backfill.

### Capture settings

```sh
clio settings use-capture --api-key sk-... [--model gpt-4o-mini] [--base-url https://api.openai.com/v1]
```

---

## 12. Other Commands

### Export and import

```sh
# Export all memories to JSONL
clio export --output memories.jsonl

# Export one namespace only
clio export --output memories.jsonl --namespace project:clio

# Import from JSONL
clio import --input memories.jsonl
```

### Manage embeddings

```sh
# Embed all memories that do not yet have embeddings
clio embed --all

# Embed a specific memory
clio embed --id <id>
```

### Inspect the database

```sh
# Show database schema summary
clio schema

# List all namespaces in use
clio namespaces
```

---

## 13. CLI Quick Reference

### Command table

| Command | Description | Key flags |
|---|---|---|
| `init` | Initialise database | `--namespace` |
| `context` | Show detected namespace | |
| `remember` | Store a memory | `--content`, `--kind`, `--title`, `--tags`, `--upsert` |
| `recall` | Full-text search | `--query`, `--namespace`, `--kind`, `--tags`, `--limit` |
| `show` | Show memory by ID | |
| `recent` | List recent memories | `--namespace`, `--limit` |
| `search` | Semantic search | `--query`, `--namespace`, `--limit` |
| `capture` | LLM-classify text | `--text`, `--dry-run` |
| `archive` | Soft-archive a memory | |
| `unarchive` | Restore an archived memory | |
| `link` | Link two memories | `--relationship` |
| `suggest-links` | Find similar unlinked memories | `--memory-id`, `--threshold` |
| `stats` | Statistics | `--namespace` |
| `activity` | Activity feed | `--namespace`, `--limit` |
| `namespaces` | List namespaces | |
| `migrate` | Import from Claude or ChatGPT | `--source`, `--file`, `--classify` |
| `export` | Export to JSONL | `--output`, `--namespace` |
| `import` | Import from JSONL | `--input` |
| `embed` | Manage embeddings | `--all`, `--id` |
| `settings` | View or update settings | subcommands |
| `serve` | Start MCP server | |
| `setup` | Generate MCP config | client name |
| `schema` | Show DB schema summary | |

### Global flags

These flags apply to every command:

| Flag | Description |
|---|---|
| `--db-path` | Override the database path |
| `--json` | Output as JSON |

---

## 14. Troubleshooting

### Database locked errors

If you see `database is locked` errors, another process has an exclusive lock on the SQLite file. This can happen when:

- The MCP server and CLI are running simultaneously (WAL mode should prevent this, but check)
- A long-running query is active
- Another tool has the database open

Solution: Close other connections or wait a few seconds and retry.

### Embedding errors

**"embeddings are disabled"**

Run `clio settings auto-embed --enable` to re-enable auto-embedding.

**"embedding backend not available"**

The `fastembed` feature flag is not enabled. Build with:
```sh
cargo install --path crates/clio-cli --features fastembed
cargo install --path crates/clio-mcp --features fastembed
```

**"OpenAI API key required"**

You've selected OpenAI embeddings but not configured an API key:
```sh
clio settings use-openai-embeddings --api-key sk-...
```

### Capture errors

**"capture pipeline is not enabled"**

Enable capture first:
```sh
clio settings use-capture --api-key sk-...
```

**"LLM returned unparseable JSON"**

The LLM response could not be parsed. This is rare with GPT-4o-mini. Try again or check your API endpoint.

### MCP connection issues

**Agent cannot connect to Clio**

1. Verify `clio-mcp` is on your PATH: `which clio-mcp`
2. Check the MCP config has the correct absolute path
3. Verify `CLIO_DB_PATH` points to an existing database file
4. Test manually: `CLIO_DB_PATH=/path/to/memory.db clio-mcp` (should start and wait for JSON-RPC on stdin)

**"Memory not found" errors**

The memory ID may be incorrect or the memory may have been deleted. Use `clio recall` to find the correct ID.

### Namespace detection not working

If `clio context` shows `global` when you expected a project namespace:

1. Check for a `.clio-namespace` file in the current directory
2. Check for a `.git` directory, `Cargo.toml`, or `package.json` in the directory tree
3. Verify you're running the command from within the project directory (not from outside)

Create an explicit namespace file:
```sh
clio init --namespace project:my-project
```

### Memory not appearing in search

1. Check if it was archived: `clio recall --include-archived --query "your terms"`
2. Check the namespace: `clio recall --namespace project:your-project --query "your terms"`
3. Try semantic search: `clio search --query "conceptually related content"`
4. Verify the memory exists: `clio show <id>`

### Performance issues

**Slow semantic search**

Semantic search must compute cosine similarity against all embedded memories. For large databases (10k+ memories), this can take a few seconds. Consider:
- Using `--namespace` to filter the search space
- Using `clio recall` (FTS) for keyword-based searches

**Slow capture**

The capture pipeline calls an external LLM API. Latency depends on your network and the API provider. Use `--dry-run` to test without storing.

### Getting help

1. Check this troubleshooting section
2. Review the [CLI Reference](cli-reference.md) for command details
3. Check the [Settings Reference](reference/settings.md) for configuration options
4. Review the [MCP Contract](reference/mcp-contract.md) for tool behaviour
5. Open an issue on the repository with:
   - The exact command you ran
   - The full error message
   - Your operating system
   - Output of `clio schema` (for schema-related issues)

---

## Related Documentation

- [Desktop App (Tauri)](tauri-app.md) — visual interface for browsing and editing memories
- [MCP Agent Setup](mcp-agent-setup.md) — connecting 10+ AI agents to Clio
- [CLI Reference](cli-reference.md) — quick-reference command listing
- [Settings Reference](reference/settings.md) — all configuration keys and defaults
- [Resource Limits](resource-limits.md) — sizing constraints and thresholds
