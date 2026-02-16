use std::error::Error;

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

pub fn load_config_from_str(raw: &str) -> Result<Config, Box<dyn Error>> {
    let config = serde_yaml::from_str(&raw)?;
    Ok(config)
}
