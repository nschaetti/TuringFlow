use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonConfig {
    pub version: u32,
    pub server: ServerConfig,
    pub tls: TlsConfig,
    pub security: SecurityConfig,
    pub routing: RoutingConfig,
    pub storage: StorageConfig,
    pub limits: LimitsConfig,
    pub logging: LoggingConfig,
}

impl DaemonConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let raw = fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }

    pub fn listen_addr(&self) -> Result<SocketAddr, Box<dyn Error>> {
        Ok(self.server.listen.parse()?)
    }

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

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub listen: String,
    pub node_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TlsConfig {
    pub server_cert: String,
    pub server_key: String,
    pub client_ca_cert: String,
    pub upstream_ca_cert: Option<String>,
    pub upstream_client_cert: Option<String>,
    pub upstream_client_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub replay_window_seconds: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutingConfig {
    pub retry_delays_ms: Vec<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub backend: String,
    pub sqlite: SqliteConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SqliteConfig {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    pub max_payload_bytes: usize,
    pub max_message_ttl_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub format: String,
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

#[derive(Debug, Clone, Deserialize)]
pub struct KingdomsConfig {
    pub version: u32,
    pub kingdoms: Vec<KingdomEntry>,
}

impl KingdomsConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let raw = fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }

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

    pub fn quotas_for(&self, kingdom_id: &str) -> Option<&KingdomQuotas> {
        self.kingdoms
            .iter()
            .find(|kingdom| kingdom.id == kingdom_id && kingdom.enabled)
            .map(|kingdom| &kingdom.quotas)
    }

    pub fn is_allowed(&self, kingdom_id: &str) -> bool {
        self.quotas_for(kingdom_id).is_some()
    }

    pub fn allowed_kingdoms(&self) -> HashMap<String, KingdomQuotas> {
        self.kingdoms
            .iter()
            .filter(|kingdom| kingdom.enabled)
            .map(|kingdom| (kingdom.id.clone(), kingdom.quotas.clone()))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct KingdomEntry {
    pub id: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub quotas: KingdomQuotas,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct KingdomQuotas {
    pub max_agents_per_node: usize,
    pub max_lease_ttl_ms: u64,
    pub max_message_ttl_ms: u64,
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
