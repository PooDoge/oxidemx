//! Reusable component builders — the third layer of the design system
//! (after `tokens` = scales, `icons` = the glyph set). These encode the
//! recurring chrome patterns (icon buttons, pills, cards, chips,
//! segmented controls) once, so every view composes the *same* widget
//! the *same* way. Colors resolve through `Kit`; sizes/radii come from
//! `tokens`. Keep new chrome here, not inlined per call-site.
//!
//! These take a `Kit` (the shared render context) + iced primitives
//! only, so every iced surface builds the same control the same way.
//! Build a `Kit` via `Kit::from_theme(theme)` (static) or
//! `Kit::from_palette(p, alpha, pulse)` (animated). See
//! `docs/design/ui-design-language.md`.

#![allow(dead_code)]

use iced::widget::{button, container, row, text, Button, Container};
use iced::{Alignment, Background, Border, Color, Element, Length};

use crate::icons::icon;
use crate::kit::Kit;
use crate::tokens;

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

/// The themed selectable **Chip** (design spec doc 1 · §Chip) — an
/// icon+label pill toggle with three states:
/// * `active` — accent text + accent@12% fill + accent border
/// * default — subtext1 text + surface0 fill + surface1 border
/// * `dim` — subtext0 text, *transparent* fill, surface1 border (ghost)
///
/// `dim` is ignored when `active` (active wins). Use for thread chips,
/// context chips, "+ New", filter/segment rows.
pub fn pill<'a, Message: Clone + 'a>(
    kit: Kit,
    icon_name: Option<&str>,
    label: impl text::IntoFragment<'a>,
    active: bool,
    msg: Message,
) -> Button<'a, Message> {
    pill_dim(kit, icon_name, label, active, false, msg)
}

/// [`pill`] with the explicit `dim` (ghost) third state.
pub fn pill_dim<'a, Message: Clone + 'a>(
    kit: Kit,
    icon_name: Option<&str>,
    label: impl text::IntoFragment<'a>,
    active: bool,
    dim: bool,
    msg: Message,
) -> Button<'a, Message> {
    let col = kit.fade(
        if active {
            kit.accent
        } else if dim {
            kit.subtext0
        } else {
            kit.subtext1
        },
        1.0,
    );
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
            } else if dim {
                Color::TRANSPARENT
            } else {
                kit.fade(kit.surface0, 1.0)
            })),
            border: Border {
                // Spec: active = accent@40% (accent@66); dim = surface1
                // outline; default = borderless (fill carries it).
                color: if active {
                    kit.fade(kit.accent, 0.4)
                } else if dim {
                    kit.fade(kit.surface1, 1.0)
                } else {
                    Color::TRANSPARENT
                },
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
        // Spec (doc 1 · Button): primary text is bold (700).
        button(label.color(kit.fade(kit.crust, 1.0)).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..Default::default()
        }))
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

/// Wrap content in the standard card (mantle / surface1 / r-card, flat).
/// Convenience builder over
/// [`catalog::surface_style`](crate::catalog::surface_style)`(kit,
/// Surface::Card)` — the style lives in the catalog (single source of
/// truth), this just saves the `container(..).style(..)` boilerplate.
pub fn card<'a, Message: 'a>(
    kit: Kit,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content).style(crate::catalog::surface_style(
        kit,
        crate::catalog::Surface::Card,
    ))
}

/// A small bordered chip container (status badges, doc/attachment chips).
/// Convenience builder over `catalog` `Surface::Chip`.
pub fn chip<'a, Message: 'a>(
    kit: Kit,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content)
        .padding([tokens::S0_5 + 1.0, tokens::S2])
        .style(crate::catalog::surface_style(
            kit,
            crate::catalog::Surface::Chip,
        ))
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

/// One segment of a segmented switcher: icon + label, accent-tinted when
/// `active`. Compose several inside a bordered container for the control.
pub fn segment<'a, Message: Clone + 'a>(
    kit: Kit,
    icon_name: &str,
    label: impl text::IntoFragment<'a>,
    active: bool,
    msg: Message,
) -> Button<'a, Message> {
    let col = kit.fade(if active { kit.accent } else { kit.subtext0 }, 1.0);
    button(
        row![
            icon(icon_name, 15.0, col),
            text(label).size(tokens::T_LABEL).color(col),
        ]
        .spacing(tokens::S1_5 - 1.0)
        .align_y(Alignment::Center),
    )
    .padding([tokens::S1, tokens::S2 + 1.0])
    .style(move |_, _| button::Style {
        background: active.then(|| Background::Color(kit.fade(kit.accent, 0.16))),
        border: Border::default().rounded(7.0),
        text_color: col,
        ..Default::default()
    })
    .on_press(msg)
}

/// A fixed-size filled icon button — a circle (`radius` ≥ half the
/// `diameter`) or a rounded square. Used for send / stop / close.
pub fn round_icon_button<'a, Message: Clone + 'a>(
    icon_name: &str,
    icon_size: f32,
    diameter: f32,
    radius: f32,
    bg: Color,
    fg: Color,
    msg: Message,
) -> Button<'a, Message> {
    button(iced::widget::center(icon(icon_name, icon_size, fg)))
        .width(Length::Fixed(diameter))
        .height(Length::Fixed(diameter))
        .padding(0)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default().rounded(radius),
            ..Default::default()
        })
        .on_press(msg)
}

/// A **StatusDot** (design spec doc 1 · §StatusDot) — a small filled
/// state dot. `tone` is the state color; when `live`, it gains a soft
/// glow (container shadow) and breathes with the shared `Kit::pulse`
/// (idle dots are static). Default diameter 6px, fully round.
pub fn status_dot<'a, Message: 'a>(kit: Kit, tone: Color, live: bool) -> Element<'a, Message> {
    status_dot_sized(kit, tone, live, 6.0)
}

/// [`status_dot`] with an explicit diameter.
pub fn status_dot_sized<'a, Message: 'a>(
    kit: Kit,
    tone: Color,
    live: bool,
    d: f32,
) -> Element<'a, Message> {
    container(iced::widget::Space::new())
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        .style(move |_| {
            // Live dots breathe (0.45‥1.0 of the tone) + glow; idle are flat.
            let k = if live { 0.45 + 0.55 * kit.pulse } else { 1.0 };
            container::Style {
                background: Some(Background::Color(kit.fade(tone, k))),
                border: Border::default().rounded(d / 2.0),
                shadow: if live {
                    iced::Shadow {
                        color: kit.fade(tone, 0.5),
                        offset: iced::Vector::ZERO,
                        blur_radius: 6.0,
                    }
                } else {
                    iced::Shadow::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// A **Badge** (design spec doc 1 · §Badge) — a tiny tinted status/count
/// label: `tone`@14% fill + `tone` text, no border, tight padding.
pub fn badge<'a, Message: 'a>(
    kit: Kit,
    tone: Color,
    label: impl text::IntoFragment<'a>,
) -> Container<'a, Message> {
    container(
        text(label)
            .size(tokens::T_MICRO)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..Default::default()
            })
            .wrapping(text::Wrapping::None)
            .color(kit.fade(tone, 1.0)),
    )
    .padding([tokens::S0_5, tokens::S1_5])
    .style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(tone, 0.14))),
        border: Border::default().rounded(tokens::R_CONTROL),
        ..Default::default()
    })
}

/// A **MiniSwitch / Toggle** (design spec doc 1 · §Toggle) — a 32×18
/// pill track with a sliding 14px knob. Track accent (on) / surface2
/// (off); knob crust (on) / subtext0 (off). The single shared impl for
/// the chat panel toggles (replaces the duplicated `mini_switch` /
/// `toggle`). For iced-native `toggler`s use `style::toggler_style`.
pub fn switch<'a, Message: Clone + 'a>(kit: Kit, on: bool, msg: Message) -> Element<'a, Message> {
    let knob = container(iced::widget::Space::new())
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(
                kit.fade(if on { kit.crust } else { kit.subtext0 }, 1.0),
            )),
            border: Border::default().rounded(7.0),
            ..Default::default()
        });
    let inner = if on {
        row![iced::widget::Space::new().width(Length::Fill), knob]
    } else {
        row![knob, iced::widget::Space::new().width(Length::Fill)]
    };
    button(
        container(inner.align_y(Alignment::Center))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(18.0))
            .padding(2)
            .style(move |_| container::Style {
                background: Some(Background::Color(
                    kit.fade(if on { kit.accent } else { kit.surface2 }, 1.0),
                )),
                border: Border::default().rounded(9.0),
                ..Default::default()
            }),
    )
    .padding(0)
    .style(|_, _| button::Style::default())
    .on_press(msg)
    .into()
}

/// The **Model pill** (design spec doc 1 · §Pill) — a labeled trigger
/// that opens a popover: a leading accent-filled provider swatch, the
/// "provider · model" label, and a trailing chevron. Surface0 fill,
/// surface1 border, pill radius. (Chevron rotation on `open` is left to
/// the caller — iced has no cheap svg rotate; swap the glyph if needed.)
pub fn model_pill<'a, Message: Clone + 'a>(
    kit: Kit,
    swatch_icon: &str,
    label: impl text::IntoFragment<'a>,
    _open: bool,
    msg: Message,
) -> Button<'a, Message> {
    let swatch = container(icon(swatch_icon, 10.0, kit.fade(kit.crust, 1.0)))
        .width(Length::Fixed(15.0))
        .height(Length::Fixed(15.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(kit.fade(kit.accent, 1.0))),
            border: Border::default().rounded(tokens::R_XS),
            ..Default::default()
        });
    let content = row![
        swatch,
        text(label)
            .size(tokens::T_LABEL)
            .wrapping(text::Wrapping::None)
            .color(kit.fade(kit.subtext1, 1.0)),
        icon("chevron", 12.0, kit.fade(kit.subtext0, 1.0)),
    ]
    .spacing(tokens::S1_5)
    .align_y(Alignment::Center);
    button(content)
        .padding([tokens::S1, tokens::S2 + 1.0])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(kit.fade(kit.surface0, 1.0))),
            border: Border {
                color: kit.fade(kit.surface1, 1.0),
                width: 1.0,
                radius: tokens::R_PILL.into(),
            },
            ..Default::default()
        })
        .on_press(msg)
}
