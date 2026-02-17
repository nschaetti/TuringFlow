use crate::pulse::widget::WidgetNode;

/// Container layout orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Children rendered on a single horizontal line.
    Horizontal,
    /// Children rendered as stacked blocks.
    Vertical,
}

/// UI container node.
#[derive(Debug, Clone)]
pub struct Container {
    orientation: Orientation,
    children: Vec<WidgetNode>,
    spacing: usize,
}

impl Container {
    /// Creates a horizontal container.
    pub fn hstack() -> Self {
        Self {
            orientation: Orientation::Horizontal,
            children: Vec::new(),
            spacing: 1,
        }
    }

    /// Creates a vertical container.
    pub fn vstack() -> Self {
        Self {
            orientation: Orientation::Vertical,
            children: Vec::new(),
            spacing: 0,
        }
    }

    /// Sets spacing between children.
    pub fn with_spacing(mut self, spacing: usize) -> Self {
        self.spacing = spacing;
        self
    }

    /// Appends a child node.
    pub fn push(mut self, child: impl Into<WidgetNode>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Returns orientation.
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// Returns spacing.
    pub fn spacing(&self) -> usize {
        self.spacing
    }

    /// Returns child nodes.
    pub fn children(&self) -> &[WidgetNode] {
        &self.children
    }
}
