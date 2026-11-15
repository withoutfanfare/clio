# Clio MCP Contract

## Purpose

This document defines the MCP surface for Clio. It is written so another coding agent can implement the MCP server without guessing tool names, payloads, defaults, or expected behaviour.

The MCP server is an adapter over the Rust core. It is not a second storage implementation.

For user-facing connection instructions, see [MCP Agent Setup](../mcp-agent-setup.md). For database structure, see [Schema Reference](schema.md).

---

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

## Tool Profiles

The environment variable `CLIO_MCP_TOOLS`, read once at startup, chooses which
tools the process offers:

| Value | Tools |
|---|---|
| unset, empty, `full` | every tool in this document (the default) |
| `core` | `memory_recall`, `memory_search`, `memory_get`, `memory_remember`, `memory_update`, `memory_resume`, `memory_context`, `memory_action`, `memory_feedback`, `memory_archive`, `memory_unarchive` |
| any other value | startup error naming the valid values; never a silent fallback |

Under `core` the other tools are absent from `tools/list` and calling one is an
unknown-tool error. Tool behaviour is identical in both profiles. The server
instructions describe only the tools on offer. Resources are unaffected. The
full list measures about 28,200 characters of definitions and the core list
about 14,800.

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

1. Search every ancestor from `cwd` for `.clio-namespace`; the nearest valid file supplies the full namespace string (e.g. `project:clio`). This explicit marker wins over any nested package manifest.
2. If no `.clio-namespace` exists, use the nearest `.git` marker and derive `project:<repo-name>` from its directory. A linked worktree takes the name of its repository instead: `<name>` for a bare `<name>.git`, or the folder that holds `.git`.
3. Otherwise use the nearest `Cargo.toml` or `package.json` and derive `project:<dir-name>` from its directory.
4. If no marker is found, fall back to `global`.

Auto-detection is controlled by `context.auto_detect` in settings (default `true`). When `false`, `cwd` is ignored.

**Namespace resolution precedence** (highest to lowest):
1. Explicit `namespace` field in the tool input
2. Auto-detected namespace from `cwd`
3. `global`

Tools that accept `cwd`: `memory_remember`, `memory_recall`, `memory_capture`,
`memory_session_checkpoint`, `memory_search`, `memory_context`, `memory_resume`,
and `memory_action`.

A bare project name is the same namespace as its `project:` form in every
tool that takes `namespace` or `cwd` from the list above: `clio` and
`project:clio` both mean `project:clio` (core's `canonical_namespace`).
`global` and any name that already has a `kind:` prefix are used as written.
`memory_stats`, `memory_activity`, `memory_move` and `memory_namespace_delete`
take the namespace exactly as given.

**Exception — classified storage** (`memory_capture`, and distillation via
`memory_session_checkpoint`): the model's classification may promote a memory to
`global`, and that promotion ranks above the auto-detected `cwd` namespace:
explicit `namespace` → model's `global` promotion → auto-detected from `cwd` →
model's suggested namespace. This is one shared rule in core
(`capture::resolve_namespace`) for every classified path, so capture and distill
cannot disagree about where the same classification lands.

## Runtime Lifecycle and Concurrency

One MCP process opens one SQLite connection, holds one bounded namespace-list
cache, caches settings for 30 seconds, and creates its embedding backend once.
Changing embedding settings therefore requires an MCP process restart; other
settings are reloaded after the cache window.

SQLite phases are serialised through one mutex. Slow provider phases run on the
blocking pool without that mutex: semantic query embedding, capture
classification, checkpoint distillation, and automatic embedding after a
remember/update/capture/checkpoint write. A record vector generated after a write is
stored only when the record's `updated_at` is still unchanged, so releasing the
mutex cannot let a stale vector overwrite a newer edit.

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
  "archived_only": false,
  "items": []
}
```

This is the shape of `response_format: "json"`, unchanged since it was first
published: programs decode it into core's `RecallResult`. The `compact` format
uses the same envelope with [compact items](#compact-item), and adds a `note`
string when the reply needs explaining (a widened or empty `memory_recall`, an
empty `memory_search`).

### Compact item

What each entry of `items` holds in a list reply with
`response_format: "compact"`:

```json
{
  "id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "namespace": "project:ai",
  "kind": "decision",
  "title": "Use SQLite",
  "excerpt": "SQLite is the default shared local store.",
  "created": "2026-03-02",
  "score": 7.813
}
```

- `excerpt`: the summary when the memory has one, otherwise the first 200
  characters of its content followed by `...`
- `created`: the creation date, so an old status note is not read as current
- `score`: omitted when the listing is unranked. Higher is always a better
  match: it is the magnitude of the core `rank` (BM25 for `memory_recall`,
  similarity for `memory_search`), rounded to three places. Items are already
  ordered best first
- `title` is omitted when the memory has none

### Response formats

Allowed values on read and list tools:

| Value | Use it for | What it returns |
|---|---|---|
| `markdown` (default) | reading | cards: id, namespace, kind, creation date, tags, title and a short excerpt |
| `compact` | structured processing by a model | the same fields as one-line JSON ([compact items](#compact-item)), about the size of the Markdown reply |
| `json` | a program that needs whole records | the complete memory records, several times larger |

Rules:

- `json` is a compatibility contract. It returns exactly the field set it
  always has, for every tool, because existing programs decode it into core
  types: the desktop app's remote mode reads `memory_recall`, `memory_search`
  and `memory_get` replies as `RecallResult` and `Memory`. Fields may be added
  to core records; none may be removed or renamed, and the default shape of a
  tool's `json` reply never changes. A new, smaller shape gets a new format
  value, as `compact` did
- `compact` has its own shape on the list tools (`memory_recall`,
  `memory_recent`, `memory_search`, `memory_context`). On a tool with only one
  JSON shape (`memory_get`, `memory_resume`, `memory_stats`, `memory_activity`,
  `memory_suggest_links`, `memory_inbox`) it returns that shape
- the write tools `memory_remember` and `memory_update` take `json` (default:
  the whole stored record, which the desktop app decodes as `Memory`) or
  `compact` (a short receipt). They have no Markdown reply
- no JSON reply is pretty-printed; every one is a single line
- for one complete record, use `memory_get`

Measured on a 27,000-memory store, the same ten `memory_recall` results were
3,388 characters as `markdown`, 3,616 as `compact` and 14,004 as `json`
(16,283 while `json` was still pretty-printed).

## Tool Definitions

## `work_report`

Stores a direct work observation and returns its committed receipt as a JSON text result.
Input is `{ "report": WorkReport }`. Every field below is required except `supersedes`;
unknown fields are rejected. Identity is explicit and never inferred from `cwd` or namespace settings.

| Report field | Contract |
|---|---|
| `project`, `task` | Exact task identity; required nonblank strings, each at most 240 characters |
| `task_title` | Display title; required nonblank string, at most 240 characters |
| `source`, `session_id`, `run_id` | Explicit reporter and session/run identities; required nonblank strings, at most 240 characters each |
| `worktree` | Originating worktree; required nonblank string, at most 2,000 characters |
| `revision` | Reported revision; required nonblank string, at most 240 characters |
| `sequence` | Non-negative integer; register a new run at `0`, increment for new observations |
| `observed_at` | Non-negative Unix seconds, no later than server receipt time |
| `state` | `running`, `waiting`, `stopped` or `implemented` |
| `summary`, `next_step` | Strings, at most 1,000 characters each; may be empty |
| `next_actor` | `agent`, `user`, `other` or `none` |
| `evidence` | Array of up to 32 nonblank reference strings, at most 2,000 characters each |
| `evidence_status` | `current` or `unavailable`; availability is reported, not independently verified |
| `supersedes` | Optional `null` or `{ "source": "previous-source", "run_id": "previous-run" }`; predecessor identifiers follow the same string limits |

Text cannot contain NUL. `(source, run_id, sequence)` identifies a report.
Exact retries return the original `{id, received_at, report}` receipt, including its original receipt time.
Different content under the same identity fails. The server supplies `received_at` as Unix seconds after validating the observation.

Run project, task, session and worktree bindings cannot change. A successor must
reference an existing run in the same task, and a run can have only one successor.
Lower sequences remain historical without replacing the highest sequence.
Validation, identity conflict or storage failure returns an error without a new receipt.
`implemented` is a reporter's claim, never human acceptance.

For a complete dynamically timestamped report, see [Direct Work Reporting](../work-reporting.md).
Wrap that report object in the `report` property when calling this tool.
Raw MCP calls have no durable client queue; the caller must retry the exact payload after uncertain delivery.

## `work_overview`

Reads reports using the server's current time and returns `{ "tasks": [...] }` as JSON text.

```json
{ "project": "sample-project", "stale_after_secs": 300 }
```

Both fields are optional. Omit `project` or use `null` to include all projects;
otherwise matching is exact and the string cannot be blank or contain NUL.
`stale_after_secs` defaults to `300` and must be non-negative. Unknown fields are rejected.

Each task contains `project`, `task`, `task_title`, `state`, `next_actor`, `stale`,
`source_unavailable` and `runs`. Each run contains its latest `receipt`, plus
`superseded` and `stale`; the receipt includes the complete submitted report.

The highest sequence determines each run's latest observation. Freshness uses
`now - observed_at > stale_after_secs`; arrival or replay does not renew it.
Task flags consider runs without successors. Different state, next actor or next
step among those runs produces `conflict`, including stale runs, with next actor `other`.
Otherwise, stale `running` becomes `reporting_missing`, and `waiting` with next
actor `user` becomes `needs_user`; other states keep their reported value.
Unavailable evidence remains visible through `source_unavailable`.
An empty result does not establish that no work is happening elsewhere.

## `work_history`

Reads immutable receipts for one explicit run as a JSON text array:

```json
{ "source": "sample-client", "run_id": "sample-run" }
```

Both strings are required; unknown fields are rejected. Receipts are ordered by
sequence, including reports from a superseded run. An unknown run returns `[]`.
This tool does not change freshness, status or acceptance.

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
- `response_format`: `json`

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

`response_format: "json"` (the default) returns the complete stored memory
record, on one line. The desktop app decodes it as `Memory`, so this default
must not change.

`response_format: "compact"` returns a receipt instead, because the caller
already holds the content it sent:

```json
{"id":"01954d70-cf20-7d42-bb3b-ff2f0f0de123","namespace":"project:ai","title":"Use SQLite","updated_at":"2026-03-02T19:15:00Z"}
```

`updated_at` is the value a later `memory_update` needs as
`expected_updated_at`. `title` is `null` when the memory has none. Any other
`response_format` is rejected before anything is stored.

### Failure cases

- validation error
- storage failure
- malformed metadata

## `memory_update`

Patch an existing memory in place.

### Why it exists

Interactive clients need to edit a known record without relying on a
`source + source_ref` upsert key.

### Input

`memory_id` and a non-empty `expected_updated_at` are required. Copy
`expected_updated_at` from the record's last observed `updated_at` value and
supply only the fields to change:

```json
{
  "memory_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "title": "Use SQLite over SSH",
  "summary": null,
  "expected_updated_at": "2026-03-02T19:15:00Z"
}
```

Patchable fields are `namespace`, `kind`, `title`, `summary`, `content`,
`tags`, `source`, `source_ref`, `confidence`, `importance`, `metadata`,
`valid_from`, and `valid_until`.

### Behaviour

- omitted fields remain unchanged
- JSON `null` clears nullable fields: `title`, `summary`, `source`,
  `source_ref`, `confidence`, `valid_from`, and `valid_until`
- `metadata` is not nullable; send `{}` to clear it
- namespace auto-detection does not run for updates; omit `namespace` to retain
  it, or supply the new namespace explicitly
- validates the complete merged record before committing the patch
- preserves the existing `id` and `created_at`
- updates tags and full-text search data atomically through the core repository
- rejects the whole patch if `expected_updated_at` differs from the stored
  timestamp
- attempts to regenerate the embedding when server-side `auto_embed` is enabled;
  an embedding failure is logged and does not roll back the stored edit
- the Tauri editor sends only changed fields with the last observed
  `updated_at`, then refreshes its local record from the response

### Response

`response_format: "json"` (the default) returns the complete updated memory
record. `response_format: "compact"` returns the same receipt as
`memory_remember` (`id`, `namespace`, `title`, `updated_at`), with the new
`updated_at` ready for the next patch. `response_format: "markdown"` is
accepted on both write tools and answered with the same receipt, so a model
that passes the reading default does not have its write refused. Any other
value is rejected before the patch is applied.

### Failure cases

- validation error
- missing or blank `expected_updated_at`
- memory not found
- stale `expected_updated_at` → `Conflict: memory <id> changed after it was read; fetch the latest record and retry`
- storage failure

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
  "archived_only": false,
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
- `sort_by`: null (default: `updated_at DESC`). It orders a listing; with `query` it has no effect, because keyword results are ordered by relevance
- `include_archived`: `false`
- `archived_only`: `false`
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

- if `query` is present, use FTS recall: a memory must contain every word
- if that strict search finds nothing and `query` has more than one word, retry
  once with core's any-word search (`RecallQuery.match_any_term`, the search
  resume briefs use) under the same filters. The reply then opens with a note
  saying a wider search was used (the first line in `markdown`, a `note` field
  in `compact`), so a sentence such as "how do we deploy a new release to
  Atlas" is never answered with a bare zero. `json` returns the widened items
  in its usual envelope, with no extra field
- if a query still matches nothing, the reply is a one-line note that suggests
  `memory_search` and fewer or different keywords (`markdown`), an empty
  envelope carrying that `note` (`compact`), or the plain empty envelope
  (`json`)
- if `query` is absent, return recent records subject to filters
- by default, archived records are excluded
- support namespace, kind, and tags filters
- `archived_only: true` returns archived records only and takes precedence over `include_archived`
- support pagination with a full-ID tie-breaker; an empty page retains the filtered total
- the response envelope includes `archived_only`, confirming whether the backend applied that filter; older servers omit it, so clients must require `true` before labelling results Archive
- when `namespace` is explicitly provided, filter to that namespace only
- when `cwd` is provided and `namespace` is omitted, use scoped recall: search detected namespace first, then fill remaining slots from `global`; project-scoped results appear before global results
- when detection falls back to `global`, search only `global` unless `global: true` requests all namespaces

### Structured response

`response_format: "compact"` returns the recall envelope with
[compact items](#compact-item), on one line:

```json
{"total":1,"count":1,"offset":0,"limit":10,"archived_only":false,"items":[{"id":"01954d70-cf20-7d42-bb3b-ff2f0f0de123","namespace":"project:ai","kind":"decision","title":"Use SQLite","excerpt":"SQLite is the default shared local store.","created":"2026-03-02","score":7.813}]}
```

`response_format: "json"` returns the same envelope with each item the complete
memory record plus the core `rank` (BM25 for a keyword query: negative, lower
is better; `null` for a listing). This is what `RecallResult` decodes. Shown
indented here; the reply itself is one line:

```json
{
  "total": 1,
  "count": 1,
  "offset": 0,
  "limit": 10,
  "archived_only": false,
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

A count line, then one card per item:

```text
### Use SQLite
`01954d70-cf20-7d42-bb3b-ff2f0f0de123` · project:ai/decision · 2026-03-02 · sqlite, memory
> SQLite is the default shared local store.
```

- heading: the title, or the id when there is none
- id, namespace/kind, creation date (so an old status note is not read as
  current) and tags
- the summary, or the first 200 characters of the content

Rank and full timestamps are left out; they are in the `json` reply.

### Failure cases

- unknown `sort_by` → `Invalid sort_by '<value>'. Must be one of: updated_desc, updated_asc, importance_desc, importance_asc, created_desc, created_asc.` (it is never silently ignored; an empty value counts as not given)
- an importance bound that can match nothing (`importance_min` above 5, `importance_max` below 1) → `<field> must be between 1 and 5.`
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

**Deprecated** — use `memory_recall` with no `query` (it falls through to a
recent-style listing). Retained for compatibility.

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
  "archived_only": false,
  "offset": 0,
  "limit": 10,
  "response_format": "json"
}
```

### Behaviour

- recall without a full-text query
- sorted by `updated_at DESC` by default, or by `sort_by` when specified; an
  unknown `sort_by` and an importance bound outside 1–5 are rejected as in
  `memory_recall`
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
- `archived_only`: `false` (takes precedence over `include_archived`)
- `offset`: `0`
- `limit`: `10`
- `response_format`: `markdown`

`compact` and `json` behave as in `memory_recall`.

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
["global", "project:ai", "project:my-app", "tool:codex"]
```

A JSON array of namespace strings, sorted lexicographically.

### Failure cases

- storage failure

## `memory_namespace_details`

List every namespace with its memory count and most recent activity.

### Why it exists

Allows a client to show the impact of deleting a workspace before asking for
confirmation, including when the database is reached through the remote MCP
bridge.

### Input

No input parameters.

### Behaviour

- includes archived memories in each count
- returns one entry per namespace, sorted lexicographically
- returns an empty array when the database has no memories

### Response

```json
[
  {
    "name": "project:my-app",
    "memory_count": 12,
    "last_activity": "2026-08-15T07:42:10Z"
  }
]
```

### Failure cases

- storage failure

## `memory_namespace_delete`

Permanently delete one non-global namespace and all its memories, after taking
a database backup.

### Why it exists

Provides the remote desktop bridge with the same recoverable workspace deletion
flow as a local database. It is intentionally narrower than the general cleanup
workflow: the caller must name exactly one workspace.

### Input

```json
{
  "namespace": "project:unused-app"
}
```

### Behaviour

- reject an empty namespace or `global`
- take a transactionally consistent SQLite backup before deleting anything
- permanently delete every memory in the named namespace
- clear the server cache after successful deletion
- return success with a zero count if the named namespace has no memories

### Response

```json
{
  "backup_path": "/path/to/backups/clio-backup-2026-08-15T08-00-00.db",
  "namespaces_deleted": ["project:unused-app"],
  "memories_purged": 12
}
```

### Failure cases

- empty namespace
- protected `global` namespace
- backup failure (nothing is deleted)
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
6. If confidence is below `capture.review_threshold`, queue the suggested memory with `source_route = "capture"` and return a `Queued` outcome
7. Otherwise store as a memory with `source = "capture"`, auto-embed when enabled, and return a `Stored` outcome

### Response

Returns an internally tagged result. A stored result has `outcome = "Stored"`
alongside the full memory record. Abridged example:

```json
{
  "outcome": "Stored",
  "id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "source": "capture",
  "source_ref": null,
  "content": "We decided to move the auth service to a separate repo.",
  "namespace": "project:ai"
}
```

A low-confidence result has `outcome = "Queued"` alongside the review item.
It is not yet a durable memory. Abridged example:

```json
{
  "outcome": "Queued",
  "id": "01954d70-cf20-7d42-bb3b-ff2f0f0de456",
  "content": "We may move the auth service.",
  "suggested_namespace": "project:ai",
  "source_route": "capture",
  "source_ref": null,
  "status": "pending"
}
```

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
| `api_key` | null | falls back to `OPENAI_API_KEY_CLIO`, then the shared `OPENAI_API_KEY` with a warning — see [settings](settings.md#api-key-resolution) |
| `base_url` | `https://api.openai.com/v1` | override for proxies or compatible APIs |
| `model` | `gpt-4o-mini` | any OpenAI-compatible chat model |

Configure via CLI: `clio settings use-capture --api-key <key> [--model <model>] [--base-url <url>]`

## `memory_session_checkpoint`

Distil one session delta and commit it as an exact-once checkpoint keyed by `source + session_id + cursor`.

### Why it exists

Stop-hook capture retries after a lost response, a provider 429 or an outage. Without a server-side identity, a retry either duplicates atoms or silently loses the delta. A checkpoint gives every capture attempt exactly one durable outcome: the first delivery stores the atoms and the result envelope atomically; every later delivery of the same key replays that stored result.

### Input

```json
{
  "text": "…redacted session-delta digest…",
  "source": "claude-session",
  "session_id": "b2c3d4",
  "cursor": 1240,
  "namespace": "project:clio",
  "cwd": "/Users/alice/code/my-project",
  "branch": "develop",
  "ticket": "clio-123"
}
```

### Input defaults

- `namespace`: null — an explicit value overrides the namespace of every extracted memory
- `cwd`: null (used for default-namespace detection, recorded in memory metadata)
- `branch`, `ticket`: null (stored on the checkpoint row)

### Validation rules

- `text`, `source` and `session_id` are required and must not be empty
- `cursor` must be zero or positive
- an unseen cursor older than the latest committed cursor for the session is
  rejected; operator dead-letter recovery is available only through the CLI's
  explicit `checkpoint --recover-stale` flag

### Behaviour

1. Preflight: if a checkpoint for `(source, session_id, cursor)` exists, return its stored result with `replayed: true` — the model is not called
2. Otherwise distil `text` with the configured capture model outside both the write transaction and the shared SQLite mutex
3. Open one write transaction, recheck the key under the lock, then store every extracted memory or review item and the checkpoint record atomically
4. Any failure rolls back all writes — no partial session can persist; the key stays open for retry
5. An empty extraction commits an empty checkpoint: success, never redistilled
6. Auto-embedding runs best-effort after commit and cannot fail the checkpoint
7. Namespace resolution per memory matches `clio distill`: explicit `namespace` → LLM `global` promotion → `cwd`-detected default → LLM suggestion
8. Atoms carrying explicit open-loop data (`attention.explicitness = "explicit"`) open attention atomically with the stored memory; suggested/inferred open loops queue for review whatever their confidence, and approval creates memory + attention in one transaction
9. An atom's `resolves` identifier never completes anything automatically: a stable reference to an open/snoozed attention item records a reviewable `resolution_candidate` event; anything fuzzy leaves state unchanged
10. `ticket` becomes a deterministic lowercase `ticket:<id>` tag and `branch` is recorded in memory metadata — the model never invents session identifiers

### Response

```json
{
  "checkpoint_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "replayed": false,
  "stored_memory_ids": ["01954d70-…"],
  "queued_review_ids": [],
  "created_at": "2026-07-29T16:04:48Z"
}
```

`replayed: true` means the key had already completed and the stored envelope was returned unchanged.

### Failure cases

- capture not enabled / API key missing → configuration error (same as `memory_capture`)
- LLM API request failed → storage error with HTTP status; the key stays open for retry
- LLM returned unparseable JSON → validation error; the key stays open for retry
- storage failure → transaction rolled back, actionable storage error

## `memory_get_links` — direction parameter

`memory_get_links` accepts an optional `direction` field:

- `"outgoing"` (default): the existing compatible shape — raw outgoing links
- `"incoming"` / `"both"`: edge contexts `{ from_memory_id, to_memory_id, direction, relationship, metadata, created_at }`, where `direction` is relative to the requested memory

Linked recall (`memory_recall` with `include_links`) likewise returns each linked memory once with a `link_context` array preserving every edge's direction, relationship and metadata.

## `memory_resume`

Build a deterministic, evidence-backed resume brief: what deserves attention now, and why.

### Why it exists

Handoff and context briefs answer "what is this project?"; resume answers "what should I pick up right now?". One core policy selects eligible open work, blocked items, constraints (with a modest global prior), recent decisions, prompt-relevant knowledge and recent activity — every item carrying a `reason`. Adapters and hooks request the brief; they never recreate ordering, eligibility or budgeting.

### Input

```json
{ "cwd": "/Users/alice/code/my-project", "session_id": "b2c3d4" }
{ "query": "checkpoint retry semantics", "session_id": "b2c3d4", "max_items": 12 }
```

### Input defaults

- `query`: null — the relevant-knowledge section abstains entirely without one
- `namespace`: null (auto-detected from `cwd`)
- `session_id`: null (no repeat suppression, no `surfaced` events)
- `max_items`: 20; `char_budget`: null; `response_format`: markdown

### Behaviour

0. Relevant knowledge is fused retrieval (`scoring.fusion`, on by default): any-word keyword search and semantic search over the project namespace, merged by rank position. An item only the semantic side found carries the reason `similar in meaning to the task`; the rest keep `matches task terms`. With no embedding backend, a failed embedding or an overrun of `scoring.fusion_budget_ms`, the section is the keyword result alone and the call still succeeds. The section is empty when the most similar memory in the namespace is below `scoring.quiet_similarity` and no keyword match either shares three of the query's chosen terms (all of them, when it has fewer) or contains an identifier the query names: nothing there is about the task. That rule needs the semantic side: with `scoring.fusion` off, or when the call falls back to keywords (no backend, a failed embedding, the budget overrun), nothing is quietened
1. Prompt briefs prioritise Relevant knowledge, then Needs attention, Waiting on, Active constraints, Recent decisions and Recent activity. Session starts put one recent receipt first, followed by attention, waiting, constraints and decisions.
2. Empty sections release capacity. Attention and waiting reserve slots; constraints also reserve a slot at session start. Small prompt budgets can omit constraints. On a prompt call (a `query` is given) Relevant knowledge is allocated its ten slots whether or not it fills them: the slots it leaves empty may go to Needs attention and Waiting on only, and constraints, decisions and activity get what they would have had beside a full section. A brief with nothing matched stays short instead of padding itself. Session start is unaffected.
3. Items are deduplicated across sections by memory ID; archived, expired, resolved and cancelled records never appear. Within Relevant knowledge a near-copy of an item already chosen (same title, or stored embeddings from the same model at least `scoring.dedup_similarity` similar) is skipped and the next candidate takes its slot. Receipts are left out before either side's candidates are cut. Fused knowledge reads 50 merged candidates (30 from each side); if filtering and near-copy skipping leave slots empty after a full 50, it reads once more at 100 (60 from each side), and nothing beyond that is considered. Keyword-only knowledge reads one window of 50. Near-copy skipping applies to this section only, not to `memory_search` results
4. A resume never counts as use for ranking: `access_count` and `last_accessed_at` stay unchanged. It is not write-free, though (see 5)
5. With `session_id`, default selection records idempotent surfaced events. Attention is suppressed until its state changes. Constraints and decisions are suppressed only on query-bearing requests after acknowledged delivery, and edits make them eligible again. Session starts never suppress these sections. Deferred selection records nothing until acknowledgement; without a session nothing is written.
6. The character budget counts the serialised representation (title + bounded content + reason); at least one item is always kept
7. Event-write failure logs a warning and still returns the brief

### Response

Markdown by default, one line per item with its kind, title, id and creation date, then why it is shown and its bounded content. `response_format: "json"` returns `{ namespace, sections: [{ heading, items: [{ memory_id, kind, title, content, source, created_at, state, reason }] }], total_items, generated_at }`.

## `memory_action`

Manage follow-up attention on memories — the open-loops lifecycle.

### Why it exists

Memories record what was said; attention records what is still owed. An explicit user commitment ("I'll verify the deployment tomorrow") should open attention immediately, resurface at the right moment with a reason, and resolve with evidence — without ever rewriting the underlying memory or losing its audit history.

### Input

One merged tool with an `action` discriminator, mirroring `memory_inbox`:

```json
{ "action": "add", "content": "Verify the deployment after release", "owner": "user", "due_at": "2026-08-01T00:00:00Z" }
{ "action": "add", "memory_id": "0195…", "trigger": "project-session" }
{ "action": "list", "namespace": "project:clio", "status": "open" }
{ "action": "eligible", "namespace": "project:clio", "scope": "session-b2c3" }
{ "action": "complete", "id": "0195…", "evidence": "0195…", "reason": "shipped" }
{ "action": "snooze", "id": "0195…", "until": "2026-08-15T00:00:00Z" }
{ "action": "cancel", "id": "0195…", "reason": "obsolete" }
{ "action": "attach_external", "id": "0195…", "external_system": "things", "external_ref": "things-id" }
{ "action": "history", "id": "0195…" }
```

`id` accepts either the attention item ID or the underlying memory ID.
`namespace` accepts a bare project name (`clio` means `project:clio`), as
`memory_recall` does.

### Behaviour

- `add` opens attention on an existing memory (`memory_id`) or creates a `task` memory from `content` and opens attention on it — memory, attention row and initial event commit atomically. Creation is idempotent per memory.
- Statuses are only `open`, `snoozed`, `resolved`, `cancelled`. Invalid transitions are validation errors; `resolved`/`cancelled` are terminal.
- `eligible` is a pure read returning each item with a machine-readable `reason` (`overdue`, `reminder_due`, `project_session`, `dormant`). Passing `scope` suppresses items already surfaced in that session/topic for the same reason and state.
- `complete` records a `resolved` event and, when `evidence` is given, a `resolved_by` link from the followed-up memory to the evidence memory. The source memory is never rewritten.
- `attach_external` records a verified external reference and keeps the item open; it does not mark anything delivered.
- `history` returns the append-only event trail for the item's memory.

The `overview` response includes `memory_titles`, keyed by full memory ID, for evidence that is neither archived nor expired. When `namespace` is supplied, projected evidence must also belong to that namespace. Without `namespace`, the overview and its titles deliberately span all workspaces, the same as unscoped recall. Missing or differently scoped evidence has no entry. Titles fall back to the first non-empty content line, then “Untitled memory”. This projection does not update access tracking. `review_pending` counts both pending and edited captures across all workspaces.

### Failure cases

- missing required fields per action → validation error naming the field
- unknown action → validation error listing the valid actions
- unknown `status` filter on `list` → validation error listing the four statuses (it is never treated as "no matches")
- id resolving to no attention item → not-found error
- invalid transition (e.g. completing a cancelled item) → validation error stating both states

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
- the response echoes `namespace` (null for all workspaces), confirming the applied scope; older backends omit it
- `by_week` covers up to 52 ISO weeks, ordered newest first
- `top_tags` returns the 20 most frequent tags by count
- `embedding_coverage` is a percentage (0–100) of memories that have a stored embedding
- `total_links` counts links whose source memory belongs to the selected namespace; links to another namespace count once under their source
- `link_density` is scoped outgoing links divided by scoped total memories (including archived records)
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
- scans non-archived memories whose stored model and dimensions match the active backend, then computes cosine similarity
- returns candidates with similarity ≥ `threshold`, sorted by similarity descending
- embeddings must be enabled in settings for this tool to work
- after changing provider or model, restart the MCP client and run `clio embed backfill` before expecting all memories to participate

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

`memory_recall` uses full-text search (keyword matching via FTS5). `memory_search` uses cosine similarity over stored embedding vectors to find conceptually related memories even when query terms do not appear in the content, and merges in any-word keyword matches so a query that names an identifier still finds it. Semantic results need embeddings to be enabled in `clio-settings.json` and memories to have been embedded; without them the keyword matches are returned alone.

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

- `namespace`: null (auto-detected from `cwd` when provided; otherwise `global`)
- `cwd`: null (no auto-detection)
- `include_archived`: `false`
- `limit`: `10`
- `response_format`: `markdown`

### Validation rules

- `query` is required and must not be empty
- `limit` must be a positive integer
- `response_format` must be `markdown`, `compact` or `json`

### Behaviour

- embed the query text using the active embedding backend
- compute cosine similarity only against stored embeddings with the active model name and matching vector dimensions, dropping matches below `scoring.min_similarity`
- run any-word keyword search over the same memories and merge the top 30 of each by rank position (reciprocal rank fusion): an item scores `1 / (scoring.fusion_k + position)` in each list that holds it, 10 by default. The exact-phrase keyword boost this replaces never matched a sentence
- a keyword match containing an identifier the query names (a ticket id, or a name with `_` or `::`) ranks ahead of the merged order (`scoring.identifiers_first`)
- if the backend is missing, the embedding fails or `scoring.fusion_budget_ms` is overrun, return the keyword matches alone. There is no error. A `markdown` or `compact` reply carries a note saying so; a `json` reply does not, because its shape is the core recall envelope and cannot change
- return nothing from a namespace whose most similar memory is below `scoring.quiet_similarity` and no keyword match either shares three of the query's chosen terms (all of them, when it has fewer) or contains an identifier the query names (the threshold is 0.45; `0` switches the rule off). When nothing is close in meaning but some keyword matches do agree, only those are returned. Scoped search applies this to the project and to `global` separately. The rule needs the semantic side, so a keyword-only fallback is never quietened
- only namespace, archive and expiry filters exist for this tool; the core request type carries nothing else, so no filter can be accepted and applied to one side only
- results are not de-duplicated: near-copy skipping belongs to the resume brief
- with `scoring.fusion` off: vectors alone, ranked by cosine similarity adjusted by the phrase boost and the configured scoring, and an error when no backend is available
- when `namespace` is explicitly provided, filter to that namespace; a bare project name is canonicalised first, as in `memory_recall`
- when `cwd` is provided and `namespace` is omitted, use scoped recall: search detected namespace first, then fill remaining slots from `global`; project-scoped results appear before global results
- when detection falls back to `global`, search only `global` unless `global: true` requests all namespaces
- exclude archived memories by default
- return results best first
- the ranking value is returned as `score` in a compact item and as `rank` in a `json` record; higher is a better match. **It is a fused rank score, not a similarity**: at most `2 / (fusion_k + 1)`, about 0.18 at the default, and it says only where an item stood in the two lists. A consumer that showed the old value as a similarity percentage must stop. An identifier match placed first keeps its own score, so scores are not strictly descending

### Response

Returns a recall envelope identical in shape to `memory_recall`:
[compact items](#compact-item) with `compact`, complete records with `json`.
The `json` shape, shown indented:

```json
{
  "total": 1,
  "count": 1,
  "offset": 0,
  "limit": 10,
  "archived_only": false,
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
      "rank": 0.174
    }
  ]
}
```

### Failure cases

- embeddings disabled in settings → `Configuration error: embeddings are disabled`
- embedding backend not available at compile time → configuration error
- embedding backend unreachable (e.g. OpenAI API key missing) → configuration error
- no embedding backend in this process → `Embedding backend not available. Ensure embeddings are configured in settings. Use memory_recall (keyword search) meanwhile.`
- nothing similar enough, or no memories embedded yet → no error; the reply is a one-line note suggesting `memory_recall` with exact keywords (`markdown`), an empty envelope carrying that `note` (`compact`), or the plain empty envelope (`json`)
- memories embedded with another model or dimensionality are ignored until `clio embed backfill` replaces them
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

The `handoff` preset requires `query` (a ticket id or topic) and assembles a pickup brief: Directly Relevant (FTS, which includes `ticket:<id>`-tagged memories), Active Constraints, and Recent Receipts matching the query or ticket tag.

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
- `query`: null (used with `custom`; required by `handoff`)
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
| `handoff` | Directly Relevant, Active Constraints, Recent Receipts matching the query or ticket tag (requires `query`) |
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

With `json` each entry of `items` is a complete memory record; with `compact`
it is a [compact item](#compact-item) without `score`. The reply is one line.

### Failure cases

- invalid preset name → validation error
- storage failure

## `memory_inbox`

Review the capture inbox (low-confidence memories awaiting review). A single
tool dispatched by `action`. Replaces the former `memory_inbox_list`,
`memory_inbox_approve`, `memory_inbox_reject`, and `memory_inbox_edit` tools.

### Input

`action` is required. `review_id` is required for `approve`/`reject`/`edit`,
ignored for `list`.

```json
{ "action": "list", "limit": 20, "response_format": "markdown" }
```

```json
{ "action": "list", "limit": 100, "response_format": "json", "include_status_scope": true }
```

```json
{ "action": "approve", "review_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123" }
```

```json
{ "action": "reject", "review_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123" }
```

```json
{
  "action": "edit",
  "review_id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "namespace": "project:ai",
  "kind": "decision",
  "title": "Updated title",
  "tags": ["new-tag"],
  "importance": 4
}
```

### Defaults

- `action`: required (`list` | `approve` | `reject` | `edit`)
- `limit`: `20` (list only)
- `response_format`: `markdown` (list only)
- `include_status_scope`: `false` (JSON list only)

### Behaviour

- `list`: returns items with `status IN ('pending', 'edited')`, ordered by `created_at ASC, id ASC`; edited captures remain visible until approved or rejected
- JSON lists normally return an array. With `include_status_scope: true`, the response is `{ "items": [...], "includes_edited": true }`. Existing callers retain their response format. The desktop adapter requires this confirmation before accepting remote inbox results, including an empty list; older backends require an update. The desktop UI still receives an array.
- `approve`: creates a memory from the suggested fields via
  `repository::remember()`, preserving `source_route` as `source`, plus
  `source_ref` and metadata; it sets status to `approved` and `reviewed_at` to
  now, auto-embeds when enabled, and returns the created memory
- `reject`: sets status to `rejected` and `reviewed_at` to now, returns the item
- `edit`: updates only the provided fields, sets status to `edited`; the item
  still requires `action: "approve"` afterwards

### Failure cases

- unknown `action`
- `approve`/`reject`/`edit` without `review_id`
- review item not found, or already approved/rejected

## `memory_cache_clear`

Clear the current MCP process's namespace-list cache.

Individual memory, recall, recent and embedding reads deliberately go directly
to SQLite so separate MCP processes observe committed changes. Only the
namespace list is cached, with a 30-second TTL. This tool clears that entry
immediately; it does not affect another MCP process.

### Input

No parameters.

### Response

```json
{
  "memory_cleared": 0,
  "recall_cleared": 0,
  "namespace_cleared": 1,
  "embedding_cleared": 0
}
```

## Tool Annotation Guidance

Recommended annotation intent:

| Tool | readOnlyHint | destructiveHint | idempotentHint |
|---|---:|---:|---:|
| `memory_remember` | false | false | false |
| `memory_update` | false | false | false |
| `memory_recall` | true | false | true |
| `memory_get` | true | false | true |
| `memory_recent` | true | false | true |
| `memory_link` | false | false | true |
| `memory_archive` | false | false | true |
| `memory_unarchive` | false | false | true |
| `memory_delete` | false | true | true |
| `memory_move` | false | false | false |
| `memory_namespaces` | true | false | true |
| `memory_namespace_details` | true | false | true |
| `memory_namespace_delete` | false | true | true |
| `memory_get_links` | true | false | true |
| `memory_capture` | false | false | false |
| `memory_session_checkpoint` | false | false | true |
| `memory_search` | true | false | true |
| `memory_stats` | true | false | true |
| `memory_activity` | true | false | true |
| `memory_suggest_links` | true | false | false |
| `memory_context` | true | false | true |
| `memory_inbox` | false | false | false |
| `memory_resume` | true | false | true |
| `memory_action` | false | false | false |
| `memory_cache_clear` | false | false | true |

## Output Discipline

The MCP server should avoid overwhelming clients.

Rules:

- honour `limit`
- do not dump large datasets by default
- for Markdown responses, prefer concise summaries to full raw content lists
- `compact` list responses follow the same rule; never pretty-print JSON
- full content is appropriate for `memory_get`, the item resource, and any call
  that asks for `json`
- never shrink what `json` returns; add a format value instead

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
- update flow, including tags, FTS data and auto-embedding
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
- capture result discrimination (`Stored` and `Queued`) and provenance-preserving approval
- stats flow (all namespaces, scoped to namespace)
- activity flow (creates, updates, archives classified correctly)
- suggest-links flow (above threshold returned, already-linked excluded)
- validation errors
- resource lookup
- keyword-only fallback from `memory_search` and the embedding disabled error from `memory_suggest_links`
- embedding model/dimension isolation and stale-vector backfill
- cross-connection freshness for individual memory reads
- non-empty `json` replies from `memory_recall`, `memory_search`,
  `memory_recent`, `memory_remember`, `memory_update` and `memory_get` decoding
  into the core `RecallResult` and `Memory` types, from the requests the desktop
  app sends (`json_replies_decode_into_the_core_types_programs_use`)

## Invariants For Implementers

The MCP implementation is incorrect if:

- it bypasses the Rust core for storage logic
- it returns field names that do not match the documented contract
- its defaults differ from the CLI/core semantics
- archived records appear unexpectedly in default recall
- error messages are opaque enough that an agent cannot recover

Two adapter behaviours are deliberate presentation choices, not different
semantics: the retry of an empty multi-word `memory_recall` (it calls core's
any-word search and labels the result), and the `compact` projection of list
and write replies (`json` still returns the complete core records). The CLI
does neither yet; the retry is due to move into core so that every adapter
shares it.

## Acceptance Criteria

The MCP contract is ready for implementation when:

- each phase-one workflow maps to a tool or resource
- defaults and required fields are explicit
- structured and human-readable outputs are defined
- another coding agent can implement the server without inventing tool semantics

---

## Related Documentation

- [MCP Agent Setup](../mcp-agent-setup.md) — connection instructions for 10+ AI agents
- [Schema Reference](schema.md) — database tables, indexes, FTS
- [Settings Reference](settings.md) — configuration keys and defaults
- [Architecture](../../context/ARCHITECTURE.md) — system overview and crate boundaries
- [Documentation Index](../README.md) — all available documentation

Ordinary keyword recall retries an empty multi-word query using any-word matching.
Scoped recall widens only when both project and global strict results are empty.
Pagination beyond an existing result set does not widen. JSON envelopes are unchanged.

Ordinary recall, recent lists, semantic search and context briefs exclude expired
memories (including linked expansion). CLI recall/search/recent accept
`--include-expired`; MCP equivalents accept `include_expired: true` for history.
Direct ID reads, audit/history and export/import preserve expired records.

Session checkpoint distillation includes the earlier-title digest used by CLI
checkpoints, preserving corrections while avoiding repeated knowledge.

Core semantic retrieval, namespace moves, statistics and activity filters accept
bare project names. The reserved `global` namespace is case-insensitive; names
with a colon retain their spelling. Migration 018 also normalises global casing.

Distillation keeps at most three knowledge items. Explicit commitments take
priority and may extend this to five non-receipt items, plus one receipt.
