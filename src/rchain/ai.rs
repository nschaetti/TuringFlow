use crate::rchain::tools::ToolCall;

/// Assistant message returned by chat models.
#[derive(Debug, Clone)]
pub struct AIMessage {
    /// Natural language content.
    pub content: String,
    /// Optional tool call requests emitted by the model.
    pub tool_calls: Vec<ToolCall>,
}
