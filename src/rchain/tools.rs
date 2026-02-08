use std::error::Error;
use std::io::Cursor;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::ImageOutputFormat;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone)]
pub enum ToolParamType {
    Integer,
    Number,
    String,
    Boolean,
    Object,
    Array,
}

impl ToolParamType {
    fn as_str(&self) -> &'static str {
        match self {
            ToolParamType::Integer => "integer",
            ToolParamType::Number => "number",
            ToolParamType::String => "string",
            ToolParamType::Boolean => "boolean",
            ToolParamType::Object => "object",
            ToolParamType::Array => "array",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolParam {
    pub name: String,
    pub description: Option<String>,
    pub kind: ToolParamType,
    pub required: bool,
}

impl ToolParam {
    pub fn new(
        name: impl Into<String>,
        kind: ToolParamType,
        required: bool,
        description: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description,
            kind,
            required,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub params: Vec<ToolParam>,
}

impl ToolFunction {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            params: Vec::new(),
        }
    }

    pub fn with_param(mut self, param: ToolParam) -> Self {
        self.params.push(param);
        self
    }

    fn to_schema(&self) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();

        for param in &self.params {
            let mut param_def = Map::new();
            param_def.insert(
                "type".to_string(),
                Value::String(param.kind.as_str().to_string()),
            );
            if let Some(description) = &param.description {
                param_def.insert("description".to_string(), Value::String(description.clone()));
            }
            properties.insert(param.name.clone(), Value::Object(param_def));
            if param.required {
                required.push(Value::String(param.name.clone()));
            }
        }

        let mut schema = Map::new();
        schema.insert("type".to_string(), Value::String("object".to_string()));
        schema.insert("properties".to_string(), Value::Object(properties));
        if !required.is_empty() {
            schema.insert("required".to_string(), Value::Array(required));
        }
        Value::Object(schema)
    }
}

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub function: ToolFunction,
}

impl ToolDefinition {
    pub fn from_function(function: ToolFunction) -> Self {
        Self { function }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.function.name,
                "description": self.function.description,
                "parameters": self.function.to_schema(),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

impl ToolCall {
    fn args_as_string(&self) -> String {
        match &self.args {
            Value::String(value) => value.clone(),
            other => serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string()),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "type": "function",
            "function": {
                "name": self.name,
                "arguments": self.args_as_string(),
            }
        })
    }
}

pub fn encode_image_base64(path: impl AsRef<Path>) -> Result<String, Box<dyn Error>> {
    let image = image::open(path)?;
    let mut buffer = Vec::new();
    image.write_to(&mut Cursor::new(&mut buffer), ImageOutputFormat::Png)?;
    Ok(STANDARD.encode(&buffer))
}
