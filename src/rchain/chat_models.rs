use std::env;
use std::error::Error;

use reqwest::blocking::Client;
use serde_json::{json, Map, Value};

use crate::rchain::ai::AIMessage;
use crate::rchain::human::HumanMessage;
use crate::rchain::tools::{ToolCall, ToolDefinition};

#[derive(Debug, Clone)]
pub struct ChatFireworks {
    model: String,
    temperature: f64,
    api_key: String,
    base_url: String,
    client: Client,
    tools: Option<Vec<ToolDefinition>>,
}

impl ChatFireworks {
    pub fn new(model: impl Into<String>, temperature: f64) -> Result<Self, Box<dyn Error>> {
        let api_key = env::var("FIREWORKS_API_KEY")
            .map_err(|_| "FIREWORKS_API_KEY is not set in the environment")?;
        Ok(Self {
            model: model.into(),
            temperature,
            api_key,
            base_url: "https://api.fireworks.ai/inference/v1/chat/completions".to_string(),
            client: Client::new(),
            tools: None,
        })
    }

    pub fn bind_tools(&self, tools: Vec<ToolDefinition>) -> Self {
        let mut bound = self.clone();
        bound.tools = Some(tools);
        bound
    }

    pub fn invoke(&self, messages: &[HumanMessage]) -> Result<AIMessage, Box<dyn Error>> {
        let chat_messages = messages
            .iter()
            .map(|message| ChatMessage::user(message.clone()))
            .collect::<Vec<_>>();
        self.invoke_messages(&chat_messages)
    }

    pub fn invoke_messages(&self, messages: &[ChatMessage]) -> Result<AIMessage, Box<dyn Error>> {
        let mut payload = Map::new();
        payload.insert("model".to_string(), Value::String(self.model.clone()));
        payload.insert(
            "messages".to_string(),
            Value::Array(messages.iter().map(|message| message.to_json()).collect()),
        );
        payload.insert("temperature".to_string(), json!(self.temperature));
        if let Some(tools) = &self.tools {
            payload.insert(
                "tools".to_string(),
                Value::Array(tools.iter().map(|tool| tool.to_json()).collect()),
            );
        }
        let payload = Value::Object(payload);

        let response = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("Fireworks API error {status}: {body}").into());
        }

        let body: serde_json::Value = response.json()?;
        let message = &body["choices"][0]["message"];
        let content = message["content"].as_str().unwrap_or("").to_string();
        let tool_calls = parse_tool_calls(message);

        Ok(AIMessage {
            content,
            tool_calls,
        })
    }
}

#[derive(Debug, Clone)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    role: MessageRole,
    content: Value,
    tool_call_id: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

impl ChatMessage {
    pub fn user(message: HumanMessage) -> Self {
        Self {
            role: MessageRole::User,
            content: message.to_json(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn user_text(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: Value::String(content.into()),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn user_parts(parts: Vec<Value>) -> Self {
        Self {
            role: MessageRole::User,
            content: Value::Array(parts),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn assistant_from_ai(message: &AIMessage) -> Self {
        let content = if message.content.is_empty() {
            Value::Null
        } else {
            Value::String(message.content.clone())
        };
        Self {
            role: MessageRole::Assistant,
            content,
            tool_call_id: None,
            tool_calls: if message.tool_calls.is_empty() {
                None
            } else {
                Some(message.tool_calls.clone())
            },
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: Value::String(content.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "role".to_string(),
            Value::String(
                match self.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                }
                .to_string(),
            ),
        );
        map.insert("content".to_string(), self.content.clone());
        if let Some(tool_call_id) = &self.tool_call_id {
            map.insert(
                "tool_call_id".to_string(),
                Value::String(tool_call_id.clone()),
            );
        }
        if let Some(tool_calls) = &self.tool_calls {
            map.insert(
                "tool_calls".to_string(),
                Value::Array(tool_calls.iter().map(|call| call.to_json()).collect()),
            );
        }
        Value::Object(map)
    }
}

fn parse_tool_calls(message: &Value) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();
    if let Some(calls) = message["tool_calls"].as_array() {
        for call in calls {
            let id = call["id"].as_str().unwrap_or("").to_string();
            let name = call["function"]["name"].as_str().unwrap_or("").to_string();
            let arguments = &call["function"]["arguments"];
            let args = match arguments {
                Value::String(raw) => {
                    serde_json::from_str(raw).unwrap_or(Value::String(raw.clone()))
                }
                other => other.clone(),
            };
            if !name.is_empty() {
                tool_calls.push(ToolCall { id, name, args });
            }
        }
    }
    tool_calls
}
