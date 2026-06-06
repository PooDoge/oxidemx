//! OxideMX MX indicator popup.
//!
//! Spawned by `oxidemxd` when it handles a `ShowPopup(x, y, w, h)`
//! D-Bus call from the GNOME Shell indicator. Opens a frameless,
//! always-on-top iced window positioned below the indicator icon via
//! the `oxidemx-cursor` extension's `MoveOverlay` D-Bus method.
//!
//! Lifecycle: the popup is a short-lived process. The daemon spawns it
//! on demand; the window dismisses itself when focus is lost or the
//! user clicks outside.

pub mod actions;
pub mod app;
pub mod config_watcher;
pub mod gsettings_bridge;
pub mod view;

// CLI args and window geometry — exported so app.rs can import from
// the crate root without `use crate::main::...`.
pub use cli::{Args, PanelRect};
pub const POPUP_W: f32 = 360.0;
pub const POPUP_H: f32 = 480.0;

pub mod cli {
    use clap::Parser;

    #[derive(Parser, Debug, Clone)]
    #[command(version)]
    pub struct Args {
        /// Indicator panel rect in stage-absolute logical pixels (x,y,w,h).
        /// Passed by the daemon's ShowPopup handler so we can position
        /// the popup tip below the indicator.
        #[arg(long, value_parser = parse_rect)]
        pub panel_rect: Option<PanelRect>,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct PanelRect {
        pub x: i32,
        pub y: i32,
        pub w: i32,
        pub h: i32,
    }

    pub fn parse_rect(s: &str) -> Result<PanelRect, String> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 4 {
            return Err(format!("expected x,y,w,h; got {s:?}"));
        }
        let parse_i =
            |s: &str| s.parse::<i32>().map_err(|e| format!("parse int {s:?}: {e}"));
        Ok(PanelRect {
            x: parse_i(parts[0])?,
            y: parse_i(parts[1])?,
            w: parse_i(parts[2])?,
            h: parse_i(parts[3])?,
        })
    }
}

use oxidemx_window::frameless_topmost;
use tracing::info;

const APP_ID: &str = "org.oxidemx.popup";

fn main() -> iced::Result {
    use clap::Parser;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    info!(?args, "oxidemx-popup starting");

    let window = frameless_topmost(APP_ID, iced::Size::new(POPUP_W, POPUP_H));
    iced::application(
        move || app::boot(args.clone()),
        app::update,
        app::view,
    )
    .title("OxideMX Popup")
    .window(window)
    .style(|_state, _theme| iced::theme::Style {
        background_color: iced::Color::TRANSPARENT,
        text_color: iced::Color::WHITE,
    })
    .subscription(app::subscription)
    .run()
}
