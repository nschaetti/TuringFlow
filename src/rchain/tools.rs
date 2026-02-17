use std::error::Error;
use std::io::Cursor;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::ImageOutputFormat;
use serde_json::{json, Map, Value};

/// JSON schema primitive types supported for tool parameters.
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

/// One function parameter definition.
#[derive(Debug, Clone)]
pub struct ToolParam {
    /// Parameter name.
    pub name: String,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// JSON schema type.
    pub kind: ToolParamType,
    /// Whether the parameter is required.
    pub required: bool,
}

impl ToolParam {
    /// Builds a parameter definition.
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

/// Callable tool function definition.
#[derive(Debug, Clone)]
pub struct ToolFunction {
    /// Function name.
    pub name: String,
    /// Function description.
    pub description: String,
    /// Parameter definitions.
    pub params: Vec<ToolParam>,
}

impl ToolFunction {
    /// Creates a function definition.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            params: Vec::new(),
        }
    }

    /// Appends one parameter definition.
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
                param_def.insert(
                    "description".to_string(),
                    Value::String(description.clone()),
                );
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

/// Tool wrapper matching chat-completions function-calling schema.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// Function declaration.
    pub function: ToolFunction,
}

impl ToolDefinition {
    /// Wraps a function definition as a tool.
    pub fn from_function(function: ToolFunction) -> Self {
        Self { function }
    }

    /// Serializes the tool declaration to JSON.
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

/// Tool call emitted by a model.
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Provider-generated call id.
    pub id: String,
    /// Tool/function name.
    pub name: String,
    /// Arguments payload.
    pub args: Value,
}

impl ToolCall {
    fn args_as_string(&self) -> String {
        match &self.args {
            Value::String(value) => value.clone(),
            other => serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string()),
        }
    }

    /// Serializes a tool call payload to provider JSON format.
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

/// Normalizes arbitrary image bytes to PNG and returns Base64 payload.
pub fn encode_image_base64_from_bytes(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let image = image::load_from_memory(bytes)?;
    let mut buffer = Vec::new();
    image.write_to(&mut Cursor::new(&mut buffer), ImageOutputFormat::Png)?;
    Ok(STANDARD.encode(&buffer))
}
