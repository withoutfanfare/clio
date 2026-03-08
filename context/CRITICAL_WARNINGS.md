# Critical Warnings

Rules that, if violated, produce incorrect behaviour. Read before writing any code.

## Business Logic Belongs in the Core Only

All persistence logic, validation, search ranking, upsert semantics, and archive behaviour live in `clio-core`. CLI and MCP are thin adapters.

**The implementation is incorrect if:**
- CLI or MCP open ad hoc SQL queries not represented in the core
- CLI and MCP implement different validation rules
- CLI, MCP, and future Tauri write different row shapes

## Archive Means Hidden, Not Deleted

- `archived_at` is a soft-delete marker (UTC timestamp)
- Archived records MUST be excluded from default search and recent queries
- Archived records MUST remain queryable when explicitly requested
- Phase one does not expose hard delete in normal workflows

**The implementation is incorrect if:**
- Archived records appear in default recall

## Tags and FTS Must Stay in Sync

- `tags_text` on the `memories` table is derived from `memory_tags`
- FTS5 triggers keep the virtual table synchronised on insert/update/delete
- Any tag write must update both `memory_tags` and `tags_text`

**The implementation is incorrect if:**
- Tags and `tags_text` drift permanently out of sync

## Upsert Must Not Create Duplicates

- Upsert is keyed on `source + source_ref` (both must be present)
- If a match exists: update in place, preserve original `id` and `created_at`, replace tags, refresh `updated_at`
- If no match: insert a new row
- If only one of `source` or `source_ref` is present: do not attempt upsert, treat as normal insert

**The implementation is incorrect if:**
- Upsert creates duplicates when `source + source_ref` should match

## MCP Server Must Not Bypass the Core

- Every MCP tool calls the Rust core rather than raw SQL
- MCP defaults must match CLI/core semantics exactly
- MCP field names must match the documented contract

**The implementation is incorrect if:**
- MCP bypasses the Rust core for storage logic
- MCP defaults differ from CLI/core semantics
- MCP returns field names that do not match the contract

## Error Messages Must Be Actionable

- Errors are concise, specific, and recoverable
- No generic exception dumps
- Categories: validation, not found, conflict, storage, configuration

**The implementation is incorrect if:**
- Error messages are opaque enough that an agent cannot recover

## Security Baseline

- Never log full memory contents at info level
- Do not expose network listeners by default
- Use filesystem paths under the current user only
- Keep export/import explicit — no surprise sync behaviour

## Migration Safety

- Every schema change ships as a new migration
- Never edit an already-applied migration
- Prefer additive changes
- Document schema changes in `docs/reference/schema.md` before implementation

## Access Tracking Must Be Fire-and-Forget

- `touch_accessed()` updates `last_accessed_at` and `access_count` when memories are read
- Access tracking failures MUST log a warning and continue — never fail the parent operation
- The 60-second throttle prevents hot memories from generating excessive writes

**The implementation is incorrect if:**
- An access tracking failure causes `get()`, `recall()`, or `semantic_recall()` to return an error

## Auto-Links Must Be Distinguished From Human Links

- Links created by the auto-link inference task use relationship `auto:relates_to`
- The `auto:` prefix is the only distinction between machine-created and human-created links
- `suggest_links()` already excludes existing links in both directions, so auto-links are idempotent

**The implementation is incorrect if:**
- Auto-created links use a relationship without the `auto:` prefix
- Auto-link inference creates duplicate links between the same memories

## Temporal Scoring Must Be Backwards-Compatible

- When `decay_lambda == 0.0` in `ScoringConfig`, the original ranking (`rank ASC, updated_at DESC` for FTS, `updated_at DESC` for recent) MUST be preserved
- Default settings enable scoring (`decay_lambda = 0.01`)
- Existing settings files without `scoring` config load cleanly via `#[serde(default)]`

**The implementation is incorrect if:**
- Omitting `scoring` from `clio-settings.json` changes recall behaviour from previous versions

## Python Archive Is Read-Only

The `archive/python-prototype/` directory is reference material only. Do not extend or modify it unless explicitly requested.
