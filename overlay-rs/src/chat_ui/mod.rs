//! The redesigned AI chat surface (Contract 2 of the radial-ai
//! spec). Widget content layered above the chat shell's painted
//! caps: header furniture (title/status/buttons — the travelling
//! puck and drag pill stay canvas-drawn in `chat_shell`), thread
//! chip strip, conversation body with agent cards, footer input,
//! and the memories / scheduled-task management views.
//!
//! Styling follows `design/oxidemx-radial-ai/ai-chat.jsx` — every
//! colour is a theme palette key resolved at view time, never a
//! hardcoded hex. `alpha` fades the whole surface in lockstep with
//! `chat_shell::chat_alpha` (iced has no opacity wrapper, so each
//! colour is scaled instead).

pub mod body;
pub mod cards;
pub mod footer;
pub mod header;
pub mod icons;
pub mod memories;
pub mod palette;
pub mod skills;
pub mod tasks;
pub mod threads;
pub mod tokens;

use iced::{Color, Element, Length};

use crate::app::Message;
use crate::radial::RadialState;

/// Shared palette + fade kit threaded through the submodules so
/// every element resolves the same theme keys the same way.
#[derive(Clone, Copy)]
pub struct Kit {
    pub alpha: f32,
    /// 0‥1 wall-clock sine shared with the window chrome's status
    /// cues — drives the activity dot's breathing while a turn is
    /// in flight.
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
    pub fn from_state(state: &RadialState, alpha: f32) -> Self {
        let p = &state.theme.theme.colors;
        let c = |hex: &str, fb: Color| {
            oxidemx_shared::theme::parse_hex_rgba(hex)
                .map(|(r, g, b, a)| Color::from_rgba(r as f32, g as f32, b as f32, a as f32))
                .unwrap_or(fb)
        };
        Kit {
            alpha,
            pulse: state
                .show_time
                .map(|t| {
                    let secs = t.elapsed().as_secs_f32();
                    ((secs * std::f32::consts::TAU / 1.4).sin() + 1.0) / 2.0
                })
                .unwrap_or(0.0),
            crust: c(&p.crust, Color::from_rgb(0.04, 0.05, 0.06)),
            mantle: c(&p.mantle, Color::from_rgb(0.06, 0.07, 0.09)),
            surface0: c(&p.surface0, Color::from_rgb(0.10, 0.11, 0.14)),
            surface1: c(&p.surface1, Color::from_rgb(0.14, 0.16, 0.20)),
            surface2: c(&p.surface2, Color::from_rgb(0.18, 0.20, 0.25)),
            overlay0: c(&p.overlay0, Color::from_rgb(0.25, 0.27, 0.33)),
            text: c(&p.text, Color::WHITE),
            subtext0: c(&p.subtext0, Color::from_rgb(0.60, 0.65, 0.71)),
            subtext1: c(&p.subtext1, Color::from_rgb(0.78, 0.82, 0.86)),
            accent: c(&p.accent, Color::from_rgb(0.0, 0.83, 1.0)),
            green: c(&p.green, Color::from_rgb(0.0, 0.90, 0.46)),
            yellow: c(&p.yellow, Color::from_rgb(1.0, 0.84, 0.31)),
            mauve: c(&p.mauve, Color::from_rgb(0.70, 0.53, 1.0)),
            red: c(&p.red, Color::from_rgb(1.0, 0.32, 0.32)),
        }
    }

    /// Scale a colour's alpha by the surface fade × `k`.
    pub fn fade(&self, c: Color, k: f32) -> Color {
        Color {
            a: c.a * (self.alpha * k).clamp(0.0, 1.0),
            ..c
        }
    }

    /// Subtle themed scrollbar shared by every chat scrollable —
    /// thin transparent rail with an `overlay0` scroller (the iced
    /// default is a bright wide bar that fights the design).
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

/// Assemble the whole chat surface. Region layout mirrors the
/// parked cap geometry: header content sits inside the top arc
/// (`EDGE_PAD‥EDGE_PAD+HEADER_H`), the footer input sits on the
/// bottom arc, and the body fills the middle.
pub fn view(state: &RadialState, alpha: f32) -> Element<'_, Message> {
    let kit = Kit::from_state(state, alpha);

    let middle: Element<'_, Message> = if state.ai_show_skills {
        skills::view(state, &kit)
    } else if state.ai_show_memories {
        memories::view(state, &kit)
    } else if state.ai_show_tasks {
        tasks::view(state, &kit)
    } else if state.ai_show_threads {
        body::threads_list(state, &kit)
    } else {
        body::conversation(state, &kit)
    };

    let content = iced::widget::column![
        header::view(state, &kit),
        threads::strip(state, &kit),
        iced::widget::container(middle)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(iced::Padding {
                top: 4.0,
                right: 16.0,
                bottom: 4.0,
                left: 16.0,
            }),
        footer::view(state, &kit),
    ];

    let base = iced::widget::container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(iced::Padding {
            // Slight downward offset that releases as the content
            // fades in — reads as the chat rising into place.
            top: (1.0 - alpha) * 18.0,
            ..iced::Padding::default()
        });

    // Slash-command palette floats above everything when open.
    if state.ai_palette.is_some() {
        iced::widget::stack![base, palette::overlay(state, kit)].into()
    } else {
        base.into()
    }
}
