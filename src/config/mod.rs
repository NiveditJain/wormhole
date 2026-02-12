pub mod credentials;
pub mod types;

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use tracing::debug;

pub use types::*;

pub fn load_config() -> WormholeConfig {
    let config_path = WormholeConfig::config_file_path();

    let mut figment = Figment::from(Serialized::defaults(WormholeConfig::default()));

    if config_path.exists() {
        debug!("Loading config from {}", config_path.display());
        figment = figment.merge(Toml::file(&config_path));
    }

    figment = figment.merge(Env::prefixed("WORMHOLE_").split("_"));

    match figment.extract::<WormholeConfig>() {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("Failed to parse config, using defaults: {}", e);
            WormholeConfig::default()
        }
    }
}
