//! Haptic-feedback configuration for the MX Master 4 daemon.
//!
//! The legacy Python overlay stored this under `config.json`'s
//! `haptics` key:
//!   - `enabled`            (bool)
//!   - `default_pattern`    (one of HAPTIC_PATTERNS)
//!   - `per_event`          ({ menu_appear, slice_change, confirm,
//!                             invalid }) → pattern names
//!   - `debounce_ms`        (u32) overall trigger debounce
//!   - `slice_debounce_ms`  (u32) slice-change-specific debounce
//!   - `reentry_debounce_ms`(u32) re-entry into same slice debounce
//!
//! The 16 haptic patterns are the Logitech HID++ "haptic waveform"
//! IDs the daemon hands to the device. Names are stored as strings
//! so users can edit JSON by hand without breaking on enum drift.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HapticsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Fallback pattern used when an event has no per-event override.
    #[serde(default = "default_pattern")]
    pub default_pattern: String,

    /// Per-event pattern overrides keyed by event name. Missing keys
    /// fall back to `default_pattern`.
    #[serde(default = "default_per_event")]
    pub per_event: PerEventPatterns,

    /// Minimum gap between any two haptic triggers (ms).
    #[serde(default = "default_debounce")]
    pub debounce_ms: u32,

    /// Specific debounce for cross-slice transitions.
    #[serde(default = "default_slice_debounce")]
    pub slice_debounce_ms: u32,

    /// Specific debounce for re-entry into the same slice.
    #[serde(default = "default_reentry_debounce")]
    pub reentry_debounce_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerEventPatterns {
    #[serde(default = "default_menu_appear_pattern")]
    pub menu_appear: String,
    #[serde(default = "default_slice_change_pattern")]
    pub slice_change: String,
    #[serde(default = "default_confirm_pattern")]
    pub confirm: String,
    #[serde(default = "default_invalid_pattern")]
    pub invalid: String,
}

impl Default for PerEventPatterns {
    fn default() -> Self {
        PerEventPatterns {
            menu_appear: default_menu_appear_pattern(),
            slice_change: default_slice_change_pattern(),
            confirm: default_confirm_pattern(),
            invalid: default_invalid_pattern(),
        }
    }
}

fn default_enabled() -> bool {
    true
}
fn default_pattern() -> String {
    "subtle_collision".into()
}
fn default_menu_appear_pattern() -> String {
    "damp_state_change".into()
}
fn default_slice_change_pattern() -> String {
    "subtle_collision".into()
}
fn default_confirm_pattern() -> String {
    "sharp_state_change".into()
}
fn default_invalid_pattern() -> String {
    "angry_alert".into()
}
fn default_per_event() -> PerEventPatterns {
    PerEventPatterns::default()
}
fn default_debounce() -> u32 {
    20
}
fn default_slice_debounce() -> u32 {
    20
}
fn default_reentry_debounce() -> u32 {
    50
}

impl Default for HapticsConfig {
    fn default() -> Self {
        HapticsConfig {
            enabled: default_enabled(),
            default_pattern: default_pattern(),
            per_event: default_per_event(),
            debounce_ms: default_debounce(),
            slice_debounce_ms: default_slice_debounce(),
            reentry_debounce_ms: default_reentry_debounce(),
        }
    }
}

/// All MX Master 4 haptic waveforms — name + display label +
/// short description. The daemon resolves the name to a HID++
/// waveform ID; the settings UI shows the display label in the
/// pattern picker.
pub const HAPTIC_PATTERNS: &[(&str, &str, &str)] = &[
    ("sharp_state_change", "Sharp Click", "Crisp, sharp feedback"),
    ("damp_state_change", "Soft Click", "Softer, dampened feedback"),
    ("sharp_collision", "Sharp Bump", "Strong collision feedback"),
    ("damp_collision", "Soft Bump", "Gentle collision feedback"),
    ("subtle_collision", "Subtle", "Very light, subtle feedback"),
    ("whisper_collision", "Whisper", "Barely perceptible feedback"),
    ("happy_alert", "Happy", "Positive notification feel"),
    ("angry_alert", "Alert", "Warning / error feel"),
    ("completed", "Complete", "Success / completion feel"),
    ("square", "Square Wave", "Mechanical square pattern"),
    ("wave", "Wave", "Smooth wave pattern"),
    ("firework", "Firework", "Burst pattern"),
    ("mad", "Strong Alert", "Strong error pattern"),
    ("knock", "Knock", "Knocking pattern"),
    ("jingle", "Jingle", "Musical jingle pattern"),
    ("ringing", "Ringing", "Ring / vibrate pattern"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_object_picks_up_defaults() {
        let cfg: HapticsConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.default_pattern, "subtle_collision");
        assert_eq!(cfg.per_event.menu_appear, "damp_state_change");
        assert_eq!(cfg.debounce_ms, 20);
    }

    #[test]
    fn partial_per_event_keeps_other_defaults() {
        let json = r#"{ "per_event": { "confirm": "happy_alert" } }"#;
        let cfg: HapticsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.per_event.confirm, "happy_alert");
        // Other keys still default.
        assert_eq!(cfg.per_event.menu_appear, "damp_state_change");
    }
}
