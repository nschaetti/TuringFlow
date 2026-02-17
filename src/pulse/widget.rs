use crate::pulse::layout::Container;
use crate::pulse::theme::Theme;

/// Leaf text widget.
#[derive(Debug, Clone)]
pub struct TextBox {
    text: String,
}

impl TextBox {
    /// Creates a text box.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// Returns inner text.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Polymorphic widget node.
#[derive(Debug, Clone)]
pub enum WidgetNode {
    /// Container node.
    Container(Container),
    /// Text leaf node.
    TextBox(TextBox),
}

impl WidgetNode {
    /// Renders the widget tree into printable lines.
    pub fn render_lines(&self, theme: &Theme) -> Vec<String> {
        match self {
            WidgetNode::TextBox(text_box) => vec![format!(
                "{}{}{}",
                theme.widgets.textbox_prefix,
                text_box.text(),
                theme.widgets.textbox_suffix
            )],
            WidgetNode::Container(container) => {
                let rendered_children = container
                    .children()
                    .iter()
                    .map(|child| child.render_lines(theme))
                    .collect::<Vec<_>>();

                match container.orientation() {
                    crate::pulse::layout::Orientation::Vertical => {
                        let mut lines = Vec::new();
                        for (index, block) in rendered_children.iter().enumerate() {
                            lines.extend(block.clone());
                            if index + 1 < rendered_children.len() {
                                lines.extend(vec![String::new(); container.spacing()]);
                            }
                        }
                        lines
                    }
                    crate::pulse::layout::Orientation::Horizontal => {
                        if rendered_children.is_empty() {
                            return Vec::new();
                        }

                        let mut single_line = String::new();
                        for (index, block) in rendered_children.iter().enumerate() {
                            if index > 0 {
                                single_line.push_str(&" ".repeat(container.spacing()));
                            }
                            if let Some(first_line) = block.first() {
                                single_line.push_str(first_line);
                            }
                        }
                        vec![single_line]
                    }
                }
            }
        }
    }
}

impl From<TextBox> for WidgetNode {
    fn from(value: TextBox) -> Self {
        WidgetNode::TextBox(value)
    }
}

impl From<Container> for WidgetNode {
    fn from(value: Container) -> Self {
        WidgetNode::Container(value)
    }
}
