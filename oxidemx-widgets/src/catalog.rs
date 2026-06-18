//! The style **catalog** — variant-keyed style functions that resolve a
//! widget's appearance from the [`Kit`] render context. This is the
//! design system's realization of iced's `Catalog` pattern: instead of
//! a custom `Theme` type (our colors come from `Kit`/`Palette`, not from
//! `iced::Theme`'s palette, so a custom Theme would buy nothing and cost
//! a `markdown::Catalog` + highlighter reimpl), we centralize every
//! recurring `container::Style` / `button::Style` here, keyed by a
//! semantic variant. Views call `.style(catalog::surface(kit, …))`
//! instead of hand-writing closures — one source of truth per variant,
//! re-tinted by the active theme through `Kit`.

#![allow(dead_code)]

use iced::widget::{button, container};
use iced::{Background, Border, Color};

use crate::kit::Kit;
use crate::tokens;

/// Container surfaces — every recurring panel/card/chip/bubble style.
#[derive(Clone, Copy)]
pub enum Surface {
    /// Elevated card: mantle, surface1 border, r-card, e1 shadow.
    Card,
    /// Popover / dialog panel: mantle, surface1 border, r-panel.
    Panel,
    /// Small bordered chip/badge: surface0, surface2 border, r-control.
    Chip,
    /// Recessed code/preview well: crust, surface1 border, r-control.
    CrustWell,
    /// A flat list-row card (panel rows: tasks / memories / skills) —
    /// like [`Surface::Card`] but with no elevation shadow.
    Row,
    /// A message bubble (tail-asymmetric radius); `true` = user side.
    Bubble(bool),
    /// A semantic-tinted card (approvals=yellow, errors=red): `tone` bg
    /// at `bg_k`, border at `border_k`, `radius`.
    Tinted {
        tone: Color,
        bg_k: f32,
        border_k: f32,
        radius: f32,
    },
}

/// Resolve a [`Surface`] to a container style closure.
pub fn surface_style(kit: Kit, s: Surface) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| match s {
        Surface::Card => container::Style {
            background: Some(Background::Color(kit.fade(kit.mantle, 0.96))),
            border: Border {
                color: kit.fade(kit.surface1, 1.0),
                width: 1.0,
                radius: tokens::R_CARD.into(),
            },
            shadow: tokens::e1(),
            ..Default::default()
        },
        Surface::Panel => container::Style {
            background: Some(Background::Color(kit.fade(kit.mantle, 0.98))),
            border: Border {
                color: kit.fade(kit.surface2, 1.0),
                width: 1.0,
                radius: tokens::R_PANEL.into(),
            },
            shadow: tokens::e2(),
            ..Default::default()
        },
        Surface::Chip => container::Style {
            background: Some(Background::Color(kit.fade(kit.surface0, 0.9))),
            border: Border {
                color: kit.fade(kit.surface2, 1.0),
                width: 1.0,
                radius: tokens::R_CONTROL.into(),
            },
            ..Default::default()
        },
        Surface::CrustWell => container::Style {
            background: Some(Background::Color(kit.fade(kit.crust, 1.0))),
            border: Border {
                color: kit.fade(kit.surface1, 1.0),
                width: 1.0,
                radius: tokens::R_CONTROL.into(),
            },
            ..Default::default()
        },
        // Flat row: Card colors, no shadow. r=10 (the established panel
        // row radius — between R_BUTTON and R_CARD on the scale).
        Surface::Row => container::Style {
            background: Some(Background::Color(kit.fade(kit.mantle, 0.96))),
            border: Border {
                color: kit.fade(kit.surface1, 1.0),
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        },
        Surface::Bubble(is_user) => container::Style {
            background: Some(Background::Color(if is_user {
                kit.fade(kit.accent, 0.11)
            } else {
                kit.fade(kit.surface0, 1.0)
            })),
            border: Border {
                color: if is_user {
                    kit.fade(kit.accent, 0.23)
                } else {
                    kit.fade(kit.text, 0.05)
                },
                width: 1.0,
                // Tail asymmetry: the corner nearest the sender is tight.
                radius: if is_user {
                    iced::border::Radius::default()
                        .top_left(14.0)
                        .top_right(14.0)
                        .bottom_left(14.0)
                        .bottom_right(4.0)
                } else {
                    iced::border::Radius::default()
                        .top_left(14.0)
                        .top_right(14.0)
                        .bottom_left(4.0)
                        .bottom_right(14.0)
                },
            },
            ..Default::default()
        },
        Surface::Tinted {
            tone,
            bg_k,
            border_k,
            radius,
        } => container::Style {
            background: Some(Background::Color(kit.fade(tone, bg_k))),
            border: Border {
                color: kit.fade(tone, border_k),
                width: 1.0,
                radius: radius.into(),
            },
            ..Default::default()
        },
    }
}

/// Button variants — every recurring button style.
#[derive(Clone, Copy)]
pub enum Btn {
    /// Borderless, transparent until hover (inline icon actions).
    Ghost,
    /// Fully transparent, no hover (a clickable wrapper).
    Plain,
    /// Filled with `tone` (crust text) — primary actions.
    Fill(Color),
    /// Outlined; tints to `tone` on hover.
    Outline(Color),
    /// A rounded `tone`-tinted pill at alpha `k` (e.g. the latest pill).
    Tinted(Color, f32),
}

/// Resolve a [`Btn`] to a button style closure.
pub fn button_style(kit: Kit, b: Btn) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        match b {
            Btn::Ghost => button::Style {
                background: hovered.then(|| Background::Color(kit.fade(kit.surface1, 0.7))),
                border: Border::default().rounded(tokens::R_CONTROL),
                ..Default::default()
            },
            Btn::Plain => button::Style::default(),
            Btn::Fill(tone) => button::Style {
                background: Some(Background::Color(kit.fade(tone, 1.0))),
                border: Border::default().rounded(tokens::R_CONTROL),
                text_color: kit.fade(kit.crust, 1.0),
                ..Default::default()
            },
            Btn::Outline(tone) => button::Style {
                border: Border {
                    color: kit.fade(
                        if hovered { tone } else { kit.surface2 },
                        if hovered { 0.5 } else { 1.0 },
                    ),
                    width: 1.0,
                    radius: tokens::R_CONTROL.into(),
                },
                text_color: kit.fade(if hovered { tone } else { kit.subtext1 }, 1.0),
                ..Default::default()
            },
            Btn::Tinted(tone, k) => button::Style {
                background: Some(Background::Color(kit.fade(tone, k))),
                border: Border::default().rounded(tokens::R_PILL),
                text_color: kit.fade(kit.crust, 1.0),
                ..Default::default()
            },
        }
    }
}
