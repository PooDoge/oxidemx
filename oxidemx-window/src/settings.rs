//! Frameless, transparent, always-on-top xdg-shell window settings.
//!
//! Returns an `iced::window::Settings` shaped for an overlay-style
//! window: no decorations, transparent root (so the rounded corners
//! drawn by the outermost container reveal the desktop underneath),
//! non-resizable, always on top. Caller supplies the wayland
//! `app_id` (which becomes the xdg-toplevel app_id and lets the
//! `oxidemx-cursor` extension find the window for positioning)
//! and the size in logical pixels.

use iced::window::{Level, Settings};

pub fn frameless_topmost(app_id: &str, size: iced::Size) -> Settings {
    let mut s = Settings {
        size,
        decorations: false,
        transparent: true,
        // Must be true even though the surface has no decorations:
        // `resizable: false` makes winit pin min = max = initial size
        // on Wayland, so a programmatic `window::resize` (the chat's
        // grip commit) changes the compositor-side viewport while
        // iced never re-lays-out — the old buffer gets stretched
        // over the new window. With resizable on, the configure
        // round trip reaches iced and the surface really resizes.
        resizable: true,
        // Floor for the native grip resize (xdg_toplevel.resize):
        // the disc needs its 484 px square and the chat contract
        // sets 560 as the height minimum. The compositor enforces
        // this during the interactive resize.
        min_size: Some(iced::Size::new(484.0, 560.0)),
        level: Level::AlwaysOnTop,
        position: iced::window::Position::Centered,
        ..Settings::default()
    };
    s.platform_specific.application_id = app_id.to_string();
    s.platform_specific.override_redirect = true;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::window::Level;

    #[test]
    fn produces_correct_flags() {
        let s = frameless_topmost("org.example.test", iced::Size::new(400.0, 300.0));
        assert!(!s.decorations, "decorations must be off");
        assert!(s.transparent, "transparent must be on");
        assert!(
            s.resizable,
            "resizable must be on (programmatic chat resize)"
        );
        assert_eq!(s.platform_specific.application_id, "org.example.test");
        assert_eq!(s.size, iced::Size::new(400.0, 300.0));
        match s.level {
            Level::AlwaysOnTop => {}
            _ => panic!("level must be AlwaysOnTop"),
        }
    }
}
