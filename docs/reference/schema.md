# Clio Schema

## Purpose

This document defines the storage contract for Clio, the Rust shared memory system. It exists so any coding agent can implement the core, CLI, MCP server, or later Tauri app without inventing its own database rules.

This schema is the canonical persistence contract for phase one.

## Scope

This document covers:

- table design
- field semantics
- indexes
- full-text search strategy
- validation and invariants
- archive and upsert behaviour
- migration rules

This document does not define:

- MCP tool naming
- CLI flag syntax
- UI workflows

Those are covered elsewhere.

## Design Principles

- local-first
- human-inspectable
- easy to migrate
- strong default retrieval performance
- stable enough for multiple interfaces
- minimal enough to avoid premature complexity

## Storage Engine

Use SQLite.

Required capabilities:

- WAL mode
- FTS5
- foreign keys

Connection pragmas:

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
```

## High-Level Data Model

The system stores:

- memories as canonical records
- tags as normalised labels
- links as typed edges
- vector embeddings for semantic search (optional)
- migrations as DB version markers

Not yet stored:

- event history
- sync checkpoints
- user accounts

## Core Entities

## `memories`

The primary record type.

```sql
CREATE TABLE memories (
    id TEXT PRIMARY KEY,
    namespace TEXT NOT NULL DEFAULT 'global',
    kind TEXT NOT NULL DEFAULT 'note',
    title TEXT,
    summary TEXT,
    content TEXT NOT NULL,
    tags_text TEXT NOT NULL DEFAULT '',
    source TEXT,
    source_ref TEXT,
    confidence REAL,
    importance INTEGER NOT NULL DEFAULT 3,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    valid_from TEXT,
    valid_until TEXT,
    archived_at TEXT,
    last_accessed_at TEXT,
    access_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (length(namespace) BETWEEN 1 AND 120),
    CHECK (length(kind) BETWEEN 1 AND 50),
    CHECK (title IS NULL OR length(title) <= 240),
    CHECK (summary IS NULL OR length(summary) <= 1000),
    CHECK (importance BETWEEN 1 AND 5),
    CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0))
);
```

### Field semantics

| Field | Meaning | Rules |
|---|---|---|
| `id` | opaque stable memory id | UUIDv7 preferred |
| `namespace` | logical scope | required, never empty |
| `kind` | memory type | required, never empty |
| `title` | short label | optional |
| `summary` | concise preview | optional |
| `content` | full durable content | required |
| `tags_text` | FTS helper text | derived from `memory_tags` |
| `source` | writing system | optional |
| `source_ref` | external idempotency key | optional |
| `confidence` | certainty of the memory | nullable decimal |
| `importance` | relative significance | integer 1..5 |
| `metadata_json` | extension payload | JSON object only |
| `valid_from` | not-before timestamp | nullable UTC string |
| `valid_until` | expiry/relevance end | nullable UTC string |
| `archived_at` | soft deletion marker | null means active |
| `last_accessed_at` | last time the memory was read by recall/get/search | nullable UTC string; null means never accessed |
| `access_count` | number of times the memory has been read | integer, default 0 |
| `created_at` | insert time | required UTC string |
| `updated_at` | last write time | required UTC string |

### Semantic rules

- `content` is the only mandatory human payload field.
- `title` and `summary` are optional because some clients will store raw findings or notes.
- `metadata_json` must always decode to an object. Arrays and scalars are invalid.
- `valid_from` and `valid_until` are advisory fields for future filtering. Phase one may store them without heavily using them.
- `source` and `source_ref` are optional individually, but only meaningful for upsert when both are set.

### Recommended `kind` values

These are guidance, not a hard enum:

- `note`
- `fact`
- `decision`
- `summary`
- `task`
- `observation`
- `constraint`

## `memory_tags`

Normalised tags for filtering and FTS support.

```sql
CREATE TABLE memory_tags (
    memory_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (memory_id, tag),
    FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
    CHECK (length(tag) BETWEEN 1 AND 60)
);
```

### Tag rules

- tags are stored lowercase
- tags are trimmed
- tags are unique per memory
- tags are not globally registered in phase one

## `memory_links`

Typed edges between memory records.

```sql
CREATE TABLE memory_links (
    from_memory_id TEXT NOT NULL,
    to_memory_id TEXT NOT NULL,
    relationship TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    PRIMARY KEY (from_memory_id, to_memory_id, relationship),
    FOREIGN KEY (from_memory_id) REFERENCES memories(id) ON DELETE CASCADE,
    FOREIGN KEY (to_memory_id) REFERENCES memories(id) ON DELETE CASCADE,
    CHECK (length(relationship) BETWEEN 1 AND 60)
);
```

### Link semantics

- links are directional
- reciprocal links are not implied
- duplicate links of the same type between the same nodes are disallowed
- `metadata_json` allows future explanation or provenance of the edge

Recommended relationship labels:

- `relates_to`
- `supports`
- `contradicts`
- `derived_from`
- `supersedes`
- `references`

## `schema_migrations`

Tracks applied migrations.

```sql
CREATE TABLE schema_migrations (
    version TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL
);
```

## `memory_embeddings`

Stores vector embeddings for semantic search. Added by migration `002_embeddings`.

```sql
CREATE TABLE memory_embeddings (
    memory_id TEXT PRIMARY KEY,
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    embedding BLOB NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
);
```

### Field semantics

| Field | Meaning | Rules |
|---|---|---|
| `memory_id` | FK to `memories.id` | primary key, cascades on delete |
| `model` | embedding model name | e.g. `all-MiniLM-L6-v2`, `text-embedding-3-small` |
| `dimensions` | vector size | must match the model's output dimensionality |
| `embedding` | serialised f32 vector | little-endian 4 bytes per dimension |
| `created_at` | when the embedding was generated | ISO-8601 UTC string |

### Embedding rules

- exactly one embedding row per memory (INSERT OR REPLACE on re-embed)
- embedding is stored as a BLOB of little-endian f32 values (`dimensions × 4` bytes)
- the row is deleted automatically when the parent memory is deleted (`ON DELETE CASCADE`)
- a missing row means the memory has not been embedded yet; this is not an error
- the active backend is controlled by `clio-settings.json` alongside the database file

### Semantic search query contract

Cosine similarity is computed in application code (no SQLite extension required):

```sql
SELECT e.memory_id, e.embedding
FROM memory_embeddings e
JOIN memories m ON m.id = e.memory_id
WHERE m.archived_at IS NULL
  -- optionally: AND m.namespace = :namespace
;
```

All candidate rows are fetched and ranked in Rust. Results are sorted by cosine similarity descending before applying `LIMIT`.

## `review_queue`

Holds captures awaiting human review before becoming durable memories. Added by migration `003_review_queue`.

```sql
CREATE TABLE IF NOT EXISTS review_queue (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    suggested_namespace TEXT NOT NULL DEFAULT 'global',
    suggested_kind TEXT NOT NULL DEFAULT 'note',
    suggested_title TEXT,
    suggested_summary TEXT,
    suggested_tags TEXT NOT NULL DEFAULT '',
    suggested_importance INTEGER NOT NULL DEFAULT 3,
    suggested_confidence REAL,
    source_route TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'approved', 'rejected', 'edited')),
    created_at TEXT NOT NULL,
    reviewed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_review_queue_status ON review_queue(status);
```

### Field semantics

| Field | Meaning | Rules |
|---|---|---|
| `id` | opaque review item id | UUIDv7 |
| `content` | original raw text | required |
| `suggested_namespace` | LLM-suggested namespace | default `global` |
| `suggested_kind` | LLM-suggested kind | default `note` |
| `suggested_title` | LLM-suggested title | optional |
| `suggested_summary` | LLM-suggested summary | optional |
| `suggested_tags` | space-separated suggested tags | default empty |
| `suggested_importance` | LLM-suggested importance | 1–5, default 3 |
| `suggested_confidence` | LLM classification confidence | nullable 0.0–1.0 |
| `source_route` | capture route identifier | optional (e.g. `inbox-watcher`, `voice`) |
| `metadata_json` | extension payload | JSON object |
| `status` | review state | `pending`, `approved`, `rejected`, `edited` |
| `created_at` | when the item was queued | ISO-8601 UTC |
| `reviewed_at` | when the item was reviewed | nullable ISO-8601 UTC |

### Review queue workflow

- Items enter the queue when capture confidence is below `review_threshold` in settings
- `approve` converts the item to a memory via `repository::remember()` and sets status to `approved`
- `reject` sets status to `rejected` — the item remains in the table but is not converted
- `edit` updates suggested fields and sets status to `edited` — still requires approval
- `review_threshold` is an optional float in `CaptureConfig`; when `None`, all captures bypass the queue

## Indexes

Required indexes:

```sql
CREATE INDEX idx_memories_namespace ON memories(namespace);
CREATE INDEX idx_memories_kind ON memories(kind);
CREATE INDEX idx_memories_updated_at ON memories(updated_at DESC);
CREATE INDEX idx_memories_archived_at ON memories(archived_at);
CREATE INDEX idx_memories_source ON memories(source);

CREATE UNIQUE INDEX idx_memories_source_ref
    ON memories(source, source_ref)
    WHERE source IS NOT NULL AND source_ref IS NOT NULL;

CREATE INDEX idx_memories_last_accessed_at
    ON memories(last_accessed_at DESC)
    WHERE last_accessed_at IS NOT NULL;

CREATE INDEX idx_memory_tags_tag ON memory_tags(tag);
CREATE INDEX idx_memory_links_from ON memory_links(from_memory_id);
CREATE INDEX idx_memory_links_to ON memory_links(to_memory_id);
```

### Index rationale

- `namespace`, `kind`, and `updated_at` support the most common recall filters
- `archived_at` supports the default active-only behaviour
- `source + source_ref` supports safe idempotent writes
- tag and link indexes support relationship and tag traversal later

## Full-Text Search

Use FTS5 with the `memories` table as the content source.

```sql
CREATE VIRTUAL TABLE memory_fts USING fts5(
    title,
    summary,
    content,
    tags_text,
    content='memories',
    content_rowid='rowid',
    tokenize='porter unicode61'
);
```

### Why this FTS shape

- `title` carries strong intent
- `summary` improves short-result quality
- `content` is the main payload
- `tags_text` lets tags influence recall without complex joins

### Triggers

```sql
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memory_fts(rowid, title, summary, content, tags_text)
    VALUES (new.rowid, new.title, new.summary, new.content, new.tags_text);
END;

CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, title, summary, content, tags_text)
    VALUES ('delete', old.rowid, old.title, old.summary, old.content, old.tags_text);
END;

CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, title, summary, content, tags_text)
    VALUES ('delete', old.rowid, old.title, old.summary, old.content, old.tags_text);
    INSERT INTO memory_fts(rowid, title, summary, content, tags_text)
    VALUES (new.rowid, new.title, new.summary, new.content, new.tags_text);
END;
```

### Ranking guidance

Phase one should use BM25 or equivalent FTS5 ranking with a slight weighting preference toward:

1. title
2. summary
3. content
4. tags

Exact coefficients can vary slightly, but the implementation should document them and keep them consistent across interfaces.

## Query Contracts

These are behavioural expectations rather than the only allowed SQL.

## Search recall

Expected behaviour:

- search active memories by default
- allow filtering by namespace, kind, and tags
- sort primarily by relevance, secondarily by recency

Illustrative SQL:

```sql
SELECT m.*, bm25(memory_fts, 4.0, 2.0, 1.0, 0.5) AS rank
FROM memory_fts
JOIN memories m ON m.rowid = memory_fts.rowid
WHERE memory_fts MATCH :query
  AND m.archived_at IS NULL

-- With temporal scoring enabled (default):
ORDER BY
  ((-bm25(memory_fts, 4.0, 2.0, 1.0, 0.5))
   * exp(-:decay * (julianday('now') - julianday(COALESCE(m.last_accessed_at, m.updated_at))))
   * (1.0 + min(0.5, :boost * ln(1.0 + CAST(m.access_count AS REAL))))
   * (CAST(m.importance AS REAL) / 3.0)
  ) DESC

-- With scoring disabled (decay_lambda = 0.0):
ORDER BY rank ASC, m.updated_at DESC

LIMIT :limit OFFSET :offset;
```

## Recent recall

Expected behaviour:

- no FTS query required
- active-only by default
- sorted by most recent update

```sql
SELECT *
FROM memories
WHERE archived_at IS NULL

-- With temporal scoring enabled:
ORDER BY
  (exp(-:decay * (julianday('now') - julianday(COALESCE(m.last_accessed_at, m.updated_at))))
   * (1.0 + min(0.5, :boost * ln(1.0 + CAST(m.access_count AS REAL))))
   * (CAST(m.importance AS REAL) / 3.0)
  ) DESC

-- With scoring disabled:
ORDER BY updated_at DESC

LIMIT :limit OFFSET :offset;
```

## Tag filtering

Match all tags:

```sql
SELECT m.*
FROM memories m
WHERE m.id IN (
    SELECT memory_id
    FROM memory_tags
    WHERE tag IN (:tag1, :tag2)
    GROUP BY memory_id
    HAVING COUNT(DISTINCT tag) = 2
);
```

Match any tags:

```sql
SELECT DISTINCT m.*
FROM memories m
JOIN memory_tags t ON t.memory_id = m.id
WHERE t.tag IN (:tag1, :tag2);
```

## Upsert Contract

When both `source` and `source_ref` are present and the caller requests upsert:

- search for an existing row with the same `source` and `source_ref`
- if found, update that row in place
- preserve the original `id` and `created_at`
- replace tags and FTS state
- refresh `updated_at`

If no match exists:

- insert a new row

If only one of `source` or `source_ref` is present:

- do not attempt upsert
- treat the write as a normal insert unless the interface validation rejects it

## Archive Contract

Archive means:

- set `archived_at` to a UTC timestamp
- keep the record queryable when explicitly requested
- exclude it from default search and recent queries

Phase one should not expose hard delete in normal workflows.

## Timestamps

All timestamps should be stored as ISO-8601 UTC strings.

Format guidance:

```text
2026-03-02T19:15:00Z
```

Do not store local time zone offsets unless there is a strong reason to do so consistently everywhere.

## Namespace Rules

Namespaces are free-form strings but should follow these conventions:

- `global`
- `project:<slug>`
- `tool:<slug>`
- `person:<slug>`
- `topic:<slug>`

Guidance:

- namespaces must be explicit in user-facing tools
- interfaces may default to `global`
- code should not silently rewrite namespaces

## Export Contract

Phase one must support export to JSONL.

Each line represents one memory record:

```json
{
  "id": "01954d70-cf20-7d42-bb3b-ff2f0f0de123",
  "namespace": "project:ai",
  "kind": "decision",
  "title": "Use SQLite",
  "summary": "SQLite is the shared local store.",
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
  "last_accessed_at": "2026-03-02T20:00:00Z",
  "access_count": 5,
  "created_at": "2026-03-02T19:15:00Z",
  "updated_at": "2026-03-02T19:15:00Z"
}
```

The export format should be easy for:

- backup
- migration
- test fixtures
- future sync

## Migration Policy

- every change ships as a new migration
- never edit an already-applied migration
- prefer additive changes
- document schema changes here before implementation
- if a breaking storage change becomes unavoidable, add a migration note and version bump

### Applied migrations

| Version | Contents |
|---|---|
| `001_initial` | `memories`, `memory_tags`, `memory_links`, all indexes, `memory_fts` virtual table, insert/update/delete triggers |
| `002_embeddings` | `memory_embeddings` table for vector embedding storage |
| `003_review_queue` | `review_queue` table for capture review workflow, `idx_review_queue_status` index |
| `004_access_tracking` | `last_accessed_at` and `access_count` columns on `memories`, partial index on `last_accessed_at` |

## Invariants For Implementers

Any implementation is incorrect if it violates these rules:

- archived records appear in default recall
- tags and `tags_text` drift permanently out of sync
- CLI, MCP, and future Tauri write different row shapes
- upsert creates duplicates when `source + source_ref` should match
- business logic lives outside the core and diverges by interface

## Testing Requirements

At minimum, the Rust implementation should test:

- migration bootstrap
- insert memory
- update via upsert
- FTS recall
- recent recall
- namespace filtering
- tag filtering with match-any and match-all
- archive hiding by default
- link creation
- JSONL export shape

## Future-Compatible Extensions

Reserved likely future tables:

- `memory_events` — edit history and change log
- `memory_sync_state` — checkpoint data for future sync
- `memory_collections` — named groupings across namespaces

These should not be added until a real use case exists.

## Acceptance Criteria

The schema is ready for coding agents when:

- it can be implemented directly as SQL migrations
- the core CRUD and recall flows are fully expressible
- the MCP contract can map to the stored entities without inventing new fields
- the future Tauri app can browse and edit records using the same tables
