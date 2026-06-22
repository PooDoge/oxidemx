//! Design tokens (palette / accents / fonts / dims) for the OxideMX Freya UI.
//! Values mirror the Claude Design "Collapsible Panels" system.
use freya::prelude::Color;

pub const SIDEBAR_FULL_W: f32 = 274.0;
pub const SIDEBAR_RAIL_W: f32 = 60.0;
pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Cyan,
    Purple,
    Orange,
    Green,
}

impl Accent {
    pub fn color(self) -> Color {
        match self {
            Accent::Cyan => Color::from_rgb(0, 212, 255),
            Accent::Purple => Color::from_rgb(179, 136, 255),
            Accent::Orange => Color::from_rgb(255, 171, 64),
            Accent::Green => Color::from_rgb(123, 224, 106),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    accent: Accent,
}

impl Default for Theme {
    fn default() -> Self { Self { accent: Accent::Cyan } }
}

impl Theme {
    pub fn with_accent(accent: Accent) -> Self { Self { accent } }
    pub fn bg(&self) -> Color { Color::from_rgb(5, 7, 11) }
    pub fn surface(&self) -> Color { Color::from_rgb(13, 17, 23) }
    pub fn text(&self) -> Color { Color::from_rgb(240, 244, 248) }
    pub fn subtext(&self) -> Color { Color::from_rgb(148, 163, 184) }
    pub fn accent(&self) -> Color { self.accent.color() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark_cyan() {
        let t = Theme::default();
        assert_eq!(t.bg(), Color::from_rgb(5, 7, 11));
        assert_eq!(t.accent(), Color::from_rgb(0, 212, 255));
    }

    #[test]
    fn accent_switch_changes_accent_only() {
        let t = Theme::with_accent(Accent::Purple);
        assert_eq!(t.accent(), Color::from_rgb(179, 136, 255));
        assert_eq!(t.bg(), Color::from_rgb(5, 7, 11));
    }
}
