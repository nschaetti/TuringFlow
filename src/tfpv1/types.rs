//! Canonical TFPv1 wire types and validators.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::tfpv1::agent_ref::AgentRef;

/// Protocol version literal expected by all TFPv1 payloads.
pub const TFPV1_VERSION: &str = "TFPv1";

/// Agent registration request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// Protocol version.
    pub version: String,
    /// Kingdom where agents are registered.
    pub kingdom_id: String,
    /// Registering node metadata.
    pub node: NodeRegistration,
    /// Agents announced by this node.
    pub agents: Vec<AgentRegistration>,
    /// Requested lease TTL.
    pub lease_ttl_ms: u64,
}

impl RegisterRequest {
    /// Validates request shape and required fields.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_version(&self.version)?;
        validate_non_empty("kingdom_id", &self.kingdom_id)?;
        self.node.validate()?;

        if self.agents.is_empty() {
            return Err(ValidationError::new("agents", "must not be empty"));
        }
        for agent in &self.agents {
            agent.validate()?;
        }

        if self.lease_ttl_ms == 0 {
            return Err(ValidationError::new(
                "lease_ttl_ms",
                "must be greater than 0",
            ));
        }

        Ok(())
    }
}

/// Agent registration response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub version: String,
    pub lease_id: String,
    pub expires_at: String,
    pub accepted: Vec<String>,
}

/// Registering node metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegistration {
    pub node_id: String,
    pub hostname: String,
    pub deliver_url: String,
}

impl NodeRegistration {
    /// Validates node registration fields.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty("node.node_id", &self.node_id)?;

        if !AgentRef::validate_hostname(&self.hostname) {
            return Err(ValidationError::new("node.hostname", "invalid hostname"));
        }

        if !self.deliver_url.starts_with("https://") {
            return Err(ValidationError::new(
                "node.deliver_url",
                "must start with https://",
            ));
        }

        Ok(())
    }
}

/// Agent registration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistration {
    pub agent_ref: String,
    pub agent_id: String,
}

impl AgentRegistration {
    /// Validates one agent registration entry.
    pub fn validate(&self) -> Result<(), ValidationError> {
        AgentRef::parse(&self.agent_ref)
            .map_err(|_| ValidationError::new("agents[].agent_ref", "invalid agent_ref"))?;
        validate_non_empty("agents[].agent_id", &self.agent_id)?;
        Ok(())
    }
}

/// Lease heartbeat request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub version: String,
    pub lease_id: String,
    pub kingdom_id: String,
    pub node_id: String,
    pub agents: Vec<String>,
}

impl HeartbeatRequest {
    /// Validates heartbeat payload.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_version(&self.version)?;
        validate_non_empty("lease_id", &self.lease_id)?;
        validate_non_empty("kingdom_id", &self.kingdom_id)?;
        validate_non_empty("node_id", &self.node_id)?;

        if self.agents.is_empty() {
            return Err(ValidationError::new("agents", "must not be empty"));
        }
        for agent_ref in &self.agents {
            AgentRef::parse(agent_ref)
                .map_err(|_| ValidationError::new("agents[]", "invalid agent_ref"))?;
        }
        Ok(())
    }
}

/// Lease heartbeat response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub version: String,
    pub lease_id: String,
    pub expires_at: String,
}

/// Agent resolution response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveResponse {
    pub version: String,
    pub found: bool,
    pub kingdom_id: String,
    pub agent_ref: String,
    pub agent_id: Option<String>,
    pub status: Option<String>,
    pub route: Option<AgentRoute>,
    pub lease_expires_at: Option<String>,
}

/// Resolved delivery route for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRoute {
    pub node_id: String,
    pub deliver_url: String,
}

/// Message send request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest {
    pub version: String,
    pub kingdom_id: String,
    pub message: Envelope,
}

impl SendRequest {
    /// Validates send request payload.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_version(&self.version)?;
        validate_non_empty("kingdom_id", &self.kingdom_id)?;
        self.message.validate()
    }
}

/// Message send response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResponse {
    pub version: String,
    pub accepted: bool,
    pub delivery_id: String,
    pub status: String,
    pub destination: String,
}

/// Delivery acknowledgement request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub version: String,
    pub delivery_id: String,
    pub message_id: String,
    pub from_ref: String,
    pub status: AckStatus,
    pub timestamp: String,
    pub result: Option<Value>,
}

impl AckRequest {
    /// Validates ACK payload.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_version(&self.version)?;
        validate_non_empty("delivery_id", &self.delivery_id)?;
        validate_non_empty("message_id", &self.message_id)?;
        AgentRef::parse(&self.from_ref)
            .map_err(|_| ValidationError::new("from_ref", "invalid agent_ref"))?;
        validate_rfc3339("timestamp", &self.timestamp)?;
        Ok(())
    }
}

/// Delivery acknowledgement response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckResponse {
    pub version: String,
    pub accepted: bool,
}

/// Routed message envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: String,
    pub message_id: String,
    pub trace_id: String,
    #[serde(default)]
    pub trace: Option<TraceMetadata>,
    pub timestamp: String,
    pub from_ref: String,
    pub to_ref: String,
    pub kind: MessageKind,
    pub ttl_ms: u64,
    pub requires_ack: bool,
    pub routing: Routing,
    pub payload: Payload,
    pub meta: Option<Meta>,
}

impl Envelope {
    /// Validates envelope invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_version(&self.version)?;
        validate_non_empty("message_id", &self.message_id)?;
        validate_non_empty("trace_id", &self.trace_id)?;
        if let Some(trace) = &self.trace {
            trace.validate()?;
            if trace.trace_id != self.trace_id {
                return Err(ValidationError::new(
                    "trace.trace_id",
                    "must match envelope trace_id",
                ));
            }
        }
        validate_rfc3339("timestamp", &self.timestamp)?;
        AgentRef::parse(&self.from_ref)
            .map_err(|_| ValidationError::new("from_ref", "invalid agent_ref"))?;
        AgentRef::parse(&self.to_ref)
            .map_err(|_| ValidationError::new("to_ref", "invalid agent_ref"))?;

        if self.ttl_ms == 0 {
            return Err(ValidationError::new("ttl_ms", "must be greater than 0"));
        }

        self.routing.validate()?;
        self.payload.validate()?;

        if let Some(meta) = &self.meta {
            meta.validate()?;
        }

        Ok(())
    }
}

/// Distributed tracing metadata attached to one envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
}

impl TraceMetadata {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty("trace.trace_id", &self.trace_id)?;
        validate_non_empty("trace.span_id", &self.span_id)?;
        if let Some(parent_span_id) = &self.parent_span_id {
            validate_non_empty("trace.parent_span_id", parent_span_id)?;
        }
        Ok(())
    }
}

/// Envelope semantic kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Event,
    Request,
    Response,
    Error,
}

/// Routing metadata attached to an envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routing {
    pub hops_max: u16,
    pub path: Vec<RouteHop>,
}

impl Routing {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.hops_max == 0 {
            return Err(ValidationError::new(
                "routing.hops_max",
                "must be greater than 0",
            ));
        }
        for hop in &self.path {
            hop.validate()?;
        }
        Ok(())
    }
}

/// One hop in routing path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteHop {
    pub node: String,
    pub at: String,
}

impl RouteHop {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty("routing.path[].node", &self.node)?;
        validate_rfc3339("routing.path[].at", &self.at)?;
        Ok(())
    }
}

/// Message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload {
    pub content_type: String,
    pub body: Value,
}

impl Payload {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty("payload.content_type", &self.content_type)?;
        Ok(())
    }
}

/// Optional envelope metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub priority: Priority,
    pub tags: Option<Vec<String>>,
}

impl Meta {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(tags) = &self.tags {
            for tag in tags {
                if tag.trim().is_empty() {
                    return Err(ValidationError::new("meta.tags[]", "tag must not be empty"));
                }
            }
        }
        Ok(())
    }
}

/// Message priority hint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Normal,
    High,
}

/// Acknowledgement status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckStatus {
    Accepted,
    Processed,
    Failed,
}

/// Validation error returned by type validators.
#[derive(Debug, Clone)]
pub struct ValidationError {
    field: &'static str,
    message: &'static str,
}

impl ValidationError {
    fn new(field: &'static str, message: &'static str) -> Self {
        Self { field, message }
    }

    /// Returns the field path associated with this error.
    pub fn field(&self) -> &'static str {
        self.field
    }

    /// Returns a static validation message.
    pub fn message(&self) -> &'static str {
        self.message
    }
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl Error for ValidationError {}

/// Validates protocol version field.
pub fn validate_version(version: &str) -> Result<(), ValidationError> {
    if version == TFPV1_VERSION {
        Ok(())
    } else {
        Err(ValidationError::new("version", "must be exactly TFPv1"))
    }
}

/// Validates RFC3339 timestamp fields.
pub fn validate_rfc3339(field: &'static str, input: &str) -> Result<(), ValidationError> {
    OffsetDateTime::parse(input, &Rfc3339)
        .map(|_| ())
        .map_err(|_| ValidationError::new(field, "must be a valid RFC3339 timestamp"))
}

/// Validates non-empty string fields.
pub fn validate_non_empty(field: &'static str, input: &str) -> Result<(), ValidationError> {
    if input.trim().is_empty() {
        Err(ValidationError::new(field, "must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Envelope, MessageKind, Payload, Routing};

    #[test]
    fn validates_envelope_ok() {
        let message = Envelope {
            version: "TFPv1".to_string(),
            message_id: "msg_01".to_string(),
            trace_id: "trc_01".to_string(),
            trace: None,
            timestamp: "2026-02-15T12:34:56Z".to_string(),
            from_ref: "planner@node-a.local".to_string(),
            to_ref: "executor@node-b.local".to_string(),
            kind: MessageKind::Request,
            ttl_ms: 10_000,
            requires_ack: true,
            routing: Routing {
                hops_max: 8,
                path: vec![],
            },
            payload: Payload {
                content_type: "application/json".to_string(),
                body: json!({"ok": true}),
            },
            meta: None,
        };

        assert!(message.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_timestamp() {
        let message = Envelope {
            version: "TFPv1".to_string(),
            message_id: "msg_01".to_string(),
            trace_id: "trc_01".to_string(),
            trace: None,
            timestamp: "not-a-timestamp".to_string(),
            from_ref: "planner@node-a.local".to_string(),
            to_ref: "executor@node-b.local".to_string(),
            kind: MessageKind::Request,
            ttl_ms: 10_000,
            requires_ack: true,
            routing: Routing {
                hops_max: 8,
                path: vec![],
            },
            payload: Payload {
                content_type: "application/json".to_string(),
                body: json!({"ok": true}),
            },
            meta: None,
        };

        assert!(message.validate().is_err());
    }
}
