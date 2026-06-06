//! Window-shell helpers for OxideMX's iced-rendered surfaces.
//!
//! Two modules:
//!   * [`settings`] — `frameless_topmost(app_id, size)` builds a
//!     decorationless, transparent, always-on-top, non-resizable
//!     `iced::window::Settings` keyed by the wayland app_id (which
//!     becomes the xdg-toplevel app_id on Wayland).
//!   * [`cursor_helper`] — async client for the `oxidemx-cursor`
//!     GNOME shell extension's `org.oxidemx.CursorHelper` D-Bus
//!     service. The extension positions our windows via
//!     `MoveOverlay` since Mutter does not advertise
//!     `wlr-layer-shell` on stable GNOME.
//!
//! Consumed by `overlay-rs` (the radial menu overlay) and
//! `popup-rs` (the indicator-spawned popup).

pub mod cursor_helper;
pub mod settings;

// Re-export the most common helper at crate root for ergonomics.
pub use settings::frameless_topmost;
