//! Runtime configuration schemas for daemon and kingdoms.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use serde::Deserialize;

/// Main daemon configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DaemonConfig {
    /// Schema version.
    pub version: u32,
    /// Server section.
    pub server: ServerConfig,
    /// TLS section.
    pub tls: TlsConfig,
    /// Security section.
    pub security: SecurityConfig,
    /// Routing section.
    pub routing: RoutingConfig,
    /// Storage section.
    pub storage: StorageConfig,
    /// Limits section.
    pub limits: LimitsConfig,
    /// Logging section.
    pub logging: LoggingConfig,
}

impl DaemonConfig {
    /// Loads and validates daemon config from YAML.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let raw = fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }

    /// Parses `server.listen` into a socket address.
    pub fn listen_addr(&self) -> Result<SocketAddr, Box<dyn Error>> {
        Ok(self.server.listen.parse()?)
    }

    /// Validates daemon configuration invariants.
    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        if self.version != 1 {
            return Err("config version must be 1".into());
        }
        if self.server.node_id.trim().is_empty() {
            return Err("server.node_id must not be empty".into());
        }
        self.server
            .listen
            .parse::<SocketAddr>()
            .map_err(|_| "server.listen must be a valid socket address")?;

        ensure_file_path("tls.server_cert", &self.tls.server_cert)?;
        ensure_file_path("tls.server_key", &self.tls.server_key)?;
        ensure_file_path("tls.client_ca_cert", &self.tls.client_ca_cert)?;

        if let Some(path) = &self.tls.upstream_ca_cert {
            ensure_file_path("tls.upstream_ca_cert", path)?;
        }
        if let Some(path) = &self.tls.upstream_client_cert {
            ensure_file_path("tls.upstream_client_cert", path)?;
        }
        if let Some(path) = &self.tls.upstream_client_key {
            ensure_file_path("tls.upstream_client_key", path)?;
        }

        if self.security.replay_window_seconds <= 0 {
            return Err("security.replay_window_seconds must be > 0".into());
        }

        if self.routing.retry_delays_ms.is_empty() {
            return Err("routing.retry_delays_ms must not be empty".into());
        }

        if self
            .routing
            .retry_delays_ms
            .windows(2)
            .any(|window| window[0] > window[1])
        {
            return Err("routing.retry_delays_ms must be sorted ascending".into());
        }

        if self.storage.backend != "sqlite" {
            return Err("storage.backend must be 'sqlite'".into());
        }
        if self.storage.sqlite.path.trim().is_empty() {
            return Err("storage.sqlite.path must not be empty".into());
        }

        if self.limits.max_payload_bytes == 0 {
            return Err("limits.max_payload_bytes must be > 0".into());
        }
        if self.limits.max_message_ttl_ms == 0 {
            return Err("limits.max_message_ttl_ms must be > 0".into());
        }

        self.logging.validate()?;

        Ok(())
    }
}

/// Server configuration section.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Socket address to bind (`host:port`).
    pub listen: String,
    /// Node identifier.
    pub node_id: String,
}

/// TLS configuration section.
#[derive(Debug, Clone, Deserialize)]
pub struct TlsConfig {
    /// Server certificate path.
    pub server_cert: String,
    /// Server private key path.
    pub server_key: String,
    /// Client CA certificate path.
    pub client_ca_cert: String,
    /// Optional upstream CA path.
    pub upstream_ca_cert: Option<String>,
    /// Optional upstream client certificate path.
    pub upstream_client_cert: Option<String>,
    /// Optional upstream client key path.
    pub upstream_client_key: Option<String>,
}

/// Security settings.
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    /// Allowed replay window in seconds.
    pub replay_window_seconds: i64,
}

/// Router settings.
#[derive(Debug, Clone, Deserialize)]
pub struct RoutingConfig {
    /// Retry delays in milliseconds.
    pub retry_delays_ms: Vec<u64>,
}

/// Storage settings.
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    /// Storage backend (`sqlite`).
    pub backend: String,
    /// SQLite-specific settings.
    pub sqlite: SqliteConfig,
}

/// SQLite storage section.
#[derive(Debug, Clone, Deserialize)]
pub struct SqliteConfig {
    /// Database file path.
    pub path: String,
}

/// Message and payload limits.
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    /// Maximum payload bytes.
    pub max_payload_bytes: usize,
    /// Maximum message TTL in milliseconds.
    pub max_message_ttl_ms: u64,
}

/// Logging settings.
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Log format (`json` or `plain`).
    pub format: String,
    /// Log level name.
    pub level: String,
}

impl LoggingConfig {
    fn validate(&self) -> Result<(), Box<dyn Error>> {
        match self.format.as_str() {
            "json" | "plain" => {}
            _ => return Err("logging.format must be either 'json' or 'plain'".into()),
        }
        match self.level.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            _ => {
                return Err("logging.level must be one of trace|debug|info|warn|error".into());
            }
        }
        Ok(())
    }

    /// Converts configured level name to `tracing::Level`.
    pub fn level(&self) -> tracing::Level {
        match self.level.as_str() {
            "trace" => tracing::Level::TRACE,
            "debug" => tracing::Level::DEBUG,
            "warn" => tracing::Level::WARN,
            "error" => tracing::Level::ERROR,
            _ => tracing::Level::INFO,
        }
    }
}

/// Kingdom allowlist and quotas configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct KingdomsConfig {
    /// Schema version.
    pub version: u32,
    /// Kingdom entries.
    pub kingdoms: Vec<KingdomEntry>,
}

impl KingdomsConfig {
    /// Loads and validates kingdoms config from YAML.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let raw = fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }

    /// Validates kingdoms invariants.
    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        if self.version != 1 {
            return Err("kingdoms config version must be 1".into());
        }
        if self.kingdoms.is_empty() {
            return Err("kingdoms must not be empty".into());
        }

        let mut seen = HashSet::new();
        for kingdom in &self.kingdoms {
            if kingdom.id.trim().is_empty() {
                return Err("kingdom id must not be empty".into());
            }
            if !seen.insert(kingdom.id.clone()) {
                return Err(format!("duplicate kingdom id '{}': not allowed", kingdom.id).into());
            }
            kingdom.quotas.validate(&kingdom.id)?;
        }

        Ok(())
    }

    /// Returns quotas for an enabled kingdom.
    pub fn quotas_for(&self, kingdom_id: &str) -> Option<&KingdomQuotas> {
        self.kingdoms
            .iter()
            .find(|kingdom| kingdom.id == kingdom_id && kingdom.enabled)
            .map(|kingdom| &kingdom.quotas)
    }

    /// Returns whether a kingdom is enabled.
    pub fn is_allowed(&self, kingdom_id: &str) -> bool {
        self.quotas_for(kingdom_id).is_some()
    }

    /// Returns map of allowed kingdom ids to quotas.
    pub fn allowed_kingdoms(&self) -> HashMap<String, KingdomQuotas> {
        self.kingdoms
            .iter()
            .filter(|kingdom| kingdom.enabled)
            .map(|kingdom| (kingdom.id.clone(), kingdom.quotas.clone()))
            .collect()
    }
}

/// One kingdom entry in the allowlist.
#[derive(Debug, Clone, Deserialize)]
pub struct KingdomEntry {
    /// Kingdom identifier.
    pub id: String,
    /// Whether this kingdom is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Quotas applied to this kingdom.
    pub quotas: KingdomQuotas,
}

fn default_enabled() -> bool {
    true
}

/// Resource quotas applied per kingdom.
#[derive(Debug, Clone, Deserialize)]
pub struct KingdomQuotas {
    /// Maximum concurrent agents per node.
    pub max_agents_per_node: usize,
    /// Maximum lease TTL for registration.
    pub max_lease_ttl_ms: u64,
    /// Maximum message TTL accepted.
    pub max_message_ttl_ms: u64,
    /// Maximum payload size accepted.
    pub max_payload_bytes: usize,
}

impl KingdomQuotas {
    fn validate(&self, kingdom_id: &str) -> Result<(), Box<dyn Error>> {
        if self.max_agents_per_node == 0 {
            return Err(
                format!("kingdoms.{kingdom_id}.quotas.max_agents_per_node must be > 0").into(),
            );
        }
        if self.max_lease_ttl_ms == 0 {
            return Err(
                format!("kingdoms.{kingdom_id}.quotas.max_lease_ttl_ms must be > 0").into(),
            );
        }
        if self.max_message_ttl_ms == 0 {
            return Err(
                format!("kingdoms.{kingdom_id}.quotas.max_message_ttl_ms must be > 0").into(),
            );
        }
        if self.max_payload_bytes == 0 {
            return Err(
                format!("kingdoms.{kingdom_id}.quotas.max_payload_bytes must be > 0").into(),
            );
        }
        Ok(())
    }
}

fn ensure_file_path(field: &str, value: &str) -> Result<(), Box<dyn Error>> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty").into());
    }
    if !Path::new(value).exists() {
        return Err(format!("{field} path does not exist: {value}").into());
    }
    Ok(())
}
