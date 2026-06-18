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
pub mod memories;
pub mod palette;
pub mod skills;
pub mod tasks;
pub mod threads;
pub mod widgets;

// The token + icon layers of the design system now live in the shared
// `oxidemx-widgets` crate so every iced surface (settings / popup /
// Mission Control) shares them. Re-exported here so `super::tokens` /
// `super::icons` paths across chat_ui keep resolving unchanged.
pub use oxidemx_widgets::{icons, tokens};

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
        // Colors come from the SHARED palette (one hex-parsing + fallback
        // source for every iced surface); the overlay only adds the
        // animation context (alpha fade + breathing pulse).
        let pulse = state
            .show_time
            .map(|t| {
                let secs = t.elapsed().as_secs_f32();
                ((secs * std::f32::consts::TAU / 1.4).sin() + 1.0) / 2.0
            })
            .unwrap_or(0.0);
        Self::from_palette(
            &oxidemx_widgets::palette::Palette::from_theme(&state.theme.theme),
            alpha,
            pulse,
        )
    }

    /// Build a `Kit` from the shared palette + animation context. Pure
    /// (no `RadialState`), so any iced surface can reuse it (static UIs
    /// pass `alpha = 1.0`, `pulse = 0.0`).
    pub fn from_palette(p: &oxidemx_widgets::palette::Palette, alpha: f32, pulse: f32) -> Self {
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

    // Overlays, back to front: slash palette, then the image lightbox
    // (modal, on top of everything).
    let mut layers: Vec<Element<'_, Message>> = vec![base.into()];
    if state.ai_palette.is_some() {
        layers.push(palette::overlay(state, kit));
    }
    if let Some(lb) = body::lightbox(state, &kit) {
        layers.push(lb);
    }
    if layers.len() == 1 {
        layers.pop().unwrap()
    } else {
        iced::widget::Stack::with_children(layers).into()
    }
}
