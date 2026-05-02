//! JuhRadial MX — Rust + GTK4 + layer-shell overlay.
//!
//! Replacement for the legacy Python overlay/. Same daemon, same D-Bus
//! contract (`org.kde.juhradialmx`), wildly simpler positioning thanks to
//! the Wayland layer-shell protocol.
//!
//! High-level flow:
//!
//!   1. `gtk4_layer_shell` initialises a transparent overlay surface
//!      anchored to the cursor's monitor.
//!   2. A `zbus` listener subscribes to `MenuRequested(x, y)` /
//!      `HideMenu()` from the daemon. On a request we set the layer-shell
//!      margins to position the surface at the cursor and `present()`
//!      the window.
//!   3. A `gtk::DrawingArea` paints the radial wheel with cairo. Hover
//!      and click hit-testing use cursor coords from the daemon (drag
//!      mode) or `gtk::EventControllerMotion` (toggle mode); both are
//!      monitor-local so there's no coord-space confusion.
//!
//! See `RUST_GTK4_OVERLAY_DESIGN.md` at the project root for the full
//! design rationale.

mod app;
mod window;
mod radial;
mod theme;
mod actions;
mod config;
mod dbus;
mod input;
mod tray;
mod render {
    pub mod slices;
    pub mod icons;
    pub mod animation;
}
mod editor {
    pub mod window;
    pub mod slice_panel;
    pub mod icon_picker;
    pub mod preview;
}

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    app::run()
}
