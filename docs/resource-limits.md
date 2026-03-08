# Resource Limits

All numeric limits, sizing constraints, and operational thresholds in Clio. Values are from source code and cannot be overridden unless noted.

---

## SQLite Pragmas

Applied on every database connection in `crates/clio-core/src/db.rs`. These are unconditional and cannot be changed via settings.

| Pragma | Value | Notes |
|---|---|---|
| `journal_mode` | `WAL` | Write-ahead logging for concurrent reads |
| `foreign_keys` | `ON` | Referential integrity enforced at the database layer |
| `busy_timeout` | `5000` ms | Waits up to 5 seconds before returning a locked-database error |
| `synchronous` | `NORMAL` | Balanced durability; safe with WAL mode |
| `temp_store` | `MEMORY` | Temporary tables and indices held in RAM |

---

## Field Validation

Enforced in `crates/clio-core/src/validate.rs` and `crates/clio-core/src/repository.rs` before any write reaches the database.

### Memory Fields

| Field | Constraint | Details |
|---|---|---|
| `content` | Max 1 MiB | 1,048,576 bytes; required, must not be empty |
| `metadata` | Max 64 KiB serialised | 65,536 bytes as JSON string |
| `title` | Max 240 characters | Optional |
| `summary` | Max 1,000 characters | Optional |
| `kind` | Max 50 characters | Required, must not be empty |
| `namespace` | Max 120 characters | Required, must not be empty |
| `importance` | 1–5 (integer) | Required; defaults to 3 in most surfaces |
| `confidence` | 0.0–1.0 (float) | Optional |

### Tags

| Constraint | Value |
|---|---|
| Maximum tags per memory | 50 |
| Minimum tag length | 1 character (after trimming) |
| Maximum tag length | 60 characters (after trimming) |

### Links

| Field | Constraint |
|---|---|
| `relationship` | 1–60 characters; required, must not be empty |

---

## Embedding Sizing

Configured in `crates/clio-core/src/embeddings.rs`. The active backend determines vector dimensions; all dimensions are stored as 32-bit floats (4 bytes each).

### Dimensions by Model

| Backend | Model | Dimensions | Storage per Memory |
|---|---|---|---|
| Local (fastembed) | `all-MiniLM-L6-v2` | 384 | 1,536 bytes |
| Local (fastembed) | `all-MiniLM-L12-v2` | 384 | 1,536 bytes |
| Local (fastembed) | `bge-small-en-v1.5` | 384 | 1,536 bytes |
| OpenAI | `text-embedding-3-small` | 1,536 | 6,144 bytes |
| OpenAI | `text-embedding-ada-002` | 1,536 | 6,144 bytes |
| OpenAI | `text-embedding-3-large` | 3,072 | 12,288 bytes |

### Storage Projections at Scale

| Backend | Model | 10,000 Memories |
|---|---|---|
| Local | `all-MiniLM-L6-v2` | ~15 MB |
| OpenAI | `text-embedding-3-small` | ~60 MB |
| OpenAI | `text-embedding-3-large` | ~120 MB |

Figures cover the `memory_embeddings` BLOB column only; they exclude index overhead and other table data.

---

## MCP Limits

Defined in `crates/clio-mcp/src/main.rs`. Applied to all query tools (`memory_recall`, `memory_search`, `memory_list`).

| Limit | Value | Notes |
|---|---|---|
| `MAX_LIMIT` | 500 | Hard cap; all caller-supplied limits are silently clamped to this value |
| Default limit | 10 | Used when the caller omits the `limit` parameter |

---

## Daemon Limits

### Inbox Watcher

Defined in `crates/clio-daemon/src/watcher.rs`.

| Limit | Value | Notes |
|---|---|---|
| Maximum inbox file size | 10 MiB (10,485,760 bytes) | Files exceeding this are moved to `_processed/` without being stored |

### Auto-Link Inference

Defaults defined in `crates/clio-core/src/settings.rs`. All four values are configurable via the `daemon.auto_link` settings block.

| Setting | Default | Config Key |
|---|---|---|
| Batch size | 50 memories per pass | `daemon.auto_link.batch_size` |
| Interval | 3,600 seconds (1 hour) | `daemon.auto_link.interval_secs` |
| Max links per memory per pass | 3 | `daemon.auto_link.max_links_per_memory` |
| Similarity threshold | 0.80 | `daemon.auto_link.threshold` |

Auto-link inference is **disabled by default** (`daemon.auto_link.enabled = false`).

---

## Operational Limits

Defined in `crates/clio-core/src/repository.rs`. These are hard-coded and cannot be changed via settings.

| Behaviour | Limit | Source |
|---|---|---|
| Graph traversal depth | Max 5 hops | `get_neighbours()` clamps any caller-supplied depth with `.min(5)` |
| Access tracking throttle | 60 seconds | A repeated read of the same memory within 60 seconds does not increment `access_count` or update `last_accessed_at` |
| Access tracking batch size | 500 IDs per SQL statement | `touch_accessed()` chunks ID lists to avoid oversized parameter lists |

---

## Related Documentation

- [Schema Reference](reference/schema.md) — database table definitions
- [Settings Reference](reference/settings.md) — all configuration keys
