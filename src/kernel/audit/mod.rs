use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::params;

use crate::kernel::errors::KernelErrorCode;
use crate::tfpv1::storage::sqlite::open_connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditDecision {
    Allow,
    Deny,
}

impl AuditDecision {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub ts_ms: i64,
    pub trace_id: String,
    pub kingdom_id: String,
    pub principal_id: Option<String>,
    pub agent_ref: String,
    pub tool_id: Option<String>,
    pub syscall: String,
    pub resource_json: Option<String>,
    pub decision: AuditDecision,
    pub rule_id: Option<String>,
    pub error_code: Option<KernelErrorCode>,
    pub error_message: Option<String>,
    pub latency_ms: i64,
}

pub trait AuditSink: Send + Sync {
    fn record(&self, record: &AuditRecord);
}

#[derive(Debug, Default)]
pub struct NoopAuditSink;

impl AuditSink for NoopAuditSink {
    fn record(&self, _record: &AuditRecord) {}
}

#[derive(Debug)]
pub struct SqliteAuditSink {
    db_path: String,
    retention_ms: i64,
    purge_every: u64,
    writes: AtomicU64,
}

impl SqliteAuditSink {
    pub fn new(db_path: impl Into<String>, retention_ms: i64, purge_every: u64) -> Self {
        Self {
            db_path: db_path.into(),
            retention_ms,
            purge_every: purge_every.max(1),
            writes: AtomicU64::new(0),
        }
    }

    fn insert_record(&self, record: &AuditRecord) -> Result<(), String> {
        let conn = open_connection(&self.db_path).map_err(|e| e.to_string())?;
        conn.execute(
            "
            INSERT INTO syscall_audit_log (
                ts_ms,
                trace_id,
                kingdom_id,
                principal_id,
                agent_ref,
                tool_id,
                syscall,
                resource_json,
                decision,
                rule_id,
                error_code,
                error_message,
                latency_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ",
            params![
                record.ts_ms,
                record.trace_id,
                record.kingdom_id,
                record.principal_id,
                record.agent_ref,
                record.tool_id,
                record.syscall,
                record.resource_json,
                record.decision.as_str(),
                record.rule_id,
                record.error_code.map(|c| c.as_str().to_string()),
                record.error_message,
                record.latency_ms,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn maybe_purge(&self, ts_ms: i64) {
        let writes = self.writes.fetch_add(1, Ordering::Relaxed) + 1;
        if writes % self.purge_every != 0 {
            return;
        }

        if self.retention_ms <= 0 {
            return;
        }

        let cutoff = ts_ms.saturating_sub(self.retention_ms);
        if let Ok(conn) = open_connection(&self.db_path) {
            let _ = conn.execute(
                "DELETE FROM syscall_audit_log WHERE ts_ms < ?1",
                params![cutoff],
            );
        }
    }
}

impl AuditSink for SqliteAuditSink {
    fn record(&self, record: &AuditRecord) {
        let _ = self.insert_record(record);
        self.maybe_purge(record.ts_ms);
    }
}

pub fn now_epoch_ms() -> i64 {
    let now = std::time::SystemTime::now();
    match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
        Err(_) => 0,
    }
}
