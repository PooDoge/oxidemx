//! Resolved settings as delivered to a widget (init / SettingsChanged).
//! A sorted Vec of pairs, not a map — postcard-friendly and deterministic.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SettingValue {
    Str(String),
    Num(f64),
    Bool(bool),
    Location { name: String, lat: f64, lon: f64 },
}

pub type Settings = Vec<(String, SettingValue)>;
