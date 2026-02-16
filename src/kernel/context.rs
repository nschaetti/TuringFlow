#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub trace_id: String,
    pub kingdom_id: String,
    pub agent_ref: String,
    pub tool_id: Option<String>,
}

impl ExecutionContext {
    pub fn principal_candidates(&self) -> Vec<String> {
        let mut principals = Vec::with_capacity(2);
        if let Some(tool_id) = &self.tool_id {
            principals.push(format!("agent_tool:{}:{}", self.agent_ref, tool_id));
        }
        principals.push(format!("agent:{}", self.agent_ref));
        principals
    }
}
