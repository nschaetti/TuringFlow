//! Lightweight LLM integration helpers.
//!
//! The module contains typed wrappers for chat models, embeddings, tool calls,
//! and multimodal helpers used by CLI commands and experiments.

/// Generic AI response traits and structures.
pub mod ai;
/// Chat model client abstractions.
pub mod chat_models;
/// Embedding model client abstractions.
pub mod embeddings;
/// Human/user message helper types.
pub mod human;
/// Tool schema and invocation payload helpers.
pub mod tools;
