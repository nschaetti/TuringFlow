use serde_json::Value;

/// User message payload representation.
#[derive(Debug, Clone)]
pub enum HumanContent {
    /// Plain text content.
    Text(String),
    /// Structured multi-part payload.
    Parts(Vec<Value>),
}

/// User message wrapper.
#[derive(Debug, Clone)]
pub struct HumanMessage {
    /// Message content.
    pub content: HumanContent,
}

impl HumanMessage {
    /// Creates a plain-text human message.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: HumanContent::Text(content.into()),
        }
    }

    /// Creates a multi-part human message.
    pub fn from_parts(parts: Vec<Value>) -> Self {
        Self {
            content: HumanContent::Parts(parts),
        }
    }

    /// Converts the message to JSON wire format.
    pub fn to_json(&self) -> Value {
        match &self.content {
            HumanContent::Text(text) => Value::String(text.clone()),
            HumanContent::Parts(parts) => Value::Array(parts.clone()),
        }
    }
}
