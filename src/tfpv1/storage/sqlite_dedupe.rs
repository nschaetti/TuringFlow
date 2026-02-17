//! SQLite-backed deduplication cache for message ids.

use std::error::Error;
use std::fmt::{Display, Formatter};

use rusqlite::params;
use time::OffsetDateTime;

use crate::tfpv1::storage::sqlite::open_connection;

/// Dedupe store backed by SQLite.
#[derive(Debug, Clone)]
pub struct SqliteDedupe {
    db_path: String,
}

impl SqliteDedupe {
    /// Creates a dedupe store bound to `db_path`.
    pub fn new(db_path: impl Into<String>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    /// Inserts `message_id` if absent and not expired.
    pub fn check_and_insert(
        &mut self,
        message_id: &str,
        expires_at: OffsetDateTime,
    ) -> Result<DedupeResult, SqliteDedupeError> {
        let mut conn = open_connection(&self.db_path)
            .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        let expires_at_ms = epoch_ms(expires_at);

        let tx = conn
            .transaction()
            .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;
        tx.execute(
            "DELETE FROM dedupe WHERE expires_at_ms <= ?1",
            params![now_ms],
        )
        .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;

        let inserted = tx
            .execute(
                "
                INSERT OR IGNORE INTO dedupe (message_id, expires_at_ms, created_at_ms)
                VALUES (?1, ?2, ?3)
                ",
                params![message_id, expires_at_ms, now_ms],
            )
            .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;

        tx.commit()
            .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;

        if inserted == 0 {
            Ok(DedupeResult::Duplicate)
        } else {
            Ok(DedupeResult::Inserted)
        }
    }

    /// Removes expired dedupe rows at current time.
    pub fn cleanup_expired_now(&mut self) -> Result<(), SqliteDedupeError> {
        let conn = open_connection(&self.db_path)
            .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;
        let now_ms = epoch_ms(OffsetDateTime::now_utc());
        conn.execute(
            "DELETE FROM dedupe WHERE expires_at_ms <= ?1",
            params![now_ms],
        )
        .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Returns current dedupe table cardinality.
    pub fn len(&self) -> Result<usize, SqliteDedupeError> {
        let conn = open_connection(&self.db_path)
            .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;
        let count = conn
            .query_row("SELECT COUNT(*) FROM dedupe", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| SqliteDedupeError::Storage(e.to_string()))?;
        Ok(count.max(0) as usize)
    }
}

/// Result of a dedupe insertion attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupeResult {
    /// Message id was inserted for the first time.
    Inserted,
    /// Message id already existed.
    Duplicate,
}

/// Dedupe storage error.
#[derive(Debug, Clone)]
pub enum SqliteDedupeError {
    /// Storage/backend failure.
    Storage(String),
}

impl Display for SqliteDedupeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SqliteDedupeError::Storage(message) => write!(f, "storage error: {message}"),
        }
    }
}

impl Error for SqliteDedupeError {}

/// Validates whether `message_ts` is inside the replay window.
pub fn within_replay_window(
    message_ts: OffsetDateTime,
    now: OffsetDateTime,
    allowed_skew_seconds: i64,
) -> bool {
    let skew = allowed_skew_seconds.max(0);
    let delta = (now - message_ts).whole_seconds().abs();
    delta <= skew
}

fn epoch_ms(value: OffsetDateTime) -> i64 {
    let nanos = value.unix_timestamp_nanos();
    (nanos / 1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use time::Duration;

    use super::{within_replay_window, DedupeResult, SqliteDedupe};

    #[test]
    fn dedupe_rejects_duplicate_message_ids() {
        let path = temp_db_path();
        crate::tfpv1::storage::sqlite::initialize_database(&path).expect("db init");
        let mut cache = SqliteDedupe::new(path.to_string_lossy().to_string());
        let now = time::OffsetDateTime::now_utc();

        let first = cache
            .check_and_insert("msg_1", now + Duration::seconds(60))
            .expect("inserted");
        let second = cache
            .check_and_insert("msg_1", now + Duration::seconds(60))
            .expect("duplicate");

        assert_eq!(first, DedupeResult::Inserted);
        assert_eq!(second, DedupeResult::Duplicate);
        assert_eq!(cache.len().expect("len"), 1);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn dedupe_cleans_expired_entries() {
        let path = temp_db_path();
        crate::tfpv1::storage::sqlite::initialize_database(&path).expect("db init");
        let mut cache = SqliteDedupe::new(path.to_string_lossy().to_string());
        let now = time::OffsetDateTime::now_utc();

        cache
            .check_and_insert("expired", now - Duration::seconds(1))
            .expect("insert expired");
        cache
            .check_and_insert("fresh", now + Duration::seconds(5))
            .expect("insert fresh");
        cache.cleanup_expired_now().expect("cleanup");

        assert_eq!(cache.len().expect("len"), 1);
        assert_eq!(
            cache
                .check_and_insert("expired", now + Duration::seconds(10))
                .expect("reinsert"),
            DedupeResult::Inserted
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn replay_window_validation_works() {
        let now = time::OffsetDateTime::now_utc();

        assert!(within_replay_window(now - Duration::seconds(30), now, 60));
        assert!(within_replay_window(now + Duration::seconds(30), now, 60));
        assert!(!within_replay_window(now - Duration::seconds(61), now, 60));
        assert!(!within_replay_window(now + Duration::seconds(61), now, 60));
    }

    fn temp_db_path() -> std::path::PathBuf {
        let file_name = format!(
            "turingflow_dedupe_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        );
        std::env::temp_dir().join(file_name)
    }
}
