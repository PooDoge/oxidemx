//! Design tokens for the OxideMX Freya UI. Values mirror the Claude Design
//! "Collapsible Panels" `freya.json` (default cyan accent). Flat names; the
//! `accent_NN` alpha ramp is computed via `Theme::with_alpha`.
use freya::prelude::Color;

pub const SIDEBAR_FULL_W: f32 = 274.0;
pub const SIDEBAR_RAIL_W: f32 = 60.0;
pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Cyan,
    Violet,
    Amber,
    Lime,
}

impl Accent {
    pub fn accent(self) -> Color {
        match self {
            Accent::Cyan   => Color::from_rgb(0, 212, 255),
            Accent::Violet => Color::from_rgb(179, 136, 255),
            Accent::Amber  => Color::from_rgb(255, 171, 64),
            Accent::Lime   => Color::from_rgb(123, 224, 106),
        }
    }
    pub fn accent_hi(self) -> Color {
        match self {
            Accent::Cyan   => Color::from_rgb(10, 189, 198),
            Accent::Violet => Color::from_rgb(139, 108, 255),
            Accent::Amber  => Color::from_rgb(255, 143, 63),
            Accent::Lime   => Color::from_rgb(82, 194, 74),
        }
    }
    pub fn accent_dim(self) -> Color {
        match self {
            Accent::Cyan   => Color::from_rgb(8, 145, 168),
            Accent::Violet => Color::from_rgb(122, 91, 208),
            Accent::Amber  => Color::from_rgb(207, 130, 50),
            Accent::Lime   => Color::from_rgb(70, 162, 62),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone { Accent, Blue, Green, Peach, Teal, Mauve }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    accent: Accent,
}

impl Default for Theme {
    fn default() -> Self { Self { accent: Accent::Cyan } }
}

impl Theme {
    pub fn with_accent(accent: Accent) -> Self { Self { accent } }

    /// Apply an alpha (0..=255) to a base color — the design's `${color}NN` ramp.
    pub fn with_alpha(base: Color, a: u8) -> Color {
        Color::from_argb(a, base.r(), base.g(), base.b())
    }

    // surfaces (low → high elevation)
    pub fn bg_deep(&self) -> Color { Color::from_rgb(10, 12, 16) }       // crust
    pub fn panel(&self) -> Color { Color::from_rgb(15, 17, 23) }         // mantle
    pub fn bg(&self) -> Color { Color::from_rgb(18, 20, 24) }            // base
    pub fn surface(&self) -> Color { Color::from_rgb(26, 29, 36) }       // surface0
    pub fn surface_hi(&self) -> Color { Color::from_rgb(36, 40, 50) }    // surface1
    pub fn surface_max(&self) -> Color { Color::from_rgb(46, 52, 64) }   // surface2
    pub fn overlay(&self) -> Color { Color::from_rgb(64, 70, 84) }       // overlay0
    // text
    pub fn text(&self) -> Color { Color::from_rgb(240, 244, 248) }
    pub fn subtext_hi(&self) -> Color { Color::from_rgb(200, 208, 220) } // subtext1
    pub fn subtext(&self) -> Color { Color::from_rgb(154, 165, 181) }    // subtext0
    pub fn faint(&self) -> Color { Color::from_rgb(93, 102, 117) }
    // accent ramp
    pub fn accent(&self) -> Color { self.accent.accent() }
    pub fn accent_hi(&self) -> Color { self.accent.accent_hi() }
    pub fn accent_dim(&self) -> Color { self.accent.accent_dim() }
    // semantic tones
    pub fn green(&self) -> Color { Color::from_rgb(0, 230, 118) }
    pub fn yellow(&self) -> Color { Color::from_rgb(255, 213, 79) }
    pub fn red(&self) -> Color { Color::from_rgb(255, 82, 82) }
    pub fn blue(&self) -> Color { Color::from_rgb(74, 158, 255) }
    pub fn mauve(&self) -> Color { Color::from_rgb(179, 136, 255) }
    pub fn peach(&self) -> Color { Color::from_rgb(255, 171, 64) }
    pub fn teal(&self) -> Color { Color::from_rgb(10, 189, 198) }
    // backdrop (radial wash behind the thread)
    pub fn bg0(&self) -> Color { Color::from_rgb(6, 10, 18) }   // #060a12
    pub fn bg1(&self) -> Color { Color::from_rgb(10, 16, 24) }  // #0a1018
    pub fn bg2(&self) -> Color { Color::from_rgb(7, 11, 17) }   // #070b11
    pub fn shadow_deep(&self) -> Color { Color::from_argb(158, 0, 0, 0) } // rgba(0,0,0,0.62)
    // hairlines (tinted white)
    pub fn hairline(&self) -> Color { Color::from_argb(15, 255, 255, 255) }        // ~.06
    pub fn hairline_strong(&self) -> Color { Color::from_argb(26, 255, 255, 255) } // ~.10

    /// Resolve a per-attachment / per-model tone to its base color.
    pub fn tone(&self, t: Tone) -> Color {
        match t {
            Tone::Accent => self.accent(),
            Tone::Blue   => self.blue(),
            Tone::Green  => self.green(),
            Tone::Peach  => self.peach(),
            Tone::Teal   => self.teal(),
            Tone::Mauve  => self.mauve(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark_cyan() {
        let t = Theme::default();
        assert_eq!(t.bg(), Color::from_rgb(18, 20, 24));      // base #121418
        assert_eq!(t.accent(), Color::from_rgb(0, 212, 255)); // #00d4ff
    }

    #[test]
    fn palette_surfaces_and_text() {
        let t = Theme::default();
        assert_eq!(t.panel(), Color::from_rgb(15, 17, 23));    // mantle #0f1117
        assert_eq!(t.surface(), Color::from_rgb(26, 29, 36));  // surface0 #1a1d24
        assert_eq!(t.text(), Color::from_rgb(240, 244, 248));  // #f0f4f8
        assert_eq!(t.subtext(), Color::from_rgb(154, 165, 181)); // subtext0 #9aa5b5
    }

    #[test]
    fn accent_switch_changes_accent_only() {
        let t = Theme::with_accent(Accent::Violet);
        assert_eq!(t.accent(), Color::from_rgb(179, 136, 255)); // #b388ff
        assert_eq!(t.bg(), Color::from_rgb(18, 20, 24));        // unchanged
    }

    #[test]
    fn with_alpha_sets_argb() {
        let c = Theme::with_alpha(Color::from_rgb(0, 212, 255), 0x1a);
        assert_eq!(c, Color::from_argb(0x1a, 0, 212, 255));
    }

    #[test]
    fn backdrop_trio_and_shadow() {
        let t = Theme::default();
        assert_eq!(t.bg0(), Color::from_rgb(6, 10, 18));    // #060a12
        assert_eq!(t.bg1(), Color::from_rgb(10, 16, 24));   // #0a1018
        assert_eq!(t.bg2(), Color::from_rgb(7, 11, 17));    // #070b11
        assert_eq!(t.shadow_deep(), Color::from_argb(158, 0, 0, 0)); // rgba(0,0,0,0.62)
    }

    #[test]
    fn tone_resolves_and_tracks_accent() {
        let t = Theme::default();
        assert_eq!(t.tone(Tone::Blue), t.blue());
        assert_eq!(t.tone(Tone::Accent), t.accent());
        let v = Theme::with_accent(Accent::Violet);
        assert_eq!(v.tone(Tone::Accent), v.accent()); // accent tone follows the accent
    }
}
