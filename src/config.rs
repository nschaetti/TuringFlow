use std::error::Error;
use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
}

#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    pub model: String,
    pub temperature: Option<f64>,
}

pub fn load_config(path: impl AsRef<Path>) -> Result<Config, Box<dyn Error>> {
    let raw = fs::read_to_string(path)?;
    let config = serde_yaml::from_str(&raw)?;
    Ok(config)
}
