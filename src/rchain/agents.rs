use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use serde_json::Value;

use crate::rchain::ai::AIMessage;
use crate::rchain::chat_models::{ChatFireworks, ChatMessage};
use crate::rchain::tools::{ToolCall, ToolDefinition};

/// Callable tool registered on an agent.
#[derive(Clone)]
pub struct AgentTool {
    /// Tool declaration sent to the model.
    pub definition: ToolDefinition,
    /// Tool execution handler receiving parsed JSON args.
    pub handler: Arc<dyn Fn(&Value) -> Result<String, String> + Send + Sync>,
}

impl AgentTool {
    /// Creates a new callable tool.
    pub fn new(
        definition: ToolDefinition,
        handler: Arc<dyn Fn(&Value) -> Result<String, String> + Send + Sync>,
    ) -> Self {
        Self {
            definition,
            handler,
        }
    }
}

/// Per-step stream message emitted by the agent loop.
#[derive(Debug, Clone)]
pub enum AgentStreamMessage {
    /// Assistant model response (may include tool calls).
    Assistant(AIMessage),
    /// Tool result associated with one model tool call.
    Tool {
        tool_call: ToolCall,
        content: String,
    },
}

/// Stream update payload similar to LangChain `stream_mode="updates"` chunks.
#[derive(Debug, Clone)]
pub struct AgentUpdate {
    /// Messages produced in this update.
    pub messages: Vec<AgentStreamMessage>,
}

/// Minimal create_agent equivalent: model + tools + system prompt + tool loop.
#[derive(Clone)]
pub struct Agent {
    llm: ChatFireworks,
    handlers: HashMap<String, Arc<dyn Fn(&Value) -> Result<String, String> + Send + Sync>>,
    system_prompt: Option<String>,
}

/// Creates an agent that loops on tool calls until no calls remain.
pub fn create_agent(
    model: ChatFireworks,
    tools: Vec<AgentTool>,
    system_prompt: Option<String>,
) -> Agent {
    let mut handlers = HashMap::new();
    let mut definitions = Vec::new();

    for tool in tools {
        let name = tool.definition.function.name.clone();
        handlers.insert(name, tool.handler.clone());
        definitions.push(tool.definition);
    }

    let llm = model.bind_tools(definitions);

    Agent {
        llm,
        handlers,
        system_prompt,
    }
}

impl Agent {
    /// Executes the agent loop and streams updates to a callback.
    pub fn stream_updates<F>(
        &self,
        input: impl Into<String>,
        recursion_limit: usize,
        mut on_update: F,
    ) -> Result<AIMessage, Box<dyn Error>>
    where
        F: FnMut(AgentUpdate),
    {
        if recursion_limit == 0 {
            return Err("recursion_limit must be >= 1".into());
        }

        let mut messages = Vec::new();
        if let Some(system_prompt) = &self.system_prompt {
            messages.push(ChatMessage::system_text(system_prompt.clone()));
        }
        messages.push(ChatMessage::user_text(input.into()));

        for _ in 0..recursion_limit {
            let response = self.llm.invoke_messages(&messages)?;
            on_update(AgentUpdate {
                messages: vec![AgentStreamMessage::Assistant(response.clone())],
            });

            if response.tool_calls.is_empty() {
                return Ok(response);
            }

            messages.push(ChatMessage::assistant_from_ai(&response));

            for tool_call in response.tool_calls {
                let handler = self.handlers.get(&tool_call.name).ok_or_else(|| {
                    format!("Unknown tool '{}' requested by model", tool_call.name)
                })?;

                let content = handler(&tool_call.args).map_err(|error| {
                    format!(
                        "Tool '{}' failed for call '{}': {}",
                        tool_call.name, tool_call.id, error
                    )
                })?;

                on_update(AgentUpdate {
                    messages: vec![AgentStreamMessage::Tool {
                        tool_call: tool_call.clone(),
                        content: content.clone(),
                    }],
                });

                messages.push(ChatMessage::tool_result(tool_call.id, content));
            }
        }

        Err(format!("recursion limit reached ({recursion_limit})").into())
    }
}
