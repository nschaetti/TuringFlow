use crate::rchain::tools::ToolCall;

#[derive(Debug, Clone)]
pub struct AIMessage {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}
