use crate::error::{ClioError, Result};
use crate::models::RememberInput;

/// Maximum content size (1 MiB).
const MAX_CONTENT_BYTES: usize = 1_048_576;

/// Maximum number of tags per memory.
const MAX_TAGS: usize = 50;

/// Maximum serialised metadata size (64 KiB).
const MAX_METADATA_BYTES: usize = 65_536;

/// Validate a remember input before persisting.
pub fn remember_input(input: &RememberInput) -> Result<()> {
    if input.content.is_empty() {
        return Err(ClioError::Validation("content is required.".into()));
    }

    if input.content.len() > MAX_CONTENT_BYTES {
        return Err(ClioError::Validation(
            "content must not exceed 1 MiB.".into(),
        ));
    }

    if input.namespace.is_empty() {
        return Err(ClioError::Validation(
            "namespace must not be empty.".into(),
        ));
    }

    if input.namespace.len() > 120 {
        return Err(ClioError::Validation(
            "namespace must be at most 120 characters.".into(),
        ));
    }

    if input.kind.is_empty() {
        return Err(ClioError::Validation("kind must not be empty.".into()));
    }

    if input.kind.len() > 50 {
        return Err(ClioError::Validation(
            "kind must be at most 50 characters.".into(),
        ));
    }

    if let Some(ref title) = input.title {
        if title.len() > 240 {
            return Err(ClioError::Validation(
                "title must be at most 240 characters.".into(),
            ));
        }
    }

    if let Some(ref summary) = input.summary {
        if summary.len() > 1000 {
            return Err(ClioError::Validation(
                "summary must be at most 1000 characters.".into(),
            ));
        }
    }

    if !(1..=5).contains(&input.importance) {
        return Err(ClioError::Validation(
            "importance must be between 1 and 5.".into(),
        ));
    }

    if let Some(confidence) = input.confidence {
        if !(0.0..=1.0).contains(&confidence) {
            return Err(ClioError::Validation(
                "confidence must be between 0.0 and 1.0.".into(),
            ));
        }
    }

    if !input.metadata.is_object() {
        return Err(ClioError::Validation(
            "metadata must be a JSON object.".into(),
        ));
    }

    // Reject oversized metadata.
    if let Ok(serialised) = serde_json::to_string(&input.metadata) {
        if serialised.len() > MAX_METADATA_BYTES {
            return Err(ClioError::Validation(
                "metadata must not exceed 64 KiB when serialised.".into(),
            ));
        }
    }

    if input.tags.len() > MAX_TAGS {
        return Err(ClioError::Validation(
            format!("at most {MAX_TAGS} tags are allowed."),
        ));
    }

    for tag in &input.tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() || trimmed.len() > 60 {
            return Err(ClioError::Validation(
                "each tag must be between 1 and 60 characters.".into(),
            ));
        }
    }

    Ok(())
}
