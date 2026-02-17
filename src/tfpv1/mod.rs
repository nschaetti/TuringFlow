//! TFPv1 protocol stack.
//!
//! This module groups all inter-agent transport concerns:
//! identity parsing, request/response types, mTLS helpers, routing, and storage.
//!
//! It deliberately excludes user-originated communication channels which are
//! modeled by the kernel `user.*` syscall surface.

/// Agent reference parser and normalization rules.
pub mod agent_ref;
/// Replay-window and deduplication helpers.
pub mod dedupe;
/// Wire-compatible structured error payloads.
pub mod errors;
/// Mutual-TLS server and certificate identity helpers.
pub mod mtls;
/// Registry behavior contracts.
pub mod registry;
/// Outbound router and delivery strategy.
pub mod router;
/// Persistent storage backends and migrations.
pub mod storage;
/// Daemon and kingdom configuration schema.
pub mod system_config;
/// Canonical TFPv1 request/response data structures.
pub mod types;
