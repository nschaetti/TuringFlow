//! In-memory registry implementation used in tests and local flows.

use std::collections::{HashMap, HashSet};

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::tfpv1::agent_ref::AgentRef;
use crate::tfpv1::types::{
    AgentRoute, HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse,
    ResolveResponse, TFPV1_VERSION,
};

/// Agent lookup projection.
#[derive(Debug, Clone)]
pub struct AgentLookup {
    pub kingdom_id: String,
    pub agent_ref: String,
    pub agent_id: String,
    pub node_id: String,
    pub deliver_url: String,
    pub lease_expires_at: String,
}

/// In-memory registry state.
#[derive(Debug, Default)]
pub struct Registry {
    leases: HashMap<String, LeaseRecord>,
    agents: HashMap<String, AgentRecord>,
    seq: u64,
}

impl Registry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers agents and creates a lease.
    pub fn register(
        &mut self,
        request: RegisterRequest,
    ) -> Result<RegisterResponse, RegistryError> {
        let now = OffsetDateTime::now_utc();
        self.cleanup_expired(now);

        let mut accepted = Vec::with_capacity(request.agents.len());
        let mut normalized_agents = Vec::with_capacity(request.agents.len());

        for agent in request.agents {
            let parsed = AgentRef::parse(&agent.agent_ref)
                .map_err(|_| RegistryError::Invalid("agents[].agent_ref"))?;
            let agent_ref = parsed.normalized();
            accepted.push(agent_ref.clone());
            normalized_agents.push((agent_ref, agent.agent_id));
        }

        let lease_id = self.next_lease_id(now);
        let expires_at = now + duration_from_ms(request.lease_ttl_ms);
        let node_id = request.node.node_id.clone();
        let deliver_url = request.node.deliver_url.clone();

        let lease = LeaseRecord {
            kingdom_id: request.kingdom_id.clone(),
            node_id: node_id.clone(),
            lease_ttl_ms: request.lease_ttl_ms,
            agent_refs: accepted.clone(),
            expires_at,
        };

        self.leases.insert(lease_id.clone(), lease.clone());

        for (agent_ref, agent_id) in normalized_agents {
            self.agents.insert(
                agent_ref,
                AgentRecord {
                    kingdom_id: request.kingdom_id.clone(),
                    agent_id,
                    node_id: node_id.clone(),
                    deliver_url: deliver_url.clone(),
                    lease_id: lease_id.clone(),
                    expires_at,
                },
            );
        }

        Ok(RegisterResponse {
            version: TFPV1_VERSION.to_string(),
            lease_id,
            expires_at: format_rfc3339(expires_at),
            accepted,
        })
    }

    /// Processes heartbeat and extends lease.
    pub fn heartbeat(
        &mut self,
        request: HeartbeatRequest,
    ) -> Result<HeartbeatResponse, RegistryError> {
        let now = OffsetDateTime::now_utc();
        self.cleanup_expired(now);

        let lease = self
            .leases
            .get(&request.lease_id)
            .cloned()
            .ok_or(RegistryError::LeaseExpired)?;

        if lease.kingdom_id != request.kingdom_id || lease.node_id != request.node_id {
            return Err(RegistryError::IdentityMismatch);
        }

        if lease.expires_at <= now {
            self.remove_lease_and_agents(&request.lease_id);
            return Err(RegistryError::LeaseExpired);
        }

        let provided: Vec<String> = request
            .agents
            .iter()
            .map(|agent_ref| {
                AgentRef::parse(agent_ref)
                    .map(|parsed| parsed.normalized())
                    .map_err(|_| RegistryError::Invalid("agents[]"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let provided_set: HashSet<&str> = provided.iter().map(|s| s.as_str()).collect();
        let lease_set: HashSet<&str> = lease.agent_refs.iter().map(|s| s.as_str()).collect();

        if !provided_set.is_subset(&lease_set) {
            return Err(RegistryError::Invalid(
                "agents[] must be registered under the lease",
            ));
        }

        let expires_at = now + duration_from_ms(lease.lease_ttl_ms);

        if let Some(lease_record) = self.leases.get_mut(&request.lease_id) {
            lease_record.expires_at = expires_at;
        }

        for agent_ref in &lease.agent_refs {
            if let Some(agent) = self.agents.get_mut(agent_ref) {
                if agent.lease_id == request.lease_id {
                    agent.expires_at = expires_at;
                }
            }
        }

        Ok(HeartbeatResponse {
            version: TFPV1_VERSION.to_string(),
            lease_id: request.lease_id,
            expires_at: format_rfc3339(expires_at),
        })
    }

    /// Resolves one route from `(kingdom_id, agent_ref)`.
    pub fn resolve(&mut self, kingdom_id: &str, agent_ref: &str) -> ResolveResponse {
        let now = OffsetDateTime::now_utc();
        self.cleanup_expired(now);

        let normalized = match AgentRef::parse(agent_ref) {
            Ok(parsed) => parsed.normalized(),
            Err(_) => agent_ref.to_string(),
        };

        if let Some(agent) = self.agents.get(&normalized) {
            if agent.kingdom_id == kingdom_id {
                return ResolveResponse {
                    version: TFPV1_VERSION.to_string(),
                    found: true,
                    kingdom_id: kingdom_id.to_string(),
                    agent_ref: normalized,
                    agent_id: Some(agent.agent_id.clone()),
                    status: Some("online".to_string()),
                    route: Some(AgentRoute {
                        node_id: agent.node_id.clone(),
                        deliver_url: agent.deliver_url.clone(),
                    }),
                    lease_expires_at: Some(format_rfc3339(agent.expires_at)),
                };
            }
        }

        ResolveResponse {
            version: TFPV1_VERSION.to_string(),
            found: false,
            kingdom_id: kingdom_id.to_string(),
            agent_ref: normalized,
            agent_id: None,
            status: None,
            route: None,
            lease_expires_at: None,
        }
    }

    /// Looks up a specific agent in one kingdom.
    pub fn lookup_agent(&mut self, kingdom_id: &str, agent_ref: &str) -> Option<AgentLookup> {
        let now = OffsetDateTime::now_utc();
        self.cleanup_expired(now);

        let normalized = AgentRef::parse(agent_ref).ok()?.normalized();
        let agent = self.agents.get(&normalized)?;
        if agent.kingdom_id != kingdom_id {
            return None;
        }

        Some(AgentLookup {
            kingdom_id: agent.kingdom_id.clone(),
            agent_ref: normalized,
            agent_id: agent.agent_id.clone(),
            node_id: agent.node_id.clone(),
            deliver_url: agent.deliver_url.clone(),
            lease_expires_at: format_rfc3339(agent.expires_at),
        })
    }

    /// Looks up an agent across kingdoms.
    pub fn lookup_agent_any(&mut self, agent_ref: &str) -> Option<AgentLookup> {
        let now = OffsetDateTime::now_utc();
        self.cleanup_expired(now);

        let normalized = AgentRef::parse(agent_ref).ok()?.normalized();
        let agent = self.agents.get(&normalized)?;

        Some(AgentLookup {
            kingdom_id: agent.kingdom_id.clone(),
            agent_ref: normalized,
            agent_id: agent.agent_id.clone(),
            node_id: agent.node_id.clone(),
            deliver_url: agent.deliver_url.clone(),
            lease_expires_at: format_rfc3339(agent.expires_at),
        })
    }

    /// Performs immediate expiry cleanup.
    pub fn cleanup_expired_now(&mut self) {
        self.cleanup_expired(OffsetDateTime::now_utc());
    }

    fn cleanup_expired(&mut self, now: OffsetDateTime) {
        let expired: Vec<String> = self
            .leases
            .iter()
            .filter_map(|(lease_id, lease)| {
                if lease.expires_at <= now {
                    Some(lease_id.clone())
                } else {
                    None
                }
            })
            .collect();

        for lease_id in expired {
            self.remove_lease_and_agents(&lease_id);
        }
    }

    fn remove_lease_and_agents(&mut self, lease_id: &str) {
        if let Some(lease) = self.leases.remove(lease_id) {
            for agent_ref in lease.agent_refs {
                if let Some(agent) = self.agents.get(&agent_ref) {
                    if agent.lease_id == lease_id {
                        self.agents.remove(&agent_ref);
                    }
                }
            }
        }
    }

    fn next_lease_id(&mut self, now: OffsetDateTime) -> String {
        self.seq = self.seq.saturating_add(1);
        format!("lease_{}_{}", now.unix_timestamp_nanos(), self.seq)
    }
}

#[derive(Debug, Clone)]
struct LeaseRecord {
    kingdom_id: String,
    node_id: String,
    lease_ttl_ms: u64,
    agent_refs: Vec<String>,
    expires_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
struct AgentRecord {
    kingdom_id: String,
    agent_id: String,
    node_id: String,
    deliver_url: String,
    lease_id: String,
    expires_at: OffsetDateTime,
}

/// In-memory registry operation errors.
#[derive(Debug, Clone, Copy)]
pub enum RegistryError {
    /// Lease expired or unknown.
    LeaseExpired,
    /// Heartbeat identity mismatch.
    IdentityMismatch,
    /// Invalid request field.
    Invalid(&'static str),
}

fn duration_from_ms(ms: u64) -> Duration {
    let clamped = ms.min(i64::MAX as u64);
    Duration::milliseconds(clamped as i64)
}

fn format_rfc3339(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
