//! CLI command handlers.
//!
//! These modules are thin adapters from CLI argument parsing to runtime services.

/// Tool-calling arithmetic demo.
pub mod calc;
/// User-plane ingress command.
pub mod chat;
/// Direct queue inspection helper.
pub mod debug_user;
/// Embeddings command.
pub mod embeddings;
/// Vision/image command.
pub mod image;
/// User-plane outbound inbox command.
pub mod inbox;
/// Shared runtime wiring for command handlers.
pub mod runtime;
/// Agentic multimodal end-to-end demo.
pub mod test_agent2;
/// OpenAI-compatible variant of agentic multimodal demo.
pub mod test_agent2_openai;
