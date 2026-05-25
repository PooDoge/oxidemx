//! iced widget styles wired to the active `Palette`.
//!
//! iced 0.14 styles are functions of `&Theme` returning a per-widget
//! `Style` struct. Most widgets accept either a built-in style
//! (`button::primary`, `container::bordered_box`) or a closure. We
//! ship our own closures here so the whole settings UI re-themes
//! when the user picks a different palette in the Theme picker.
//!
//! Helpers are organised by widget: `card_*`, `sidebar_*`, `btn_*`,
//! etc. They're returned as boxed closures because every callsite
//! needs to capture the live palette.

use crate::palette::Palette;
use iced::widget::{button, container, pick_list, rule, scrollable, slider, text, toggler};
use iced::{Background, Border, Color, Shadow, Theme};

// ============================================================================
// CONTAINERS
// ============================================================================

/// Window-level frame: sets the body background and ensures every
/// child without an explicit container style gets the right colour.
pub fn window(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let bg = palette.crust;
    let text = palette.text;
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(text),
        ..Default::default()
    }
}

/// Settings card — the elevated `panel_bg` panel with a 1px hairline
/// border and 10 px radius.
pub fn card(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let bg = palette.mantle;
    let border = palette.hairline;
    let text = palette.text;
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: Some(text),
        ..Default::default()
    }
}

/// Quieter card variant for nested groups (e.g. info boxes).
pub fn card_quiet(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let bg = palette.crust;
    let border = palette.hairline_faint;
    let text = palette.text;
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: Some(text),
        ..Default::default()
    }
}

/// Sidebar rail — `mantle` background + 1px right border.
pub fn sidebar(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let bg = palette.mantle;
    let border = palette.hairline;
    let text = palette.text;
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(text),
        // Right edge only — iced doesn't have per-edge borders, so we
        // approximate with a 1 px shadow leaning right. Acceptable
        // hack; lands cleaner once iced exposes per-edge borders.
        shadow: Shadow {
            color: border,
            offset: iced::Vector::new(1.0, 0.0),
            blur_radius: 0.0,
        },
        ..Default::default()
    }
}

/// Header bar — sits above the sidebar+content. Same window
/// background; thin bottom hairline drawn separately as a Rule.
pub fn header(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let bg = palette.crust;
    let text = palette.text;
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(text),
        ..Default::default()
    }
}

/// Footer / status bar.
pub fn footer(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let bg = palette.crust;
    let text = palette.subtext0;
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(text),
        ..Default::default()
    }
}

/// Device chip / badge — small uppercase pill with hairline border.
pub fn chip(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let border = palette.hairline_strong;
    let text = palette.subtext0;
    move |_| container::Style {
        background: None,
        border: Border {
            color: border,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(text),
        ..Default::default()
    }
}

/// Page-content scroll wrapper.
pub fn page(palette: &Palette) -> impl Fn(&Theme) -> container::Style + 'static {
    let bg = palette.base;
    let text = palette.text;
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(text),
        ..Default::default()
    }
}

// ============================================================================
// BUTTONS
// ============================================================================

/// Primary suggested-action button. Solid accent fill.
#[allow(dead_code)]
pub fn btn_primary(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let accent = palette.accent;
    let on_accent = if palette.is_dark {
        palette.crust
    } else {
        Color::WHITE
    };
    move |_, status| {
        let pressed = matches!(status, button::Status::Pressed);
        let hovered = matches!(status, button::Status::Hovered);
        let mut bg = accent;
        if pressed {
            bg.a = 0.9;
        }
        let _ = hovered;
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: on_accent,
            border: Border {
                color: accent,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Secondary / outline button.
pub fn btn_secondary(
    palette: &Palette,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let border = palette.hairline_strong;
    let border_hover = palette.accent;
    let text = palette.text;
    let row_hover = palette.row_hover;
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: if hovered {
                Some(Background::Color(row_hover))
            } else {
                None
            },
            text_color: text,
            border: Border {
                color: if hovered { border_hover } else { border },
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
    palette: &Palette,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let text_active = palette.text;
    let text_dim = palette.subtext0;
    let row_hover = palette.row_hover;
    let row_active = palette.row_active;
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let bg = if active {
            row_active
        } else if hovered {
            row_hover
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if active || hovered {
                text_active
            } else {
                text_dim
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
pub fn btn_flat(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let text = palette.text;
    let row_hover = palette.row_hover;
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: if hovered {
                Some(Background::Color(row_hover))
            } else {
                None
            },
            text_color: text,
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
pub fn btn_danger(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    let danger = palette.danger;
    let border = palette.hairline_strong;
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
                color: if hovered { danger } else { border },
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
pub fn slider_style(palette: &Palette) -> impl Fn(&Theme, slider::Status) -> slider::Style + 'static {
    let track_bg = palette.surface0;
    let highlight = palette.accent;
    let handle = palette.text;
    let handle_hover = palette.accent;
    move |_, status| {
        let hovered = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
        slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Color(highlight),
                    Background::Color(track_bg),
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
                background: Background::Color(if hovered { handle_hover } else { handle }),
                border_color: Color::TRANSPARENT,
                border_width: 0.0,
            },
        }
    }
}

pub fn toggler_style(palette: &Palette) -> impl Fn(&Theme, toggler::Status) -> toggler::Style + 'static {
    let off_bg = palette.surface0;
    let off_border = palette.hairline_strong;
    let on_bg = palette.accent;
    let knob_off = palette.text;
    let knob_on = if palette.is_dark {
        palette.crust
    } else {
        Color::WHITE
    };
    move |_, status| {
        let active = matches!(
            status,
            toggler::Status::Active { is_toggled: true }
                | toggler::Status::Hovered { is_toggled: true }
                | toggler::Status::Disabled { is_toggled: true }
        );
        toggler::Style {
            background: Background::Color(if active { on_bg } else { off_bg }),
            background_border_color: if active { on_bg } else { off_border },
            background_border_width: 1.0,
            foreground: Background::Color(if active { knob_on } else { knob_off }),
            foreground_border_color: Color::TRANSPARENT,
            foreground_border_width: 0.0,
            text_color: None,
            border_radius: None,
            padding_ratio: 0.15,
        }
    }
}

pub fn pick_list_style(
    palette: &Palette,
) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style + 'static {
    let bg = palette.mantle;
    let border = palette.hairline_strong;
    let border_hover = palette.accent;
    let text = palette.text;
    let placeholder = palette.subtext0;
    move |_, status| {
        let hovered = matches!(
            status,
            pick_list::Status::Hovered | pick_list::Status::Opened { .. }
        );
        pick_list::Style {
            background: Background::Color(bg),
            border: Border {
                color: if hovered { border_hover } else { border },
                width: 1.0,
                radius: 6.0.into(),
            },
            text_color: text,
            placeholder_color: placeholder,
            handle_color: text,
        }
    }
}

// ============================================================================
// SCROLLBAR + RULE
// ============================================================================

pub fn scrollable_style(
    palette: &Palette,
) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style + 'static {
    let thumb = if palette.is_dark {
        Color::from_rgba(1.0, 1.0, 1.0, 0.14)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.18)
    };
    let thumb_hover = if palette.is_dark {
        Color::from_rgba(1.0, 1.0, 1.0, 0.24)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.30)
    };
    let panel_bg = palette.crust;
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

pub fn rule_style(palette: &Palette) -> impl Fn(&Theme) -> rule::Style + 'static {
    let color = palette.hairline;
    move |_| rule::Style {
        color,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

// ============================================================================
// TEXT — explicit colour helpers used inline by the views
// ============================================================================

pub fn text_dim(palette: &Palette) -> impl Fn(&Theme) -> text::Style + 'static {
    let color = palette.subtext0;
    move |_| text::Style { color: Some(color) }
}

pub fn text_faint(palette: &Palette) -> impl Fn(&Theme) -> text::Style + 'static {
    let color = palette.overlay0;
    move |_| text::Style { color: Some(color) }
}

pub fn text_accent(palette: &Palette) -> impl Fn(&Theme) -> text::Style + 'static {
    let color = palette.accent;
    move |_| text::Style { color: Some(color) }
}
