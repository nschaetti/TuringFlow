//! Pulse terminal UI primitives.
//!
//! This module exposes composable layout widgets and theme definitions used by
//! the text UI application.

/// Main Pulse application state and rendering entry-point.
pub mod app;
/// Container and layout orientation primitives.
pub mod layout;
/// Theme and color tokens.
pub mod theme;
/// Reusable widget nodes.
pub mod widget;

/// Re-export of [`app::PulseApp`].
pub use app::PulseApp;
/// Re-exports of layout primitives.
pub use layout::{Container, Orientation};
/// Re-exports of theme primitives.
pub use theme::{Theme, ThemeColors};
/// Re-exports of widget primitives.
pub use widget::{TextBox, WidgetNode};
