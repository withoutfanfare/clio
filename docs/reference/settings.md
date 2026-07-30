# Settings Reference

All configuration keys in `clio-settings.json`. The file lives alongside the database (e.g. `~/Library/Application Support/clio/clio-settings.json` on macOS). Missing keys fall back to defaults via `#[serde(default)]`.

## Top-Level Keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `embeddings` | object | `{ "provider": "local", "model": "all-MiniLM-L6-v2" }` | Embedding backend configuration |
| `auto_embed` | bool | `true` | Automatically embed memories on write |
| `capture` | object | see below | LLM-based capture pipeline |
| `context` | object | see below | Namespace auto-detection |
| `scoring` | object | see below | Temporal relevance scoring |
| `daemon` | object | see below | Always-on daemon |
| `attention` | object | see below | Follow-up attention lifecycle policy |
| `remote` | object? | `null` | Optional shared SSH route used by local adapters |

## embeddings

Three variants (tagged by `provider`):

**Local (default)**

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `provider` | string | `"local"` | Backend type |
| `model` | string | `"all-MiniLM-L6-v2"` | ONNX model name (384 dimensions) |

**OpenAI**

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `provider` | string | `"openai"` | Backend type |
| `api_key` | string? | `null` | API key (falls back to the environment — see [API key resolution](#api-key-resolution)) |
| `model` | string | `"text-embedding-3-small"` | Model name (1,536 dimensions) |
| `base_url` | string? | `null` | Optional base URL override for proxies |

**Disabled**

| Key | Type | — | Description |
|-----|------|---|-------------|
| `provider` | string | `"disabled"` | Turns off all embedding functionality |

The MCP server creates its embedding backend at process start. After changing
the provider or model, restart each MCP client, then run:

```sh
clio embed backfill
```

Semantic search and link suggestions compare only embeddings whose model and
dimensions match the active backend. Backfill replaces missing or stale rows;
repeat it with an appropriate `--batch-size` until every memory is refreshed.

## capture

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Whether the capture pipeline is active |
| `api_key` | string? | `null` | OpenAI-compatible API key (falls back to the environment — see [API key resolution](#api-key-resolution)) |
| `base_url` | string | `"https://api.openai.com/v1"` | API endpoint |
| `model` | string | `"gpt-4o-mini"` | Classification model |
| `review_threshold` | float? | `null` | Confidence below this routes to review queue; `null` disables review |

## API key resolution

Every setting that takes an `api_key` resolves it in this order, using the first
value that is present and not blank:

1. The `api_key` in settings.
2. `OPENAI_API_KEY_CLIO` — a key used only by Clio.
3. `OPENAI_API_KEY` — the shared key, **with a warning** on stderr.

Prefer `OPENAI_API_KEY_CLIO`. A key shared with other tools cannot be attributed
in provider billing, so there is no way to tell what Clio itself is costing. Step
3 exists so existing installs keep working; the warning names the caller
(`capture`, `openai embeddings`, `auto-title`) so it is clear which part of Clio
reached for the shared key.

A variable exported as an empty string is treated as absent rather than as a key,
so a blank export falls through to the next step instead of sending an
unauthenticated request.

Change only the model, without replacing the API key or endpoint:

```sh
clio settings set-capture-model gpt-5.6-luna
```

Show the active shared capture configuration without displaying credentials:

```sh
clio settings show-capture
```

On a Mac configured with `settings use-remote`, both commands use Atlas. The
desktop app exposes the same control under **Settings > Capture model** and
preserves the API key, endpoint and review threshold. Its suggested values are
the benchmarked `gpt-4.1`, `gpt-5.6-luna` and `gpt-5.6-terra` models, but the
field accepts another OpenAI-compatible model ID for future comparisons.

The Tauri app and the CLI see the change immediately. Other running MCP
processes reload non-embedding settings within 30 seconds; they do not need a
restart.

Use `capture --model <model> --dry-run` or `distill --model <model> --dry-run`
for a one-off comparison that does not change the active setting. Add
`--metrics` to include latency and provider-reported token usage.

### Desktop control suitability

The desktop app currently changes only `capture.model`. Other suitable future
controls are `capture.review_threshold`, auto-title behaviour, recall scoring,
namespace auto-detection, consolidation thresholds and cleanup defaults. They
are non-secret values with immediate, understandable effects.

Keep API keys and provider endpoints out of the desktop interface. Embedding
provider/model changes require client restarts and a vector backfill; remote
route changes can disconnect the app; daemon settings are local-only and need a
daemon restart. Those settings should remain guided CLI or deployment tasks
unless the app also implements their full validation and recovery workflows.

## context

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto_detect` | bool | `true` | Auto-detect namespace from working directory |

## scoring

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `decay_lambda` | float | `0.01` | Exponential decay rate (0.0 = disabled, 0.01 = 75% at 30 days) |
| `access_boost_weight` | float | `0.1` | Weight for access frequency boost (0.0 = disabled) |

## daemon

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Whether the daemon is active |
| `inbox_paths` | string[] | `[]` | Directories to watch for inbox drop files |
| `socket_path` | string? | platform default | Unix domain socket path |
| `log_dir` | string? | platform default | Rolling log file directory |
| `http_port` | int? | `null` | Optional HTTP loopback API port |
| `auto_link` | object | see below | Auto-link inference settings |
| `maintenance` | object | see below | Periodic backup / integrity jobs |

### daemon.auto_link

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Whether auto-link inference is active |
| `threshold` | float | `0.80` | Cosine similarity threshold for linking |
| `interval_secs` | int | `3600` | Seconds between inference passes |
| `max_links_per_memory` | int | `3` | Maximum inferred links a memory may hold in **total**, not per pass. Bounds links *out of* a memory; recall walks edges both ways, so total degree can exceed it |
| `batch_size` | int | `50` | Memories processed per pass |
| `exclude_kinds` | string[] | `["receipt"]` | Memory kinds skipped as both source and target — see below |

`exclude_kinds` keeps boilerplate out of the link graph. Receipts are per-session
write-ups of what was done; they share a great deal of phrasing, so they attract
each other on similarity while carrying little conceptual content. Measured on live
data at threshold 0.6, receipts averaged 4.86 links each against 2.03 for `fact` —
the most substantive kind was the least connected, and receipts accounted for
roughly a third of all link mass.

An explicit `clio suggest-links` request is unaffected and still considers every
candidate: a person asking for suggestions should not have results withheld.

### daemon.maintenance

Periodic local jobs run by the daemon. Intervals default to `0` (disabled); set
one to opt in. Both jobs are pure-local — no network, no LLM. (Consolidation is
not run here; it is triggered per session by the session-stop hook.)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `backup_interval_secs` | int | `0` | Seconds between database backups (`0` = off). E.g. `604800` for weekly |
| `backup_max_backups` | int | `7` | Timestamped backups to retain |
| `integrity_interval_secs` | int | `0` | Seconds between integrity checks, log-only (`0` = off) |

## remote

Set with `clio settings use-remote`. When present, normal CLI data commands,
session hooks, generated MCP client configurations and Tauri use the remote
database. `clio --local` bypasses the route. The daemon remains local-only.

| Key | Type | Description |
|-----|------|-------------|
| `host` | string | SSH host or alias |
| `db_path` | string | Absolute database path on the remote host |
| `mcp_binary` | string | Absolute `clio-mcp` path on the remote host |
| `cli_binary` | string | Absolute `clio` path on the remote host |
| `bridge_command` | string | Absolute local `clio` path used by MCP and Tauri |

## cleanup

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `stale_months` | int | `6` | A namespace with no activity for this many months is "stale by age" |
| `dev_roots` | string[] | `["~/Development", "~/Projects", "~/Code", "~/dev", "~/src"]` | Roots scanned for the "folder gone" heuristic; `~` expands to `$HOME` |
| `record_cwd` | bool | `true` | Record the working directory in memory metadata at capture time, for reliable future path matching |

## consolidate

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto_threshold` | int | `10` | Consolidate a namespace automatically once it has this many new memories since the last consolidation (used by `clio consolidate --if-due`) |

## attention

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `dormant_days` | int | `14` | Days an open attention item may sit untouched before eligibility reports it as `dormant`. `0` disables dormancy surfacing |

## Example

```json
{
  "embeddings": { "provider": "local", "model": "all-MiniLM-L6-v2" },
  "auto_embed": true,
  "capture": {
    "enabled": true,
    "api_key": "sk-...",
    "base_url": "https://api.openai.com/v1",
    "model": "gpt-4o-mini",
    "review_threshold": 0.7
  },
  "context": { "auto_detect": true },
  "scoring": { "decay_lambda": 0.01, "access_boost_weight": 0.1 },
  "daemon": {
    "enabled": true,
    "inbox_paths": ["~/clio-inbox"],
    "auto_link": {
      "enabled": true,
      "threshold": 0.80,
      "interval_secs": 3600,
      "batch_size": 50
    }
  },
  "remote": null
}
```

---

## Related Documentation

- [Resource Limits](../resource-limits.md) — sizing constraints and thresholds
- [Schema Reference](schema.md) — database table definitions
- [Getting Started](../getting-started.md) — setup walkthrough
