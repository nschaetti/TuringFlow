/// Immutable metadata propagated across kernel calls.
///
/// # Invariants
///
/// - `agent_ref` identifies the principal that initiated the syscall.
/// - `tool_id`, when present, narrows authorization to a tool-scoped principal.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Trace identifier used for audit correlation.
    pub trace_id: String,
    /// Current span identifier.
    pub span_id: Option<String>,
    /// Parent span identifier when known.
    pub parent_span_id: Option<String>,
    /// Isolation domain identifier.
    pub kingdom_id: String,
    /// Calling agent reference (`name@host` or `id:...`).
    pub agent_ref: String,
    /// Optional tool identifier when a specific tool triggered the syscall.
    pub tool_id: Option<String>,
}

impl ExecutionContext {
    /// Returns principal candidates in precedence order.
    ///
    /// Tool-scoped principal is returned first when `tool_id` is present,
    /// followed by the broader agent principal.
    pub fn principal_candidates(&self) -> Vec<String> {
        let mut principals = Vec::with_capacity(2);
        if let Some(tool_id) = &self.tool_id {
            principals.push(format!("agent_tool:{}:{}", self.agent_ref, tool_id));
        }
        principals.push(format!("agent:{}", self.agent_ref));
        principals
    }
}
