//! iced widget styles wired to the active theme — the **form-widget +
//! settings-surface** half of the design system (slider / toggler /
//! pick_list / rule / the settings card/sidebar/nav chrome). The
//! chat-overlay's design-language surfaces live in [`catalog`](crate::catalog);
//! both halves now resolve from one render context, [`Kit`](crate::kit::Kit).
//!
//! iced 0.14 styles are functions of `&Theme` returning a per-widget
//! `Style` struct. We ship closures here so the whole settings UI
//! re-themes when the user picks a different palette.
//!
//! Every function takes `impl Into<Kit>`, so the existing call-sites that
//! pass `&Palette` keep working (there's a `From<&Palette> for Kit`) while
//! the crate runs on a single `Kit` context. New code can pass a `Kit`
//! directly. Helpers are organised by widget: `card_*`, `btn_*`, etc.

use crate::kit::Kit;
use iced::widget::{button, container, pick_list, rule, scrollable, slider, text, toggler};
use iced::{Background, Border, Color, Shadow, Theme};

// ============================================================================
// CONTAINERS
// ============================================================================

/// Window-level frame: sets the body background and ensures every
/// child without an explicit container style gets the right colour.
pub fn window(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: Some(Background::Color(kit.crust)),
        text_color: Some(kit.text),
        ..Default::default()
    }
}

/// Settings card — the elevated `mantle` panel with a 1px hairline
/// border and 10 px radius.
pub fn card(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: Some(Background::Color(kit.mantle)),
        border: Border {
            color: kit.hairline,
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: Some(kit.text),
        ..Default::default()
    }
}

/// Quieter card variant for nested groups (e.g. info boxes).
pub fn card_quiet(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: Some(Background::Color(kit.crust)),
        border: Border {
            color: kit.hairline_faint,
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: Some(kit.text),
        ..Default::default()
    }
}

/// Sidebar rail — `mantle` background + 1px right border.
pub fn sidebar(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: Some(Background::Color(kit.mantle)),
        border: Border {
            color: kit.hairline,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(kit.text),
        // Right edge only — iced doesn't have per-edge borders, so we
        // approximate with a 1 px shadow leaning right. Acceptable
        // hack; lands cleaner once iced exposes per-edge borders.
        shadow: Shadow {
            color: kit.hairline,
            offset: iced::Vector::new(1.0, 0.0),
            blur_radius: 0.0,
        },
        ..Default::default()
    }
}

/// Header bar — sits above the sidebar+content. Same window
/// background; thin bottom hairline drawn separately as a Rule.
pub fn header(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: Some(Background::Color(kit.crust)),
        text_color: Some(kit.text),
        ..Default::default()
    }
}

/// Footer / status bar.
pub fn footer(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: Some(Background::Color(kit.crust)),
        text_color: Some(kit.subtext0),
        ..Default::default()
    }
}

/// Device chip / badge — small uppercase pill with hairline border.
pub fn chip(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: None,
        border: Border {
            color: kit.hairline_strong,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(kit.subtext0),
        ..Default::default()
    }
}

/// Page-content scroll wrapper.
pub fn page(kit: impl Into<Kit>) -> impl Fn(&Theme) -> container::Style + 'static {
    let kit = kit.into();
    move |_| container::Style {
        background: Some(Background::Color(kit.base)),
        text_color: Some(kit.text),
        ..Default::default()
    }
}

// ============================================================================
// BUTTONS
// ============================================================================

/// Primary suggested-action button. Solid accent fill.
#[allow(dead_code)]
pub fn btn_primary(
    kit: impl Into<Kit>,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let kit = kit.into();
    let on_accent = if kit.is_dark { kit.crust } else { Color::WHITE };
    move |_, status| {
        let pressed = matches!(status, button::Status::Pressed);
        let mut bg = kit.accent;
        if pressed {
            bg.a = 0.9;
        }
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: on_accent,
            border: Border {
                color: kit.accent,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Secondary / outline button.
pub fn btn_secondary(
    kit: impl Into<Kit>,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let kit = kit.into();
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: if hovered {
                Some(Background::Color(kit.row_hover))
            } else {
                None
            },
            text_color: kit.text,
            border: Border {
                color: if hovered {
                    kit.accent
                } else {
                    kit.hairline_strong
                },
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Sidebar nav item. Active and inactive variants — caller picks
/// which closure to apply.
pub fn nav_item(
    kit: impl Into<Kit>,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let kit = kit.into();
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let bg = if active {
            kit.row_active
        } else if hovered {
            kit.row_hover
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if active || hovered {
                kit.text
            } else {
                kit.subtext0
            },
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Flat / tertiary button (icon-only, header bar actions).
#[allow(dead_code)]
pub fn btn_flat(kit: impl Into<Kit>) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let kit = kit.into();
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: if hovered {
                Some(Background::Color(kit.row_hover))
            } else {
                None
            },
            text_color: kit.text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Destructive button — outline + danger text.
pub fn btn_danger(
    kit: impl Into<Kit>,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let kit = kit.into();
    let danger = kit.danger;
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: if hovered {
                Some(Background::Color(Color::from_rgba(
                    danger.r, danger.g, danger.b, 0.08,
                )))
            } else {
                None
            },
            text_color: danger,
            border: Border {
                color: if hovered { danger } else { kit.hairline_strong },
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

// ============================================================================
// FORMS — slider, toggler, pick_list
// ============================================================================

/// Custom slider styling. Currently unused — sliders pick up their
/// look from the iced::Theme::custom built in main(). Kept here in
/// case a per-slider override is needed later.
#[allow(dead_code)]
pub fn slider_style(
    kit: impl Into<Kit>,
) -> impl Fn(&Theme, slider::Status) -> slider::Style + 'static {
    let kit = kit.into();
    move |_, status| {
        let hovered = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
        slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Color(kit.accent),
                    Background::Color(kit.surface0),
                ),
                width: 6.0,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 3.0.into(),
                },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 10.0 },
                background: Background::Color(if hovered { kit.accent } else { kit.text }),
                border_color: Color::TRANSPARENT,
                border_width: 0.0,
            },
        }
    }
}

pub fn toggler_style(
    kit: impl Into<Kit>,
) -> impl Fn(&Theme, toggler::Status) -> toggler::Style + 'static {
    let kit = kit.into();
    let knob_on = if kit.is_dark { kit.crust } else { Color::WHITE };
    move |_, status| {
        let active = matches!(
            status,
            toggler::Status::Active { is_toggled: true }
                | toggler::Status::Hovered { is_toggled: true }
                | toggler::Status::Disabled { is_toggled: true }
        );
        toggler::Style {
            background: Background::Color(if active { kit.accent } else { kit.surface0 }),
            background_border_color: if active {
                kit.accent
            } else {
                kit.hairline_strong
            },
            background_border_width: 1.0,
            foreground: Background::Color(if active { knob_on } else { kit.text }),
            foreground_border_color: Color::TRANSPARENT,
            foreground_border_width: 0.0,
            text_color: None,
            border_radius: None,
            padding_ratio: 0.15,
        }
    }
}

pub fn pick_list_style(
    kit: impl Into<Kit>,
) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style + 'static {
    let kit = kit.into();
    move |_, status| {
        let hovered = matches!(
            status,
            pick_list::Status::Hovered | pick_list::Status::Opened { .. }
        );
        pick_list::Style {
            background: Background::Color(kit.mantle),
            border: Border {
                color: if hovered {
                    kit.accent
                } else {
                    kit.hairline_strong
                },
                width: 1.0,
                radius: 6.0.into(),
            },
            text_color: kit.text,
            placeholder_color: kit.subtext0,
            handle_color: kit.text,
        }
    }
}

// ============================================================================
// SCROLLBAR + RULE
// ============================================================================

pub fn scrollable_style(
    kit: impl Into<Kit>,
) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style + 'static {
    let kit = kit.into();
    let thumb = if kit.is_dark {
        Color::from_rgba(1.0, 1.0, 1.0, 0.14)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.18)
    };
    let thumb_hover = if kit.is_dark {
        Color::from_rgba(1.0, 1.0, 1.0, 0.24)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.30)
    };
    let panel_bg = kit.crust;
    move |_, status| {
        let hovered = matches!(
            status,
            scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
        );
        let thumb_color = if hovered { thumb_hover } else { thumb };
        let rail = scrollable::Rail {
            background: None,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
            scroller: scrollable::Scroller {
                background: Background::Color(thumb_color),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
            },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            // The "drag past edge" overlay — keep it muted.
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(panel_bg),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                shadow: Shadow {
                    color: Color::TRANSPARENT,
                    offset: iced::Vector::ZERO,
                    blur_radius: 0.0,
                },
                icon: thumb_color,
            },
        }
    }
}

pub fn rule_style(kit: impl Into<Kit>) -> impl Fn(&Theme) -> rule::Style + 'static {
    let kit = kit.into();
    move |_| rule::Style {
        color: kit.hairline,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

// ============================================================================
// TEXT — explicit colour helpers used inline by the views
// ============================================================================

pub fn text_dim(kit: impl Into<Kit>) -> impl Fn(&Theme) -> text::Style + 'static {
    let color = kit.into().subtext0;
    move |_| text::Style { color: Some(color) }
}

pub fn text_faint(kit: impl Into<Kit>) -> impl Fn(&Theme) -> text::Style + 'static {
    let color = kit.into().overlay0;
    move |_| text::Style { color: Some(color) }
}

pub fn text_accent(kit: impl Into<Kit>) -> impl Fn(&Theme) -> text::Style + 'static {
    let color = kit.into().accent;
    move |_| text::Style { color: Some(color) }
}
