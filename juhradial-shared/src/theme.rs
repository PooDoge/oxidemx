use serde::{Deserialize, Serialize};
use std::fmt;

/// Built-in theme identifier. Bundled themes live in
/// `daemon/src/bundled_themes.rs`; the `Custom` variant lets users drop a
/// `<name>.toml` into `~/.local/share/juhradial/themes/`.
///
/// Stored as a plain string in `config.json` (e.g. `"theme": "dracula"`),
/// so we use untagged-string serde rather than serde's default enum
/// representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeName {
    Dracula,
    Nord,
    JuhradialMx,
    Solarized,
    Gruvbox,
    Custom(String),
}

impl ThemeName {
    pub fn as_str(&self) -> &str {
        match self {
            ThemeName::Dracula => "dracula",
            ThemeName::Nord => "nord",
            ThemeName::JuhradialMx => "juhradial-mx",
            ThemeName::Solarized => "solarized",
            ThemeName::Gruvbox => "gruvbox",
            ThemeName::Custom(s) => s,
        }
    }
}

impl fmt::Display for ThemeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for ThemeName {
    fn from(s: &str) -> Self {
        match s {
            "dracula" => ThemeName::Dracula,
            "nord" => ThemeName::Nord,
            "juhradial-mx" => ThemeName::JuhradialMx,
            "solarized" => ThemeName::Solarized,
            "gruvbox" => ThemeName::Gruvbox,
            other => ThemeName::Custom(other.to_string()),
        }
    }
}

impl Serialize for ThemeName {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThemeName {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = <String as Deserialize>::deserialize(de)?;
        Ok(ThemeName::from(s.as_str()))
    }
}

impl Default for ThemeName {
    fn default() -> Self {
        ThemeName::Dracula
    }
}
