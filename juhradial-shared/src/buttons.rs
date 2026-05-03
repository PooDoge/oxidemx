//! Mouse button assignments — what each MX Master 4 button does
//! when pressed. The daemon's `daemon::config::ButtonsConfig`
//! deserialises from the same JSON shape; both crates ship their
//! own copy so neither has to depend on the other (settings is
//! the editor; daemon is the consumer).

use serde::{Deserialize, Serialize};

/// Action a mouse button can fire. String tag matches the daemon's
/// enum variants (lowercase + underscore) so the on-disk config
/// stays compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonAction {
    RadialMenu,
    VirtualDesktops,
    MiddleClick,
    Back,
    Forward,
    Copy,
    Paste,
    Undo,
    Redo,
    Screenshot,
    Smartshift,
    ScrollLeftRight,
    VolumeUp,
    VolumeDown,
    PlayPause,
    Mute,
    ZoomIn,
    ZoomOut,
    None,
    Custom,
}

impl ButtonAction {
    /// Human-friendly label for the picker UI.
    pub fn label(self) -> &'static str {
        match self {
            ButtonAction::RadialMenu => "Radial Menu",
            ButtonAction::VirtualDesktops => "Virtual Desktops",
            ButtonAction::MiddleClick => "Middle Click",
            ButtonAction::Back => "Back",
            ButtonAction::Forward => "Forward",
            ButtonAction::Copy => "Copy",
            ButtonAction::Paste => "Paste",
            ButtonAction::Undo => "Undo",
            ButtonAction::Redo => "Redo",
            ButtonAction::Screenshot => "Screenshot",
            ButtonAction::Smartshift => "SmartShift",
            ButtonAction::ScrollLeftRight => "Scroll Left / Right",
            ButtonAction::VolumeUp => "Volume Up",
            ButtonAction::VolumeDown => "Volume Down",
            ButtonAction::PlayPause => "Play / Pause",
            ButtonAction::Mute => "Mute",
            ButtonAction::ZoomIn => "Zoom In",
            ButtonAction::ZoomOut => "Zoom Out",
            ButtonAction::None => "Do Nothing",
            ButtonAction::Custom => "Custom…",
        }
    }

    /// Full enumeration in the order the picker should display.
    /// `Custom` is intentionally excluded — that variant requires a
    /// shell-out / per-action editor that isn't built yet.
    pub fn all() -> &'static [ButtonAction] {
        &[
            ButtonAction::RadialMenu,
            ButtonAction::VirtualDesktops,
            ButtonAction::MiddleClick,
            ButtonAction::Back,
            ButtonAction::Forward,
            ButtonAction::Smartshift,
            ButtonAction::ScrollLeftRight,
            ButtonAction::Copy,
            ButtonAction::Paste,
            ButtonAction::Undo,
            ButtonAction::Redo,
            ButtonAction::Screenshot,
            ButtonAction::VolumeUp,
            ButtonAction::VolumeDown,
            ButtonAction::PlayPause,
            ButtonAction::Mute,
            ButtonAction::ZoomIn,
            ButtonAction::ZoomOut,
            ButtonAction::None,
        ]
    }
}

/// Per-button action assignments. Matches the "buttons" section in
/// config.json that the daemon reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonsConfig {
    #[serde(default = "default_gesture")]
    pub gesture: ButtonAction,
    #[serde(default = "default_thumb")]
    pub thumb: ButtonAction,
    #[serde(default = "default_middle")]
    pub middle: ButtonAction,
    #[serde(default = "default_shift_wheel")]
    pub shift_wheel: ButtonAction,
    #[serde(default = "default_forward")]
    pub forward: ButtonAction,
    #[serde(default = "default_back")]
    pub back: ButtonAction,
    #[serde(default = "default_horizontal_scroll")]
    pub horizontal_scroll: ButtonAction,
}

fn default_gesture() -> ButtonAction {
    ButtonAction::VirtualDesktops
}
fn default_thumb() -> ButtonAction {
    ButtonAction::RadialMenu
}
fn default_middle() -> ButtonAction {
    ButtonAction::MiddleClick
}
fn default_shift_wheel() -> ButtonAction {
    ButtonAction::Smartshift
}
fn default_forward() -> ButtonAction {
    ButtonAction::Forward
}
fn default_back() -> ButtonAction {
    ButtonAction::Back
}
fn default_horizontal_scroll() -> ButtonAction {
    ButtonAction::ScrollLeftRight
}

impl Default for ButtonsConfig {
    fn default() -> Self {
        ButtonsConfig {
            gesture: default_gesture(),
            thumb: default_thumb(),
            middle: default_middle(),
            shift_wheel: default_shift_wheel(),
            forward: default_forward(),
            back: default_back(),
            horizontal_scroll: default_horizontal_scroll(),
        }
    }
}

/// Stable identifier for each MX Master 4 button. Kept as an enum
/// so the editor UI can route Set messages without stringly-typed
/// keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Gesture,
    Thumb,
    Middle,
    ShiftWheel,
    Forward,
    Back,
    HorizontalScroll,
}

impl MouseButton {
    pub fn label(self) -> &'static str {
        match self {
            MouseButton::Gesture => "Gestures",
            MouseButton::Thumb => "Show Actions Ring",
            MouseButton::Middle => "Middle Button",
            MouseButton::ShiftWheel => "Shift Wheel Mode",
            MouseButton::Forward => "Forward",
            MouseButton::Back => "Back",
            MouseButton::HorizontalScroll => "Horizontal Scroll",
        }
    }

    pub fn all() -> &'static [MouseButton] {
        &[
            MouseButton::Middle,
            MouseButton::ShiftWheel,
            MouseButton::Forward,
            MouseButton::HorizontalScroll,
            MouseButton::Back,
            MouseButton::Gesture,
            MouseButton::Thumb,
        ]
    }

    pub fn get(self, cfg: &ButtonsConfig) -> ButtonAction {
        match self {
            MouseButton::Gesture => cfg.gesture,
            MouseButton::Thumb => cfg.thumb,
            MouseButton::Middle => cfg.middle,
            MouseButton::ShiftWheel => cfg.shift_wheel,
            MouseButton::Forward => cfg.forward,
            MouseButton::Back => cfg.back,
            MouseButton::HorizontalScroll => cfg.horizontal_scroll,
        }
    }

    pub fn set(self, cfg: &mut ButtonsConfig, value: ButtonAction) {
        match self {
            MouseButton::Gesture => cfg.gesture = value,
            MouseButton::Thumb => cfg.thumb = value,
            MouseButton::Middle => cfg.middle = value,
            MouseButton::ShiftWheel => cfg.shift_wheel = value,
            MouseButton::Forward => cfg.forward = value,
            MouseButton::Back => cfg.back = value,
            MouseButton::HorizontalScroll => cfg.horizontal_scroll = value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_round_trip() {
        let cfg = ButtonsConfig {
            shift_wheel: ButtonAction::Smartshift,
            horizontal_scroll: ButtonAction::ScrollLeftRight,
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"shift_wheel\":\"smartshift\""));
        assert!(json.contains("\"horizontal_scroll\":\"scroll_left_right\""));
    }
}
