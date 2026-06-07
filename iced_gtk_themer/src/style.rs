use iced::{Color, Theme, Border, Background, Shadow};
use iced::widget::{self, button, text_input, container};
use iced::theme::palette;
use std::collections::HashMap;

/// Helper to get a color or a default
fn get_color(colors: &HashMap<String, Color>, keys: &[&str], default: Color) -> Color {
    for k in keys {
        if let Some(&c) = colors.get(*k) {
            return c;
        }
    }
    default
}

/// Helper to get the correct text color
fn get_fg(colors: &HashMap<String, Color>, keys: &[&str], default: Color) -> Color {
    get_color(colors, keys, default)
}

/// Standard GTK Button Style (Secondary)
pub fn button_secondary(colors: &HashMap<String, Color>, _theme: &Theme, status: button::Status) -> button::Style {
    let bg = get_color(colors, &["btn_bg_color", "headerbar_bg_color", "window_bg_color"], Color::from_rgb(0.9, 0.9, 0.9));
    let fg = get_fg(colors, &["btn_fg_color", "window_fg_color", "theme_fg_color"], Color::BLACK);
    let border = get_color(colors, &["btn_border_color", "border_color"], Color::from_rgb(0.5, 0.5, 0.5));
    
    let active_bg = palette::deviate(bg, 0.05);

    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(palette::lighten(bg, 0.05))),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(active_bg)),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(palette::darken(bg, 0.05))),
            text_color: palette::darken(fg, 0.2),
            border: Border {
                color: palette::darken(border, 0.1),
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
    }
}

/// Primary GTK Button Style
pub fn button_primary(colors: &HashMap<String, Color>, _theme: &Theme, status: button::Status) -> button::Style {
    let bg = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));
    let fg = get_fg(colors, &["theme_selected_fg_color"], Color::WHITE);
    let border = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));
    
    let active_bg = palette::deviate(bg, 0.05);

    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(palette::lighten(bg, 0.05))),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(active_bg)),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(palette::darken(bg, 0.05))),
            text_color: palette::darken(fg, 0.2),
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
    }
}

/// Destructive GTK Button Style
pub fn button_destructive(colors: &HashMap<String, Color>, _theme: &Theme, status: button::Status) -> button::Style {
    let bg = get_color(colors, &["destructive_bg_color", "error_bg_color", "error_color"], Color::from_rgb(0.8, 0.2, 0.2));
    let fg = get_fg(colors, &["destructive_fg_color", "error_fg_color"], Color::WHITE);
    let border = bg;
    
    let active_bg = palette::deviate(bg, 0.05);

    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(palette::lighten(bg, 0.05))),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(active_bg)),
            text_color: fg,
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(palette::darken(bg, 0.05))),
            text_color: palette::darken(fg, 0.2),
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
    }
}

/// View Switcher Inactive Tab Style
pub fn button_view_switcher(colors: &HashMap<String, Color>, _theme: &Theme, status: button::Status) -> button::Style {
    let fg = get_fg(colors, &["headerbar_fg_color", "window_fg_color"], Color::BLACK);
    let hover_bg = get_color(colors, &["headerbar_shade_color", "view_switcher_hover_bg"], Color::from_rgba(0.5, 0.5, 0.5, 0.15));
    let active_bg = get_color(colors, &["headerbar_shade_color", "view_switcher_active_bg"], Color::from_rgba(0.5, 0.5, 0.5, 0.25));

    match status {
        button::Status::Active => button::Style {
            background: None,
            text_color: fg,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(hover_bg)),
            text_color: fg,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(active_bg)),
            text_color: fg,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: None,
            text_color: palette::darken(fg, 0.3),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
            ..Default::default()
        },
    }
}

/// View Switcher Active Tab Style
pub fn button_view_switcher_active(colors: &HashMap<String, Color>, _theme: &Theme, status: button::Status) -> button::Style {
    let bg = get_color(colors, &["headerbar_shade_color", "view_switcher_active_bg"], Color::from_rgba(0.5, 0.5, 0.5, 0.2));
    let fg = get_fg(colors, &["headerbar_fg_color", "window_fg_color"], Color::BLACK);

    let mut style = button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 6.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
        ..Default::default()
    };

    match status {
        button::Status::Hovered => {
            style.background = Some(Background::Color(palette::lighten(bg, 0.05)));
        }
        button::Status::Pressed => {
            style.background = Some(Background::Color(palette::darken(bg, 0.05)));
        }
        button::Status::Disabled => {
            style.text_color = palette::darken(fg, 0.3);
            style.background = Some(Background::Color(palette::darken(bg, 0.3)));
        }
        _ => {}
    }
    style
}

/// GTK TextInput Style
pub fn text_input_style(colors: &HashMap<String, Color>, _theme: &Theme, status: text_input::Status) -> text_input::Style {
    let bg = get_color(colors, &["view_bg_color", "window_bg_color"], Color::WHITE);
    let fg = get_color(colors, &["view_fg_color", "window_fg_color", "theme_fg_color"], Color::BLACK);
    let border = get_color(colors, &["border_color", "btn_border_color"], Color::from_rgb(0.5, 0.5, 0.5));
    let active_border = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));
    let placeholder = palette::deviate(fg, 0.3);
    let icon = fg;
    let selection = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));

    match status {
        text_input::Status::Active => text_input::Style {
            background: Background::Color(bg),
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon,
            placeholder,
            value: fg,
            selection,
        },
        text_input::Status::Hovered => text_input::Style {
            background: Background::Color(bg),
            border: Border {
                color: active_border,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon,
            placeholder,
            value: fg,
            selection,
        },
        text_input::Status::Focused { .. } => text_input::Style {
            background: Background::Color(bg),
            border: Border {
                color: active_border,
                width: 2.0,
                radius: 6.0.into(),
            },
            icon,
            placeholder,
            value: fg,
            selection,
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(palette::darken(bg, 0.05)),
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon: placeholder,
            placeholder,
            value: placeholder,
            selection,
        },
    }
}

/// GTK Checkbox Style
pub fn checkbox_style(colors: &HashMap<String, Color>, _theme: &Theme, status: widget::checkbox::Status) -> widget::checkbox::Style {
    let bg = get_color(colors, &["view_bg_color", "window_bg_color"], Color::WHITE);
    let border = get_color(colors, &["border_color", "btn_border_color"], Color::from_rgb(0.5, 0.5, 0.5));
    let active_bg = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));
    let active_fg = get_color(colors, &["theme_selected_fg_color"], Color::WHITE);
    let fg = get_color(colors, &["window_fg_color", "theme_fg_color"], Color::BLACK);

    match status {
        widget::checkbox::Status::Active { is_checked } => widget::checkbox::Style {
            background: Background::Color(if is_checked { active_bg } else { bg }),
            icon_color: if is_checked { active_fg } else { Color::TRANSPARENT },
            border: Border {
                color: if is_checked { active_bg } else { border },
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Some(fg),
        },
        widget::checkbox::Status::Hovered { is_checked } => widget::checkbox::Style {
            background: Background::Color(if is_checked { palette::lighten(active_bg, 0.05) } else { palette::deviate(bg, 0.05) }),
            icon_color: if is_checked { active_fg } else { Color::TRANSPARENT },
            border: Border {
                color: if is_checked { palette::lighten(active_bg, 0.05) } else { active_bg },
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Some(fg),
        },
        widget::checkbox::Status::Disabled { is_checked } => widget::checkbox::Style {
            background: Background::Color(palette::darken(bg, 0.05)),
            icon_color: if is_checked { active_fg } else { Color::TRANSPARENT },
            border: Border {
                color: border,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Some(palette::darken(fg, 0.2)),
        },
    }
}

/// GTK Radio Style
pub fn radio_style(colors: &HashMap<String, Color>, _theme: &Theme, status: widget::radio::Status) -> widget::radio::Style {
    let bg = get_color(colors, &["view_bg_color", "window_bg_color"], Color::WHITE);
    let border = get_color(colors, &["border_color", "btn_border_color"], Color::from_rgb(0.5, 0.5, 0.5));
    let active_bg = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));
    let fg = get_color(colors, &["window_fg_color", "theme_fg_color"], Color::BLACK);

    let base = widget::radio::Style {
        background: Background::Color(bg),
        dot_color: active_bg,
        border_width: 1.0,
        border_color: border,
        text_color: Some(fg),
    };

    match status {
        widget::radio::Status::Active { .. } => base,
        widget::radio::Status::Hovered { .. } => widget::radio::Style {
            background: Background::Color(palette::deviate(bg, 0.05)),
            border_color: active_bg,
            ..base
        },
    }
}

/// GTK Slider Style
pub fn slider_style(colors: &HashMap<String, Color>, _theme: &Theme, status: widget::slider::Status) -> widget::slider::Style {
    let bg = get_color(colors, &["window_bg_color"], Color::from_rgb(0.9, 0.9, 0.9));
    let active_bg = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));
    let border = get_color(colors, &["border_color"], Color::from_rgb(0.5, 0.5, 0.5));
    
    let rail_color = palette::darken(bg, 0.1);

    widget::slider::Style {
        breakpoint: iced::widget::slider::Breakpoint { color: Color::TRANSPARENT },
        rail: widget::slider::Rail {
            backgrounds: (Background::Color(active_bg), Background::Color(rail_color)),
            width: 4.0,
            border: Border {
                radius: 2.0.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
        },
        handle: widget::slider::Handle {
            shape: widget::slider::HandleShape::Circle { radius: 8.0 },
            background: Background::Color(match status {
                widget::slider::Status::Active => active_bg,
                widget::slider::Status::Hovered => palette::lighten(active_bg, 0.05),
                widget::slider::Status::Dragged => palette::darken(active_bg, 0.05),
            }),
            border_width: 1.0,
            border_color: match status {
                widget::slider::Status::Active => border,
                _ => active_bg,
            },
        },
    }
}

/// GTK PickList Style
pub fn pick_list_style(colors: &HashMap<String, Color>, _theme: &Theme, status: widget::pick_list::Status) -> widget::pick_list::Style {
    let bg = get_color(colors, &["btn_bg_color", "window_bg_color"], Color::WHITE);
    let fg = get_color(colors, &["btn_fg_color", "window_fg_color"], Color::BLACK);
    let border = get_color(colors, &["btn_border_color", "border_color"], Color::from_rgb(0.5, 0.5, 0.5));
    let active_border = get_color(colors, &["theme_selected_bg_color"], Color::from_rgb(0.2, 0.5, 0.8));

    let base = widget::pick_list::Style {
        text_color: fg,
        placeholder_color: palette::deviate(fg, 0.3),
        handle_color: fg,
        background: Background::Color(bg),
        border: Border {
            radius: 6.0.into(),
            width: 1.0,
            color: border,
        },
    };

    match status {
        widget::pick_list::Status::Active => base,
        widget::pick_list::Status::Hovered | widget::pick_list::Status::Opened { .. } => widget::pick_list::Style {
            border: Border {
                color: active_border,
                ..base.border
            },
            ..base
        },
    }
}

/// GTK Container Style (Card)
pub fn container_card(colors: &HashMap<String, Color>, _theme: &Theme) -> container::Style {
    let bg = get_color(colors, &["card_bg_color", "view_bg_color"], Color::WHITE);
    let fg = get_color(colors, &["card_fg_color", "view_fg_color"], Color::BLACK);
    let border = get_color(colors, &["border_color"], Color::from_rgb(0.5, 0.5, 0.5));

    container::Style {
        icon_color: None,
        text_color: Some(fg),
        background: Some(Background::Color(bg)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: border,
        },
        shadow: Shadow::default(),
        ..Default::default()
    }
}
