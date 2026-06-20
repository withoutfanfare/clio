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

    match get_or_create_runtime().block_on(generate_title_ai_async(
        content, &api_key, &base_url, &model,
    )) {
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

/// Resolve the title for a memory.
///
/// Priority: explicit title > AI-generated > string-based extraction.
pub fn resolve_title(
    explicit_title: Option<String>,
    content: &str,
    settings: &Settings,
) -> Option<String> {
    if explicit_title.is_some() {
        return explicit_title;
    }

    if let Some(ai_title) = generate_title_ai(content, settings) {
        return Some(ai_title);
    }

    Some(generate_title(content))
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

    #[test]
    fn resolve_title_uses_explicit_when_provided() {
        let settings = Settings::default();
        let result = resolve_title(Some("My Title".into()), "content", &settings);
        assert_eq!(result, Some("My Title".into()));
    }

    #[test]
    fn resolve_title_falls_back_to_string_based() {
        let settings = Settings::default(); // auto_title.enabled = false
        let result = resolve_title(None, "Some content here", &settings);
        assert_eq!(result, Some("Some content here".into()));
    }
}
