# MCP Agent Setup

> For the full MCP tool and resource contract, see [MCP Contract](reference/mcp-contract.md).

Clio exposes an MCP (Model Context Protocol) server that gives AI coding agents persistent, structured memory across sessions. The server runs over stdio and provides 21 tools for reading, writing, searching, and linking memories.

This section covers connection setup for six AI agents, then explains **how to actually use Clio** once connected — workflows, prompting patterns, and practical examples.

**New to Clio?** Start with the [Getting Started Guide](getting-started.md) to install and initialise the database first.

---

### How it works

```text
┌──────────────┐    stdio (JSON-RPC)    ┌──────────┐    rusqlite    ┌───────────┐
│  AI Agent    │ ◄─────────────────────► │ clio-mcp │ ◄────────────► │ SQLite DB │
│ (Claude,     │                         │ (MCP     │               │ memory.db │
│  Codex, etc) │                         │  server) │               └───────────┘
└──────────────┘                         └──────────┘
```

The MCP server is a thin adapter over `clio-core`. All 21 tools map directly to the same functions the CLI uses. Memories stored by one agent are immediately available to every other agent and the CLI.

---

### One-command setup

Each command auto-installs Clio into the correct config file. No paths to copy, no JSON to paste.

```sh
clio setup claude-code   # → ~/.claude.json
clio setup codex         # → ~/.codex/config.toml
clio setup opencode      # → ~/.config/opencode/opencode.json
clio setup copilot       # → ~/.copilot/mcp-config.json
clio setup gemini        # → ~/.gemini/settings.json
clio setup kilo          # → ~/.kilocode/mcp.json
clio setup kimi          # → ~/.kimi/mcp.json
clio setup cursor        # → ~/.cursor/mcp.json
clio setup windsurf      # → ~/.windsurf/mcp.json
clio setup generic       # prints config snippet (no file write)
```

- Reads the existing config file, merges Clio in, writes it back
- Preserves all your other MCP servers
- Safe to run multiple times — detects if Clio is already configured
- Preview with `--dry-run` before writing
- Use `--json` to get a raw config snippet instead of auto-installing

---

### Connection: Claude Code

Add to `~/.claude.json` (not `~/.claude/settings.json`):

```json
{
  "mcpServers": {
    "clio": {
      "type": "stdio",
      "command": "/path/to/clio-mcp",
      "env": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      }
    }
  }
}
```

Merge into the existing `mcpServers` object if you already have other servers configured. Restart Claude Code after adding.

Or run `clio setup claude-code` for the exact paths on your machine.

---

### Connection: OpenAI Codex CLI

Add to `~/.codex/config.toml` (global) or `.codex/config.toml` (project-scoped):

```toml
[mcp_servers.clio]
command = "/path/to/clio-mcp"

[mcp_servers.clio.env]
CLIO_DB_PATH = "/path/to/memory.db"
```

Or use the CLI:

```sh
codex mcp add clio -- /path/to/clio-mcp
```

Run `clio setup generic --json` to get your exact paths, then substitute them into the config above.

Reference: [Codex MCP documentation](https://developers.openai.com/codex/mcp/)

---

### Connection: OpenCode

Add to `~/.config/opencode/opencode.json` (global) or `opencode.json` (project-scoped):

```json
{
  "mcp": {
    "clio": {
      "type": "local",
      "command": ["/path/to/clio-mcp"],
      "environment": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      }
    }
  }
}
```

**Gotchas:** OpenCode uses `"type": "local"` (not `"stdio"`), `"command"` is an **array** (not a string), and environment variables go under `"environment"` (not `"env"`).

Or use the CLI:

```sh
opencode mcp add
```

and follow the interactive prompts.

Reference: [OpenCode MCP servers](https://opencode.ai/docs/mcp-servers/)

---

### Connection: Kilo Code

Edit the global `mcp_settings.json` (accessible from Kilo Code Settings) or create `.kilocode/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "clio": {
      "command": "/path/to/clio-mcp",
      "args": [],
      "env": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      },
      "alwaysAllow": [],
      "disabled": false
    }
  }
}
```

To auto-approve all Clio tools without prompting, add the tool names to `alwaysAllow`:

```json
"alwaysAllow": [
  "memory_remember", "memory_recall", "memory_get",
  "memory_recent", "memory_search", "memory_capture",
  "memory_link", "memory_get_links", "memory_suggest_links",
  "memory_archive", "memory_unarchive", "memory_namespaces",
  "memory_stats", "memory_activity"
]
```

Project-level config in `.kilocode/mcp.json` takes precedence over global settings and can be committed to version control.

Reference: [Kilo Code MCP documentation](https://kilo.ai/docs/automate/mcp/using-in-kilo-code)

---

### Connection: Cursor

Add to `~/.cursor/mcp.json`:

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

Or run `clio setup cursor` for exact paths.

---

### Connection: Windsurf

Add to `~/.windsurf/mcp.json`:

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

Or run `clio setup windsurf` for exact paths.

---

### Connection: Kimi Code CLI

Add to `~/.kimi/mcp.json`:

```json
{
  "mcpServers": {
    "clio": {
      "command": "/path/to/clio-mcp",
      "args": [],
      "env": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      }
    }
  }
}
```

Or use the CLI:

```sh
kimi mcp add --transport stdio clio -- /path/to/clio-mcp
```

You can also pass a config file on launch:

```sh
kimi --mcp-config-file /path/to/mcp.json
```

Other useful commands:

```sh
kimi mcp list              # list configured servers
kimi mcp remove clio       # remove a server
```

Run `clio setup generic --json` to get your exact paths, then substitute them into the config above.

Reference: [Kimi Code CLI MCP documentation](https://moonshotai.github.io/kimi-cli/en/customization/mcp.html)

---

### Connection: GitHub Copilot CLI

Add to `~/.copilot/mcp-config.json`:

```json
{
  "mcpServers": {
    "clio": {
      "type": "stdio",
      "command": "/path/to/clio-mcp",
      "args": [],
      "env": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      }
    }
  }
}
```

Or use the interactive command inside Copilot CLI:

```text
/mcp add
```

Then select **STDIO** as the transport type and enter the command path.

Other useful commands:

```sql
/mcp show          # list all configured servers
/mcp show clio     # view server details and available tools
/mcp edit clio     # edit configuration
/mcp delete clio   # remove server
```

**Note:** The `type` field accepts both `"local"` and `"stdio"`. Use `"stdio"` for compatibility with VS Code and other MCP clients. Only `PATH` is inherited automatically — all other environment variables must be set explicitly in `env`.

You can also pass a temporary config for a single session:

```sh
copilot --additional-mcp-config /path/to/extra-mcp.json
```

Reference: [Copilot CLI MCP documentation](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-mcp-servers)

---

### Connection: Gemini CLI

Add to `~/.gemini/settings.json` (global) or `.gemini/settings.json` (project-scoped):

```json
{
  "mcpServers": {
    "clio": {
      "command": "/path/to/clio-mcp",
      "args": [],
      "env": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      }
    }
  }
}
```

Or use the CLI:

```sh
gemini mcp add clio /path/to/clio-mcp -e CLIO_DB_PATH=/path/to/memory.db
```

**Optional fields:**

| Field | Default | Description |
|---|---|---|
| `cwd` | — | Working directory for the server process |
| `timeout` | 600000 | Request timeout in milliseconds |
| `trust` | false | Bypass tool confirmation prompts |

To auto-approve all Clio tool calls without prompting:

```json
{
  "mcpServers": {
    "clio": {
      "command": "/path/to/clio-mcp",
      "env": {
        "CLIO_DB_PATH": "/path/to/memory.db"
      },
      "trust": true
    }
  }
}
```

**Environment variable expansion:** Gemini CLI supports `$VAR_NAME` and `${VAR_NAME}` syntax in env values, so you can reference existing environment variables:

```json
"env": {
  "CLIO_DB_PATH": "$HOME/Library/Application Support/clio/memory.db"
}
```

**Security note:** Gemini CLI redacts sensitive environment variables by default. Any variable your MCP server needs must be explicitly listed in `env`.

Reference: [Gemini CLI MCP documentation](https://google-gemini.github.io/gemini-cli/docs/tools/mcp-server.html)

---

### Connection: Any MCP-compatible client

Run `clio setup generic` to get a ready-to-paste JSON block with your exact binary and database paths. Adapt the format to match your client's configuration schema.

---

## MCP Tools Reference

Once connected, AI agents have access to 21 tools. Every tool returns either JSON or Markdown depending on the `response_format` parameter (default: `markdown` for human-readable output in chat, `json` for structured data).

### Write tools

| Tool | Purpose | Key parameters |
|---|---|---|
| `memory_remember` | Store or upsert a memory | `content` (required), `kind`, `title`, `summary`, `tags`, `namespace`, `cwd`, `importance`, `upsert`, `source`, `source_ref` |
| `memory_capture` | Send unstructured text to an LLM for automatic classification, then store | `text` (required), `namespace`, `cwd` |
| `memory_link` | Create a typed directional link between two memories | `from_memory_id`, `to_memory_id`, `relationship` |
| `memory_archive` | Soft-archive a memory (reversible) | `memory_id` |
| `memory_unarchive` | Restore an archived memory | `memory_id` |
| `memory_delete` | Permanently delete a memory | `memory_id` |
| `memory_inbox_approve` | Approve a captured memory from the inbox | `memory_id` |
| `memory_inbox_reject` | Reject a captured memory from the inbox | `memory_id` |
| `memory_inbox_edit` | Edit a captured memory in the inbox before approval | `memory_id`, `content`, `kind`, `title`, `tags` |

### Read tools

| Tool | Purpose | Key parameters |
|---|---|---|
| `memory_recall` | Full-text search (keyword matching via FTS5) | `query`, `namespace`, `cwd`, `kind`, `tags`, `match_all_tags`, `include_archived`, `limit`, `offset` |
| `memory_search` | Semantic search (cosine similarity over embeddings) | `query` (required), `namespace`, `cwd`, `include_archived`, `limit` |
| `memory_get` | Fetch one memory by ID | `memory_id` |
| `memory_recent` | List most recently updated memories | `namespace`, `limit` |
| `memory_get_links` | Get all outgoing links from a memory | `memory_id` |
| `memory_suggest_links` | Find semantically similar but unlinked memories | `memory_id`, `threshold`, `limit` |
| `memory_namespaces` | List all namespaces in the database | — |
| `memory_stats` | Aggregate statistics (counts, breakdowns, coverage) | `namespace` |
| `memory_activity` | Recent activity feed (creates, updates, archives) | `namespace`, `limit` |
| `memory_context` | Get namespace context for a working directory | `cwd` |
| `memory_inbox_list` | List memories awaiting review in the inbox | `namespace`, `limit` |

### Namespace auto-detection

Four tools accept a `cwd` (working directory) parameter: `memory_remember`, `memory_recall`, `memory_capture`, `memory_search`. When `cwd` is provided and `namespace` is omitted, the server walks up the directory tree looking for:

1. `.clio-namespace` file (highest priority — contains the full namespace string)
2. `.git` directory (derives `project:<repo-name>`)
3. `Cargo.toml` or `package.json` (derives `project:<dir-name>`)
4. Falls back to `global`

Most AI agents pass `cwd` automatically, so memories are scoped to the right project without any explicit configuration.

---

## Agent Workflows

These are practical patterns for how AI agents should use Clio during a session.

### Session start — load context

At the beginning of a session, an agent should orient itself by loading existing project context:

```bash
1. memory_namespaces        → discover what namespaces exist
2. memory_recent            → see what was worked on recently
3. memory_recall            → search for context relevant to the current task
4. memory_search            → find conceptually related memories
```

This gives the agent awareness of prior decisions, known facts, and existing preferences without the user needing to re-explain everything.

### During work — store decisions and discoveries

As an agent works, it should store important context that would be useful in future sessions:

```bash
memory_remember with:
  kind: "decision"   → architectural choices, trade-offs made
  kind: "fact"       → discovered constraints, API behaviours, version requirements
  kind: "snippet"    → useful code patterns, configuration fragments
  kind: "preference" → user preferences for style, tooling, workflow
  kind: "process"    → defined workflows, deployment steps, debugging procedures
```

**Good things to store:**
- Architectural decisions and the rationale behind them
- Bug root causes and their fixes
- User preferences ("always use British English", "prefer Pinia over Vuex")
- Environment-specific configuration
- API quirks and undocumented behaviours
- Build and deployment procedures

**Bad things to store:**
- Temporary debugging state
- In-progress work that will change
- Speculative conclusions from reading a single file
- Information that duplicates what's in the codebase itself

### After a batch of work — enrich the knowledge graph

```sql
1. memory_suggest_links     → find connections between recent and existing memories
2. memory_link              → create the links that make sense
```

This builds a web of connected knowledge that makes future recall richer.

### Idempotent writes

For memories that get updated over time (e.g., a project's deployment process), use `upsert: true` with a stable `source` + `source_ref` pair:

```json
{
  "content": "Deploy with: cargo build --release && scp ...",
  "kind": "process",
  "source": "codex",
  "source_ref": "deploy-process-v1",
  "upsert": true
}
```

The same `source_ref` will update the existing memory rather than creating a duplicate.

### Capture for unstructured input

When a user dumps unstructured text, notes, or stream-of-consciousness into a session, `memory_capture` routes it through an LLM classifier that assigns kind, title, summary, tags, namespace, importance, and confidence automatically:

```json
{
  "text": "We decided to move auth to a separate service because the monolith builds were taking 12 minutes and auth changes were blocking frontend deploys",
  "cwd": "/Users/alice/code/my-project"
}
```

The capture pipeline requires an OpenAI-compatible API key configured via `clio settings use-capture`.

---

## Prompting Patterns

These are patterns you can put in your CLAUDE.md, Codex instructions, or agent system prompts to make AI agents use Clio effectively.

### Basic: recall before answering

```bash
Before answering questions about this project, search Clio for relevant context:
- Use memory_recall for keyword searches
- Use memory_search for conceptual/semantic searches
- Check memory_recent to see what was worked on recently
```

### Intermediate: store decisions automatically

```bash
When you make an architectural decision or discover an important fact:
1. Store it with memory_remember using kind "decision" or "fact"
2. Include relevant tags for discoverability
3. Set importance 4-5 for critical decisions, 1-2 for minor notes
4. Pass cwd so the namespace is auto-detected
```

### Advanced: full lifecycle

```bash
Session start:
- Call memory_recent to see recent context
- Call memory_recall with keywords related to the current task

During work:
- Store decisions with memory_remember (kind: decision, importance: 4+)
- Store discovered facts with memory_remember (kind: fact)
- Store user preferences with memory_remember (kind: preference, importance: 5)
- Use memory_capture for unstructured notes

After completing work:
- Call memory_suggest_links on newly created memories
- Create links with memory_link where suggestions are relevant
- Call memory_stats to verify the knowledge base is growing

Always:
- Pass cwd for namespace auto-detection
- Use upsert with source_ref for evolving information
- Use tags for cross-cutting concerns (e.g., "performance", "security")
```

### Example CLAUDE.md snippet

```markdown
## Memory

This project uses Clio for persistent memory across sessions.

- Before starting work, check `memory_recent` and `memory_recall` for prior context
- Store architectural decisions as kind: decision with importance 4+
- Store user preferences as kind: preference with importance 5
- Always pass `cwd` to auto-detect the project namespace
- After completing a feature, run `memory_suggest_links` on new memories
```

### Example Codex instructions

```bash
Use the Clio MCP server for persistent memory:
- memory_recall: search by keywords before answering project questions
- memory_search: find conceptually related context when keywords aren't enough
- memory_remember: store important decisions, facts, and preferences
- Always include cwd in calls so namespace detection works
```

---

## Tool Parameters — Full Reference

### memory_remember

```json
{
  "content": "Required. The memory content.",
  "namespace": "Optional. Overrides auto-detection.",
  "cwd": "Optional. Working directory for namespace auto-detection.",
  "kind": "note | fact | decision | snippet | preference | process. Default: note",
  "title": "Optional. Short descriptive label.",
  "summary": "Optional. Concise preview.",
  "tags": ["optional", "array", "of", "strings"],
  "source": "Optional. Identifier for the writing system (e.g., 'claude', 'codex').",
  "source_ref": "Optional. External idempotency key. Required for upsert.",
  "confidence": "Optional. 0.0-1.0.",
  "importance": "1-5. Default: 3.",
  "metadata": {},
  "upsert": "false. When true, updates if source+source_ref match."
}
```

### memory_recall

```json
{
  "query": "Optional. FTS5 keyword search. Omit for recent memories.",
  "namespace": "Optional. Filter to one namespace.",
  "cwd": "Optional. Auto-detect namespace. Uses scoped recall: project first, then global.",
  "kind": "Optional. Filter by kind.",
  "tags": ["optional", "filter", "by", "tags"],
  "match_all_tags": "true. Set false to match ANY tag.",
  "include_archived": "false.",
  "limit": "10.",
  "offset": "0.",
  "response_format": "markdown | json. Default: markdown."
}
```

### memory_search

```json
{
  "query": "Required. Natural language query — embedded and compared by cosine similarity.",
  "namespace": "Optional.",
  "cwd": "Optional.",
  "include_archived": "false.",
  "limit": "10.",
  "response_format": "markdown | json."
}
```

### memory_capture

```json
{
  "text": "Required. Unstructured text to classify.",
  "namespace": "Optional. Override auto-detection and LLM suggestion.",
  "cwd": "Optional."
}
```

### memory_link

```json
{
  "from_memory_id": "Required.",
  "to_memory_id": "Required.",
  "relationship": "relates_to | supports | contradicts | supersedes | derived_from | custom. Default: relates_to.",
  "metadata": {}
}
```

### memory_suggest_links

```json
{
  "memory_id": "Required.",
  "threshold": "0.7. Cosine similarity threshold.",
  "limit": "5.",
  "response_format": "markdown | json."
}
```

### memory_get / memory_archive / memory_unarchive / memory_get_links

```json
{
  "memory_id": "Required.",
  "response_format": "markdown | json. (get only)"
}
```

### memory_recent

```json
{
  "namespace": "Optional.",
  "limit": "10.",
  "response_format": "markdown | json."
}
```

### memory_stats

```json
{
  "namespace": "Optional. Omit for all namespaces.",
  "response_format": "markdown | json."
}
```

Returns: total/active/archived counts, breakdown by namespace and kind, weekly timeline, top tags, embedding coverage percentage, total links, and link density.

### memory_activity

```json
{
  "namespace": "Optional.",
  "limit": "20.",
  "response_format": "markdown | json."
}
```

Returns: feed of recent creates, updates, and archives with memory ID, title, namespace, kind, action, and timestamp.

### memory_namespaces

No parameters. Returns a sorted array of all namespace strings in the database.

### memory_delete

```json
{
  "memory_id": "Required."
}
```

### memory_context

```json
{
  "cwd": "Required. Working directory to detect namespace for."
}
```

### memory_inbox_list

```json
{
  "namespace": "Optional.",
  "limit": "20.",
  "response_format": "markdown | json."
}
```

### memory_inbox_approve / memory_inbox_reject

```json
{
  "memory_id": "Required."
}
```

### memory_inbox_edit

```json
{
  "memory_id": "Required.",
  "content": "Optional.",
  "kind": "Optional.",
  "title": "Optional.",
  "tags": ["optional", "array"]
}
```

---

## Recall vs Search — When to Use Which

| Scenario | Tool | Why |
|---|---|---|
| You know specific words that appear in the memory | `memory_recall` | FTS5 keyword matching is fast and precise |
| You want conceptually related content | `memory_search` | Semantic embeddings find meaning, not just words |
| "What did we decide about caching?" | `memory_recall` with query "caching" | The word "caching" likely appears in the content |
| "What performance decisions have we made?" | `memory_search` with query "performance optimisation decisions" | Broader concept — may not match exact keywords |
| You want the most recent activity | `memory_recent` | Sorted by update time, no query needed |
| You know the exact memory ID | `memory_get` | Direct lookup |
| You want everything in a namespace | `memory_recall` with no query, just namespace | Returns recent memories filtered by namespace |

### Scoped recall behaviour

When `cwd` is provided and `namespace` is omitted, `memory_recall` uses a two-pass strategy:

1. Search within the auto-detected project namespace first
2. Fill remaining result slots from `global`

Project-scoped results appear before global ones. This means an agent working in a project directory automatically gets project-relevant context first, with broader knowledge as a fallback.

---

## Shared Database — Cross-Agent Memory

All agents share the same SQLite database. A memory stored by Claude Code is immediately available to Codex, OpenCode, Kilo, and the CLI. This is the core value of Clio: **one memory, every agent**.

Use the `source` field to track which agent wrote a memory:

```json
{ "source": "claude-code", ... }
{ "source": "codex", ... }
{ "source": "kilo", ... }
{ "source": "desktop", ... }
```

Use `source_ref` for idempotency across agents:

```json
{ "source": "codex", "source_ref": "auth-decision-001", "upsert": true }
```

Any agent can later update this memory by providing the same `source` + `source_ref` with `upsert: true`.

---

## MCP Resources

In addition to tools, the MCP server exposes resources for inspection-oriented clients:

| Resource URI | Returns |
|---|---|
| `memory://schema` | Summary of database tables, migrations, and indexes |
| `memory://item/{id}` | A single memory rendered for human reading |
| `memory://recent/{namespace}` | Recent memories for a namespace — useful for quick project context loading |

Resources are read-only and return Markdown content suitable for embedding in agent context.

---

## Related Documentation

- [Getting Started](getting-started.md) — installation and initial setup
- [CLI Reference](cli-reference.md) — all CLI commands and flags
- [MCP Contract](reference/mcp-contract.md) — full tool and resource definitions
- [Schema Reference](reference/schema.md) — database structure
- [Documentation Index](README.md) — all available documentation
