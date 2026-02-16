use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;

use rusqlite::{params, Connection};

const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "0001_tfpv1_core",
        include_str!("migrations/0001_tfpv1_core.sql"),
    ),
    (
        2,
        "0002_kernel_policy_audit",
        include_str!("migrations/0002_kernel_policy_audit.sql"),
    ),
];

#[derive(Debug)]
pub enum SqliteStorageError {
    Io(String),
    Sqlite(String),
}

impl Display for SqliteStorageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SqliteStorageError::Io(message) => write!(f, "I/O error: {message}"),
            SqliteStorageError::Sqlite(message) => write!(f, "SQLite error: {message}"),
        }
    }
}

impl Error for SqliteStorageError {}

impl From<rusqlite::Error> for SqliteStorageError {
    fn from(value: rusqlite::Error) -> Self {
        SqliteStorageError::Sqlite(value.to_string())
    }
}

pub fn initialize_database(path: impl AsRef<Path>) -> Result<(), SqliteStorageError> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| SqliteStorageError::Io(e.to_string()))?;
        }
    }

    let mut conn = open_connection(path)?;
    apply_migrations(&mut conn)?;

    Ok(())
}

pub fn open_connection(path: impl AsRef<Path>) -> Result<Connection, SqliteStorageError> {
    let conn = Connection::open(path)?;
    apply_runtime_pragmas(&conn)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

fn apply_runtime_pragmas(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(())
}

fn apply_migrations(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at_ms INTEGER NOT NULL
        );
        ",
    )?;

    for (version, name, sql) in MIGRATIONS {
        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            params![version],
            |row| row.get::<_, i64>(0).map(|value| value == 1),
        )?;

        if already_applied {
            continue;
        }

        let transaction = conn.transaction()?;
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, name, applied_at_ms) VALUES (?1, ?2, ?3)",
            params![version, name, now_epoch_ms()],
        )?;
        transaction.commit()?;
    }

    Ok(())
}

fn now_epoch_ms() -> i64 {
    let now = std::time::SystemTime::now();
    match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::initialize_database;

    #[test]
    fn initializes_database_and_migration_idempotently() {
        let file_name = format!(
            "turingflow_test_{}_{}.db",
            std::process::id(),
            now_nanos_for_test()
        );
        let path = std::env::temp_dir().join(file_name);

        initialize_database(&path).expect("first init should succeed");
        initialize_database(&path).expect("second init should succeed");

        let _ = std::fs::remove_file(path);
    }

    fn now_nanos_for_test() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    }
}
