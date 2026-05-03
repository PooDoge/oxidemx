//! Pointer + scroll configuration. Mirrors the legacy Python
//! overlay's `pointer` and `scroll` keys in config.json so the
//! daemon can read this struct without translation.

use serde::{Deserialize, Serialize};

/// Pointer behaviour. `speed` is a coarse 1..20 dial (the legacy
/// UI's slider range); the daemon maps it to the actual libinput /
/// HID++ acceleration profile when applying.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointerConfig {
    #[serde(default = "default_speed")]
    pub speed: u32,
    #[serde(default = "default_acceleration")]
    pub acceleration: bool,
}

fn default_speed() -> u32 {
    10
}
fn default_acceleration() -> bool {
    true
}

impl Default for PointerConfig {
    fn default() -> Self {
        PointerConfig {
            speed: default_speed(),
            acceleration: default_acceleration(),
        }
    }
}

/// Scroll wheel behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollConfig {
    /// Reverse scroll direction (macOS-style "natural" scrolling).
    #[serde(default)]
    pub natural: bool,
    /// Smooth scrolling vs ratcheted.
    #[serde(default = "default_smooth")]
    pub smooth: bool,
    /// MX-specific: SmartShift (auto-disengaged ratchet) on/off.
    #[serde(default = "default_smartshift")]
    pub smartshift: bool,
    /// Threshold (rotational velocity) at which SmartShift releases
    /// the ratchet. Daemon picks units; UI shows 1..100.
    #[serde(default = "default_smartshift_threshold")]
    pub smartshift_threshold: u32,
    /// Wheel mode: "smartshift" | "ratchet" | "free". Stored as
    /// string so editor UIs can ship new modes without enum churn.
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_smooth() -> bool {
    true
}
fn default_smartshift() -> bool {
    true
}
fn default_smartshift_threshold() -> u32 {
    50
}
fn default_mode() -> String {
    "smartshift".into()
}

impl Default for ScrollConfig {
    fn default() -> Self {
        ScrollConfig {
            natural: false,
            smooth: default_smooth(),
            smartshift: default_smartshift(),
            smartshift_threshold: default_smartshift_threshold(),
            mode: default_mode(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_object_picks_up_defaults() {
        let p: PointerConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(p.speed, 10);
        assert!(p.acceleration);
        let s: ScrollConfig = serde_json::from_str("{}").unwrap();
        assert!(s.smooth);
        assert!(s.smartshift);
        assert_eq!(s.mode, "smartshift");
    }
}
