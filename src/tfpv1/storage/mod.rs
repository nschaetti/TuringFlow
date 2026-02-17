//! Persistent storage modules.

/// SQLite bootstrap and migrations.
pub mod sqlite;
/// ACK persistence.
pub mod sqlite_ack;
/// Dedupe persistence.
pub mod sqlite_dedupe;
/// Agent registry persistence.
pub mod sqlite_registry;
/// User-plane queue persistence helpers.
pub mod sqlite_user_comms;
