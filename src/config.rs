use std::error::Error;

use serde::Deserialize;

/// Minimal application configuration.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// LLM defaults.
    pub llm: LlmConfig,
}

/// LLM runtime defaults.
#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    /// Model identifier.
    pub model: String,
    /// Optional generation temperature.
    pub temperature: Option<f64>,
}

/// Parses a YAML configuration document.
pub fn load_config_from_str(raw: &str) -> Result<Config, Box<dyn Error>> {
    let config = serde_yaml::from_str(&raw)?;
    Ok(config)
}
