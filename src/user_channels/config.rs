//! Channel connector configuration.

use std::error::Error;
use std::fs;
use std::path::Path;

use serde::Deserialize;

/// Top-level channels configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelsConfig {
    /// Schema version.
    pub version: u32,
    /// Channel configuration map.
    pub channels: ChannelsSection,
}

impl ChannelsConfig {
    /// Loads and validates channels configuration from YAML.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let raw = fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), Box<dyn Error>> {
        if self.version != 1 {
            return Err("channels config version must be 1".into());
        }
        self.channels.matrix.validate()?;
        Ok(())
    }
}

/// Connector entries.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelsSection {
    /// Matrix connector configuration.
    pub matrix: MatrixChannelConfig,
}

/// Matrix connector settings.
#[derive(Debug, Clone, Deserialize)]
pub struct MatrixChannelConfig {
    /// Enables background worker spawn.
    pub enabled: bool,
    /// Matrix homeserver base URL.
    pub homeserver: String,
    /// Environment variable name containing access token.
    pub access_token_env: String,
    /// Target room id.
    pub room_id: String,
    /// Poll interval in milliseconds.
    pub poll_interval_ms: u64,
    /// Long poll timeout in milliseconds.
    pub sync_timeout_ms: u64,
}

impl MatrixChannelConfig {
    fn validate(&self) -> Result<(), Box<dyn Error>> {
        if !self.enabled {
            return Ok(());
        }
        if self.homeserver.trim().is_empty() {
            return Err("channels.matrix.homeserver must not be empty".into());
        }
        if self.access_token_env.trim().is_empty() {
            return Err("channels.matrix.access_token_env must not be empty".into());
        }
        if self.room_id.trim().is_empty() {
            return Err("channels.matrix.room_id must not be empty".into());
        }
        if self.poll_interval_ms == 0 {
            return Err("channels.matrix.poll_interval_ms must be > 0".into());
        }
        if self.sync_timeout_ms == 0 {
            return Err("channels.matrix.sync_timeout_ms must be > 0".into());
        }
        Ok(())
    }
}
