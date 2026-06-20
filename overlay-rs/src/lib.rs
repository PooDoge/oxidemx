//! OxideMX overlay library — exposes `run()` (radial overlay) and
//! `run_chat_window()` (standalone chat window) for the two binaries.

pub mod actions;
pub mod activity;
pub mod agent;
pub mod agent_runtime;
pub mod ai_client;
pub(crate) mod anim;
pub mod app;
pub(crate) mod chat_shell;
pub(crate) mod chat_ui;
pub(crate) mod config;
pub(crate) mod dbus;
pub(crate) mod fonts;
pub mod editor {
    pub mod icon_picker;
    pub mod preview;
    pub mod slice_panel;
    pub mod window;
}
pub(crate) mod geometry;
pub(crate) mod handoff;
pub(crate) mod haptic_client;
pub(crate) mod input;
pub(crate) mod radial;
pub(crate) mod sampler;
pub(crate) mod render {
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
pub(crate) mod theme;
pub(crate) mod tray;
pub mod widget_host;

pub mod chat_window;

pub use app::run;
pub use app::run_chat_window;
