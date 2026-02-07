use serde_json::Value;

#[derive(Debug, Clone)]
pub enum HumanContent {
    Text(String),
    Parts(Vec<Value>),
}

#[derive(Debug, Clone)]
pub struct HumanMessage {
    pub content: HumanContent,
}

impl HumanMessage {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: HumanContent::Text(content.into()),
        }
    }

    pub fn from_parts(parts: Vec<Value>) -> Self {
        Self {
            content: HumanContent::Parts(parts),
        }
    }

    pub fn to_json(&self) -> Value {
        match &self.content {
            HumanContent::Text(text) => Value::String(text.clone()),
            HumanContent::Parts(parts) => Value::Array(parts.clone()),
        }
    }
}
