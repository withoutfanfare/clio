#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatModelCapabilities {
    Classic,
    ReasoningNone,
    ReasoningDefault,
    Conservative,
}

fn chat_model_capabilities(model: &str) -> ChatModelCapabilities {
    if is_model_or_variant(model, "gpt-5.6-luna") || is_model_or_variant(model, "gpt-5.6-terra") {
        ChatModelCapabilities::ReasoningNone
    } else if is_model_or_variant(model, "gpt-5") {
        ChatModelCapabilities::ReasoningDefault
    } else if is_model_or_variant(model, "gpt-4.1") || is_model_or_variant(model, "gpt-4o") {
        ChatModelCapabilities::Classic
    } else {
        ChatModelCapabilities::Conservative
    }
}

fn is_model_or_variant(model: &str, family: &str) -> bool {
    model == family
        || model
            .strip_prefix(family)
            .is_some_and(|suffix| suffix.starts_with('-') || suffix.starts_with('.'))
}

pub(crate) fn apply_chat_parameters(
    body: &mut serde_json::Value,
    model: &str,
    temperature: f64,
    max_output_tokens: Option<u64>,
) {
    match chat_model_capabilities(model) {
        ChatModelCapabilities::Classic => {
            body["temperature"] = serde_json::json!(temperature);
            if let Some(limit) = max_output_tokens {
                body["max_tokens"] = serde_json::json!(limit);
            }
        }
        ChatModelCapabilities::ReasoningNone => {
            body["reasoning_effort"] = serde_json::json!("none");
            if let Some(limit) = max_output_tokens {
                body["max_completion_tokens"] = serde_json::json!(limit);
            }
        }
        ChatModelCapabilities::ReasoningDefault => {
            if let Some(limit) = max_output_tokens {
                body["max_completion_tokens"] = serde_json::json!(limit);
            }
        }
        ChatModelCapabilities::Conservative => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_only_parameters_known_to_be_supported() {
        let mut classic = serde_json::json!({});
        apply_chat_parameters(&mut classic, "gpt-4.1", 0.1, Some(60));
        assert_eq!(classic["temperature"], 0.1);
        assert_eq!(classic["max_tokens"], 60);
        assert!(classic.get("reasoning_effort").is_none());

        let mut terra = serde_json::json!({});
        apply_chat_parameters(&mut terra, "gpt-5.6-terra", 0.1, Some(60));
        assert_eq!(terra["reasoning_effort"], "none");
        assert_eq!(terra["max_completion_tokens"], 60);
        assert!(terra.get("temperature").is_none());

        let mut terra_snapshot = serde_json::json!({});
        apply_chat_parameters(
            &mut terra_snapshot,
            "gpt-5.6-terra-2026-07-01",
            0.1,
            Some(60),
        );
        assert_eq!(terra_snapshot["reasoning_effort"], "none");
        assert_eq!(terra_snapshot["max_completion_tokens"], 60);

        let mut gpt5_pro = serde_json::json!({});
        apply_chat_parameters(&mut gpt5_pro, "gpt-5-pro", 0.1, Some(60));
        assert_eq!(gpt5_pro["max_completion_tokens"], 60);
        assert!(gpt5_pro.get("reasoning_effort").is_none());
        assert!(gpt5_pro.get("temperature").is_none());

        let mut classic_snapshot = serde_json::json!({});
        apply_chat_parameters(&mut classic_snapshot, "gpt-4.1-2025-04-14", 0.1, Some(60));
        assert_eq!(classic_snapshot["temperature"], 0.1);
        assert_eq!(classic_snapshot["max_tokens"], 60);

        let mut unknown = serde_json::json!({});
        apply_chat_parameters(&mut unknown, "compatible-provider/new-model", 0.1, Some(60));
        assert_eq!(unknown, serde_json::json!({}));
    }
}
