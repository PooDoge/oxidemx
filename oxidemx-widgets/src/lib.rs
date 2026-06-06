//! Shared iced widget primitives, palette-keyed style closures, and
//! the catppuccin-style colour palette OxideMX UIs render against.
//!
//! Three modules, one responsibility each:
//!   * [`widgets`] - small composite widgets (labeled sliders,
//!     section headers, etc.).
//!   * [`style`] - palette-keyed `Fn(&Theme) -> Style` closures
//!     for container / button / slider / toggler.
//!   * [`palette`] - the colour palette plus accent resolution
//!     helpers.
//!
//! Originally lived inside `settings-rs/`; extracted in 2026-05 so
//! `popup-rs/` (and later `overlay-rs/`) could import the same
//! primitives without copy-paste.

pub mod palette;
pub mod style;
pub mod widgets;
