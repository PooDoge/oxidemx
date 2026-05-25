//! Design tokens resolved from the active `juhradial_shared::Theme`.
//!
//! The shared crate stores themes as JSON (Catppuccin Mocha, JuhRadial
//! MX, Nord, Dracula, the 3D variants…) keyed by name. Here we
//! convert one of those into a flat `Palette` of `iced::Color` plus
//! the few derived tones the legacy CSS used (hairlines, hover
//! washes, accent alpha variants). All widget styles in `style.rs`
//! read from this struct.
//!
//! Colour math mirrors `overlay/settings_css.py`:
//!   * dark themes → white-tinted hairlines + row-hover washes
//!   * light themes → black-tinted ditto
//!   * accent_06 / accent_15 / accent_40 → 6 %/15 %/40 % alpha of
//!     the accent for backgrounds, focus rings, etc.

use iced::Color;
use juhradial_shared::theme::{parse_hex_rgba, Theme, ThemeName};

#[derive(Debug, Clone)]
pub struct Palette {
    pub is_dark: bool,

    // Surface stack — increasing elevation.
    pub crust: Color,
    pub mantle: Color,
    pub base: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub surface2: Color,

    // Text.
    pub text: Color,
    pub subtext0: Color,
    pub subtext1: Color,
    pub overlay0: Color,

    // Accent + alpha-mixed variants.
    pub accent: Color,
    pub accent_dim: Color,
    pub accent_06: Color,
    pub accent_15: Color,
    pub accent_40: Color,

    // Hairlines + row washes (CSS-derived).
    pub hairline: Color,
    pub hairline_strong: Color,
    pub hairline_faint: Color,
    pub row_hover: Color,
    pub row_active: Color,

    // Semantic.
    pub success: Color,
    pub danger: Color,
    pub warning: Color,

    // Slice colour palette (unchanged from theme).
    pub mauve: Color,
    pub pink: Color,
    pub peach: Color,
    pub teal: Color,
    pub sapphire: Color,
    pub lavender: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub blue: Color,
}

impl Palette {
    /// Build a Palette from a juhradial_shared theme. Falls back to a
    /// hardcoded dark palette if any field fails to parse — keeps the
    /// UI from going invisible on a malformed user theme.
    pub fn from_theme(theme: &Theme) -> Self {
        let c = &theme.colors;
        Palette {
            is_dark: theme.is_dark,
            crust: parse(&c.crust),
            mantle: parse(&c.mantle),
            base: parse(&c.base),
            surface0: parse(&c.surface0),
            surface1: parse(&c.surface1),
            surface2: parse(&c.surface2),
            text: parse(&c.text),
            subtext0: parse(&c.subtext0),
            subtext1: parse(&c.subtext1),
            overlay0: parse(&c.overlay0),
            accent: parse(&c.accent),
            accent_dim: parse(&c.accent_dim),
            accent_06: alpha(&c.accent, 0.06),
            accent_15: alpha(&c.accent, 0.15),
            accent_40: alpha(&c.accent, 0.40),
            hairline: tint_hairline(theme.is_dark, 0.07),
            hairline_strong: tint_hairline(theme.is_dark, 0.12),
            hairline_faint: tint_hairline(theme.is_dark, 0.04),
            row_hover: tint_hairline(theme.is_dark, 0.03),
            row_active: tint_hairline(theme.is_dark, 0.05),
            success: parse(&c.green),
            danger: parse(&c.red),
            warning: parse(&c.yellow),
            mauve: parse(&c.mauve),
            pink: parse(&c.pink),
            peach: parse(&c.peach),
            teal: parse(&c.teal),
            sapphire: parse(&c.sapphire),
            lavender: parse(&c.lavender),
            green: parse(&c.green),
            yellow: parse(&c.yellow),
            red: parse(&c.red),
            blue: parse(&c.blue),
        }
    }

    /// Resolve a Palette for the active config theme. Tries the
    /// catalogue first; if the theme name isn't known (typo, removed
    /// theme), falls back to Catppuccin Mocha — the legacy default.
    pub fn resolve(name: &ThemeName) -> Self {
        Self::resolve_named(name.as_str())
    }

    pub fn resolve_named(name: &str) -> Self {
        if let Some(theme) = Theme::load(&ThemeName::from(name)) {
            return Self::from_theme(&theme);
        }
        if let Some(fallback) = Theme::load(&ThemeName::CatppuccinMocha) {
            return Self::from_theme(&fallback);
        }
        // Last-ditch hardcoded dark palette so the UI never goes
        // unstyled. Roughly Catppuccin Mocha values.
        Palette::hardcoded_mocha()
    }

    fn hardcoded_mocha() -> Self {
        Palette {
            is_dark: true,
            crust: rgb(0x11, 0x11, 0x1b),
            mantle: rgb(0x18, 0x18, 0x25),
            base: rgb(0x1e, 0x1e, 0x2e),
            surface0: rgb(0x31, 0x32, 0x44),
            surface1: rgb(0x45, 0x47, 0x5a),
            surface2: rgb(0x58, 0x5b, 0x70),
            text: rgb(0xcd, 0xd6, 0xf4),
            subtext0: rgb(0xa6, 0xad, 0xc8),
            subtext1: rgb(0xba, 0xc2, 0xde),
            overlay0: rgb(0x6c, 0x70, 0x86),
            accent: rgb(0xb4, 0xbe, 0xfe),
            accent_dim: rgb(0x93, 0x99, 0xb2),
            accent_06: Color::from_rgba(0.71, 0.74, 1.0, 0.06),
            accent_15: Color::from_rgba(0.71, 0.74, 1.0, 0.15),
            accent_40: Color::from_rgba(0.71, 0.74, 1.0, 0.40),
            hairline: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
            hairline_strong: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            hairline_faint: Color::from_rgba(1.0, 1.0, 1.0, 0.04),
            row_hover: Color::from_rgba(1.0, 1.0, 1.0, 0.03),
            row_active: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
            success: rgb(0xa6, 0xe3, 0xa1),
            danger: rgb(0xf3, 0x8b, 0xa8),
            warning: rgb(0xf9, 0xe2, 0xaf),
            mauve: rgb(0xcb, 0xa6, 0xf7),
            pink: rgb(0xf5, 0xc2, 0xe7),
            peach: rgb(0xfa, 0xb3, 0x87),
            teal: rgb(0x94, 0xe2, 0xd5),
            sapphire: rgb(0x74, 0xc7, 0xec),
            lavender: rgb(0xb4, 0xbe, 0xfe),
            green: rgb(0xa6, 0xe3, 0xa1),
            yellow: rgb(0xf9, 0xe2, 0xaf),
            red: rgb(0xf3, 0x8b, 0xa8),
            blue: rgb(0x89, 0xb4, 0xfa),
        }
    }
}

fn parse(hex: &str) -> Color {
    parse_hex_rgba(hex)
        .map(|(r, g, b, _)| Color::from_rgb(r as f32, g as f32, b as f32))
        .unwrap_or(Color::from_rgb(1.0, 0.0, 1.0)) // magenta = "I parsed wrong"
}

fn alpha(hex: &str, a: f32) -> Color {
    parse_hex_rgba(hex)
        .map(|(r, g, b, _)| Color::from_rgba(r as f32, g as f32, b as f32, a))
        .unwrap_or(Color::from_rgba(1.0, 1.0, 1.0, a))
}

fn tint_hairline(is_dark: bool, alpha_v: f32) -> Color {
    if is_dark {
        Color::from_rgba(1.0, 1.0, 1.0, alpha_v)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, alpha_v)
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// Helper: list every theme available in the catalogue, sorted with
/// the legacy default (Catppuccin Mocha) first. Used by the theme
/// picker dropdown.
pub fn theme_catalogue() -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Theme::catalogue()
        .into_iter()
        .map(|(key, t)| (key, t.name))
        .collect();
    entries.sort_by(|(ka, _), (kb, _)| match (ka.as_str(), kb.as_str()) {
        ("catppuccin-mocha", _) => std::cmp::Ordering::Less,
        (_, "catppuccin-mocha") => std::cmp::Ordering::Greater,
        ("juhradial-mx", _) => std::cmp::Ordering::Less,
        (_, "juhradial-mx") => std::cmp::Ordering::Greater,
        _ => ka.cmp(kb),
    });
    entries
}
