//! Resolution + dispatch for slice actions. Loads the active slice list
//! (main config, possibly overridden by a per-app profile based on the
//! focused window class), and runs the slice's command on activation.

use juhradial_shared::{ActionKind, Slice};

/// Currently-active slice list. Refreshed when the config or the
/// per-app profile changes.
pub struct Slices {
    pub all: Vec<Slice>,
}

impl Slices {
    pub fn from_config(cfg: &juhradial_shared::AppConfig) -> Self {
        Slices {
            all: cfg.radial_menu.slices.clone(),
        }
    }
}

pub fn dispatch(slice: &Slice) {
    match slice.kind {
        ActionKind::Exec | ActionKind::Settings | ActionKind::Emoji => {
            // TODO: spawn slice.command via std::process::Command::new("sh").arg("-c")
            //       in a detached child so the overlay isn't blocked.
            //       Settings/Emoji are exec-shaped — same dispatch path.
        }
        ActionKind::Submenu => {
            // Submenu opens are handled in the radial widget, not here.
        }
        ActionKind::Macro => {
            // TODO: D-Bus call into the daemon to trigger macro by id.
        }
        ActionKind::Shortcut => {
            // TODO: D-Bus call into the daemon to send a key chord
            // via evdev/ydotool.
        }
        ActionKind::EasySwitch => {
            // TODO: D-Bus call into the daemon to switch host.
        }
        ActionKind::None => {}
    }
}
