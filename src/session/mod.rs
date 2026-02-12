pub mod types;

use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use tracing::debug;

use crate::config::WormholeConfig;
use crate::error::SessionError;
pub use types::{SessionId, SessionState};

pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    pub fn new() -> Result<Self> {
        let dir = WormholeConfig::sessions_dir();
        fs::create_dir_all(&dir).context("Failed to create sessions directory")?;
        Ok(Self { dir })
    }

    pub fn create(&self, session: &SessionState) -> Result<()> {
        let path = self.session_path(&session.id);
        let json = serde_json::to_string_pretty(session)?;
        fs::write(&path, json)?;
        debug!("Created session {} at {}", session.short_id(), path.display());
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<SessionState, SessionError> {
        // Try exact match first
        let path = self.session_path(id);
        if path.exists() {
            let json = fs::read_to_string(&path).map_err(SessionError::Storage)?;
            let session: SessionState =
                serde_json::from_str(&json).map_err(SessionError::Serialization)?;
            return Ok(session);
        }

        // Try prefix match
        self.find_by_prefix(id)
    }

    pub fn touch(&self, id: &str) -> Result<(), SessionError> {
        let mut session = self.get(id)?;
        session.touch();
        let path = self.session_path(&session.id);
        let json = serde_json::to_string_pretty(&session).map_err(SessionError::Serialization)?;
        fs::write(&path, json).map_err(SessionError::Storage)?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<SessionState>> {
        let mut sessions = Vec::new();
        if !self.dir.exists() {
            return Ok(sessions);
        }

        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                match fs::read_to_string(&path) {
                    Ok(json) => match serde_json::from_str::<SessionState>(&json) {
                        Ok(session) => sessions.push(session),
                        Err(e) => {
                            debug!("Failed to parse session file {}: {}", path.display(), e);
                        }
                    },
                    Err(e) => {
                        debug!("Failed to read session file {}: {}", path.display(), e);
                    }
                }
            }
        }

        sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        Ok(sessions)
    }

    pub fn delete(&self, id: &str) -> Result<(), SessionError> {
        // Resolve full ID via prefix match if needed
        let session = self.get(id)?;
        let path = self.session_path(&session.id);
        fs::remove_file(&path).map_err(SessionError::Storage)?;
        debug!("Deleted session {}", session.short_id());
        Ok(())
    }

    pub fn most_recent(&self) -> Result<Option<SessionState>> {
        let sessions = self.list()?;
        Ok(sessions.into_iter().next())
    }

    pub fn cleanup_old(&self, max_age_days: i64) -> Result<usize> {
        let cutoff = Utc::now() - chrono::Duration::days(max_age_days);
        let sessions = self.list()?;
        let mut deleted = 0;

        for session in sessions {
            if session.last_active < cutoff {
                let path = self.session_path(&session.id);
                if fs::remove_file(&path).is_ok() {
                    deleted += 1;
                    debug!("Cleaned up old session {}", session.short_id());
                }
            }
        }

        Ok(deleted)
    }

    fn session_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", id))
    }

    fn find_by_prefix(&self, prefix: &str) -> Result<SessionState, SessionError> {
        let sessions = self.list().map_err(|e| {
            SessionError::Storage(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;

        let matches: Vec<_> = sessions
            .into_iter()
            .filter(|s| s.id.starts_with(prefix))
            .collect();

        match matches.len() {
            0 => Err(SessionError::NotFound(format!(
                "No session found matching prefix '{}'",
                prefix
            ))),
            1 => Ok(matches.into_iter().next().unwrap()),
            n => Err(SessionError::NotFound(format!(
                "Ambiguous prefix '{}': matches {} sessions. Use a longer prefix.",
                prefix, n
            ))),
        }
    }
}
