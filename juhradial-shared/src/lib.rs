//! Types shared between the JuhRadial MX daemon and the Rust overlay.
//!
//! The user-facing configuration lives in `~/.config/juhradial/config.json`.
//! The daemon writes it (via the settings GUI), watches it via inotify, and
//! reads it for action dispatch. The overlay also reads it for slice labels,
//! colours, icons, and submenu structure. Keeping the structs here means
//! both sides decode identical JSON without drift.
//!
//! The first iteration of this crate is intentionally minimal — only the
//! subset of config the new Rust overlay actually needs. Daemon-only fields
//! (haptics, button-action mapping, easy-switch hosts, etc.) stay in
//! `daemon/src/config.rs` and will migrate over here as they're touched.

pub mod action;
pub mod applications;
pub mod config;
pub mod profiles;
pub mod theme;

pub use action::ActionKind;
pub use applications::{
    clean_exec_line, enumerate_applications, parse_desktop_file, parse_desktop_string,
    search as search_applications, DesktopEntry,
};
pub use config::{AppConfig, RadialMenuConfig, Slice};
pub use profiles::ProfileResolver;
pub use theme::ThemeName;
