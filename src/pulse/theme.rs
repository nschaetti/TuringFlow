use std::error::Error;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeColors {
    pub foreground: String,
    pub background: String,
    pub primary: String,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            foreground: "white".to_string(),
            background: "black".to_string(),
            primary: "cyan".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetTheme {
    pub textbox_prefix: String,
    pub textbox_suffix: String,
}

impl Default for WidgetTheme {
    fn default() -> Self {
        Self {
            textbox_prefix: "[ ".to_string(),
            textbox_suffix: " ]".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Theme {
    #[serde(default)]
    pub colors: ThemeColors,
    #[serde(default)]
    pub widgets: WidgetTheme,
}

impl Theme {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let raw = fs::read_to_string(path)?;
        let theme = serde_yaml::from_str(&raw)?;
        Ok(theme)
    }
}
