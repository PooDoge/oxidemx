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

// The token + icon layers of the design system now live in the shared
// `oxidemx-widgets` crate so every iced surface (settings / popup /
// Mission Control) shares them. Re-exported here so `super::tokens` /
// `super::icons` paths across chat_ui keep resolving unchanged.
pub use oxidemx_widgets::{icons, tokens};

use iced::{Element, Length};

use crate::app::Message;
use crate::radial::RadialState;

/// The shared render context + component builders now live in
/// `oxidemx-widgets`; re-exported so `super::Kit` / `super::widgets`
/// paths across chat_ui resolve unchanged.
pub use oxidemx_widgets::catalog::{self, Btn, Surface};
pub use oxidemx_widgets::controls as widgets;
pub use oxidemx_widgets::kit::Kit;

/// Assemble the whole chat surface. Region layout mirrors the
/// parked cap geometry: header content sits inside the top arc
/// (`EDGE_PAD‥EDGE_PAD+HEADER_H`), the footer input sits on the
/// bottom arc, and the body fills the middle.
pub fn view(state: &RadialState, alpha: f32) -> Element<'_, Message> {
    // Colors from the shared palette; the overlay adds the animation
    // context (alpha fade + breathing pulse from the morph clock).
    let pulse = state
        .show_time
        .map(|t| ((t.elapsed().as_secs_f32() * std::f32::consts::TAU / 1.4).sin() + 1.0) / 2.0)
        .unwrap_or(0.0);
    let kit = Kit::from_palette(
        &oxidemx_widgets::palette::Palette::from_theme(&state.theme.theme),
        alpha,
        pulse,
    );

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
