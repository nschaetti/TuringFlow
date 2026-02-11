use crate::pulse::widget::WidgetNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone)]
pub struct Container {
    orientation: Orientation,
    children: Vec<WidgetNode>,
    spacing: usize,
}

impl Container {
    pub fn hstack() -> Self {
        Self {
            orientation: Orientation::Horizontal,
            children: Vec::new(),
            spacing: 1,
        }
    }

    pub fn vstack() -> Self {
        Self {
            orientation: Orientation::Vertical,
            children: Vec::new(),
            spacing: 0,
        }
    }

    pub fn with_spacing(mut self, spacing: usize) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn push(mut self, child: impl Into<WidgetNode>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    pub fn spacing(&self) -> usize {
        self.spacing
    }

    pub fn children(&self) -> &[WidgetNode] {
        &self.children
    }
}
