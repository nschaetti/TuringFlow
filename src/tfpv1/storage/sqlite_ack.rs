//! SQLite ACK persistence.

use std::error::Error;
use std::fmt::{Display, Formatter};

use rusqlite::params;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::tfpv1::storage::sqlite::open_connection;
use crate::tfpv1::types::{AckRequest, AckStatus};

/// ACK store backed by SQLite.
#[derive(Debug, Clone)]
pub struct SqliteAckStore {
    db_path: String,
}

impl SqliteAckStore {
    /// Creates a store bound to a database file.
    pub fn new(db_path: impl Into<String>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    /// Creates a store using an in-memory SQLite database.
    pub fn in_memory() -> Self {
        Self {
            db_path: ":memory:".to_string(),
        }
    }

    /// Inserts or updates an ACK record.
    pub fn record_ack(&self, ack: &AckRequest) -> Result<(), SqliteAckStoreError> {
        let conn = open_connection(&self.db_path)
            .map_err(|e| SqliteAckStoreError::Storage(e.to_string()))?;
        let ack_ts_ms = parse_rfc3339_to_epoch_ms(&ack.timestamp)
            .ok_or_else(|| SqliteAckStoreError::Storage("invalid ACK timestamp".to_string()))?;
        let received_at_ms = epoch_ms(OffsetDateTime::now_utc());
        let result_json = ack.result.as_ref().map(value_to_text);

        conn.execute(
            "
            INSERT INTO acks (delivery_id, message_id, from_ref, status, ack_ts_ms, result_json, received_at_ms)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(delivery_id) DO UPDATE SET
                message_id = excluded.message_id,
                from_ref = excluded.from_ref,
                status = excluded.status,
                ack_ts_ms = excluded.ack_ts_ms,
                result_json = excluded.result_json,
                received_at_ms = excluded.received_at_ms
            ",
            params![
                ack.delivery_id,
                ack.message_id,
                ack.from_ref,
                ack_status_as_str(&ack.status),
                ack_ts_ms,
                result_json,
                received_at_ms
            ],
        )
        .map_err(|e| SqliteAckStoreError::Storage(e.to_string()))?;

        Ok(())
    }
}

/// ACK store error.
#[derive(Debug, Clone)]
pub enum SqliteAckStoreError {
    /// Storage/backend failure.
    Storage(String),
}

impl Display for SqliteAckStoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SqliteAckStoreError::Storage(message) => write!(f, "storage error: {message}"),
        }
    }
}

impl Error for SqliteAckStoreError {}

fn ack_status_as_str(status: &AckStatus) -> &'static str {
    match status {
        AckStatus::Accepted => "accepted",
        AckStatus::Processed => "processed",
        AckStatus::Failed => "failed",
    }
}

fn value_to_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn parse_rfc3339_to_epoch_ms(input: &str) -> Option<i64> {
    let ts = OffsetDateTime::parse(input, &Rfc3339).ok()?;
    Some(epoch_ms(ts))
}

fn epoch_ms(value: OffsetDateTime) -> i64 {
    let nanos = value.unix_timestamp_nanos();
    (nanos / 1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use serde_json::json;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    use crate::tfpv1::storage::sqlite::{initialize_database, open_connection};
    use crate::tfpv1::types::{AckRequest, AckStatus, TFPV1_VERSION};

    use super::SqliteAckStore;

    #[test]
    fn ack_upsert_is_idempotent() {
        let db_path = temp_db_path();
        initialize_database(&db_path).expect("db init");
        let store = SqliteAckStore::new(db_path.to_string_lossy().to_string());

        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .expect("timestamp");
        let first = AckRequest {
            version: TFPV1_VERSION.to_string(),
            delivery_id: "dlv_01".to_string(),
            message_id: "msg_01".to_string(),
            from_ref: "executor@node-b.local".to_string(),
            status: AckStatus::Accepted,
            timestamp: timestamp.clone(),
            result: None,
        };

        store.record_ack(&first).expect("first upsert");

        let second = AckRequest {
            version: TFPV1_VERSION.to_string(),
            delivery_id: "dlv_01".to_string(),
            message_id: "msg_01".to_string(),
            from_ref: "executor@node-b.local".to_string(),
            status: AckStatus::Processed,
            timestamp,
            result: Some(json!({"ok": true})),
        };

        store.record_ack(&second).expect("second upsert");

        let conn = open_connection(&db_path).expect("open");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM acks WHERE delivery_id = ?1",
                params!["dlv_01"],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1);

        let status: String = conn
            .query_row(
                "SELECT status FROM acks WHERE delivery_id = ?1",
                params!["dlv_01"],
                |row| row.get(0),
            )
            .expect("status");
        assert_eq!(status, "processed");

        let _ = std::fs::remove_file(db_path);
    }

    fn temp_db_path() -> std::path::PathBuf {
        let file_name = format!(
            "turingflow_ack_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        );
        std::env::temp_dir().join(file_name)
    }
}
