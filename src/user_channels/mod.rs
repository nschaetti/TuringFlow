//! User channel connectors.
//!
//! This module contains channel configuration parsing and connector workers that
//! bridge external user channels with kernel `user.*` syscalls.

/// Channel configuration loading and validation.
pub mod config;
/// Matrix worker bootstrap.
pub mod matrix;
