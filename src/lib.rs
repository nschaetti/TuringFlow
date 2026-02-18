//! TuringFlow core library.
//!
//! TuringFlow is split into two communication planes:
//!
//! - **Inter-agent plane** (`tfpv1`): secure node-to-node transport, registration,
//!   deduplication, routing, and acknowledgements.
//! - **User communication plane** (`kernel::syscalls::user` + `user_channels`):
//!   ingress/egress between human users and agents through policy-gated kernel
//!   syscalls.
//!
//! # Architectural layers
//!
//! 1. [`tfpv1`]: protocol types, validation, mTLS helpers, router, and storage.
//! 2. [`kernel`]: syscall-like policy enforcement and auditable decision points.
//! 3. [`user_channels`]: channel adapters (for example Matrix) that map external
//!    events to `user.*` syscalls.
//! 4. [`rchain`] and [`pulse`]: model/tool helpers and terminal UI primitives.
//!
//! # Concurrency model
//!
//! The crate favors shareable, cloneable service objects backed by `Arc` and
//! synchronization primitives at integration points (for example `RwLock` in the
//! daemon state). Provider traits are `Send + Sync` so they can be safely called
//! from multi-threaded runtimes.
//!
//! # Invariants
//!
//! - Policy evaluation is deny-by-default.
//! - Message identifiers are expected to be idempotent keys where applicable.
//! - SQLite schema migrations are append-only and idempotent.
//! - Channel workers never bypass kernel policy for user-plane ingestion.
//!
//! # Minimal usage
//!
//! ```no_run
//! use std::sync::Arc;
//! use turingflow::kernel::policy::{PolicyConfig, PolicyEngine};
//! use turingflow::kernel::syscalls::fs::HostFsProvider;
//! use turingflow::kernel::Kernel;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let yaml = r#"
//! version: 1
//! defaults:
//!   decision: deny
//! principals: []
//! "#;
//! let cfg: PolicyConfig = serde_yaml::from_str(yaml)?;
//! cfg.validate()?;
//! let root = std::env::current_dir()?;
//! let kernel = Kernel::new(PolicyEngine::new(cfg), Arc::new(HostFsProvider::new(&root)?));
//! let _ = kernel;
//! # Ok(())
//! # }
//! ```

/// Kernel policy engine, syscall surface, and auditing.
pub mod kernel;
/// Observability utilities (structured logging and tracing).
pub mod observability;
/// Terminal UI primitives and app state components.
pub mod pulse;
/// LLM and tool orchestration helpers.
pub mod rchain;
/// TFPv1 transport protocol, router, and persistence.
pub mod tfpv1;
/// User-channel connectors and channel configuration types.
pub mod user_channels;
