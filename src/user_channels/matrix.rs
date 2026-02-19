//! Matrix connector worker bootstrap.

use crate::user_channels::config::MatrixChannelConfig;

/// Spawns a background Matrix worker.
///
/// Current implementation is a safe no-op bootstrap placeholder to keep daemon
/// wiring stable while the full connector runtime is developed.
pub fn spawn_matrix_worker(config: MatrixChannelConfig, sqlite_path: String) {
    if !config.enabled {
        return;
    }

    let _ = std::thread::Builder::new()
        .name("turingflow-matrix-worker".to_string())
        .spawn(move || {
            eprintln!(
                "matrix worker placeholder enabled (homeserver={}, room={}, db={})",
                config.homeserver, config.room_id, sqlite_path
            );
        });
}
