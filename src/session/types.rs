use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::ProviderKind;

pub type SessionId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub id: SessionId,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub region: Option<String>,
    pub fallback_providers: Vec<ProviderKind>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

impl SessionState {
    pub fn new(
        provider: ProviderKind,
        model: Option<String>,
        region: Option<String>,
        fallback_providers: Vec<ProviderKind>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            provider,
            model,
            region,
            fallback_providers,
            created_at: now,
            last_active: now,
        }
    }

    /// Create a session with an explicit ID (e.g. captured from Claude Code).
    pub fn with_id(
        id: String,
        provider: ProviderKind,
        model: Option<String>,
        region: Option<String>,
        fallback_providers: Vec<ProviderKind>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            provider,
            model,
            region,
            fallback_providers,
            created_at: now,
            last_active: now,
        }
    }

    pub fn touch(&mut self) {
        self.last_active = Utc::now();
    }

    pub fn short_id(&self) -> &str {
        &self.id[..8]
    }
}
