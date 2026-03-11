# AI Title Generation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate meaningful memory titles via an OpenAI-compatible LLM when no title is supplied, with fallback to the existing string-based generator.

**Architecture:** New `title.rs` module in clio-core handles the LLM call behind the `capture` feature gate (reuses reqwest/tokio). A `resolve_title()` function encapsulates the decision logic: use explicit title > try AI > fall back to string extraction. Settings gain an `AutoTitleConfig` section that inherits from `CaptureConfig` when fields are omitted.

**Tech Stack:** Rust, reqwest, tokio, OpenAI-compatible chat completions API, serde_json.

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/clio-core/src/title.rs` | Create | LLM title generation prompt, API call, `resolve_title()` |
| `crates/clio-core/src/settings.rs` | Modify | Add `AutoTitleConfig` struct, wire into `Settings` |
| `crates/clio-core/src/lib.rs` | Modify | Add `pub mod title` |
| `crates/clio-core/src/repository.rs` | Modify | Call `resolve_title()` instead of inline `generate_title()` |
| `crates/clio-mcp/src/main.rs` | Modify | Pass settings to cache.remember() |
| `crates/clio-cli/src/main.rs` | Modify | Pass settings to repository::remember() |
| `crates/clio-tauri/src/commands/memory.rs` | Modify | Pass settings to cache.remember() |
| `crates/clio-core/src/cache.rs` | Modify | Accept settings in remember()/update() |

---

## Task 1: Add AutoTitleConfig to settings

**Files:**
- Modify: `crates/clio-core/src/settings.rs:155-198`

- [ ] **Step 1: Add AutoTitleConfig struct**

Add after `ContextConfig` (line 153):

```rust
/// Configuration for AI-powered automatic title generation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoTitleConfig {
    /// Whether AI title generation is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// API key. Falls back to capture.api_key if None.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Base URL for the API. Falls back to capture.base_url if None.
    #[serde(default)]
    pub base_url: Option<String>,

    /// Model to use. Falls back to capture.model if None.
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for AutoTitleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            base_url: None,
            model: None,
        }
    }
}
```

- [ ] **Step 2: Wire AutoTitleConfig into Settings struct**

Add field to `Settings` (after `capture`):

```rust
    /// Automatic title generation configuration.
    #[serde(default)]
    pub auto_title: AutoTitleConfig,
```

Add to `Default` impl:

```rust
    auto_title: AutoTitleConfig::default(),
```

- [ ] **Step 3: Add resolved_* helper methods on Settings**

Add an impl block on `Settings` to resolve auto_title fields with capture fallback:

```rust
impl Settings {
    /// Resolve the API key for auto-title, falling back to capture config.
    pub fn auto_title_api_key(&self) -> Option<String> {
        self.auto_title.api_key.clone()
            .or_else(|| self.capture.api_key.clone())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
    }

    /// Resolve the base URL for auto-title, falling back to capture config.
    pub fn auto_title_base_url(&self) -> String {
        self.auto_title.base_url.clone()
            .unwrap_or_else(|| self.capture.base_url.clone())
    }

    /// Resolve the model for auto-title, falling back to capture config.
    pub fn auto_title_model(&self) -> String {
        self.auto_title.model.clone()
            .unwrap_or_else(|| self.capture.model.clone())
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build -p clio-core`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add crates/clio-core/src/settings.rs
git commit -m "feat: add AutoTitleConfig to settings with capture fallback"
```

---

## Task 2: Create title.rs module

**Files:**
- Create: `crates/clio-core/src/title.rs`
- Modify: `crates/clio-core/src/lib.rs:1-18`

- [ ] **Step 1: Write tests for the string-based generate_title (moved from repository.rs)**

Create `crates/clio-core/src/title.rs` with tests first:

```rust
//! Title generation for memories.
//!
//! Provides two strategies:
//! - **String-based** (`generate_title`): extracts the first line of content,
//!   strips markdown markers, truncates on word boundaries. Always available.
//! - **AI-powered** (`generate_title_ai`): calls an OpenAI-compatible LLM for
//!   a meaningful title. Requires the `capture` feature and configured settings.

use crate::settings::Settings;

/// Maximum length of an auto-generated title (in characters).
const AUTO_TITLE_MAX_LEN: usize = 80;

/// Derive a short title from the memory content using string extraction.
///
/// Takes the first non-empty line, strips leading punctuation/whitespace,
/// and truncates on a word boundary at [`AUTO_TITLE_MAX_LEN`] characters.
pub fn generate_title(content: &str) -> String {
    let first_line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or(content.trim());

    let cleaned = first_line.trim_start_matches(|c: char| {
        c == '#' || c == '-' || c == '*' || c == '>' || c.is_whitespace()
    });

    if cleaned.len() <= AUTO_TITLE_MAX_LEN {
        return cleaned.to_string();
    }

    let truncated = &cleaned[..AUTO_TITLE_MAX_LEN];
    match truncated.rfind(' ') {
        Some(pos) => format!("{}…", &truncated[..pos]),
        None => format!("{truncated}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_title_from_plain_text() {
        assert_eq!(generate_title("Hello world"), "Hello world");
    }

    #[test]
    fn generate_title_strips_markdown_heading() {
        assert_eq!(generate_title("## My Heading\nBody text"), "My Heading");
    }

    #[test]
    fn generate_title_strips_bullet() {
        assert_eq!(generate_title("- A list item"), "A list item");
    }

    #[test]
    fn generate_title_skips_empty_lines() {
        assert_eq!(generate_title("\n\n  \nActual content"), "Actual content");
    }

    #[test]
    fn generate_title_truncates_long_content() {
        let long = "a ".repeat(100); // 200 chars
        let title = generate_title(&long);
        assert!(title.len() <= AUTO_TITLE_MAX_LEN + "…".len());
        assert!(title.ends_with('…'));
    }

    #[test]
    fn generate_title_empty_content() {
        assert_eq!(generate_title(""), "");
    }
}
```

- [ ] **Step 2: Add pub mod title to lib.rs**

Add after `pub mod stats;` (line 17 of lib.rs):

```rust
pub mod title;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p clio-core title`
Expected: all 6 tests pass

- [ ] **Step 4: Add the AI title generation prompt and function**

Add to `title.rs` after `generate_title`:

```rust
const TITLE_SYSTEM_PROMPT: &str = r#"You generate concise, descriptive titles for knowledge base entries. Given the content of a memory, respond with ONLY a short title (max 80 characters). No quotes, no punctuation at the end, no explanation — just the title text."#;

/// Generate a title using an OpenAI-compatible LLM.
///
/// Returns `None` if the call fails for any reason (network, auth, parsing).
#[cfg(feature = "capture")]
pub fn generate_title_ai(content: &str, settings: &Settings) -> Option<String> {
    if !settings.auto_title.enabled {
        return None;
    }

    let api_key = settings.auto_title_api_key()?;
    let base_url = settings.auto_title_base_url();
    let model = settings.auto_title_model();

    match get_or_create_runtime().block_on(generate_title_ai_async(content, &api_key, &base_url, &model)) {
        Ok(title) => Some(title),
        Err(e) => {
            tracing::warn!("AI title generation failed, falling back to string-based: {e}");
            None
        }
    }
}

#[cfg(feature = "capture")]
fn get_or_create_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create title generation async runtime")
    })
}

#[cfg(feature = "capture")]
async fn generate_title_ai_async(
    content: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> crate::error::Result<String> {
    use crate::error::ClioError;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    // Limit content sent to the LLM to avoid excessive token usage.
    let truncated_content: String = content.chars().take(2000).collect();

    let body = serde_json::json!({
        "model": model,
        "temperature": 0.3,
        "max_tokens": 60,
        "messages": [
            { "role": "system", "content": TITLE_SYSTEM_PROMPT },
            { "role": "user", "content": truncated_content }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ClioError::Storage(format!("title API request failed: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(ClioError::Storage(format!("title API returned {status}")));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| ClioError::Storage(format!("title API response parse error: {e}")))?;

    let raw_title = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| ClioError::Storage("title API: missing content in response".into()))?;

    // Clean up: trim whitespace, remove surrounding quotes, enforce max length.
    let cleaned = raw_title
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .chars()
        .take(240)
        .collect::<String>();

    if cleaned.is_empty() {
        return Err(ClioError::Storage("title API returned empty title".into()));
    }

    Ok(cleaned)
}

/// Stub when capture feature is not enabled.
#[cfg(not(feature = "capture"))]
pub fn generate_title_ai(_content: &str, _settings: &Settings) -> Option<String> {
    None
}
```

- [ ] **Step 5: Add resolve_title function**

Add to `title.rs`:

```rust
/// Resolve the title for a memory.
///
/// Priority: explicit title > AI-generated > string-based extraction.
pub fn resolve_title(explicit_title: Option<String>, content: &str, settings: &Settings) -> Option<String> {
    if explicit_title.is_some() {
        return explicit_title;
    }

    if let Some(ai_title) = generate_title_ai(content, settings) {
        return Some(ai_title);
    }

    Some(generate_title(content))
}
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p clio-core`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add crates/clio-core/src/title.rs crates/clio-core/src/lib.rs
git commit -m "feat: add title module with AI and string-based generation"
```

---

## Task 3: Wire resolve_title into repository.rs

**Files:**
- Modify: `crates/clio-core/src/repository.rs:1-170`

- [ ] **Step 1: Update remember() signature to accept Settings**

Change the function signature at line 44:

```rust
pub fn remember(conn: &Connection, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
```

- [ ] **Step 2: Replace inline title generation with resolve_title**

Replace the title generation block (lines 52-56) with:

```rust
    let title = crate::title::resolve_title(input.title.clone(), &input.content, settings);
```

- [ ] **Step 3: Remove generate_title function and constant from repository.rs**

Delete lines 11-38 (`AUTO_TITLE_MAX_LEN` constant and `generate_title` function) — these now live in `title.rs`.

- [ ] **Step 4: Update update_existing() to accept Settings**

Change signature at line 112:

```rust
fn update_existing(
    conn: &Connection,
    id: &str,
    input: &RememberInput,
    tags: &[String],
    tags_text: &str,
    metadata_str: &str,
    now: &str,
    settings: &crate::settings::Settings,
) -> Result<Memory> {
```

Replace inline title generation (lines 122-125) with:

```rust
    let title = crate::title::resolve_title(input.title.clone(), &input.content, settings);
```

- [ ] **Step 5: Update update_existing call sites in remember()**

Pass `settings` to the `update_existing` call (around line 62):

```rust
                return update_existing(conn, &existing_id, input, &tags, &tags_text, &metadata_str, &now, settings);
```

- [ ] **Step 6: Update the public update() function**

Change signature at line 229:

```rust
pub fn update(conn: &Connection, id: &str, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
```

Pass settings to `update_existing` call:

```rust
    update_existing(conn, id, input, &tags, &tags_text, &metadata_str, &now, settings)
```

- [ ] **Step 7: Fix internal callers in clio-core**

Update `crate::repository::remember` calls in:
- `crates/clio-core/src/capture.rs:335` — pass `settings` (already available in scope)
- `crates/clio-core/src/review.rs:199` — need to check if settings is in scope; if not, create default

Check and fix `crate::repository::update` calls in test helpers if any.

- [ ] **Step 8: Update cache.rs wrappers**

Modify `crates/clio-core/src/cache.rs`:

```rust
    pub fn remember(&self, conn: &Connection, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
        let memory = repository::remember(conn, input, settings)?;
        // ... rest unchanged
    }

    pub fn update(&self, conn: &Connection, id: &str, input: &RememberInput, settings: &crate::settings::Settings) -> Result<Memory> {
        let memory = repository::update(conn, id, input, settings)?;
        // ... rest unchanged
    }
```

- [ ] **Step 9: Verify compilation**

Run: `cargo build -p clio-core`
Expected: compiles (downstream crates will fail until next task)

- [ ] **Step 10: Commit**

```bash
git add crates/clio-core/src/repository.rs crates/clio-core/src/cache.rs crates/clio-core/src/capture.rs crates/clio-core/src/review.rs
git commit -m "feat: wire resolve_title into remember and update paths"
```

---

## Task 4: Update CLI, MCP, and Tauri callers

**Files:**
- Modify: `crates/clio-cli/src/main.rs:914`
- Modify: `crates/clio-mcp/src/main.rs:1009`
- Modify: `crates/clio-tauri/src/commands/memory.rs:48`

- [ ] **Step 1: Update CLI remember call**

At `crates/clio-cli/src/main.rs:914`, settings is already loaded as `s`. Change:

```rust
    let memory = repository::remember(&conn, &input, &s)?;
```

- [ ] **Step 2: Update CLI update call (if exists)**

Search for `repository::update` in CLI and add settings parameter.

- [ ] **Step 3: Update MCP remember call**

At `crates/clio-mcp/src/main.rs:1009`, settings is already loaded. Change:

```rust
                cache.remember(&conn, &input, &settings).map_err(|e| format_clio_error(&e))?;
```

- [ ] **Step 4: Update MCP update call (if exists)**

Search for `cache.update` in MCP and add settings parameter.

- [ ] **Step 5: Update Tauri remember call**

At `crates/clio-tauri/src/commands/memory.rs:48`, settings is available as `app.settings`. Change:

```rust
    let memory = app.cache.remember(&app.conn, &input, &app.settings)?;
```

- [ ] **Step 6: Update Tauri update call (if exists)**

Search for `cache.update` in Tauri and add settings parameter.

- [ ] **Step 7: Fix any remaining compilation errors**

Run: `cargo build --workspace`
Expected: full workspace compiles

- [ ] **Step 8: Run full test suite**

Run: `cargo test --workspace`
Expected: all tests pass

- [ ] **Step 9: Commit**

```bash
git add crates/clio-cli/src/main.rs crates/clio-mcp/src/main.rs crates/clio-tauri/src/commands/memory.rs
git commit -m "feat: pass settings through to remember/update for AI title generation"
```

---

## Task 5: Build, test, and verify

- [ ] **Step 1: Full workspace build (release)**

Run: `./build.sh`
Expected: all crates build successfully

- [ ] **Step 2: Run all tests**

Run: `cargo test --workspace`
Expected: all tests pass

- [ ] **Step 3: Restart daemon**

Run: `./build.sh restart`

- [ ] **Step 4: Manual smoke test — string-based (default)**

Use the CLI to create a memory without a title and verify a string-based title is generated:

```bash
clio remember --content "We decided to use PostgreSQL for the analytics pipeline because it handles complex queries well"
```

Expected: memory created with auto-generated title like "We decided to use PostgreSQL for the analytics pipeline because it handles"

- [ ] **Step 5: Commit any remaining changes**

```bash
git commit -m "feat: complete AI title generation feature"
```
