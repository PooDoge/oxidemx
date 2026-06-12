//! `widget.json` — the single source of truth for the picker tile, the
//! options card, and permissions (spec §5).

use serde::{Deserialize, Serialize};

/// Refresh floor (spec §8): timers and refresh_ms clamp to this.
pub const MIN_REFRESH_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    pub api_version: u32,
    pub entry: String,
    pub icon: String,
    /// "net:<host>", "open-url", "exec", "haptics".
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub slice: SliceMeta,
    #[serde(default)]
    pub options: Vec<OptionSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SliceMeta {
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_icon: Option<String>,
}

impl Default for SliceMeta {
    fn default() -> Self {
        SliceMeta { refresh_ms: default_refresh_ms(), fallback_icon: None }
    }
}

fn default_refresh_ms() -> u64 { 900_000 }

/// One entry of `options[]`. `kind` stays a String so unknown types from
/// newer widgets render as a disabled "Update OxideMX" row instead of
/// failing the parse (spec §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionSpec {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxlen: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
}

impl WidgetManifest {
    /// Structural validation — id slug, non-empty entry/icon, option keys.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        {
            return Err(format!("invalid widget id {:?} (allowed: [a-z0-9.-]+)", self.id));
        }
        if self.entry.is_empty() {
            return Err("manifest `entry` must name the wasm module".into());
        }
        if self.icon.is_empty() {
            return Err("manifest `icon` must name an icon file".into());
        }
        for o in &self.options {
            if o.key.is_empty() {
                return Err("option with empty key".into());
            }
        }
        Ok(())
    }

    /// `slice.refresh_ms` with the spec floor applied.
    pub fn effective_refresh_ms(&self) -> u64 {
        self.slice.refresh_ms.max(MIN_REFRESH_MS)
    }

    /// Manifest defaults as a JSON bag — layer 1 of settings resolution.
    pub fn defaults(&self) -> serde_json::Map<String, serde_json::Value> {
        self.options.iter()
            .filter_map(|o| o.default.clone().map(|d| (o.key.clone(), d)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEATHER: &str = r#"{
      "id": "weather",
      "name": "Weather",
      "version": "1.4.0",
      "author": "JuhLabs",
      "api_version": 1,
      "entry": "widget.wasm",
      "icon": "icon.svg",
      "permissions": ["net:api.open-meteo.com", "open-url", "haptics"],
      "slice": { "refresh_ms": 900000, "fallback_icon": "weather-clear-symbolic" },
      "options": [
        { "key": "location", "type": "location", "label": "Location",
          "hint": "city name or \"lat,lon\"", "required": true },
        { "key": "units", "type": "enum", "label": "Units",
          "values": ["c", "f"], "default": "c" },
        { "key": "refresh", "type": "select", "label": "Refresh",
          "values": [300, 900, 1800, 3600], "default": 900, "unit": "s" }
      ]
    }"#;

    #[test]
    fn parses_weather_manifest() {
        let m: WidgetManifest = serde_json::from_str(WEATHER).unwrap();
        assert_eq!(m.id, "weather");
        assert_eq!(m.api_version, 1);
        assert_eq!(m.slice.refresh_ms, 900_000);
        assert_eq!(m.options.len(), 3);
        assert_eq!(m.options[0].kind, "location");
        assert!(m.options[0].required);
        assert_eq!(m.options[1].default, Some(serde_json::json!("c")));
        assert_eq!(m.options[2].unit.as_deref(), Some("s"));
        assert!(m.validate().is_ok());
    }

    #[test]
    fn minimal_manifest_gets_defaults() {
        let m: WidgetManifest = serde_json::from_str(
            r#"{ "id": "clock", "name": "Clock", "version": "0.1.0",
                 "author": "x", "api_version": 1,
                 "entry": "widget.wasm", "icon": "icon.svg" }"#,
        ).unwrap();
        assert!(m.permissions.is_empty());
        assert_eq!(m.slice.refresh_ms, 900_000);
        assert!(m.options.is_empty());
        assert!(m.validate().is_ok());
    }

    #[test]
    fn refresh_floor_is_clamped() {
        let m: WidgetManifest = serde_json::from_str(
            r#"{ "id": "spam", "name": "Spam", "version": "0.1.0",
                 "author": "x", "api_version": 1,
                 "entry": "widget.wasm", "icon": "icon.svg",
                 "slice": { "refresh_ms": 50 } }"#,
        ).unwrap();
        assert_eq!(m.effective_refresh_ms(), MIN_REFRESH_MS);
    }

    #[test]
    fn bad_ids_are_rejected() {
        for bad in ["", "Has Spaces", "UPPER", "emoji🙂", "a/b"] {
            let mut m: WidgetManifest = serde_json::from_str(
                r#"{ "id": "ok", "name": "X", "version": "0.1.0",
                     "author": "x", "api_version": 1,
                     "entry": "widget.wasm", "icon": "icon.svg" }"#,
            ).unwrap();
            m.id = bad.into();
            assert!(m.validate().is_err(), "id {bad:?} should be rejected");
        }
    }
}
