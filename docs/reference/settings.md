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
| `api_key` | string? | `null` | API key (falls back to `OPENAI_API_KEY` env var) |
| `model` | string | `"text-embedding-3-small"` | Model name (1,536 dimensions) |
| `base_url` | string? | `null` | Optional base URL override for proxies |

**Disabled**

| Key | Type | — | Description |
|-----|------|---|-------------|
| `provider` | string | `"disabled"` | Turns off all embedding functionality |

## capture

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Whether the capture pipeline is active |
| `api_key` | string? | `null` | OpenAI-compatible API key |
| `base_url` | string | `"https://api.openai.com/v1"` | API endpoint |
| `model` | string | `"gpt-4o-mini"` | Classification model |
| `review_threshold` | float? | `null` | Confidence below this routes to review queue; `null` disables review |

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

### daemon.auto_link

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Whether auto-link inference is active |
| `threshold` | float | `0.80` | Cosine similarity threshold for linking |
| `interval_secs` | int | `3600` | Seconds between inference passes |
| `max_links_per_memory` | int | `3` | Max links created per memory per pass |
| `batch_size` | int | `50` | Memories processed per pass |

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
  }
}
```

---

## Related Documentation

- [Resource Limits](../resource-limits.md) — sizing constraints and thresholds
- [Schema Reference](schema.md) — database table definitions
- [Getting Started](../getting-started.md) — setup walkthrough
