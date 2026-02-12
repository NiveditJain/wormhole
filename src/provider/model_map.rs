use crate::types::ProviderKind;

/// Translate an Anthropic model ID to the provider-specific model ID.
pub fn translate_model_id(provider: ProviderKind, model: &str) -> String {
    match provider {
        ProviderKind::Anthropic => model.to_string(),
        ProviderKind::Bedrock => anthropic_to_bedrock(model),
        ProviderKind::Vertex => anthropic_to_vertex(model),
        ProviderKind::Foundry => model.to_string(), // Foundry uses deployment names, keep as-is
    }
}

fn anthropic_to_bedrock(model: &str) -> String {
    // If it already looks like a Bedrock model ID, pass through
    if model.contains("anthropic.") {
        return model.to_string();
    }

    // Standard mapping: claude-X-Y -> global.anthropic.claude-X-Y-v1
    let base = match model {
        "claude-opus-4-6" => "global.anthropic.claude-opus-4-6-v1",
        "claude-sonnet-4-5-20250929" => "global.anthropic.claude-sonnet-4-5-v1",
        "claude-haiku-4-5-20251001" => "global.anthropic.claude-haiku-4-5-v1",
        "claude-3-5-sonnet-20241022" => "global.anthropic.claude-3-5-sonnet-20241022-v2",
        "claude-3-5-haiku-20241022" => "global.anthropic.claude-3-5-haiku-20241022-v1",
        "claude-3-opus-20240229" => "global.anthropic.claude-3-opus-20240229-v1",
        "claude-3-sonnet-20240229" => "global.anthropic.claude-3-sonnet-20240229-v1",
        "claude-3-haiku-20240307" => "global.anthropic.claude-3-haiku-20240307-v1",
        // Fallback: attempt common pattern
        other => {
            return format!("global.anthropic.{}-v1", other);
        }
    };
    base.to_string()
}

fn anthropic_to_vertex(model: &str) -> String {
    // Vertex mostly uses the same model IDs, but some need a date suffix
    // If it already has a publisher prefix, pass through
    if model.contains('/') {
        return model.to_string();
    }
    model.to_string()
}

/// Returns a list of (model_id, display_name) tuples for the given provider.
/// Model IDs are in the canonical Anthropic format; callers should use
/// `translate_model_id` when sending requests to non-Anthropic providers.
pub fn available_models(provider: ProviderKind) -> Vec<(&'static str, &'static str)> {
    match provider {
        ProviderKind::Anthropic | ProviderKind::Bedrock | ProviderKind::Vertex => vec![
            ("claude-opus-4-6", "Claude Opus 4.6"),
            ("claude-sonnet-4-5-20250929", "Claude Sonnet 4.5"),
            ("claude-haiku-4-5-20251001", "Claude Haiku 4.5"),
            ("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet"),
            ("claude-3-5-haiku-20241022", "Claude 3.5 Haiku"),
        ],
        ProviderKind::Foundry => vec![
            ("claude-opus-4-6", "Claude Opus 4.6"),
            ("claude-sonnet-4-5-20250929", "Claude Sonnet 4.5"),
            ("claude-haiku-4-5-20251001", "Claude Haiku 4.5"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bedrock_model_mapping() {
        assert_eq!(
            translate_model_id(ProviderKind::Bedrock, "claude-opus-4-6"),
            "global.anthropic.claude-opus-4-6-v1"
        );
        assert_eq!(
            translate_model_id(ProviderKind::Bedrock, "claude-sonnet-4-5-20250929"),
            "global.anthropic.claude-sonnet-4-5-v1"
        );
        // Pass-through for already-formatted IDs
        assert_eq!(
            translate_model_id(ProviderKind::Bedrock, "global.anthropic.claude-3-opus-20240229-v1"),
            "global.anthropic.claude-3-opus-20240229-v1"
        );
    }

    #[test]
    fn test_anthropic_passthrough() {
        assert_eq!(
            translate_model_id(ProviderKind::Anthropic, "claude-opus-4-6"),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn test_vertex_passthrough() {
        assert_eq!(
            translate_model_id(ProviderKind::Vertex, "claude-opus-4-6"),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn test_bedrock_unknown_model_fallback() {
        assert_eq!(
            translate_model_id(ProviderKind::Bedrock, "claude-future-model"),
            "global.anthropic.claude-future-model-v1"
        );
    }
}
