use std::error::Error;
use std::path::Path;

use crate::pulse::theme::Theme;
use crate::pulse::widget::WidgetNode;

#[derive(Debug, Clone)]
pub struct PulseApp {
    root: WidgetNode,
    theme: Theme,
}

impl PulseApp {
    pub fn new(root: impl Into<WidgetNode>) -> Self {
        Self {
            root: root.into(),
            theme: Theme::default(),
        }
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn with_theme_file(mut self, path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        self.theme = Theme::from_file(path)?;
        Ok(self)
    }

    pub fn render_to_string(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "theme(fg={}, bg={}, primary={})\n",
            self.theme.colors.foreground, self.theme.colors.background, self.theme.colors.primary
        ));
        output.push_str(&self.root.render_lines(&self.theme).join("\n"));
        output
    }

    pub fn run(&self) {
        println!("{}", self.render_to_string());
    }
}
