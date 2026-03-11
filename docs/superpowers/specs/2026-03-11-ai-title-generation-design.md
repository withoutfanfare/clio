# AI Title Generation

Auto-generate meaningful memory titles using an LLM when no title is supplied.

## Settings

New `AutoTitleConfig` in `Settings`:

```rust
pub struct AutoTitleConfig {
    pub enabled: bool,            // default: false
    pub api_key: Option<String>,  // falls back to capture.api_key
    pub base_url: Option<String>, // falls back to capture.base_url
    pub model: Option<String>,    // falls back to capture.model
}
```

When `enabled: true` but fields are `None`, they inherit from `CaptureConfig`. Minimal config: `"auto_title": { "enabled": true }`.

## LLM Call

Focused prompt requesting a concise title (max 240 chars) for the memory content. Single chat completion, plain text response (not JSON).

## Integration

```text
input.title is Some? -> use it as-is
auto_title enabled?  -> try LLM call
  LLM succeeds?     -> use LLM title
  LLM fails?        -> fall back to generate_title() (string-based)
auto_title disabled? -> fall back to generate_title() (string-based)
```

Applied in both `remember()` and `update_existing()`.

## Architecture

- New `title.rs` module in clio-core, behind the `capture` feature gate (reuses HTTP/tokio machinery).
- `resolve_title(content: &str, settings: &Settings) -> String` keeps LLM detail out of repository.rs.
- `remember()` and `update()` receive settings to access the config.

## Files

- **New:** `crates/clio-core/src/title.rs` — `generate_title_ai()` + prompt
- **Modify:** `crates/clio-core/src/settings.rs` — add `AutoTitleConfig`, wire into `Settings`
- **Modify:** `crates/clio-core/src/repository.rs` — pass settings, call AI title with fallback
- **Modify:** `crates/clio-core/src/lib.rs` — add `pub mod title`
- **Modify:** `crates/clio-mcp/src/main.rs` — pass settings through to `remember()`
- **Modify:** `crates/clio-cli/src/main.rs` — pass settings through to `remember()`

## Future

Synchronous for now. Designed for later migration to async (daemon-based title upgrading).
