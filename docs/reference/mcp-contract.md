# Clio MCP Contract

## Purpose

This document defines the MCP surface for Clio. It is written so another coding agent can implement the MCP server without guessing tool names, payloads, defaults, or expected behaviour.

The MCP server is an adapter over the Rust core. It is not a second storage implementation.

## Scope

This document covers:

- server identity
- transport expectations
- tools
- resources
- defaults
- response shapes
- error expectations

This document does not cover:

- SQLite schema details
- CLI syntax
- Tauri UX

## Design Principles

- thin adapter over core logic
- tool names stable from phase one onward
- concise results by default
- structured data for automation
- useful Markdown for human-oriented consumption
- actionable errors for AI agents

## Server Overview

Recommended server name:

```text
clio_mcp
```

Phase-one transport:

- stdio

Optional later:

- streamable HTTP

Default assumption:

- local process launched by an AI client
- same user account as the SQLite file owner
- database path resolved by env var or platform default in the core

## Tool Design Rules

Every tool must:

- call the Rust core rather than raw SQL from the MCP crate
- validate input before calling the core
- return stable field names
- avoid overlong responses by respecting `limit`
- use the same semantics as the CLI

Every read tool should:

- prefer active records by default
- support JSON-shaped output where practical

## Namespace Auto-Detection

Several tools accept an optional `cwd` parameter (working directory path). When `cwd` is provided and `namespace` is omitted, the server attempts to detect the project namespace from the directory tree:

1. Walk up from `cwd` looking for project markers
2. `.clio-namespace` file — highest priority; contains the full namespace string (e.g. `project:clio`)
3. `.git` directory — derives `project:<repo-name>` from the directory name
4. `Cargo.toml` or `package.json` — derives `project:<dir-name>` from the directory name
5. If no marker is found, falls back to `global`

Auto-detection is controlled by `context.auto_detect` in settings (default `true`). When `false`, `cwd` is ignored.

**Namespace resolution precedence** (highest to lowest):
1. Explicit `namespace` field in the tool input
2. Auto-detected namespace from `cwd`
3. `global`

Tools that accept `cwd`: `memory_remember`, `memory_recall`, `memory_capture`, `memory_search`, `memory_context`.

## Shared Types

### Memory record

Canonical response fields for a stored memory:

```json
{
  "id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "namespace": "project:ai",
  "kind": "decision",
  "title": "Use SQLite",
  "summary": "SQLite is the default shared local store.",
  "content": "Shared memory should default to SQLite with WAL mode.",
  "tags": ["sqlite", "memory", "architecture"],
  "source": "codex",
  "source_ref": "design-001",
  "confidence": 0.93,
  "importance": 4,
  "metadata": {
    "origin": "planning-session"
  },
  "valid_from": null,
  "valid_until": null,
  "archived_at": null,
  "created_at": "2026-03-02T19:15:00Z",
  "updated_at": "2026-03-02T19:15:00Z"
}
```

### Recall envelope

Canonical structured response for search/list operations:

```json
{
  "total": 1,
  "count": 1,
  "offset": 0,
  "limit": 10,
  "items": []
}
```

### Response formats

Allowed values:

- `json`
- `markdown`

Semantics:

- `json` for machine consumption
- `markdown` for readable summaries inside AI chat surfaces

## Tool Definitions

## `memory_remember`

Store or upsert a memory record.

### Why it exists

This is the primary write path for AI clients. It should support both one-off writes and idempotent updates when `source + source_ref` is provided.

### Input

```json
{
  "namespace": "project:ai",
  "cwd": "/Users/alice/code/my-project",
  "kind": "decision",
  "title": "Use SQLite",
  "summary": "SQLite is the default shared local store.",
  "content": "Shared memory should default to SQLite with WAL mode.",
  "tags": ["sqlite", "memory", "architecture"],
  "source": "codex",
  "source_ref": "design-001",
  "confidence": 0.93,
  "importance": 4,
  "metadata": {
    "origin": "planning-session"
  },
  "valid_from": null,
  "valid_until": null,
  "upsert": true
}
```

### Input defaults

- `namespace`: auto-detected from `cwd` if provided; otherwise `global`
- `cwd`: null (no auto-detection)
- `kind`: `note`
- `tags`: `[]`
- `importance`: `3`
- `metadata`: `{}`
- `upsert`: `false`

### Validation rules

- `content` is required
- `namespace` must not be empty
- `kind` must not be empty
- `tags` must be deduplicated and lowercased
- `metadata` must be an object
- `importance` must be 1 through 5
- `confidence` must be null or 0.0 through 1.0

### Behaviour

- normal insert when `upsert` is false
- idempotent update when `upsert` is true and both `source` and `source_ref` match an existing record
- preserve original `id` on upsert

### Response

Returns the stored memory record in structured form.

### Failure cases

- validation error
- storage failure
- malformed metadata

## `memory_recall`

Search or filter memories.

### Why it exists

This is the primary retrieval tool for AI clients. It must handle both semantic-ish keyword recall through FTS and plain recent/filter queries.

### Input

```json
{
  "query": "sqlite",
  "namespace": "project:ai",
  "cwd": "/Users/alice/code/my-project",
  "kind": "decision",
  "tags": ["architecture"],
  "match_all_tags": true,
  "importance_min": 3,
  "importance_max": 5,
  "sort_by": "importance_desc",
  "include_archived": false,
  "limit": 10,
  "offset": 0,
  "response_format": "json"
}
```

### Input defaults

- `query`: null
- `namespace`: null (auto-detected from `cwd` when provided)
- `cwd`: null (no auto-detection)
- `tags`: `[]`
- `match_all_tags`: `true`
- `importance_min`: null (no lower bound)
- `importance_max`: null (no upper bound)
- `sort_by`: null (default: `updated_at DESC`; only applied when no FTS query and no scoring config)
- `include_archived`: `false`
- `limit`: `10`
- `offset`: `0`
- `response_format`: `markdown`

### Sort orders

| Value | Description |
|---|---|
| `updated_desc` | Most recently updated first (default) |
| `updated_asc` | Oldest updated first |
| `importance_desc` | Most important first |
| `importance_asc` | Least important first |
| `created_desc` | Newest created first |
| `created_asc` | Oldest created first |

### Behaviour

- if `query` is present, use FTS recall
- if `query` is absent, return recent records subject to filters
- by default, archived records are excluded
- support namespace, kind, and tags filters
- support pagination
- when `namespace` is explicitly provided, filter to that namespace only
- when `cwd` is provided and `namespace` is omitted, use scoped recall: search detected namespace first, then fill remaining slots from `global`; project-scoped results appear before global results

### Structured response

```json
{
  "total": 1,
  "count": 1,
  "offset": 0,
  "limit": 10,
  "items": [
    {
      "id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
      "namespace": "project:ai",
      "kind": "decision",
      "title": "Use SQLite",
      "summary": "SQLite is the default shared local store.",
      "content": "Shared memory should default to SQLite with WAL mode.",
      "tags": ["sqlite", "memory", "architecture"],
      "source": "codex",
      "source_ref": "design-001",
      "confidence": 0.93,
      "importance": 4,
      "metadata": {},
      "valid_from": null,
      "valid_until": null,
      "archived_at": null,
      "created_at": "2026-03-02T19:15:00Z",
      "updated_at": "2026-03-02T19:15:00Z",
      "rank": -7.813
    }
  ]
}
```

### Markdown response

Should include for each item:

- title or id
- id
- namespace
- kind
- tags
- updated timestamp
- relevance rank when available
- summary or short excerpt

### Failure cases

- invalid pagination values
- invalid response format
- storage failure

## `memory_get`

Fetch one memory by id.

### Input

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "response_format": "markdown"
}
```

### Defaults

- `response_format`: `markdown`

### Behaviour

- fetch exactly one record
- return not-found error when absent

### Responses

- `json`: full memory record
- `markdown`: detail view including metadata and content

## `memory_recent`

Return recent memories with optional filtering and sorting.

### Input

```json
{
  "namespace": "project:ai",
  "kind": "decision",
  "tags": ["architecture"],
  "match_all_tags": true,
  "importance_min": 3,
  "importance_max": 5,
  "sort_by": "importance_desc",
  "include_archived": false,
  "limit": 10,
  "response_format": "json"
}
```

### Behaviour

- recall without a full-text query
- sorted by `updated_at DESC` by default, or by `sort_by` when specified
- supports all filter parameters (kind, tags, importance range)
- active-only by default

### Defaults

- `namespace`: null
- `kind`: null (all kinds)
- `tags`: `[]`
- `match_all_tags`: `true`
- `importance_min`: null (no lower bound)
- `importance_max`: null (no upper bound)
- `sort_by`: null (default: `updated_at DESC`)
- `include_archived`: `false`
- `limit`: `10`
- `response_format`: `markdown`

## `memory_link`

Create a typed link between two memories.

### Input

```json
{
  "from_memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "to_memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de124",
  "relationship": "supports",
  "metadata": {
    "reason": "follow-up detail"
  }
}
```

### Defaults

- `relationship`: `relates_to`
- `metadata`: `{}`

### Behaviour

- verify both memories exist
- create or replace the typed edge
- preserve directional semantics

### Response

```json
{
  "from_memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "to_memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de124",
  "relationship": "supports",
  "metadata": {
    "reason": "follow-up detail"
  },
  "created_at": "2026-03-02T19:20:00Z"
}
```

## `memory_archive`

Soft-archive a memory.

### Input

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123"
}
```

### Behaviour

- set `archived_at`
- return the updated record
- should be idempotent from the caller perspective

## `memory_unarchive`

Restore an archived memory.

### Why it exists

Provides a reversible path out of the archive state without requiring a full write via `memory_remember`. Pairs with `memory_archive` to support soft-delete/restore workflows.

### Input

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123"
}
```

### Behaviour

- clear `archived_at` (set to `NULL`)
- refresh `updated_at`
- return the updated memory record
- idempotent — calling on an already-active memory is a no-op (returns the record unchanged)
- returns not-found error when the memory does not exist

### Response

Returns the full memory record in JSON.

### Failure cases

- memory not found
- storage failure

## `memory_delete`

Permanently delete a memory by ID.

### Input

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123"
}
```

### Behaviour

- permanently removes the memory row
- cascades to tags, links, and embeddings via `ON DELETE CASCADE`
- returns the deleted memory record before removal
- returns not-found error when the memory does not exist

### Response

Returns the full memory record in JSON (as it was before deletion).

### Failure cases

- memory not found
- storage failure

## `memory_move`

Move a memory to a different namespace.

### Input

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "namespace": "project:new-project"
}
```

### Validation rules

- `memory_id` must not be empty
- `namespace` must not be empty, at most 120 characters

### Behaviour

- updates the `namespace` field on the specified memory
- refreshes `updated_at`
- returns the updated memory record
- returns not-found error when the memory does not exist

### Response

Returns the full memory record in JSON.

### Failure cases

- memory not found
- validation error (empty namespace)
- storage failure

## `memory_namespaces`

List all distinct namespaces in the database.

### Why it exists

Allows clients to discover which namespaces are in use before issuing scoped queries. Useful for namespace pickers, dashboards, and agents orienting themselves in a shared database.

### Input

No input parameters.

### Behaviour

- query `SELECT DISTINCT namespace FROM memories ORDER BY namespace`
- includes namespaces from archived memories (all rows)
- returns an empty array when the database has no memories

### Response

```json
["global", "project:ai", "project:scooda", "tool:codex"]
```

A JSON array of namespace strings, sorted lexicographically.

### Failure cases

- storage failure

## `memory_get_links`

Get all typed links originating from a memory.

### Why it exists

`memory_link` creates directional edges. `memory_get_links` retrieves them so a client can traverse the knowledge graph from a known starting node.

### Input

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123"
}
```

### Behaviour

- return all `memory_links` rows where `from_memory_id` matches
- sorted by `created_at` ascending
- returns an empty array when the memory has no outgoing links
- returns not-found error when the memory does not exist

### Response

```json
[
  {
    "from_memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
    "to_memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de124",
    "relationship": "supports",
    "metadata": {
      "reason": "follow-up detail"
    },
    "created_at": "2026-03-02T19:20:00Z"
  }
]
```

### Failure cases

- memory not found
- storage failure

## `memory_capture`

Accept unstructured text, classify it via an LLM, and store it as a structured memory.

### Why it exists

Bridges the gap between raw thought capture and structured memory. A caller can send unstructured text (a note, a Slack message, a voice transcript) and receive a properly classified, tagged, and embedded memory without manually deciding kind, tags, or namespace. This is the "type a thought and the system handles the rest" workflow.

### Input

```json
{
  "text": "We decided to move the auth service to a separate repo to reduce build times.",
  "namespace": "project:ai",
  "cwd": "/Users/alice/code/my-project"
}
```

### Input defaults

- `namespace`: null (resolved from `cwd` if provided; otherwise LLM suggests one)
- `cwd`: null (no auto-detection)

### Validation rules

- `text` is required and must not be empty
- `namespace`, when provided, must follow standard namespace conventions

### Behaviour

1. Check that `capture.enabled` is `true` in settings — return configuration error if not
2. Call the configured OpenAI-compatible chat completions API with a structured classification prompt
3. Parse the LLM response into classification fields (kind, title, summary, tags, namespace, importance, confidence)
4. Apply normalisation: unknown `kind` values fall back to `note`; tags are lowercased and spaces replaced with hyphens; `importance` is clamped 1–5; `confidence` is clamped 0.0–1.0; missing fields use safe defaults
5. Namespace resolution order: explicit `namespace` field → auto-detected from `cwd` → LLM suggestion
6. Store as a memory with `source = "capture"`
7. Auto-embed the stored memory if `auto_embed` is enabled in settings
8. Return the stored memory record

### Response

Returns the full memory record in JSON, identical in shape to `memory_remember`. The `source` field will be `"capture"`.

### Failure cases

- capture not enabled → `Configuration error: capture pipeline is not enabled`
- API key not configured → `Configuration error: capture API key required: …`
- LLM API request failed → storage error with HTTP status
- LLM returned unparseable JSON → validation error
- storage failure

### Settings reference

Capture is controlled by the `capture` section of `clio-settings.json`:

```json
{
  "capture": {
    "enabled": true,
    "api_key": "sk-...",
    "base_url": "https://api.openai.com/v1",
    "model": "gpt-4o-mini"
  }
}
```

| Field | Default | Notes |
|---|---|---|
| `enabled` | `false` | must be `true` for the tool to work |
| `api_key` | null | falls back to `OPENAI_API_KEY` env var |
| `base_url` | `https://api.openai.com/v1` | override for proxies or compatible APIs |
| `model` | `gpt-4o-mini` | any OpenAI-compatible chat model |

Configure via CLI: `clio settings use-capture --api-key <key> [--model <model>] [--base-url <url>]`

## `memory_stats`

Get aggregate statistics about the memory system.

### Why it exists

Surfaces thinking patterns over time: counts by namespace and kind, weekly creation timeline, tag frequency, link density, and embedding coverage. Supports a regular review workflow without requiring raw SQL access.

### Input

```json
{
  "namespace": "project:ai",
  "response_format": "markdown"
}
```

### Input defaults

- `namespace`: null (statistics across all namespaces)
- `response_format`: `markdown`

### Behaviour

- when `namespace` is provided, all counts are scoped to that namespace only
- `by_week` covers up to 52 ISO weeks, ordered newest first
- `top_tags` returns the 20 most frequent tags by count
- `embedding_coverage` is a percentage (0–100) of memories that have a stored embedding
- `link_density` is the average number of outgoing links per memory (float)
- always reads active and archived memories for counts (not filtered by `archived_at`)

### Structured response (JSON format)

```json
{
  "total_memories": 42,
  "active_memories": 38,
  "archived_memories": 4,
  "total_embeddings": 35,
  "embedding_coverage": 83.3,
  "by_namespace": [
    ["global", 20],
    ["project:ai", 18],
    ["tool:claude", 4]
  ],
  "by_kind": [
    ["note", 22],
    ["decision", 10],
    ["fact", 10]
  ],
  "by_week": [
    ["2026-W09", 8],
    ["2026-W08", 12]
  ],
  "top_tags": [
    ["rust", 15],
    ["sqlite", 12]
  ],
  "total_links": 28,
  "link_density": 0.67
}
```

### Failure cases

- storage failure

## `memory_activity`

Show recent memory activity: creates, updates, and archives.

### Why it exists

Provides a running feed of what has been added, changed, or archived. Useful for reviewing recently captured context, auditing what an AI client stored, or catching unexpected archival.

### Input

```json
{
  "namespace": "project:ai",
  "limit": 20,
  "response_format": "markdown"
}
```

### Input defaults

- `namespace`: null (activity across all namespaces)
- `limit`: `20`
- `response_format`: `markdown`

### Behaviour

- events are classified per memory based on timestamp fields:
  - `created` — `updated_at == created_at` and `archived_at IS NULL`
  - `updated` — `updated_at > created_at` and `archived_at IS NULL`
  - `archived` — `archived_at IS NOT NULL` (uses `archived_at` as event timestamp)
- results are ordered by most recent event timestamp (`COALESCE(archived_at, updated_at) DESC`)
- when `namespace` is provided, only memories in that namespace are returned

### Structured response (JSON format)

```json
[
  {
    "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
    "title": "Use SQLite",
    "namespace": "project:ai",
    "kind": "decision",
    "action": "created",
    "timestamp": "2026-03-02T19:15:00Z"
  }
]
```

### Failure cases

- storage failure

## `memory_suggest_links`

Suggest potential links for a memory based on semantic similarity.

### Why it exists

Helps discover relationships between memories that have not been explicitly linked. Uses embedding cosine similarity to find semantically related memories, excluding memories already linked in either direction. This is the knowledge graph enrichment workflow — run it after storing a batch of memories to surface connections.

### Input

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "threshold": 0.7,
  "limit": 5,
  "response_format": "markdown"
}
```

### Input defaults

- `threshold`: `0.7` (memories with cosine similarity ≥ threshold are returned)
- `limit`: `5`
- `response_format`: `markdown`

### Behaviour

- fetches or generates the embedding for `memory_id` on-the-fly (stores it if missing)
- collects IDs of memories already linked to or from `memory_id` in either direction
- excludes the target memory itself and all already-linked memories
- scans all non-archived memories with stored embeddings and computes cosine similarity
- returns candidates with similarity ≥ `threshold`, sorted by similarity descending
- embeddings must be enabled in settings for this tool to work

### Structured response (JSON format)

```json
[
  {
    "memory": { ... },
    "similarity": 0.847
  }
]
```

Each item contains the full memory record and its cosine similarity score (0.0–1.0).

### Failure cases

- embeddings disabled in settings → configuration error
- memory not found → not-found error
- embedding backend unreachable → configuration error
- storage failure

## `memory_search`

Find memories by semantic meaning using vector embeddings.

### Why it exists

`memory_recall` uses full-text search (keyword matching via FTS5). `memory_search` uses cosine similarity over stored embedding vectors to find conceptually related memories even when query terms do not appear in the content. Requires embeddings to be enabled in `clio-settings.json` and memories to have been embedded.

### Input

```json
{
  "query": "decisions about database storage",
  "namespace": "project:ai",
  "cwd": "/Users/alice/code/my-project",
  "include_archived": false,
  "limit": 10,
  "response_format": "json"
}
```

### Input defaults

- `namespace`: null (auto-detected from `cwd` when provided; otherwise no namespace filter)
- `cwd`: null (no auto-detection)
- `include_archived`: `false`
- `limit`: `10`
- `response_format`: `markdown`

### Validation rules

- `query` is required and must not be empty
- `limit` must be a positive integer
- `response_format` must be `json` or `markdown`

### Behaviour

- embed the query text using the active embedding backend
- compute cosine similarity between the query embedding and all stored embeddings
- when `namespace` is explicitly provided, filter to that namespace
- when `cwd` is provided and `namespace` is omitted, auto-detect namespace from `cwd` and use it as a filter
- exclude archived memories by default
- return results sorted by similarity descending
- similarity score is returned in the `rank` field (range 0.0–1.0; higher is more similar)

### Response

Returns a recall envelope identical in shape to `memory_recall`. The `rank` field contains the cosine similarity score rather than a BM25 rank:

```json
{
  "total": 1,
  "count": 1,
  "offset": 0,
  "limit": 10,
  "items": [
    {
      "id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
      "namespace": "project:ai",
      "kind": "decision",
      "title": "Use SQLite",
      "summary": "SQLite is the default shared local store.",
      "content": "Shared memory should default to SQLite with WAL mode.",
      "tags": ["sqlite", "memory", "architecture"],
      "source": "codex",
      "source_ref": "design-001",
      "confidence": 0.93,
      "importance": 4,
      "metadata": {},
      "valid_from": null,
      "valid_until": null,
      "archived_at": null,
      "created_at": "2026-03-02T19:15:00Z",
      "updated_at": "2026-03-02T19:15:00Z",
      "rank": 0.847
    }
  ]
}
```

### Failure cases

- embeddings disabled in settings → `Configuration error: embeddings are disabled`
- embedding backend not available at compile time → configuration error
- embedding backend unreachable (e.g. OpenAI API key missing) → configuration error
- no memories have been embedded yet → returns empty results without error
- storage failure

## Resource Definitions

Resources exist to support inspection-oriented clients and context loading.

## `memory://schema`

Returns:

- summary of tables
- migration versions
- optionally index names and counts

Format:

- Markdown or plain-text summary is acceptable
- must not require raw SQL knowledge from the client

## `memory://item/{id}`

Returns:

- a single memory rendered for humans

Recommended content:

- title
- id
- namespace
- kind
- tags
- metadata
- created/updated timestamps
- full content

## `memory://recent/{namespace}`

Returns:

- recent memories for a namespace in a readable format
- hardcoded limit of 10 items

Use case:

- give an agent quick context for a project or tool scope

## Error Contract

Errors must be concise, specific, and actionable. Avoid generic exception dumps.

### Preferred error categories

- validation
- not found
- conflict
- storage
- configuration

### Example messages

Validation:

```text
Validation error: content is required.
```

Not found:

```text
Memory not found: 01954d70-cf20-7d42-bb3b-ff2f0f0de123.
```

Invalid metadata:

```text
Validation error: metadata must be a JSON object.
```

Invalid link:

```text
Cannot create link: target memory does not exist.
```

Storage/config:

```text
Storage error: database could not be opened at /path/to/memory.db.
```

## `memory_context`

Build a scoped context brief for agent consumption.

### Why it exists

Agents often need to load relevant context before starting work. Instead of issuing multiple recall, recent, and filter queries, `memory_context` combines them into a single call that returns a structured brief organised by sections.

### Input

```json
{
  "namespace": "project:ai",
  "cwd": "/Users/alice/code/my-project",
  "preset": "project-brief",
  "query": null,
  "max_items": 20,
  "include_links": false,
  "response_format": "markdown"
}
```

### Input defaults

- `namespace`: null (auto-detected from `cwd` when provided)
- `cwd`: null (no auto-detection)
- `preset`: `project-brief`
- `query`: null (used only with `custom` preset)
- `max_items`: `20`
- `include_links`: `false`
- `response_format`: `markdown`

### Available presets

| Preset | Sections |
|---|---|
| `project-brief` | Recent Decisions, Active Constraints, Recent Activity |
| `person-brief` | Key Facts, Recent Notes |
| `decision-history` | Decisions (ordered by created_at) |
| `active-constraints` | Constraints (non-archived) |
| `recent-activity` | Recent memories |
| `custom` | Search Results (uses `query` for FTS) |

### Structured response (JSON format)

```json
{
  "namespace": "project:ai",
  "preset": "project-brief",
  "sections": [
    {
      "heading": "Recent Decisions",
      "items": [{ ... }]
    }
  ],
  "total_memories_used": 12,
  "generated_at": "2026-03-03T08:30:00Z"
}
```

### Failure cases

- invalid preset name → validation error
- storage failure

## `memory_inbox_list`

List pending review queue items.

### Input

```json
{
  "limit": 20,
  "response_format": "markdown"
}
```

### Defaults

- `limit`: `20`
- `response_format`: `markdown`

### Behaviour

- returns items with `status = 'pending'`, ordered by `created_at ASC`

## `memory_inbox_approve`

Approve a review queue item, converting it to a stored memory.

### Input

```json
{
  "review_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123"
}
```

### Behaviour

- creates a memory from the suggested fields via `repository::remember()`
- sets review item status to `approved` and `reviewed_at` to now
- returns the created memory record

### Failure cases

- review item not found
- review item already approved/rejected

## `memory_inbox_reject`

Reject a review queue item.

### Input

```json
{
  "review_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123"
}
```

### Behaviour

- sets status to `rejected` and `reviewed_at` to now
- returns the updated review item

## `memory_inbox_edit`

Edit suggested fields on a review queue item before approval.

### Input

```json
{
  "review_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "namespace": "project:ai",
  "kind": "decision",
  "title": "Updated title",
  "tags": ["new-tag"],
  "importance": 4
}
```

### Behaviour

- updates only the fields provided (all optional)
- sets status to `edited`
- item still requires approval via `memory_inbox_approve`

## Tool Annotation Guidance

Recommended annotation intent:

| Tool | readOnlyHint | destructiveHint | idempotentHint |
|---|---:|---:|---:|
| `memory_remember` | false | false | false |
| `memory_recall` | true | false | true |
| `memory_get` | true | false | true |
| `memory_recent` | true | false | true |
| `memory_link` | false | false | true |
| `memory_archive` | false | false | true |
| `memory_unarchive` | false | false | true |
| `memory_delete` | false | true | true |
| `memory_move` | false | false | false |
| `memory_namespaces` | true | false | true |
| `memory_get_links` | true | false | true |
| `memory_capture` | false | false | false |
| `memory_search` | true | false | true |
| `memory_stats` | true | false | true |
| `memory_activity` | true | false | true |
| `memory_suggest_links` | true | false | false |
| `memory_context` | true | false | true |
| `memory_inbox_list` | true | false | true |
| `memory_inbox_approve` | false | false | false |
| `memory_inbox_reject` | false | false | true |
| `memory_inbox_edit` | false | false | false |

## Output Discipline

The MCP server should avoid overwhelming clients.

Rules:

- honour `limit`
- do not dump large datasets by default
- for Markdown responses, prefer concise summaries to full raw content lists
- full content is appropriate for `memory_get` and the item resource

## Compatibility Rules

The contract is stable once implemented.

Allowed compatible changes:

- adding optional input fields
- adding optional response fields
- adding new tools/resources without changing existing semantics

Avoid unless necessary:

- renaming existing fields
- changing defaults
- changing tool names

## Example Client Configuration

Direct binary:

```json
{
  "mcpServers": {
    "clio": {
      "command": "/absolute/path/to/clio-mcp",
      "env": {
        "CLIO_DB_PATH": "/Users/dannyharding/.clio/shared.db"
      }
    }
  }
}
```

During development:

```json
{
  "mcpServers": {
    "clio": {
      "command": "cargo",
      "args": [
        "run",
        "-p",
        "clio-mcp"
      ],
      "cwd": "/Users/dannyharding/Databases/AI",
      "env": {
        "CLIO_DB_PATH": "/Users/dannyharding/.clio/shared.db"
      }
    }
  }
}
```

## Testing Expectations

The MCP implementation should be validated for:

- remember flow
- recall flow (FTS)
- semantic search flow (`memory_search`)
- get flow
- recent flow
- link flow
- archive / unarchive round-trip
- namespace listing
- get links for a memory with and without outgoing links
- not-found behaviour (get, unarchive, get_links)
- capture flow (success, capture-disabled error, missing API key error)
- stats flow (all namespaces, scoped to namespace)
- activity flow (creates, updates, archives classified correctly)
- suggest-links flow (above threshold returned, already-linked excluded)
- validation errors
- resource lookup
- embedding disabled error from `memory_search` and `memory_suggest_links`

## Invariants For Implementers

The MCP implementation is incorrect if:

- it bypasses the Rust core for storage logic
- it returns field names that do not match the documented contract
- its defaults differ from the CLI/core semantics
- archived records appear unexpectedly in default recall
- error messages are opaque enough that an agent cannot recover

## Acceptance Criteria

The MCP contract is ready for implementation when:

- each phase-one workflow maps to a tool or resource
- defaults and required fields are explicit
- structured and human-readable outputs are defined
- another coding agent can implement the server without inventing tool semantics
