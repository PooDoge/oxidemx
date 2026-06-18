//! `Kit` — the per-frame render context the component builders consume.
//! It is the shared [`Palette`](crate::palette::Palette) flattened to the
//! roles the chrome uses, plus an animation context (`alpha` fade,
//! `pulse` phase). Static surfaces build it with `alpha = 1.0,
//! pulse = 0.0`; the overlay drives `alpha`/`pulse` from its morph
//! animation. `fade()` scales a color's alpha by the surface fade — the
//! single place opacity is applied (iced has no opacity wrapper).

use iced::Color;

use crate::palette::Palette;

/// Shared palette + fade kit threaded through views so every element
/// resolves the same theme roles the same way.
#[derive(Clone, Copy)]
pub struct Kit {
    /// Surface-wide fade (the chat rises in by scaling every color's
    /// alpha in lockstep). `1.0` = fully opaque.
    pub alpha: f32,
    /// 0‥1 animation phase (a breathing sine) — e.g. the "working" dot.
    /// `0.0` for static surfaces.
    pub pulse: f32,
    pub crust: Color,
    pub mantle: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub surface2: Color,
    pub overlay0: Color,
    pub text: Color,
    pub subtext0: Color,
    pub subtext1: Color,
    pub accent: Color,
    pub green: Color,
    pub yellow: Color,
    pub mauve: Color,
    pub red: Color,
}

impl Kit {
    /// Build a `Kit` from the shared palette + animation context. Pure
    /// (no app state), so any iced surface can construct one.
    pub fn from_palette(p: &Palette, alpha: f32, pulse: f32) -> Self {
        Kit {
            alpha,
            pulse,
            crust: p.crust,
            mantle: p.mantle,
            surface0: p.surface0,
            surface1: p.surface1,
            surface2: p.surface2,
            overlay0: p.overlay0,
            text: p.text,
            subtext0: p.subtext0,
            subtext1: p.subtext1,
            accent: p.accent,
            green: p.green,
            yellow: p.yellow,
            mauve: p.mauve,
            red: p.red,
        }
    }

    /// Build a static (fully-opaque, unanimated) `Kit` from a theme — the
    /// entry point for non-animated surfaces (settings / popup / MC).
    pub fn from_theme(theme: &oxidemx_shared::theme::Theme) -> Self {
        Self::from_palette(&Palette::from_theme(theme), 1.0, 0.0)
    }

    /// Scale a colour's alpha by the surface fade × `k`.
    pub fn fade(&self, c: Color, k: f32) -> Color {
        Color {
            a: c.a * (self.alpha * k).clamp(0.0, 1.0),
            ..c
        }
    }

    /// Subtle themed scrollbar shared by every scrollable — a thin
    /// transparent rail with an `overlay0` scroller (the iced default is
    /// a bright wide bar that fights the design).
    pub fn scrollable_style(
        &self,
    ) -> impl Fn(&iced::Theme, iced::widget::scrollable::Status) -> iced::widget::scrollable::Style
    {
        let kit = *self;
        move |theme, status| {
            let mut s = iced::widget::scrollable::default(theme, status);
            let rail = iced::widget::scrollable::Rail {
                background: None,
                border: iced::border::Border::default(),
                scroller: iced::widget::scrollable::Scroller {
                    background: iced::Background::Color(kit.fade(kit.overlay0, 0.8)),
                    border: iced::border::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                },
            };
            s.vertical_rail = rail;
            s.horizontal_rail = rail;
            s
        }
    }
}
