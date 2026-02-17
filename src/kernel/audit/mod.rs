//! Audit records for kernel policy decisions.

use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::params;

use crate::kernel::errors::KernelErrorCode;
use crate::tfpv1::storage::sqlite::open_connection;

/// Policy decision kind stored in audit records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditDecision {
    /// Request was allowed.
    Allow,
    /// Request was denied.
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

/// Single audit row written by the kernel.
#[derive(Debug, Clone)]
pub struct AuditRecord {
    /// Decision timestamp in epoch milliseconds.
    pub ts_ms: i64,
    /// Distributed trace identifier.
    pub trace_id: String,
    /// Isolation domain.
    pub kingdom_id: String,
    /// Effective principal that matched the policy.
    pub principal_id: Option<String>,
    /// Agent reference from execution context.
    pub agent_ref: String,
    /// Optional tool id from execution context.
    pub tool_id: Option<String>,
    /// Syscall name.
    pub syscall: String,
    /// Serialized resource payload used for matching.
    pub resource_json: Option<String>,
    /// Decision outcome.
    pub decision: AuditDecision,
    /// Matching policy rule id.
    pub rule_id: Option<String>,
    /// Error code when denied/failed.
    pub error_code: Option<KernelErrorCode>,
    /// Human-readable error details.
    pub error_message: Option<String>,
    /// Provider call latency.
    pub latency_ms: i64,
}

/// Sink for audit records.
pub trait AuditSink: Send + Sync {
    /// Records one audit event.
    fn record(&self, record: &AuditRecord);
}

/// No-op sink useful in tests or minimal setups.
#[derive(Debug, Default)]
pub struct NoopAuditSink;

impl AuditSink for NoopAuditSink {
    fn record(&self, _record: &AuditRecord) {}
}

/// SQLite-backed audit sink with periodic retention purge.
#[derive(Debug)]
pub struct SqliteAuditSink {
    db_path: String,
    retention_ms: i64,
    purge_every: u64,
    writes: AtomicU64,
}

impl SqliteAuditSink {
    /// Creates a sink.
    ///
    /// `purge_every` is clamped to at least `1` to avoid division-by-zero.
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

/// Returns current UNIX epoch time in milliseconds.
pub fn now_epoch_ms() -> i64 {
    let now = std::time::SystemTime::now();
    match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
        Err(_) => 0,
    }
}
