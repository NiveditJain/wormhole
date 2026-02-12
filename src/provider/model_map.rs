use std::borrow::Cow;

use crate::types::ProviderKind;

/// Translate an Anthropic model ID to the provider-specific model ID.
pub fn translate_model_id<'a>(provider: ProviderKind, model: &'a str) -> Cow<'a, str> {
    match provider {
        ProviderKind::Anthropic => Cow::Borrowed(model),
        ProviderKind::Bedrock => anthropic_to_bedrock(model),
        ProviderKind::Vertex => anthropic_to_vertex(model),
        ProviderKind::Foundry => Cow::Borrowed(model), // Foundry uses deployment names, keep as-is
    }
}

fn anthropic_to_bedrock<'a>(model: &'a str) -> Cow<'a, str> {
    // If it already looks like a Bedrock model ID, pass through
    if model.contains("anthropic.") {
        return Cow::Borrowed(model);
    }

    // Standard mapping: claude-X-Y -> global.anthropic.claude-X-Y-v1
    match model {
        "claude-opus-4-6" => Cow::Borrowed("global.anthropic.claude-opus-4-6-v1"),
        "claude-sonnet-4-5-20250929" => Cow::Borrowed("global.anthropic.claude-sonnet-4-5-v1"),
        "claude-haiku-4-5-20251001" => Cow::Borrowed("global.anthropic.claude-haiku-4-5-v1"),
        "claude-3-5-sonnet-20241022" => Cow::Borrowed("global.anthropic.claude-3-5-sonnet-20241022-v2"),
        "claude-3-5-haiku-20241022" => Cow::Borrowed("global.anthropic.claude-3-5-haiku-20241022-v1"),
        "claude-3-opus-20240229" => Cow::Borrowed("global.anthropic.claude-3-opus-20240229-v1"),
        "claude-3-sonnet-20240229" => Cow::Borrowed("global.anthropic.claude-3-sonnet-20240229-v1"),
        "claude-3-haiku-20240307" => Cow::Borrowed("global.anthropic.claude-3-haiku-20240307-v1"),
        // Fallback: attempt common pattern
        other => Cow::Owned(format!("global.anthropic.{}-v1", other)),
    }
}

fn anthropic_to_vertex<'a>(model: &'a str) -> Cow<'a, str> {
    // Vertex mostly uses the same model IDs, but some need a date suffix
    // If it already has a publisher prefix, pass through
    Cow::Borrowed(model)
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
