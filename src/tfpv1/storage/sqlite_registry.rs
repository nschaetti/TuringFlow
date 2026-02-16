use std::collections::HashSet;

use rusqlite::{params, OptionalExtension};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::tfpv1::agent_ref::AgentRef;
use crate::tfpv1::storage::sqlite::open_connection;
use crate::tfpv1::types::{
    AgentRoute, HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse,
    ResolveResponse, TFPV1_VERSION,
};

#[derive(Debug, Clone)]
pub struct AgentLookup {
    pub kingdom_id: String,
    pub agent_ref: String,
    pub agent_id: String,
    pub node_id: String,
    pub deliver_url: String,
    pub lease_expires_at: String,
}

#[derive(Debug, Clone)]
pub struct SqliteRegistry {
    db_path: String,
    seq: u64,
}

impl SqliteRegistry {
    pub fn new(db_path: impl Into<String>) -> Self {
        Self {
            db_path: db_path.into(),
            seq: 0,
        }
    }

    pub fn register(
        &mut self,
        request: RegisterRequest,
    ) -> Result<RegisterResponse, RegistryError> {
        let mut conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now = OffsetDateTime::now_utc();
        let now_ms = epoch_ms(now);
        self.cleanup_expired_inner(&conn, now_ms)?;

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
        let expires_at_ms = epoch_ms(expires_at);
        let node_id = request.node.node_id.clone();
        let deliver_url = request.node.deliver_url.clone();

        let tx = conn.transaction().map_err(RegistryError::storage)?;
        tx.execute(
            "
            INSERT INTO leases (lease_id, kingdom_id, node_id, lease_ttl_ms, expires_at_ms, created_at_ms, updated_at_ms)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                lease_id,
                request.kingdom_id,
                node_id,
                request.lease_ttl_ms as i64,
                expires_at_ms,
                now_ms,
                now_ms
            ],
        )
        .map_err(RegistryError::storage)?;

        for (agent_ref, agent_id) in normalized_agents {
            tx.execute(
                "
                INSERT INTO agents (kingdom_id, agent_ref, agent_id, node_id, deliver_url, lease_id, expires_at_ms, updated_at_ms)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(kingdom_id, agent_ref) DO UPDATE SET
                    agent_id = excluded.agent_id,
                    node_id = excluded.node_id,
                    deliver_url = excluded.deliver_url,
                    lease_id = excluded.lease_id,
                    expires_at_ms = excluded.expires_at_ms,
                    updated_at_ms = excluded.updated_at_ms
                ",
                params![
                    request.kingdom_id,
                    agent_ref,
                    agent_id,
                    node_id,
                    deliver_url,
                    lease_id,
                    expires_at_ms,
                    now_ms
                ],
            )
            .map_err(RegistryError::storage)?;
        }

        tx.commit().map_err(RegistryError::storage)?;

        Ok(RegisterResponse {
            version: TFPV1_VERSION.to_string(),
            lease_id,
            expires_at: format_rfc3339(expires_at),
            accepted,
        })
    }

    pub fn heartbeat(
        &mut self,
        request: HeartbeatRequest,
    ) -> Result<HeartbeatResponse, RegistryError> {
        let mut conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now = OffsetDateTime::now_utc();
        let now_ms = epoch_ms(now);
        self.cleanup_expired_inner(&conn, now_ms)?;

        let lease = conn
            .query_row(
                "
                SELECT kingdom_id, node_id, lease_ttl_ms, expires_at_ms
                FROM leases
                WHERE lease_id = ?1
                ",
                params![request.lease_id],
                |row| {
                    Ok(LeaseRecord {
                        kingdom_id: row.get(0)?,
                        node_id: row.get(1)?,
                        lease_ttl_ms: row.get::<_, i64>(2)? as u64,
                        expires_at_ms: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(RegistryError::storage)?
            .ok_or(RegistryError::LeaseExpired)?;

        if lease.kingdom_id != request.kingdom_id || lease.node_id != request.node_id {
            return Err(RegistryError::IdentityMismatch);
        }

        if lease.expires_at_ms <= now_ms {
            self.remove_lease_and_agents_inner(&conn, &request.lease_id)?;
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

        let mut lease_agent_refs = Vec::new();
        {
            let mut stmt = conn
                .prepare("SELECT agent_ref FROM agents WHERE lease_id = ?1")
                .map_err(RegistryError::storage)?;
            let rows = stmt
                .query_map(params![request.lease_id], |row| row.get::<_, String>(0))
                .map_err(RegistryError::storage)?;
            for row in rows {
                lease_agent_refs.push(row.map_err(RegistryError::storage)?);
            }
        }

        let provided_set: HashSet<&str> = provided.iter().map(|s| s.as_str()).collect();
        let lease_set: HashSet<&str> = lease_agent_refs.iter().map(|s| s.as_str()).collect();

        if !provided_set.is_subset(&lease_set) {
            return Err(RegistryError::Invalid(
                "agents[] must be registered under the lease",
            ));
        }

        let expires_at = now + duration_from_ms(lease.lease_ttl_ms);
        let expires_at_ms = epoch_ms(expires_at);

        let tx = conn.transaction().map_err(RegistryError::storage)?;
        tx.execute(
            "UPDATE leases SET expires_at_ms = ?1, updated_at_ms = ?2 WHERE lease_id = ?3",
            params![expires_at_ms, now_ms, request.lease_id],
        )
        .map_err(RegistryError::storage)?;
        tx.execute(
            "UPDATE agents SET expires_at_ms = ?1, updated_at_ms = ?2 WHERE lease_id = ?3",
            params![expires_at_ms, now_ms, request.lease_id],
        )
        .map_err(RegistryError::storage)?;
        tx.commit().map_err(RegistryError::storage)?;

        Ok(HeartbeatResponse {
            version: TFPV1_VERSION.to_string(),
            lease_id: request.lease_id,
            expires_at: format_rfc3339(expires_at),
        })
    }

    pub fn resolve(
        &mut self,
        kingdom_id: &str,
        agent_ref: &str,
    ) -> Result<ResolveResponse, RegistryError> {
        let conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        self.cleanup_expired_inner(&conn, now_ms)?;

        let normalized = match AgentRef::parse(agent_ref) {
            Ok(parsed) => parsed.normalized(),
            Err(_) => agent_ref.to_string(),
        };

        let record = conn
            .query_row(
                "
                SELECT agent_id, node_id, deliver_url, expires_at_ms
                FROM agents
                WHERE kingdom_id = ?1 AND agent_ref = ?2
                ",
                params![kingdom_id, normalized],
                |row| {
                    Ok(AgentRecord {
                        agent_id: row.get(0)?,
                        node_id: row.get(1)?,
                        deliver_url: row.get(2)?,
                        expires_at_ms: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(RegistryError::storage)?;

        if let Some(agent) = record {
            return Ok(ResolveResponse {
                version: TFPV1_VERSION.to_string(),
                found: true,
                kingdom_id: kingdom_id.to_string(),
                agent_ref: normalized,
                agent_id: Some(agent.agent_id),
                status: Some("online".to_string()),
                route: Some(AgentRoute {
                    node_id: agent.node_id,
                    deliver_url: agent.deliver_url,
                }),
                lease_expires_at: Some(format_rfc3339(offset_from_epoch_ms(agent.expires_at_ms))),
            });
        }

        Ok(ResolveResponse {
            version: TFPV1_VERSION.to_string(),
            found: false,
            kingdom_id: kingdom_id.to_string(),
            agent_ref: normalized,
            agent_id: None,
            status: None,
            route: None,
            lease_expires_at: None,
        })
    }

    pub fn lookup_agent(
        &mut self,
        kingdom_id: &str,
        agent_ref: &str,
    ) -> Result<Option<AgentLookup>, RegistryError> {
        let conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        self.cleanup_expired_inner(&conn, now_ms)?;

        let normalized = AgentRef::parse(agent_ref)
            .map_err(|_| RegistryError::Invalid("agent_ref"))?
            .normalized();

        let record = conn
            .query_row(
                "
                SELECT kingdom_id, agent_id, node_id, deliver_url, expires_at_ms
                FROM agents
                WHERE kingdom_id = ?1 AND agent_ref = ?2
                ",
                params![kingdom_id, normalized],
                |row| {
                    Ok(AgentLookup {
                        kingdom_id: row.get(0)?,
                        agent_ref: normalized.clone(),
                        agent_id: row.get(1)?,
                        node_id: row.get(2)?,
                        deliver_url: row.get(3)?,
                        lease_expires_at: format_rfc3339(offset_from_epoch_ms(row.get(4)?)),
                    })
                },
            )
            .optional()
            .map_err(RegistryError::storage)?;

        Ok(record)
    }

    pub fn lookup_agent_any(
        &mut self,
        agent_ref: &str,
    ) -> Result<Option<AgentLookup>, RegistryError> {
        let conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        self.cleanup_expired_inner(&conn, now_ms)?;

        let normalized = AgentRef::parse(agent_ref)
            .map_err(|_| RegistryError::Invalid("agent_ref"))?
            .normalized();

        let record = conn
            .query_row(
                "
                SELECT kingdom_id, agent_id, node_id, deliver_url, expires_at_ms
                FROM agents
                WHERE agent_ref = ?1
                ORDER BY updated_at_ms DESC
                LIMIT 1
                ",
                params![normalized],
                |row| {
                    Ok(AgentLookup {
                        kingdom_id: row.get(0)?,
                        agent_ref: normalized.clone(),
                        agent_id: row.get(1)?,
                        node_id: row.get(2)?,
                        deliver_url: row.get(3)?,
                        lease_expires_at: format_rfc3339(offset_from_epoch_ms(row.get(4)?)),
                    })
                },
            )
            .optional()
            .map_err(RegistryError::storage)?;

        Ok(record)
    }

    pub fn cleanup_expired_now(&mut self) -> Result<(), RegistryError> {
        let conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        self.cleanup_expired_inner(&conn, now_ms)
    }

    pub fn count_agents_for_node(
        &mut self,
        kingdom_id: &str,
        node_id: &str,
    ) -> Result<usize, RegistryError> {
        let conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        self.cleanup_expired_inner(&conn, now_ms)?;

        let count = conn
            .query_row(
                "
                SELECT COUNT(*)
                FROM agents
                WHERE kingdom_id = ?1 AND node_id = ?2
                ",
                params![kingdom_id, node_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(RegistryError::storage)?;

        Ok(count.max(0) as usize)
    }

    pub fn additional_agents_for_node_registration(
        &mut self,
        kingdom_id: &str,
        node_id: &str,
        agent_refs: &[String],
    ) -> Result<usize, RegistryError> {
        let conn = open_connection(&self.db_path).map_err(RegistryError::storage)?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        self.cleanup_expired_inner(&conn, now_ms)?;

        let normalized: HashSet<String> = agent_refs
            .iter()
            .map(|agent_ref| {
                AgentRef::parse(agent_ref)
                    .map(|parsed| parsed.normalized())
                    .map_err(|_| RegistryError::Invalid("agents[].agent_ref"))
            })
            .collect::<Result<HashSet<_>, _>>()?;

        let mut additional = 0usize;
        for agent_ref in normalized {
            let existing_node: Option<String> = conn
                .query_row(
                    "
                    SELECT node_id
                    FROM agents
                    WHERE kingdom_id = ?1 AND agent_ref = ?2
                    ",
                    params![kingdom_id, agent_ref],
                    |row| row.get(0),
                )
                .optional()
                .map_err(RegistryError::storage)?;

            if existing_node.as_deref() != Some(node_id) {
                additional = additional.saturating_add(1);
            }
        }

        Ok(additional)
    }

    fn cleanup_expired_inner(
        &self,
        conn: &rusqlite::Connection,
        now_ms: i64,
    ) -> Result<(), RegistryError> {
        conn.execute(
            "DELETE FROM leases WHERE expires_at_ms <= ?1",
            params![now_ms],
        )
        .map_err(RegistryError::storage)?;
        Ok(())
    }

    fn remove_lease_and_agents_inner(
        &self,
        conn: &rusqlite::Connection,
        lease_id: &str,
    ) -> Result<(), RegistryError> {
        conn.execute("DELETE FROM leases WHERE lease_id = ?1", params![lease_id])
            .map_err(RegistryError::storage)?;
        Ok(())
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
    expires_at_ms: i64,
}

#[derive(Debug, Clone)]
struct AgentRecord {
    agent_id: String,
    node_id: String,
    deliver_url: String,
    expires_at_ms: i64,
}

#[derive(Debug, Clone)]
pub enum RegistryError {
    LeaseExpired,
    IdentityMismatch,
    Invalid(&'static str),
    Storage(String),
}

impl RegistryError {
    fn storage(error: impl ToString) -> Self {
        Self::Storage(error.to_string())
    }
}

fn duration_from_ms(ms: u64) -> Duration {
    let clamped = ms.min(i64::MAX as u64);
    Duration::milliseconds(clamped as i64)
}

fn epoch_ms(value: OffsetDateTime) -> i64 {
    let nanos = value.unix_timestamp_nanos();
    (nanos / 1_000_000) as i64
}

fn offset_from_epoch_ms(ms: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

fn format_rfc3339(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use crate::tfpv1::storage::sqlite::initialize_database;
    use crate::tfpv1::types::{
        AgentRegistration, HeartbeatRequest, NodeRegistration, RegisterRequest, TFPV1_VERSION,
    };

    use super::{RegistryError, SqliteRegistry};

    #[test]
    fn register_and_resolve_agent() {
        let db_path = temp_db_path("register_resolve");
        initialize_database(&db_path).expect("db init");
        let mut registry = SqliteRegistry::new(db_path.to_string_lossy().to_string());

        let register = RegisterRequest {
            version: TFPV1_VERSION.to_string(),
            kingdom_id: "kingdom-main".to_string(),
            node: NodeRegistration {
                node_id: "node-a".to_string(),
                hostname: "node-a.local".to_string(),
                deliver_url: "https://127.0.0.1:9443".to_string(),
            },
            agents: vec![AgentRegistration {
                agent_ref: "planner@node-a.local".to_string(),
                agent_id: "ag_01A".to_string(),
            }],
            lease_ttl_ms: 10_000,
        };

        registry.register(register).expect("register");
        let resolved = registry
            .resolve("kingdom-main", "planner@node-a.local")
            .expect("resolve");

        assert!(resolved.found);
        assert_eq!(resolved.agent_id.as_deref(), Some("ag_01A"));

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn heartbeat_extends_lease() {
        let db_path = temp_db_path("heartbeat_ok");
        initialize_database(&db_path).expect("db init");
        let mut registry = SqliteRegistry::new(db_path.to_string_lossy().to_string());

        let register = RegisterRequest {
            version: TFPV1_VERSION.to_string(),
            kingdom_id: "kingdom-main".to_string(),
            node: NodeRegistration {
                node_id: "node-a".to_string(),
                hostname: "node-a.local".to_string(),
                deliver_url: "https://127.0.0.1:9443".to_string(),
            },
            agents: vec![AgentRegistration {
                agent_ref: "planner@node-a.local".to_string(),
                agent_id: "ag_01A".to_string(),
            }],
            lease_ttl_ms: 10_000,
        };

        let response = registry.register(register).expect("register");
        thread::sleep(Duration::from_millis(5));
        let heartbeat = HeartbeatRequest {
            version: TFPV1_VERSION.to_string(),
            lease_id: response.lease_id.clone(),
            kingdom_id: "kingdom-main".to_string(),
            node_id: "node-a".to_string(),
            agents: vec!["planner@node-a.local".to_string()],
        };

        let hb = registry.heartbeat(heartbeat).expect("heartbeat");
        assert_eq!(hb.lease_id, response.lease_id);
        assert_ne!(hb.expires_at, response.expires_at);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn heartbeat_fails_when_lease_expired() {
        let db_path = temp_db_path("heartbeat_expired");
        initialize_database(&db_path).expect("db init");
        let mut registry = SqliteRegistry::new(db_path.to_string_lossy().to_string());

        let register = RegisterRequest {
            version: TFPV1_VERSION.to_string(),
            kingdom_id: "kingdom-main".to_string(),
            node: NodeRegistration {
                node_id: "node-a".to_string(),
                hostname: "node-a.local".to_string(),
                deliver_url: "https://127.0.0.1:9443".to_string(),
            },
            agents: vec![AgentRegistration {
                agent_ref: "planner@node-a.local".to_string(),
                agent_id: "ag_01A".to_string(),
            }],
            lease_ttl_ms: 20,
        };

        let response = registry.register(register).expect("register");
        thread::sleep(Duration::from_millis(40));

        let heartbeat = HeartbeatRequest {
            version: TFPV1_VERSION.to_string(),
            lease_id: response.lease_id,
            kingdom_id: "kingdom-main".to_string(),
            node_id: "node-a".to_string(),
            agents: vec!["planner@node-a.local".to_string()],
        };

        let result = registry.heartbeat(heartbeat);
        assert!(matches!(result, Err(RegistryError::LeaseExpired)));

        let _ = std::fs::remove_file(db_path);
    }

    fn temp_db_path(suffix: &str) -> std::path::PathBuf {
        let file_name = format!(
            "turingflow_registry_test_{}_{}_{}.db",
            suffix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        );
        std::env::temp_dir().join(file_name)
    }
}
