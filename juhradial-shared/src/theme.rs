//! Theme catalogue.
//!
//! Each theme is a JSON file under `juhradial-shared/themes/`, included
//! into the binary at compile time via `include_str!`. The Python
//! overlay's `themes.py` is the source of truth for the bundled set
//! today; both daemon and Rust overlay decode the same JSON via the
//! `Theme` struct here so colour palettes never drift between the two.
//!
//! User-contributed themes live in
//! `~/.local/share/juhradial/themes/<name>.json` and are loaded by
//! `Theme::load_user(name)` — same struct, same parser.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

/// Identifier for the active theme. Stored as a plain string in
/// `config.json` (e.g. `"theme": "dracula"`); custom themes shipped
/// via the user's data dir use the `Custom` variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThemeName {
    JuhradialMx,
    CatppuccinMocha,
    Nord,
    Dracula,
    CatppuccinLatte,
    GithubLight,
    SolarizedLight,
    Blossom3d,
    Neon3d,
    Pastel3d,
    Crystal3d,
    Custom(String),
}

impl ThemeName {
    pub fn as_str(&self) -> &str {
        match self {
            ThemeName::JuhradialMx => "juhradial-mx",
            ThemeName::CatppuccinMocha => "catppuccin-mocha",
            ThemeName::Nord => "nord",
            ThemeName::Dracula => "dracula",
            ThemeName::CatppuccinLatte => "catppuccin-latte",
            ThemeName::GithubLight => "github-light",
            ThemeName::SolarizedLight => "solarized-light",
            ThemeName::Blossom3d => "3d-blossom",
            ThemeName::Neon3d => "3d-neon",
            ThemeName::Pastel3d => "3d-pastel",
            ThemeName::Crystal3d => "3d-crystal",
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
            "juhradial-mx" => ThemeName::JuhradialMx,
            "catppuccin-mocha" => ThemeName::CatppuccinMocha,
            "nord" => ThemeName::Nord,
            "dracula" => ThemeName::Dracula,
            "catppuccin-latte" => ThemeName::CatppuccinLatte,
            "github-light" => ThemeName::GithubLight,
            "solarized-light" => ThemeName::SolarizedLight,
            "3d-blossom" => ThemeName::Blossom3d,
            "3d-neon" => ThemeName::Neon3d,
            "3d-pastel" => ThemeName::Pastel3d,
            "3d-crystal" => ThemeName::Crystal3d,
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
        // Matches DEFAULT_THEME in overlay/themes.py.
        ThemeName::JuhradialMx
    }
}

/// Decoded palette + metadata for a single theme. Mirrors the JSON
/// schema produced from `overlay/themes.py`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_dark: bool,

    /// Pre-rendered radial wheel image filename (under
    /// `assets/radial-wheels/`). `None` for vector themes.
    #[serde(default)]
    pub radial_image: Option<String>,

    /// Layout + colour overrides used only by the 3D themes that pin
    /// a pre-rendered wheel image. Vector themes don't set this.
    #[serde(default)]
    pub radial_params: Option<RadialParams>,

    pub colors: ThemeColors,
}

/// Per-slice palette. Field names match the keys in the JSON theme
/// files exactly (which match the Catppuccin nomenclature most of the
/// other themes were styled after). Each field is the standard hex
/// `#rrggbb` notation; a `parse_hex` helper turns it into floats for
/// cairo at render time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeColors {
    pub crust: String,
    pub mantle: String,
    pub base: String,
    pub surface0: String,
    pub surface1: String,
    pub surface2: String,
    pub overlay0: String,
    pub overlay1: String,
    pub text: String,
    pub subtext1: String,
    pub subtext0: String,
    pub accent: String,
    pub accent2: String,
    pub accent_dim: String,

    // Slice colours — keyed by these names from a slice's `color` field.
    pub green: String,
    pub yellow: String,
    pub red: String,
    pub blue: String,
    pub mauve: String,
    pub pink: String,
    pub peach: String,
    pub teal: String,
    pub sapphire: String,
    pub lavender: String,
}

/// Layout + special-effect knobs for the pre-rendered radial-wheel
/// themes. Tuples in the source JSON deserialize as fixed-length
/// arrays; we keep them as `Vec<u8>` to allow either RGB (3 elements)
/// or RGBA (4 elements) without separate types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadialParams {
    pub image_size: i32,
    pub icon_radius: i32,
    pub ring_inner: i32,
    pub ring_outer: i32,
    pub icon_scale: f32,
    pub icon_color: Vec<u8>,
    pub icon_shadow_alpha: u16,
    pub highlight_fill: Vec<u8>,
    pub highlight_border: Vec<u8>,
    pub hover_glow: Vec<u8>,
    pub center_bg: Vec<u8>,
    pub center_border: Vec<u8>,
    pub center_border_width: f32,
    pub center_text_color: Vec<u8>,
    pub icon_bold: f32,
}

impl ThemeColors {
    /// Look up *any* colour key by name and return its hex string.
    /// Covers every named slot in `ThemeColors` (surfaces, text,
    /// accents, slice palette). Returns `None` for unknown keys
    /// — caller decides the fallback. Used by tooltip styling and
    /// other "user picks a palette colour by name" surfaces.
    pub fn lookup(&self, key: &str) -> Option<&str> {
        let s = match key {
            "crust" => &self.crust,
            "mantle" => &self.mantle,
            "base" => &self.base,
            "surface0" => &self.surface0,
            "surface1" => &self.surface1,
            "surface2" => &self.surface2,
            "overlay0" => &self.overlay0,
            "overlay1" => &self.overlay1,
            "text" => &self.text,
            "subtext1" => &self.subtext1,
            "subtext0" => &self.subtext0,
            "accent" => &self.accent,
            "accent2" => &self.accent2,
            "accent_dim" => &self.accent_dim,
            "green" => &self.green,
            "yellow" => &self.yellow,
            "red" => &self.red,
            "blue" => &self.blue,
            "mauve" => &self.mauve,
            "pink" => &self.pink,
            "peach" => &self.peach,
            "teal" => &self.teal,
            "sapphire" => &self.sapphire,
            "lavender" => &self.lavender,
            _ => return None,
        };
        Some(s.as_str())
    }

    /// Look up a slice colour key (e.g. "green", "sapphire") and
    /// return it as a cairo-friendly RGBA tuple in the [0.0, 1.0]
    /// range. Falls back to the theme's `accent` for unknown keys so
    /// a missing colour name renders something rather than panicking.
    pub fn slice_color_rgba(&self, key: &str) -> (f64, f64, f64, f64) {
        let hex = match key {
            "green" => &self.green,
            "yellow" => &self.yellow,
            "red" => &self.red,
            "blue" => &self.blue,
            "mauve" => &self.mauve,
            "pink" => &self.pink,
            "peach" => &self.peach,
            "teal" => &self.teal,
            "sapphire" => &self.sapphire,
            "lavender" => &self.lavender,
            "accent" => &self.accent,
            "accent2" => &self.accent2,
            _ => &self.accent,
        };
        parse_hex_rgba(hex).unwrap_or((1.0, 1.0, 1.0, 1.0))
    }
}

/// Parse `#rrggbb` or `#rrggbbaa` into `(r, g, b, a)` in [0.0, 1.0].
pub fn parse_hex_rgba(hex: &str) -> Option<(f64, f64, f64, f64)> {
    let s = hex.strip_prefix('#')?;
    let bytes = match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()? as f64 / 255.0;
            let g = u8::from_str_radix(&s[2..4], 16).ok()? as f64 / 255.0;
            let b = u8::from_str_radix(&s[4..6], 16).ok()? as f64 / 255.0;
            (r, g, b, 1.0)
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()? as f64 / 255.0;
            let g = u8::from_str_radix(&s[2..4], 16).ok()? as f64 / 255.0;
            let b = u8::from_str_radix(&s[4..6], 16).ok()? as f64 / 255.0;
            let a = u8::from_str_radix(&s[6..8], 16).ok()? as f64 / 255.0;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(bytes)
}

// =============================================================================
// BUNDLED THEMES — embedded via include_str! at compile time.
// =============================================================================

macro_rules! bundle {
    ($name:literal) => {
        ($name, include_str!(concat!("../themes/", $name, ".json")))
    };
}

/// All bundled themes, in display order. Each entry is `(theme_name,
/// raw_json)`. We keep the JSON unparsed at module load and only
/// deserialize on demand so the binary stays cold-startup-friendly.
pub const BUNDLED_THEME_JSON: &[(&str, &str)] = &[
    bundle!("juhradial-mx"),
    bundle!("catppuccin-mocha"),
    bundle!("nord"),
    bundle!("dracula"),
    bundle!("catppuccin-latte"),
    bundle!("github-light"),
    bundle!("solarized-light"),
    bundle!("3d-blossom"),
    bundle!("3d-neon"),
    bundle!("3d-pastel"),
    bundle!("3d-crystal"),
];

impl Theme {
    /// Resolve a `ThemeName` to a fully-loaded `Theme`. Tries the
    /// user's data dir first (`~/.local/share/juhradial/themes/`)
    /// then the bundled set. Returns `None` when neither has it.
    pub fn load(name: &ThemeName) -> Option<Theme> {
        if let Some(t) = Self::load_user(name.as_str()) {
            return Some(t);
        }
        Self::load_bundled(name.as_str())
    }

    pub fn load_bundled(name: &str) -> Option<Theme> {
        for (n, json) in BUNDLED_THEME_JSON {
            if *n == name {
                return serde_json::from_str(json).ok();
            }
        }
        None
    }

    pub fn load_user(name: &str) -> Option<Theme> {
        let path = user_themes_dir()?.join(format!("{name}.json"));
        let json = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }

    /// Map of every available theme (bundled + user) keyed by its
    /// short name. Used by the editor's theme picker.
    pub fn catalogue() -> HashMap<String, Theme> {
        let mut out = HashMap::new();
        for (n, json) in BUNDLED_THEME_JSON {
            if let Ok(t) = serde_json::from_str::<Theme>(json) {
                out.insert((*n).to_string(), t);
            }
        }
        if let Some(dir) = user_themes_dir() {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    let name = match path.file_stem().and_then(|s| s.to_str()) {
                        Some(s) => s.to_string(),
                        None => continue,
                    };
                    if let Ok(json) = std::fs::read_to_string(&path) {
                        if let Ok(t) = serde_json::from_str::<Theme>(&json) {
                            out.insert(name, t);
                        }
                    }
                }
            }
        }
        out
    }
}

/// Public: where user-overlay themes live. Settings UI uses this
/// to write custom palettes that the bundled-theme catalogue then
/// picks up automatically.
pub fn user_themes_dir_pub() -> Option<PathBuf> {
    user_themes_dir()
}

/// Delete a saved user theme by its slug. Returns `Ok(())` even
/// when the file is already gone (idempotent — the user pressing
/// Delete twice should not error). Bundled themes can't be
/// removed; this only touches `{user_themes_dir}/{slug}.json`.
pub fn delete_user_theme(slug: &str) -> Result<(), std::io::Error> {
    let dir = match user_themes_dir() {
        Some(d) => d,
        None => return Ok(()),
    };
    let path = dir.join(format!("{slug}.json"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Enumerate the slugs (filename stems) of every saved user
/// theme. Empty when the user-themes dir doesn't exist yet, or no
/// `*.json` files live inside. Sorted alphabetically so the
/// settings UI lists them deterministically across reloads.
pub fn list_user_theme_slugs() -> Vec<String> {
    let dir = match user_themes_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                return None;
            }
            p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
        })
        .collect();
    out.sort();
    out
}

/// Persist a `Theme` as `{user_themes_dir}/{slug}.json`. Settings
/// UI calls this from its custom-palette editor. The slug is also
/// what the picker sees as the theme name on next reload.
pub fn save_user_theme(slug: &str, theme: &Theme) -> Result<PathBuf, std::io::Error> {
    let dir = user_themes_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no XDG_DATA_HOME / HOME for user-theme dir",
        )
    })?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{slug}.json"));
    let json = serde_json::to_string_pretty(theme)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    Ok(path)
}

fn user_themes_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            return Some(p.join("juhradial").join("themes"));
        }
    }
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join(".local")
            .join("share")
            .join("juhradial")
            .join("themes")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_theme_parses() {
        for (name, _) in BUNDLED_THEME_JSON {
            let t = Theme::load_bundled(name)
                .unwrap_or_else(|| panic!("bundled theme {name} did not parse"));
            assert!(!t.colors.crust.is_empty());
            assert!(!t.colors.accent.is_empty());
        }
    }

    #[test]
    fn three_d_themes_have_radial_params() {
        for n in ["3d-blossom", "3d-neon", "3d-pastel", "3d-crystal"] {
            let t = Theme::load_bundled(n).unwrap();
            assert!(t.radial_params.is_some(), "{n} missing radial_params");
            assert!(t.radial_image.is_some(), "{n} missing radial_image");
        }
    }

    #[test]
    fn vector_themes_have_no_radial_image() {
        for n in [
            "juhradial-mx",
            "catppuccin-mocha",
            "nord",
            "dracula",
            "catppuccin-latte",
            "github-light",
            "solarized-light",
        ] {
            let t = Theme::load_bundled(n).unwrap();
            assert!(t.radial_image.is_none(), "{n} unexpectedly has radial_image");
        }
    }

    #[test]
    fn parse_hex_rgba_round_trips() {
        let (r, g, b, a) = parse_hex_rgba("#ff8040").unwrap();
        assert!((r - 1.0).abs() < 1e-6);
        assert!((g - 0.5019607).abs() < 1e-3);
        assert!((b - 0.2509803).abs() < 1e-3);
        assert!((a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn parse_hex_rgba_handles_alpha() {
        let (_, _, _, a) = parse_hex_rgba("#000000ff").unwrap();
        assert!((a - 1.0).abs() < 1e-6);
        let (_, _, _, a) = parse_hex_rgba("#0000007f").unwrap();
        assert!((a - 0.5).abs() < 1e-2);
    }

    #[test]
    fn slice_color_falls_back_to_accent() {
        let t = Theme::load_bundled("dracula").unwrap();
        let unknown = t.colors.slice_color_rgba("nonexistent");
        let accent = parse_hex_rgba(&t.colors.accent).unwrap();
        assert_eq!(unknown, accent);
    }
}
