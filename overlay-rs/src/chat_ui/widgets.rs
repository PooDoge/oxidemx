//! Reusable component builders — the third layer of the design system
//! (after `tokens` = scales, `icons` = the glyph set). These encode the
//! recurring chrome patterns (icon buttons, pills, cards, chips,
//! segmented controls) once, so every view composes the *same* widget
//! the *same* way. Colors resolve through `Kit`; sizes/radii come from
//! `tokens`. Keep new chrome here, not inlined per call-site.
//!
//! Cross-app note: these take a `Kit` + iced primitives only, so the
//! module can be lifted into a shared crate (e.g. `oxidemx-widgets`) and
//! reused by settings / popup / mission-control once those adopt the
//! same `Kit` palette. See `docs/design/ui-design-language.md`.

#![allow(dead_code)]

use iced::widget::{button, container, row, text, Button, Container};
use iced::{Alignment, Background, Border, Color, Element, Length};

use super::icons::icon;
use super::tokens;
use super::Kit;

/// A borderless ("ghost") icon button — the default for inline actions
/// (copy, select, attach, close, …). Subtle hover tint.
pub fn ghost_icon_button<'a, Message: Clone + 'a>(
    kit: Kit,
    name: &str,
    size: f32,
    color: Color,
    msg: Message,
) -> Button<'a, Message> {
    button(icon(name, size, color))
        .padding([tokens::S0_5, tokens::S1])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: hovered.then(|| Background::Color(kit.fade(kit.surface1, 0.7))),
                border: Border::default().rounded(tokens::R_CONTROL),
                ..Default::default()
            }
        })
        .on_press(msg)
}

/// An icon + label pill button (chips, the model pill, "New"). When
/// `active`, it takes the accent tint + border; otherwise a quiet
/// outline. `accent_pill` true uses the accent surface, false a plain
/// outline.
pub fn pill<'a, Message: Clone + 'a>(
    kit: Kit,
    icon_name: Option<&str>,
    label: impl text::IntoFragment<'a>,
    active: bool,
    msg: Message,
) -> Button<'a, Message> {
    let col = kit.fade(if active { kit.accent } else { kit.subtext1 }, 1.0);
    let mut content = row![].spacing(tokens::S1_5).align_y(Alignment::Center);
    if let Some(n) = icon_name {
        content = content.push(icon(n, 13.0, col));
    }
    content = content.push(
        text(label)
            .size(tokens::T_LABEL)
            .wrapping(text::Wrapping::None)
            .color(col),
    );
    button(content)
        .padding([tokens::S1, tokens::S3])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if active {
                kit.fade(kit.accent, 0.12)
            } else {
                kit.fade(kit.surface0, 1.0)
            })),
            border: Border {
                color: kit.fade(if active { kit.accent } else { kit.surface1 }, 1.0),
                width: 1.0,
                radius: tokens::R_PILL.into(),
            },
            text_color: col,
            ..Default::default()
        })
        .on_press(msg)
}

/// A tinted action button — `tone` is the semantic color; `primary`
/// fills it (tone bg + crust text), else an outline that tints on hover.
pub fn action_button<'a, Message: Clone + 'a>(
    kit: Kit,
    label: impl text::IntoFragment<'a>,
    tone: Color,
    primary: bool,
    msg: Message,
) -> Button<'a, Message> {
    let label = text(label).size(tokens::T_LABEL);
    let btn = if primary {
        button(label.color(kit.fade(kit.crust, 1.0)))
    } else {
        button(label.color(kit.fade(kit.subtext1, 1.0)))
    };
    btn.padding([tokens::S1, tokens::S3])
        .style(move |_, status| {
            let hov = matches!(status, button::Status::Hovered);
            if primary {
                button::Style {
                    background: Some(Background::Color(kit.fade(tone, 1.0))),
                    border: Border::default().rounded(tokens::R_CONTROL),
                    text_color: kit.fade(kit.crust, 1.0),
                    ..Default::default()
                }
            } else {
                button::Style {
                    border: Border {
                        color: kit.fade(
                            if hov { tone } else { kit.surface2 },
                            if hov { 0.5 } else { 1.0 },
                        ),
                        width: 1.0,
                        radius: tokens::R_CONTROL.into(),
                    },
                    text_color: kit.fade(if hov { tone } else { kit.subtext1 }, 1.0),
                    ..Default::default()
                }
            }
        })
        .on_press(msg)
}

/// Wrap content in the standard elevated card (mantle, surface1 border,
/// r-card, e1 shadow). `tone` paints the 2.5px left status rule when
/// `Some` (agent/tool cards); `None` for a plain card.
pub fn card<'a, Message: 'a>(
    kit: Kit,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content).style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(kit.mantle, 0.96))),
        border: Border {
            color: kit.fade(kit.surface1, 1.0),
            width: 1.0,
            radius: tokens::R_CARD.into(),
        },
        shadow: tokens::e1(),
        ..Default::default()
    })
}

/// A small bordered chip container (status badges, doc/attachment chips).
pub fn chip<'a, Message: 'a>(
    kit: Kit,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content)
        .padding([tokens::S0_5 + 1.0, tokens::S2])
        .style(move |_| container::Style {
            background: Some(Background::Color(kit.fade(kit.surface0, 0.9))),
            border: Border {
                color: kit.fade(kit.surface2, 1.0),
                width: 1.0,
                radius: tokens::R_CONTROL.into(),
            },
            ..Default::default()
        })
}

/// A 2.5px vertical status rule (the left edge of agent/tool cards).
pub fn status_rule<'a, Message: 'a>(kit: Kit, tone: Color) -> Element<'a, Message> {
    container(iced::widget::Space::new())
        .width(Length::Fixed(2.5))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(kit.fade(tone, 1.0))),
            ..Default::default()
        })
        .into()
}
