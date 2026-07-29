# Domain Rules

## Core Entities

### Memory

The primary record type. A durable, searchable note stored in SQLite.

| Field | Type | Required | Default | Constraints |
|---|---|---|---|---|
| `id` | TEXT | auto | UUIDv7 | primary key |
| `namespace` | TEXT | yes | `global` | 1–120 chars |
| `kind` | TEXT | yes | `note` | 1–50 chars |
| `title` | TEXT | no | null | max 240 chars |
| `summary` | TEXT | no | null | max 1000 chars |
| `content` | TEXT | yes | — | cannot be empty |
| `tags_text` | TEXT | auto | `''` | derived from memory_tags |
| `source` | TEXT | no | null | writing system identifier |
| `source_ref` | TEXT | no | null | external idempotency key |
| `confidence` | REAL | no | null | 0.0–1.0 |
| `importance` | INTEGER | yes | 3 | 1–5 |
| `metadata_json` | TEXT | yes | `'{}'` | must be a JSON object |
| `valid_from` | TEXT | no | null | ISO-8601 UTC |
| `valid_until` | TEXT | no | null | ISO-8601 UTC |
| `archived_at` | TEXT | no | null | null = active |
| `created_at` | TEXT | yes | now | ISO-8601 UTC |
| `updated_at` | TEXT | yes | now | ISO-8601 UTC |
| `last_accessed_at` | TEXT | no | null | ISO-8601 UTC, null = never accessed |
| `access_count` | INTEGER | yes | 0 | incremented on read |

### Memory Kinds

Not a closed enum — guidance only:
- `note` — general purpose
- `fact` — verified information
- `decision` — architectural or product decision
- `summary` — condensed context
- `task` — actionable item
- `observation` — noted behaviour or finding
- `constraint` — known limitation or rule
- `snippet` — reusable code or text fragment
- `knowledgebase` — reference article or documentation entry

### Tag

Normalised label attached to a memory.

- Stored lowercase, trimmed
- Unique per memory (deduplicated)
- 1–60 characters
- Not globally registered in phase one

### MemoryLink

Typed directional edge between two memories.

- Links are directional — reciprocal links are not implied
- Duplicate links of the same type between the same nodes are disallowed
- Relationship labels: `relates_to`, `supports`, `contradicts`, `derived_from`, `supersedes`, `references`, `auto:relates_to`
- The `auto:` prefix distinguishes machine-created links from human-created ones
- `metadata_json` allows explanation or provenance of the edge

## Namespaces

Explicit scope for organising memories. Free-form strings with conventions:

| Pattern | Purpose |
|---|---|
| `global` | Default, cross-cutting |
| `project:<slug>` | Codebase-specific memory |
| `tool:<slug>` | Tool-local operational state |
| `person:<slug>` | Person-scoped context |
| `topic:<slug>` | Subject-area grouping |

### Namespace Resolution

Effective namespace for any write or read operation is resolved in this order (highest priority first):

1. **Explicit `--namespace` flag or `namespace` field** — caller-supplied value; always wins
2. **Auto-detected from `cwd`** — when `context.auto_detect` is `true` in settings, the directory tree is walked upward from `cwd`:
   - First search every ancestor for `.clio-namespace`; the nearest valid file supplies the full namespace string (e.g. `project:clio`). An explicit repository-level file therefore overrides nested package manifests.
   - If no `.clio-namespace` exists, use the nearest `.git` marker and derive `project:<slugified-dir-name>`.
   - If neither exists, use the nearest `Cargo.toml` or `package.json` and derive `project:<slugified-dir-name>`.
3. **`global`** — final fallback when no explicit namespace and no project marker found

Auto-detection is controlled by `context.auto_detect` in `clio-settings.json` (default `true`). When `false`, step 2 is skipped.

Namespace slugs are lowercased; characters that are not alphanumeric, `-`, or `_` are replaced with `-`; leading/trailing hyphens are stripped.

Rules:
- Namespaces must not be empty; `global` is always the final fallback
- Code must not silently rewrite a caller-supplied namespace
- Explicit `--namespace` always overrides auto-detection

### Scoped Recall

When recall uses an auto-detected (non-global) namespace and no explicit `namespace` filter was given, `recall_scoped()` applies a two-pass strategy:

1. Search within the detected project namespace with full query/kind/tag/pagination constraints
2. If results are fewer than `limit`, fill remaining slots by searching `global` (with the same filters, reduced limit)
3. Merge: project-scoped items appear first; same-`id` duplicates are dropped
4. Return a single `RecallResult` with merged `items` and combined `total`

When the caller provides an explicit `namespace`, normal single-namespace recall is used — no fallback to `global`.
When auto-detection falls back to `global`, default recall is global-only; callers must use the explicit global/all-namespaces flag for an unscoped search.

### Namespace Init

`clio init --namespace project:foo` writes a `.clio-namespace` file in the current directory containing the namespace string. Future invocations from that directory (or any subdirectory) will auto-detect this namespace.

## Workflows

### Remember (Write)

1. Validate input (content required, namespace/kind not empty, tags deduplicated/lowercased, metadata is object, importance 1–5, confidence null or 0.0–1.0)
2. If `upsert: true` and both `source` and `source_ref` are present, use the database unique constraint to insert or update atomically
3. On conflict: update in place, preserve `id` and `created_at`, replace tags, refresh `updated_at`
4. If there is no conflict, or the write is not an upsert, insert a row with a UUIDv7 id
5. Sync `tags_text` with `memory_tags`
6. Return stored memory record

### Recall (Search)

1. If `query` present: FTS5 search with BM25 ranking (weights: title 4.0, summary 2.0, content 1.0, tags 0.5)
2. If `query` absent: recent records by `updated_at DESC`
2a. When `ScoringConfig` is active (`decay_lambda > 0.0`), results are ranked by a composite score: BM25 relevance × time decay × access frequency boost × importance factor
    - Time decay: `exp(-λ × days_since_last_activity)` where activity = `last_accessed_at` or `updated_at`
    - Access boost: `1 + min(0.5, weight × ln(1 + access_count))` — logarithmic, capped at 1.5×
    - Importance factor: `importance / 3.0` — importance-3 is neutral (1.0×)
    - When scoring is disabled (`decay_lambda = 0.0`), the original ranking is preserved
3. Apply filters: namespace, kind, tags (match-all or match-any)
4. Exclude archived by default (unless `include_archived: true`)
5. Apply pagination (`limit`, `offset`)

### Archive

1. Set `archived_at` to current UTC timestamp
2. Record excluded from default recall
3. Record still queryable when explicitly requested
4. Idempotent — archiving an already-archived record is a no-op

### Link

1. Verify both source and target memories exist
2. Create typed directional edge
3. Idempotent — creating an existing link is a no-op

### Capture (LLM Classification)

The capture pipeline accepts unstructured text and produces a structured memory via LLM classification.

1. Verify `capture.enabled` in settings — abort with configuration error if not enabled
2. Resolve API key from `capture.api_key` in settings or `OPENAI_API_KEY` env var
3. POST to `capture.base_url/chat/completions` with the classification system prompt and the raw text as the user message; temperature 0.1
4. Extract `choices[0].message.content` from the API response
5. Parse the JSON response through `parse_classification()`:
   - `kind`: must be one of `note`, `fact`, `decision`, `summary`, `task`, `observation`, `constraint` — any other value falls back to `note`
   - `title`: truncated to 240 characters; missing defaults to `"Untitled"`
   - `summary`: truncated to 1000 characters; missing defaults to `""`
   - `tags`: lowercased, spaces replaced with hyphens, up to 5 tags, max 60 chars each
   - `namespace`: used as-is if valid; missing defaults to `"global"`
   - `importance`: clamped to 1–5; missing defaults to `3`
   - `confidence`: clamped to 0.0–1.0; missing defaults to `0.5`
   - Markdown fences in the LLM response are stripped before JSON parsing
6. If `namespace` was explicitly supplied by the caller, it overrides the LLM's suggestion
7. If confidence is below `capture.review_threshold`, return `CaptureResult::Queued` and preserve the capture source, `source_ref`, metadata, and suggested fields in `review_queue`
8. Otherwise store via the normal remember path and return `CaptureResult::Stored`
9. On approval, promote the queued item atomically, preserving its source, `source_ref`, metadata, and suggested fields; auto-embed when enabled

**Classification invariants:**
- Unknown kinds must be normalised to `note`, never stored as-is
- Tags must be normalised exactly as in the remember path (lowercase, hyphenated, deduplicated)
- Single-item captures use `source = "capture"`; distilled captures retain their caller-supplied source and per-item `source_ref`
- Capture returns an explicit `Stored` or `Queued` outcome; a queued outcome is not yet a memory
- The raw input text is always stored as `content` unchanged

### Session Checkpoints (Exact-Once Capture)

A checkpoint commits the distillation of one session delta exactly once, keyed by `(source, session_id, cursor)`.

1. Preflight: a completed key returns its stored result envelope (`replayed: true`) without calling the model
2. Model extraction runs strictly outside the write transaction
3. One `BEGIN IMMEDIATE` transaction rechecks the key under the write lock, stores every extracted memory or review item through the shared capture path, and inserts the checkpoint record — all atomically
4. Any failure rolls back every write; the key stays open and the client job stays retryable
5. An empty extraction is a successful checkpoint and is never redistilled
6. Auto-embedding runs best-effort after commit and can never fail the checkpoint

**Open-loop extraction (within a checkpoint):**

- Distilled atoms may carry optional `attention` data (explicitness, owner, due/remind, trigger, waiting-on, completion condition) and an optional `resolves` identifier
- Only `explicitness: "explicit"` — a clear user commitment or requested reminder — opens attention automatically, atomically with the stored memory; unknown explicitness values are normalised to `suggested`
- Suggested/inferred open loops always queue for review regardless of confidence; approving the review item creates the memory and its attention in one transaction (the attention payload rides in the review item's metadata)
- "Could", "might" and assistant suggestions are never explicit; completed routine steps and externally tracked work are not open loops
- A `resolves` identifier auto-completes its target only when it is a stable Clio reference to an open/snoozed attention item and the transcript is explicit; the storing memory becomes the `resolved_by` evidence. Unknown, fuzzy or terminal targets leave state unchanged — a failed resolution never fails the checkpoint
- Branch and ticket context is applied deterministically after model parsing: `ticket:<id>` tags (lowercase) and branch metadata come from the checkpoint request, never from the model
- Immediate explicit capture through `memory_remember`/`memory_action` remains the primary path; checkpoint extraction is the asynchronous safety net

**Checkpoint invariants:**
- A capture attempt ends in exactly one visible state: durable success (checkpoint row), pending retry (no row) or a surfaced error — never silent loss
- A repeated or concurrent delivery of the same key replays the original result; no duplicate atoms
- Per-atom provenance is `source` + `source_ref = {session_id}@{cursor}-{index}`, preserving the `UNIQUE(source, source_ref)` upsert rule
- Checkpoint rows are the replay proof: never deleted to tidy up
- Checkpoint idempotency is additional to, and must not weaken, source/source-ref upsert idempotency

### Attention Lifecycle (Open Loops)

An attention item marks one memory as operationally open. Memories record what was said; attention records what is still owed.

- One optional attention row per memory (`UNIQUE(memory_id)`); creation is idempotent and commits atomically with its `attention_opened` event
- Statuses are only `open`, `snoozed`, `resolved`, `cancelled`; invalid transitions are validation errors, not no-ops; `resolved` and `cancelled` are terminal
- Completion never deletes or rewrites the source memory — it records a `resolved` event and, when evidence is supplied, a `resolved_by` link from the followed-up memory to the evidence memory
- Eligibility is a pure read returning a machine-readable reason per item: `overdue` (past `due_at`), `reminder_due` (past `remind_at`, including expired snoozes), `project_session` (the `project-session` trigger in the item's own namespace), `dormant` (untouched for `attention.dormant_days`)
- An item surfaces at most once per session/topic scope for the same reason and state — the `surfaced` event's idempotency key includes the item's `updated_at`, so any state change re-arms surfacing
- Automatic surfacing records events but never touches `access_count`/`last_accessed_at` ranking data
- Existing `task`-kind memories remain valid without attention rows
- Adapters (CLI `clio action`, MCP `memory_action`) are thin: every rule above lives in `clio-core::attention`

### Graph Recall Safety

- Linked recall (`include_links`) follows edges in both directions; each connected memory appears once, carrying every edge context (`from`, `to`, `direction` relative to the anchor, `relationship`, `metadata`)
- Linked targets reapply the parent query's archive/expiry eligibility — hidden records cannot re-enter recall through graph expansion
- Link suggestions are candidates only: they default to the source memory's own namespace and never include archived or expired targets; cross-project relations are review-only
- `same_as`, contradiction, supersession/reversal and every cross-project relation require user acceptance; background auto-linking persists only `auto:relates_to`
- The relationship vocabulary is open but documented in `docs/reference/schema.md`; a classifier may propose a typed relation but can never accept one

### Resume Briefs (Automatic Surfacing)

One core policy (`assembly::build_resume_brief`) decides what surfaces when work resumes; adapters and hooks only request it.

- Section priority: needs-attention (eligible open work), waiting-on, active constraints (project first, plus at most two global as a modest prior), recent decisions, query-relevant knowledge, recent activity
- Every item carries a `reason`; attention-backed reasons are machine-readable (`overdue`, `reminder_due`, `project_session`, `dormant`) rendered with their evidence
- The knowledge section abstains without a query; empty sections are omitted and release capacity; non-empty critical sections keep one reserved slot at small budgets
- Items deduplicate across sections by memory ID; archived, expired, resolved and cancelled records are excluded
- All resume reads are untracked (`skip_access_tracking`): automatic delivery must never train recall ranking; deliberate recall stays tracked
- With a session scope, included attention items record one idempotent `surfaced` event and are suppressed on repeats in that scope until their state changes; event-write failure still returns the brief
- The character budget counts the serialised representation (title + bounded content + reason), keeping at least one item

### Migration (Cross-Tool Import)

The migration pipeline imports memories from other AI tools (Claude, ChatGPT) into Clio.

**Input format handling:**

Claude (`clio migrate claude <file>`):
1. If the input is a JSON array of strings → each string is one entry
2. If the input is a JSON array of objects → extract `content`, `text`, or `memory` field from each object
3. Otherwise → treat each non-empty line as one entry

ChatGPT (`clio migrate chatgpt <file>`):
1. If the input is a JSON array of objects → extract `content`, `memory`, `text`, or `value` field from each object
2. If the input is a JSON array of strings → each string is one entry
3. If the input is a single JSON object with a `memories`, `model_spec_memories`, or `data` key → recurse into that array using rules 1–2
4. Otherwise → treat each non-empty line as one entry

**Defaults per source:**

| Source | `source` field | Default namespace | Default `kind` |
|---|---|---|---|
| Claude | `"claude"` | `tool:claude` | `note` |
| ChatGPT | `"chatgpt"` | `tool:chatgpt` | `fact` |

**Deduplication strategy:**

- Every entry is stored with `upsert: true`
- `source_ref` is a deterministic 16-hex-character hash of `(source, content)` — identical content from the same source always produces the same hash
- Re-running an import against the same file updates existing records in place; no duplicates are created
- Empty entries are skipped and counted in `skipped`

**Namespace precedence:**

1. `--namespace` flag from the caller (highest priority)
2. Source default (`tool:claude` or `tool:chatgpt`)

**`--classify` option:**

When `--classify` is passed and the `capture` feature is compiled in with capture enabled in settings, each entry is routed through `capture::classify()` before storage. The LLM's namespace, kind, title, summary, tags, importance, and confidence replace the defaults. If classification fails for an individual entry, it falls back to the source defaults and the entry is still imported.

**`--dry-run` option:**

When `--dry-run` is passed, entries are parsed and `source_ref` hashes are computed but nothing is written to the database. A preview list is returned showing `source`, `source_ref`, `namespace`, `kind`, `title`, and a 120-character content preview.

**Migration invariants:**
- `source` is always `"claude"` or `"chatgpt"` — never overwritten by the caller
- `source_ref` is always a content hash — never a human-supplied value
- All entries use `upsert: true` — migration is always idempotent
- Auto-embedding runs on each stored entry when `auto_embed` is enabled
- Migration is CLI-only; there is no MCP tool for migration (human-initiated operation)

### Stats and Analytics

The stats module (`stats.rs`) computes aggregate views from the memories table without side effects.

`memory_stats(conn, namespace)`:
- when `namespace` is provided, `total_memories`, `active_memories`, `archived_memories`, and `total_embeddings` are all scoped to that namespace
- `by_namespace` and `by_kind` breakdowns are always global (not scoped), to give full picture
- `by_week` uses ISO week format (`strftime('%Y-W%W', created_at)`), covers up to 52 weeks ordered newest first
- `top_tags` returns the 20 most frequent tags across `memory_tags`, ordered by count descending
- `embedding_coverage` = `(total_embeddings / total_memories) * 100`, or `0.0` when no memories exist
- `link_density` = `total_links / total_memories`, or `0.0` when no memories exist

`recent_activity(conn, namespace, limit)`:
- events are classified by comparing timestamp fields on each memory row:
  - `archived_at IS NOT NULL` → action `"archived"`, timestamp = `archived_at`
  - `updated_at > created_at` and `archived_at IS NULL` → action `"updated"`, timestamp = `updated_at`
  - otherwise → action `"created"`, timestamp = `created_at`
- ordered by `COALESCE(archived_at, updated_at) DESC`
- one row per memory — not a true event log; each memory contributes exactly one event

**Stats invariants:**
- stats functions are read-only; they must not modify any row
- `archived_memories` = `total_memories - active_memories` (derived, not a separate query)

### Knowledge Graph

The graph features support traversal and discovery of relationships between memories.

`get_neighbours(conn, memory_id, depth)`:
- performs a breadth-first traversal of the `memory_links` graph, starting at `memory_id`
- follows links bidirectionally: both `to_memory_id` (outgoing) and `from_memory_id` (incoming)
- uses a visited set to prevent cycles and duplicate results
- `depth` controls the maximum number of hops from the root
- returns all discovered memories excluding the root itself
- errors with `NotFound` if the starting memory does not exist
- missing linked memories (due to orphaned links) are silently skipped

`suggest_links(conn, memory_id, backend, threshold, limit)`:
- retrieves the stored embedding for `memory_id` only when its model and dimensions match the active backend, otherwise generates and stores a replacement
- builds an exclusion set: the target memory itself, plus all memories already linked in either direction (via `memory_links` WHERE `from_memory_id = memory_id` UNION WHERE `to_memory_id = memory_id`)
- scans non-archived memories whose embedding model and dimensions match the active backend, computing cosine similarity
- returns candidates with similarity ≥ `threshold`, sorted by similarity descending, up to `limit`
- orphaned linked memories (those in the exclusion set but since deleted) are handled gracefully

**Graph invariants:**
- link graph traversal must be bidirectional — following only outgoing links is incorrect
- `suggest_links` must exclude both directions of existing links, not just outgoing
- `suggest_links` must exclude the target memory itself from candidates
- Embeddings from different model spaces must never be compared

### Export

- JSONL format, one memory record per line
- Includes all fields plus resolved tags array
- Suitable for backup, migration, test fixtures, future sync

### Access Tracking

Memories record `last_accessed_at` and `access_count` when read via `get()`, `recall()`, or `semantic_recall()`.

- **Throttled:** updates are skipped if the memory was accessed within the last 60 seconds
- **Fire-and-forget:** access tracking failures log a warning but do not fail the parent operation

### Auto-Link Inference

The daemon runs a periodic background task that finds semantically similar memories and creates links automatically.

**Configuration via `AutoLinkConfig`:**

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | — | whether auto-linking is active |
| `threshold` | f64 | 0.80 | minimum cosine similarity to create a link |
| `interval_secs` | u64 | 3600 | seconds between inference passes |
| `max_links_per_memory` | usize | 3 | maximum auto-links created per memory per pass |
| `batch_size` | usize | 50 | memories processed per pass |

- Links are created with relationship `auto:relates_to`
- **Watermark-based:** processes memories updated since the last pass
- Generates embeddings for memories that lack one in the active model and dimensionality

**Auto-intelligence invariants:**
- Access tracking must be fire-and-forget — never fail the parent operation
- Auto-links must use the `auto:` prefix to distinguish from human-created links
- Scoring must be backwards-compatible — `decay_lambda = 0.0` preserves original ranking

## Timestamps

All timestamps stored as ISO-8601 UTC strings: `2026-03-02T19:15:00Z`

No local time zone offsets unless there is a strong, consistent reason.

## Full-Text Search

FTS5 virtual table linked to `memories` via content-sync triggers.

Indexed fields: `title`, `summary`, `content`, `tags_text`

Tokeniser: `porter unicode61`

Ranking: BM25 with weighting preference title > summary > content > tags.

## Ticket and receipt conventions

- **Ticket tag** — memories created while working a tracked issue carry the tag
  `ticket:<issue-id>` (lowercase, e.g. `ticket:cad-42`). Tags are FTS-indexed,
  so a handoff brief for "CAD-42" finds tagged memories even when the content
  does not mention the id.
- **Receipt kind** — a `receipt` is a short record of one working session: what
  was done, what was left undone, and why it stopped. Emitted by the session
  stop hooks via `clio distill` (importance 2, tagged `receipt`). Receipts are
  activity, not knowledge: they surface in the handoff preset's "Recent
  Receipts" section and in recent-activity listings.
- **Handoff preset** — `memory_context` / `clio brief` preset `handoff`
  requires a query (ticket id or topic) and assembles: Directly Relevant (FTS),
  Active Constraints, Recent Receipts. Designed to be pasted into a ticket so
  another agent or human can pick the work up with its context attached.
