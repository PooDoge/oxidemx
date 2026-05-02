//! JuhRadial MX overlay (Rust + iced + xdg-shell).
//!
//! Replacement for the legacy Python overlay/. Same daemon, same
//! D-Bus contract (`org.kde.juhradialmx`). Mutter doesn't advertise
//! `wlr-layer-shell` on stable GNOME, so positioning is delegated to
//! the `juhradial-cursor` GNOME Shell extension's `MoveOverlay`
//! D-Bus method — the overlay is a regular xdg-shell window that
//! the extension places exactly where we want it after each show.
//!
//! Flow:
//!   1. iced application opens a transparent, decorationless,
//!      always-on-top xdg-shell window.
//!   2. zbus listener subscribes to `MenuRequested(x, y)` /
//!      `HideMenu()` / `CursorMoved(dx, dy)` from the daemon.
//!   3. On a request: present the window, fire `MoveOverlay` to the
//!      cursor position, paint the radial.
//!   4. On hide: hide the window.
//!
//! All rendering happens via iced's `canvas::Frame` — no cairo, no
//! GTK, no `*-devel` rpm-ostree layering. Pure Rust dep tree.

mod actions;
mod app;
mod config;
mod dbus;
mod editor {
    pub mod icon_picker;
    pub mod preview;
    pub mod slice_panel;
    pub mod window;
}
mod ext_positioner;
mod geometry;
mod input;
mod radial;
mod render {
    pub mod animation;
    pub mod icons;
    pub mod slices;
}
mod theme;
mod tray;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    app::run()
}
