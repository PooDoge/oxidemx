//! OxideMX overlay (Rust + iced + xdg-shell).
//!
//! Replacement for the legacy Python overlay/. Same daemon, same
//! D-Bus contract (`org.oxidemx.Daemon`). Mutter doesn't advertise
//! `wlr-layer-shell` on stable GNOME, so positioning is delegated to
//! the `oxidemx-cursor` GNOME Shell extension's `MoveOverlay`
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
mod agent;
mod ai_client;
mod anim;
mod app;
mod chat_shell;
mod chat_ui;
mod config;
mod dbus;
mod fonts;
mod editor {
    pub mod icon_picker;
    pub mod preview;
    pub mod slice_panel;
    pub mod window;
}
mod geometry;
mod handoff;
mod haptic_client;
mod input;
mod radial;
mod sampler;
mod render {
    pub mod animation;
    pub mod aurora;
    pub mod center_dome;
    pub mod disc_bevel;
    pub mod dispatch_burst;
    pub mod drop_shadow;
    pub mod hover_glow;
    pub mod hover_tilt;
    pub mod icons;
    pub mod page_fx;
    pub mod ripple;
    pub mod sdf_ring;
    pub mod slice_bevel;
    pub mod slices;
    pub mod specular_sweep;
    pub mod status_fx;
}
mod theme;
mod tray;
mod widget_host;

fn main() -> iced::Result {
    // Default filter: info-level for our crates, error-only for usvg
    // (which spams "Failed to parse marker-start value: 'none'." for
    // every freedesktop icon — the parser warns on perfectly valid
    // CSS the icons use, and the rendering is unaffected).
    let default_filter = "info,usvg=error";
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter)),
        )
        .init();

    // Debug-only smoke hook (`scripts/widget-smoke.sh`): boot the
    // widget-host worker without iced, print the first scene
    // revision, exit. Compiled out of release builds.
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--widget-smoke") {
        std::process::exit(widget_host::run_smoke());
    }

    app::run()
}
