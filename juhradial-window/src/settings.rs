//! Frameless, transparent, always-on-top xdg-shell window settings.
//!
//! Returns an `iced::window::Settings` shaped for an overlay-style
//! window: no decorations, transparent root (so the rounded corners
//! drawn by the outermost container reveal the desktop underneath),
//! non-resizable, always on top. Caller supplies the wayland
//! `app_id` (which becomes the xdg-toplevel app_id and lets the
//! `juhradial-cursor` extension find the window for positioning)
//! and the size in logical pixels.

use iced::window::{Level, Settings};

pub fn frameless_topmost(app_id: &str, size: iced::Size) -> Settings {
    let mut s = Settings::default();
    s.size = size;
    s.decorations = false;
    s.transparent = true;
    s.resizable = false;
    s.level = Level::AlwaysOnTop;
    s.platform_specific.application_id = app_id.to_string();
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
        assert!(!s.resizable, "resizable must be off");
        assert_eq!(s.platform_specific.application_id, "org.example.test");
        assert_eq!(s.size, iced::Size::new(400.0, 300.0));
        match s.level {
            Level::AlwaysOnTop => {}
            _ => panic!("level must be AlwaysOnTop"),
        }
    }
}
