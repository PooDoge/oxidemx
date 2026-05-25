//! Indicator-popup preferences. Persisted as the `popup` table in
//! the same `~/.config/juhradial/config.json` everything else lives
//! in (one file = one inotify event = one reload).
//!
//! Read by:
//!   * settings-rs (Indicator Popup tab — edits + saves)
//!   * popup-rs    (displays the popup using these knobs)
//!
//! NOT read by the GNOME extension. The extension's prefs are
//! visual/icon-only and live in GSettings (separate concern; see
//! org.gnome.shell.extensions.juhradial-indicator).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PopupMode {
    #[default]
    Simple,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostLabelStyle {
    /// Show paired hostname; falls back to "Channel N" when unknown.
    #[default]
    Hostname,
    /// Always show numeric channel ("Channel 1", "Channel 2", …).
    Channel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupConfig {
    #[serde(default)]
    pub mode: PopupMode,
    #[serde(default = "default_true")]
    pub show_host_buttons: bool,
    #[serde(default)]
    pub host_label_style: HostLabelStyle,
    #[serde(default = "default_simple_toggles")]
    pub simple_toggles: Vec<String>,
    #[serde(default = "default_power_toggles")]
    pub power_toggles: Vec<String>,
    #[serde(default = "default_power_sliders")]
    pub power_sliders: Vec<String>,
    #[serde(default = "default_true")]
    pub volume_on_scroll: bool,
    #[serde(default)]
    pub close_on_action: bool,
    #[serde(default = "default_true")]
    pub animations: bool,
}

fn default_true() -> bool { true }
fn default_simple_toggles() -> Vec<String> {
    vec!["gaming".into(), "haptics".into(), "radial".into()]
}
fn default_power_toggles() -> Vec<String> {
    vec!["gaming".into(), "haptics".into(), "radial".into(), "flow".into()]
}
fn default_power_sliders() -> Vec<String> {
    vec!["dpi".into(), "scroll".into()]
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            mode: PopupMode::default(),
            show_host_buttons: default_true(),
            host_label_style: HostLabelStyle::default(),
            simple_toggles: default_simple_toggles(),
            power_toggles: default_power_toggles(),
            power_sliders: default_power_sliders(),
            volume_on_scroll: default_true(),
            close_on_action: false,
            animations: default_true(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct QuickEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub desc: &'static str,
}

pub const QUICK_TOGGLE_CATALOG: &[QuickEntry] = &[
    QuickEntry { id: "gaming",    label: "Gaming Mode",      icon: "applications-games-symbolic",          desc: "Hides the radial, bumps DPI" },
    QuickEntry { id: "haptics",   label: "Haptic Feedback",  icon: "audio-volume-high-symbolic",           desc: "Click-and-hold ticks" },
    QuickEntry { id: "radial",    label: "Radial Overlay",   icon: "applications-graphics-symbolic",       desc: "Toggle the radial menu" },
    QuickEntry { id: "flow",      label: "Flow",             icon: "view-grid-symbolic",                   desc: "Cross-device scroll & paste" },
    QuickEntry { id: "smart",     label: "SmartShift",       icon: "system-switch-user-symbolic",          desc: "Free-spin scroll wheel" },
    QuickEntry { id: "highlight", label: "Cursor highlight", icon: "preferences-desktop-cursors-symbolic", desc: "Pulse ring on shake" },
];

pub const QUICK_SLIDER_CATALOG: &[QuickEntry] = &[
    QuickEntry { id: "dpi",      label: "Pointer DPI",          icon: "preferences-desktop-cursors-symbolic", desc: "200 – 6,400 dpi" },
    QuickEntry { id: "scroll",   label: "Scroll sensitivity",   icon: "system-switch-user-symbolic",          desc: "1 – 10" },
    QuickEntry { id: "haptic_i", label: "Haptic intensity",     icon: "audio-volume-high-symbolic",           desc: "Off – Strong" },
    QuickEntry { id: "accel",    label: "Pointer acceleration", icon: "view-grid-symbolic",                   desc: "-1.0 – 1.0" },
];

impl PopupConfig {
    /// Move `id` up by one position in `list`. No-op if not present
    /// or already first.
    pub fn move_up(list: &mut [String], id: &str) {
        if let Some(i) = list.iter().position(|x| x == id) {
            if i > 0 {
                list.swap(i, i - 1);
            }
        }
    }

    /// Move `id` down by one position in `list`. No-op if not present
    /// or already last.
    pub fn move_down(list: &mut [String], id: &str) {
        if let Some(i) = list.iter().position(|x| x == id) {
            if i + 1 < list.len() {
                list.swap(i, i + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let p = PopupConfig::default();
        let s = serde_json::to_string(&p).unwrap();
        let r: PopupConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(p, r);
    }

    #[test]
    fn missing_object_deserializes_to_defaults() {
        // Older config.json had no `popup` block at all. An empty object
        // for the popup field must produce the default.
        let s = "{}";
        let p: PopupConfig = serde_json::from_str(s).unwrap();
        assert_eq!(p, PopupConfig::default());
    }

    #[test]
    fn defaults_match_design_spec() {
        let p = PopupConfig::default();
        assert_eq!(p.mode, PopupMode::Simple);
        assert!(p.show_host_buttons);
        assert_eq!(p.host_label_style, HostLabelStyle::Hostname);
        assert_eq!(p.simple_toggles, vec!["gaming".to_string(), "haptics".into(), "radial".into()]);
        assert_eq!(p.power_toggles, vec!["gaming".to_string(), "haptics".into(), "radial".into(), "flow".into()]);
        assert_eq!(p.power_sliders, vec!["dpi".to_string(), "scroll".into()]);
        assert!(p.volume_on_scroll);
        assert!(!p.close_on_action);
        assert!(p.animations);
    }

    #[test]
    fn default_ids_exist_in_catalogs() {
        for id in &PopupConfig::default().simple_toggles {
            assert!(
                QUICK_TOGGLE_CATALOG.iter().any(|q| q.id == id),
                "default simple toggle {id:?} missing from QUICK_TOGGLE_CATALOG"
            );
        }
        for id in &PopupConfig::default().power_toggles {
            assert!(
                QUICK_TOGGLE_CATALOG.iter().any(|q| q.id == id),
                "default power toggle {id:?} missing from QUICK_TOGGLE_CATALOG"
            );
        }
        for id in &PopupConfig::default().power_sliders {
            assert!(
                QUICK_SLIDER_CATALOG.iter().any(|q| q.id == id),
                "default power slider {id:?} missing from QUICK_SLIDER_CATALOG"
            );
        }
    }

    #[test]
    fn move_up_swaps_with_predecessor() {
        let mut v: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        PopupConfig::move_up(&mut v, "b");
        assert_eq!(v, vec!["b", "a", "c"].into_iter().map(String::from).collect::<Vec<_>>());
    }

    #[test]
    fn move_up_on_first_is_noop() {
        let mut v: Vec<String> = vec!["a".into(), "b".into()];
        PopupConfig::move_up(&mut v, "a");
        assert_eq!(v, vec!["a".to_string(), "b".into()]);
    }

    #[test]
    fn move_down_swaps_with_successor() {
        let mut v: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        PopupConfig::move_down(&mut v, "b");
        assert_eq!(v, vec!["a", "c", "b"].into_iter().map(String::from).collect::<Vec<_>>());
    }

    #[test]
    fn move_down_on_last_is_noop() {
        let mut v: Vec<String> = vec!["a".into(), "b".into()];
        PopupConfig::move_down(&mut v, "b");
        assert_eq!(v, vec!["a".to_string(), "b".into()]);
    }

    #[test]
    fn move_unknown_id_is_noop() {
        let mut v: Vec<String> = vec!["a".into()];
        PopupConfig::move_up(&mut v, "missing");
        PopupConfig::move_down(&mut v, "missing");
        assert_eq!(v, vec!["a".to_string()]);
    }
}
