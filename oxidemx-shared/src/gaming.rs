//! Gaming-mode configuration shared by the daemon, settings UI, and
//! (future) overlay.
//!
//! Today this carries only the haptic-redirect knobs. The other
//! gaming-mode state (DPI profiles, overlay suppression) lives
//! daemon-side in `daemon/src/gaming.rs` + `daemon/src/macros/dpi.rs`.
//! That history is the reason the master Game-Mode toggle is *not*
//! in this struct — the toggle is a runtime D-Bus call
//! (`SetGamingMode(bool)`) rather than a persisted config field.
//!
//! See `oxidemx/HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` for the
//! full design of the haptic redirect feature.

use serde::{Deserialize, Serialize};

/// Top-level gaming-mode configuration block. Lives under
/// `AppConfig::gaming`; everything inside opts in (defaults are
/// inert so existing configs and users who don't game keep the
/// current behaviour).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GamingConfig {
    /// Gamepad rumble → MX Master 4 haptic redirect.
    #[serde(default)]
    pub haptic_redirect: HapticRedirectConfig,
}

// ────────────────────────────────────────────────────────────────────
// Haptic redirect
// ────────────────────────────────────────────────────────────────────

/// Configuration for the gamepad-rumble → mouse-haptic bridge.
///
/// Daemon module: `daemon/src/gamepad_haptics/` (to be added — see
/// `HAPTIC_GAMEPAD_BRIDGE_DESIGN.md`). The settings UI writes this
/// block; the daemon hot-reloads on inotify and applies most fields
/// without re-creating the virtual gamepad. The fields that *do*
/// force a virtual-device rebuild are called out below.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HapticRedirectConfig {
    /// Master switch for the bridge. Even while Game Mode is on, the
    /// bridge stays inert unless this is true — opt-in by design.
    /// **Rebuild trigger.**
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// `Proxy` (default) wraps a real controller and forwards its
    /// inputs while capturing rumble. `Standalone` creates the
    /// virtual pad with no real-pad backing — useful when playing
    /// keyboard-and-mouse against a game that lets the user pick a
    /// "rumble device" explicitly. **Rebuild trigger.**
    #[serde(default = "default_mode")]
    pub mode: HapticRedirectMode,

    /// SDL GUID (32 hex chars) of the real controller to wrap in
    /// `Proxy` mode. `None` = auto-pick the first joystick discovered
    /// via udev. **Rebuild trigger (in Proxy mode only).**
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_controller_guid: Option<String>,

    /// Multiplier applied to the combined rumble intensity before
    /// pattern selection. 1.0 = faithful, 0.5 = quieter, 1.5 = louder
    /// (clamped at 2.0; output still maxes at the strongest pattern).
    #[serde(default = "default_intensity_scale")]
    pub intensity_scale: f32,

    /// Combined intensity below this threshold is dropped instead of
    /// firing a haptic pulse. Filters out the quiet sustained rumble
    /// some games emit during idle states — left unfiltered it
    /// becomes a non-stop low-level buzz on the piezo.
    #[serde(default = "default_min_intensity")]
    pub min_intensity: f32,

    /// Weight on the low-frequency motor (`strong_magnitude`) when
    /// combining the two rumble channels into a single haptic
    /// intensity. The piezo only renders one channel, so we mix.
    /// Default leans on the strong channel because most games put
    /// the "feel" rumble there.
    #[serde(default = "default_strong_weight")]
    pub strong_weight: f32,

    /// Weight on the high-frequency motor (`weak_magnitude`).
    #[serde(default = "default_weak_weight")]
    pub weak_weight: f32,

    /// Maps combined intensity (0..1) to a pattern tier. See
    /// [`HapticRedirectCurve`].
    #[serde(default = "default_curve")]
    pub curve: HapticRedirectCurve,

    /// `Event` (default) emits one pulse per rumble play; continuous
    /// rumble re-pulses every `throttle_ms`. `Stream` emits a
    /// rapid-tick pattern with spacing inversely proportional to
    /// intensity — feels like a fast ratchet. See [`HapticEventMode`].
    #[serde(default = "default_event_mode")]
    pub event_mode: HapticEventMode,

    /// Minimum gap between consecutive haptic pulses (ms). Caps the
    /// rate at which we drive the piezo actuator — over-driving
    /// makes it feel buzzy and ugly. Default matches SDL2's
    /// `RUMBLE_WRITE_FREQUENCY_MS`.
    #[serde(default = "default_throttle_ms")]
    pub throttle_ms: u16,

    /// When `true` and `mode == Proxy`, also forward the rumble FF
    /// effect to the real controller so the user feels both haptic
    /// channels. Off by default (most users redirect *precisely*
    /// because they're not holding the controller).
    #[serde(default = "default_passthrough_to_pad")]
    pub passthrough_to_pad: bool,

    /// Drop a transient udev rule that sets `ID_INPUT_JOYSTICK=0` on
    /// the real controller while gaming mode is on, hiding it from
    /// non-grabbed enumeration paths (some non-SDL games / engines).
    /// Off by default; flipping requires a brief udev re-trigger
    /// cycle that can disconnect-reconnect the controller.
    /// **Rebuild trigger.**
    #[serde(default = "default_hard_hide_real_controller")]
    pub hard_hide_real_controller: bool,

    /// Period (seconds) at which the bridge emits a sub-deadzone
    /// trigger pulse on the virtual pad, to wake games' "last input
    /// was a gamepad" detection. Many engines silently suppress
    /// `FF_UPLOAD` when KB+M was the last input, so the bridge sees
    /// no rumble even though everything is wired correctly.
    ///
    /// `0` = disabled (default). A small interval (5–10 s) is enough
    /// for most engines; values above 60 are clamped.
    ///
    /// The pulse touches `ABS_RZ` (right trigger) by value 1 then
    /// back to 0 — well below any trigger-press threshold, so it
    /// won't fire weapon-trigger actions, but the event itself is
    /// what flips the input-source heuristic.
    #[serde(default = "default_keep_gamepad_active_secs")]
    pub keep_gamepad_active_secs: u16,
}

impl Default for HapticRedirectConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            mode: default_mode(),
            preferred_controller_guid: None,
            intensity_scale: default_intensity_scale(),
            min_intensity: default_min_intensity(),
            strong_weight: default_strong_weight(),
            weak_weight: default_weak_weight(),
            curve: default_curve(),
            event_mode: default_event_mode(),
            throttle_ms: default_throttle_ms(),
            passthrough_to_pad: default_passthrough_to_pad(),
            hard_hide_real_controller: default_hard_hide_real_controller(),
            keep_gamepad_active_secs: default_keep_gamepad_active_secs(),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// Enums
// ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HapticRedirectMode {
    /// Wrap a real controller via udev discovery + `EVIOCGRAB`; the
    /// game sees only our virtual pad. Inputs forward real→virtual,
    /// rumble flows back through us.
    Proxy,
    /// Create the virtual gamepad with no real-pad backing. Game
    /// sees only our pad (axes/buttons idle at neutral). Useful for
    /// keyboard-and-mouse play against games that expose a "rumble
    /// device" picker.
    Standalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HapticRedirectCurve {
    /// Pattern tier scales linearly with combined intensity.
    /// Faithful — what you'd expect from a "transparent" redirect.
    Linear,
    /// Quadratic accent on peaks. Short bursts feel sharper,
    /// sustained mid-magnitude rumble feels lighter than linear.
    /// Best for action games (gunfire, impacts).
    Eventy,
    /// Logarithmic compression with a ceiling around 70 % of the
    /// strongest pattern. Best for long sessions / ambient rumble.
    Subtle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HapticEventMode {
    /// One haptic pulse per `EV_FF play` event; continuous rumble
    /// re-pulses on the throttle interval.
    Event,
    /// Model the actuator as a low-rate vibration source — emit
    /// `Tick` patterns at a rate proportional to intensity (high
    /// intensity = ~20 ms spacing, low intensity = ~80 ms spacing).
    /// Feels like a fast ratchet; some users prefer it for racing.
    Stream,
}

// ────────────────────────────────────────────────────────────────────
// Display — settings UI's pick_list renders via `Display`.
// ────────────────────────────────────────────────────────────────────

impl HapticRedirectMode {
    pub const ALL: [HapticRedirectMode; 2] =
        [HapticRedirectMode::Proxy, HapticRedirectMode::Standalone];
}

impl std::fmt::Display for HapticRedirectMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HapticRedirectMode::Proxy => "Proxy a real controller",
            HapticRedirectMode::Standalone => "Standalone (no real pad)",
        })
    }
}

impl HapticRedirectCurve {
    pub const ALL: [HapticRedirectCurve; 3] = [
        HapticRedirectCurve::Eventy,
        HapticRedirectCurve::Linear,
        HapticRedirectCurve::Subtle,
    ];
}

impl std::fmt::Display for HapticRedirectCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HapticRedirectCurve::Eventy => "Eventy",
            HapticRedirectCurve::Linear => "Linear",
            HapticRedirectCurve::Subtle => "Subtle",
        })
    }
}

impl HapticEventMode {
    pub const ALL: [HapticEventMode; 2] = [HapticEventMode::Event, HapticEventMode::Stream];
}

impl std::fmt::Display for HapticEventMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HapticEventMode::Event => "Event",
            HapticEventMode::Stream => "Stream",
        })
    }
}

// ────────────────────────────────────────────────────────────────────
// Defaults
// ────────────────────────────────────────────────────────────────────

fn default_enabled() -> bool {
    false
}
fn default_mode() -> HapticRedirectMode {
    HapticRedirectMode::Proxy
}
fn default_intensity_scale() -> f32 {
    1.0
}
fn default_min_intensity() -> f32 {
    // Many games normalise rumble to small u16 magnitudes (low-tier
    // ambient effects, light environmental hits). A 0.08 floor was
    // muting them entirely — 0.04 lets sub-tier rumble through and
    // covers titles like SW Squadrons / Helldivers 2 that send a
    // steady stream of small events.
    0.04
}
fn default_strong_weight() -> f32 {
    1.0
}
fn default_weak_weight() -> f32 {
    0.4
}
fn default_curve() -> HapticRedirectCurve {
    HapticRedirectCurve::Eventy
}
fn default_event_mode() -> HapticEventMode {
    HapticEventMode::Event
}
fn default_throttle_ms() -> u16 {
    30
}
fn default_passthrough_to_pad() -> bool {
    false
}
fn default_hard_hide_real_controller() -> bool {
    false
}
fn default_keep_gamepad_active_secs() -> u16 {
    0
}

// ────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let cfg = GamingConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: GamingConfig = serde_json::from_str(&json).unwrap();
        // Round-trip preserves every default we documented.
        assert!(!back.haptic_redirect.enabled);
        assert_eq!(back.haptic_redirect.mode, HapticRedirectMode::Proxy);
        assert!(back.haptic_redirect.preferred_controller_guid.is_none());
        assert_eq!(back.haptic_redirect.intensity_scale, 1.0);
        assert_eq!(back.haptic_redirect.min_intensity, 0.04);
        assert_eq!(back.haptic_redirect.strong_weight, 1.0);
        assert_eq!(back.haptic_redirect.weak_weight, 0.4);
        assert_eq!(back.haptic_redirect.curve, HapticRedirectCurve::Eventy);
        assert_eq!(back.haptic_redirect.event_mode, HapticEventMode::Event);
        assert_eq!(back.haptic_redirect.throttle_ms, 30);
        assert!(!back.haptic_redirect.passthrough_to_pad);
        assert!(!back.haptic_redirect.hard_hide_real_controller);
        assert_eq!(back.haptic_redirect.keep_gamepad_active_secs, 0);
    }

    #[test]
    fn missing_block_takes_defaults() {
        // No `gaming` block in the JSON — should deserialise as
        // GamingConfig::default() via `#[serde(default)]` on AppConfig.
        let cfg: GamingConfig = serde_json::from_str("{}").unwrap();
        assert!(!cfg.haptic_redirect.enabled);
        assert_eq!(cfg.haptic_redirect.mode, HapticRedirectMode::Proxy);
    }

    #[test]
    fn partial_haptic_redirect_block_fills_defaults() {
        // User wrote half the block by hand — missing fields default.
        let json = r#"{
            "haptic_redirect": {
                "enabled": true,
                "curve": "linear"
            }
        }"#;
        let cfg: GamingConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.haptic_redirect.enabled);
        assert_eq!(cfg.haptic_redirect.curve, HapticRedirectCurve::Linear);
        // Untouched fields still get their defaults.
        assert_eq!(cfg.haptic_redirect.throttle_ms, 30);
        assert_eq!(cfg.haptic_redirect.strong_weight, 1.0);
        assert_eq!(cfg.haptic_redirect.mode, HapticRedirectMode::Proxy);
    }

    #[test]
    fn enum_variants_use_snake_case_on_disk() {
        let cfg = HapticRedirectConfig {
            mode: HapticRedirectMode::Standalone,
            curve: HapticRedirectCurve::Subtle,
            event_mode: HapticEventMode::Stream,
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains(r#""mode":"standalone""#));
        assert!(json.contains(r#""curve":"subtle""#));
        assert!(json.contains(r#""event_mode":"stream""#));
    }

    #[test]
    fn preferred_controller_guid_omitted_when_none() {
        let cfg = HapticRedirectConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        // skip_serializing_if = "Option::is_none" — keeps the JSON tidy.
        assert!(!json.contains("preferred_controller_guid"));
    }

    #[test]
    fn preferred_controller_guid_round_trips() {
        let cfg = HapticRedirectConfig {
            preferred_controller_guid: Some("030000005e040000130b000005ff0000".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: HapticRedirectConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.preferred_controller_guid.as_deref(),
            Some("030000005e040000130b000005ff0000"),
        );
    }
}
