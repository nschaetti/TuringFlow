//! Kernel syscall families.
//!
//! Each submodule defines strongly-typed request/response structs and a provider
//! trait. The [`crate::kernel::Kernel`] enforces policy before delegating to the
//! selected provider implementation.

/// Filesystem read/list/write syscalls.
pub mod fs;
/// HTTP networking syscall.
pub mod net;
/// Process execution syscall.
pub mod process;
/// Secret retrieval syscall contracts.
pub mod secret;
/// User communication plane syscalls.
pub mod user;
