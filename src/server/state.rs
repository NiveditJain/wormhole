use dashmap::DashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::config::WormholeConfig;
use crate::provider::Provider;
use crate::session::SessionId;

pub struct AppState {
    /// Session ID -> Provider mapping
    pub sessions: DashMap<SessionId, Arc<dyn Provider>>,

    /// Default provider for requests without a session header
    pub default_provider: Option<Arc<dyn Provider>>,

    /// Configuration
    pub config: WormholeConfig,

    /// Cancellation token for graceful shutdown
    #[allow(dead_code)]
    pub shutdown: CancellationToken,
}

impl AppState {
    pub fn new(config: WormholeConfig, shutdown: CancellationToken) -> Self {
        Self {
            sessions: DashMap::new(),
            default_provider: None,
            config,
            shutdown,
        }
    }

    pub fn with_default_provider(mut self, provider: Arc<dyn Provider>) -> Self {
        self.default_provider = Some(provider);
        self
    }

    pub fn add_session(&self, id: SessionId, provider: Arc<dyn Provider>) {
        self.sessions.insert(id, provider);
    }

    pub fn get_provider(&self, session_id: Option<&str>) -> Option<Arc<dyn Provider>> {
        if let Some(id) = session_id {
            self.sessions.get(id).map(|entry| entry.value().clone())
        } else {
            self.default_provider.clone()
        }
    }

    pub fn remove_session(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.sessions.remove(id).map(|(_, v)| v)
    }
}
