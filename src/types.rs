use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Anthropic,
    Bedrock,
    Vertex,
    Foundry,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::Anthropic => write!(f, "anthropic"),
            ProviderKind::Bedrock => write!(f, "bedrock"),
            ProviderKind::Vertex => write!(f, "vertex"),
            ProviderKind::Foundry => write!(f, "foundry"),
        }
    }
}

impl ProviderKind {
    pub fn all() -> &'static [ProviderKind] {
        &[
            ProviderKind::Anthropic,
            ProviderKind::Bedrock,
            ProviderKind::Vertex,
            ProviderKind::Foundry,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "Anthropic Direct",
            ProviderKind::Bedrock => "AWS Bedrock",
            ProviderKind::Vertex => "Google Vertex AI",
            ProviderKind::Foundry => "Azure Foundry",
        }
    }
}
