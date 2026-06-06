//! Full-panel theme customiser. Inspired by VS Code's "Color Theme
//! Studio", Material Theme Builder, and Photoshop's colour picker:
//!
//!   * Two-column layout. Left column shows a live mock UI with
//!     every theme slot as a clickable region; right column hosts
//!     the gradient picker once a field is selected.
//!   * Mock UI uses real surfaces / text / accent / slice swatches
//!     so the user sees what they're editing rather than a
//!     decontextualised hex grid.
//!   * Below the mockup, a small radial preview repeats the same
//!     trick for the parts of the palette that only show up in the
//!     overlay (slice colours, accent_dim).
//!   * Right column also hosts a role-grouped swatch grid (Surfaces
//!     / Text / Accents / Slice colours) as a fallback navigation
//!     for slots the mockup doesn't expose directly.
//!   * Header has slug input + Revert + Save, mirroring the
//!     existing Save flow but laid out for the wider full-panel
//!     chrome.

use iced::widget::{
    button, canvas, column, container, row, rule, scrollable, slider, text, text_input,
    Space,
};
use iced::{Alignment, Element, Length};

use crate::color_canvas;
use oxidemx_widgets::style;
use crate::{ColorChannel, Message, State, ThemeEditor};

pub fn view<'a>(state: &'a State, editor: &'a ThemeEditor) -> Element<'a, Message> {
    let pal = &state.palette;

    // ---- Header (slug input + Revert + Save) -------------------
    let dirty = editor.working != editor.original;
    let mut revert_btn = button(text("Revert").size(11)).style(style::btn_secondary(pal));
    if dirty {
        revert_btn = revert_btn.on_press(Message::RevertCustomTheme);
    }
    let header_actions = row![
        text_input("Theme name (e.g. \"midnight\")", &editor.slug)
            .on_input(Message::SetCustomThemeName)
            .padding(6)
            .size(12)
            .width(Length::Fixed(220.0)),
        revert_btn,
        button(text("Save as user theme").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::SaveCustomTheme),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let intro = text(
        "Click any element on the left mock UI to edit its colour. \
         Edits live-preview on this window; \"Save\" writes it to \
         ~/.local/share/oxidemx/themes/<name>.json and switches \
         the picker to it.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let header = column![header_actions, intro].spacing(6);

    // ---- Left column: mock UI + radial preview -----------------
    let left = column![
        mockup_window(state, editor),
        Space::new().height(Length::Fixed(12.0)),
        slice_palette_strip(editor),
        Space::new().height(Length::Fixed(12.0)),
        radial_preview_panel(state),
    ]
    .spacing(0);

    // ---- Right column: gradient picker OR hint + grouped grid --
    let right = right_column(state, editor);

    let split = row![
        container(left).width(Length::FillPortion(1)),
        Space::new().width(Length::Fixed(16.0)),
        container(scrollable(right).style(style::scrollable_style(pal)))
            .width(Length::FillPortion(1))
            .height(Length::Fill),
    ]
    .spacing(0)
    .height(Length::Fill);

    column![header, rule::horizontal(1).style(style::rule_style(pal)), split]
        .spacing(8)
        .into()
}

// ============================================================================
// Mock UI window — surfaces / text / accent / button samples,
// each clickable to open that field's picker.
// ============================================================================

fn mockup_window<'a>(_state: &'a State, editor: &'a ThemeEditor) -> Element<'a, Message> {
    // Pull every colour straight from the editor's working struct
    // so the mock UI updates frame-by-frame as the user drags
    // sliders, without depending on any palette field that the
    // settings crate's `Palette` happens not to expose.
    let working = &editor.working;
    let crust = parse_hex(&working.crust);
    let mantle = parse_hex(&working.mantle);
    let base = parse_hex(&working.base);
    let surface0 = parse_hex(&working.surface0);
    let surface1 = parse_hex(&working.surface1);
    let surface2 = parse_hex(&working.surface2);
    let overlay0 = parse_hex(&working.overlay0);
    let overlay1 = parse_hex(&working.overlay1);
    let textc = parse_hex(&working.text);
    let subtext1 = parse_hex(&working.subtext1);
    let subtext0 = parse_hex(&working.subtext0);
    let accent = parse_hex(&working.accent);
    let accent_dim = parse_hex(&working.accent_dim);

    let title_bar = clickable_band(
        "crust",
        crust,
        row![
            text(" ● ● ● ").size(13).style(text_color(subtext0)),
            Space::new().width(Length::Fill),
            text("Window title — click any region")
                .size(11)
                .style(text_color(subtext0)),
            Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .spacing(0),
    );

    let sample_text_row = row![
        clickable_text_chip("text", textc, "Body text"),
        Space::new().width(Length::Fixed(8.0)),
        clickable_text_chip("subtext1", subtext1, "Subtitle"),
        Space::new().width(Length::Fixed(8.0)),
        clickable_text_chip("subtext0", subtext0, "Caption"),
    ]
    .spacing(0)
    .align_y(Alignment::Center);

    let accent_row = row![
        accent_button("accent", accent, "Primary"),
        Space::new().width(Length::Fixed(8.0)),
        accent_button("accent_dim", accent_dim, "Secondary"),
    ]
    .spacing(0)
    .align_y(Alignment::Center);

    let surface_stack = column![
        clickable_band(
            "surface0",
            surface0,
            text("surface0  — card background").size(11).style(text_color(textc)),
        ),
        clickable_band(
            "surface1",
            surface1,
            text("surface1  — slightly lighter").size(11).style(text_color(textc)),
        ),
        clickable_band(
            "surface2",
            surface2,
            text("surface2  — highest in stack").size(11).style(text_color(textc)),
        ),
        clickable_band(
            "overlay0",
            overlay0,
            text("overlay0  — borders, dividers")
                .size(11)
                .style(text_color(textc)),
        ),
        clickable_band(
            "overlay1",
            overlay1,
            text("overlay1  — emphasis lines")
                .size(11)
                .style(text_color(textc)),
        ),
    ]
    .spacing(2);

    let mantle_band = clickable_band(
        "mantle",
        mantle,
        text("mantle — header / footer band")
            .size(10)
            .style(text_color(subtext0)),
    );

    let body = container(
        column![
            sample_text_row,
            Space::new().height(Length::Fixed(10.0)),
            accent_row,
            Space::new().height(Length::Fixed(10.0)),
            surface_stack,
        ]
        .spacing(0)
        .padding(12),
    )
    .width(Length::Fill)
    .style({
        let bg = base;
        move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(bg)),
            ..Default::default()
        }
    });

    let base_btn = button(body)
        .padding(0)
        .style(make_button_style(base, false))
        .on_press(Message::ToggleThemeColorPicker("base".into()))
        .width(Length::Fill);

    container(
        column![title_bar, mantle_band, base_btn]
            .spacing(0)
            .width(Length::Fill),
    )
    .style(move |_| iced::widget::container::Style {
        border: iced::Border {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .padding(0)
    .width(Length::Fill)
    .into()
}

// ============================================================================
// Slice colour strip — eight wedge-coloured chips. Clicking one
// opens the picker for that slice colour.
// ============================================================================

fn slice_palette_strip(editor: &ThemeEditor) -> Element<'_, Message> {
    let mut row_el = row![text("Slices:").size(11)]
        .spacing(6)
        .align_y(Alignment::Center);
    let entries: &[(&str, &str)] = &[
        ("green", &editor.working.green),
        ("yellow", &editor.working.yellow),
        ("red", &editor.working.red),
        ("blue", &editor.working.blue),
        ("mauve", &editor.working.mauve),
        ("pink", &editor.working.pink),
        ("peach", &editor.working.peach),
        ("teal", &editor.working.teal),
        ("sapphire", &editor.working.sapphire),
        ("lavender", &editor.working.lavender),
    ];
    for (name, hex) in entries.iter() {
        let color = parse_hex(hex);
        let editing = editor.editing_field.as_deref() == Some(*name);
        let chip = button(
            container(text(*name).size(10).style(text_color(contrast_text(color))))
                .center_x(Length::Fixed(60.0))
                .center_y(Length::Fixed(28.0)),
        )
        .padding(0)
        .style(make_button_style(color, editing))
        .on_press(Message::ToggleThemeColorPicker((*name).to_string()));
        row_el = row_el.push(chip);
    }
    container(row_el).into()
}

// ============================================================================
// Radial preview reuses radial_preview.rs so users see the actual
// overlay rendered with the live palette.
// ============================================================================

fn radial_preview_panel<'a>(state: &'a State) -> Element<'a, Message> {
    let pal = &state.palette;
    let v = &state.config.radial_menu.visuals;
    let font = crate::fonts::resolve(&v.font_family);
    let preview = crate::radial_preview::radial_preview_widget::<Message>(
        pal,
        state.active_slices(),
        state.selected_slice,
        state.icons.clone(),
        state.iced_handles.clone(),
        font,
        v.center_label_size,
        260.0,
    );
    container(
        column![
            text("Radial overlay preview").size(11).style(style::text_dim(pal)),
            preview,
        ]
        .spacing(6)
        .align_x(Alignment::Center),
    )
    .padding(8)
    .style(style::card_quiet(pal))
    .into()
}

// ============================================================================
// Right column: gradient picker (when a field is being edited) +
// the role-grouped swatch grid.
// ============================================================================

fn right_column<'a>(state: &'a State, editor: &'a ThemeEditor) -> Element<'a, Message> {
    let pal = &state.palette;

    let picker_panel: Element<Message> = if let Some(field) = editor.editing_field.as_deref() {
        let value = crate::theme_field_value(&editor.working, field);
        gradient_picker(state, field, &value)
    } else {
        container(
            column![
                text("Pick a colour to edit")
                    .size(14)
                    .style(style::text_dim(pal)),
                Space::new().height(Length::Fixed(4.0)),
                text(
                    "Click anywhere on the mock UI on the left, on a slice \
                     chip, on the radial preview ring, or on a swatch in the \
                     grid below. The full HSV picker opens here."
                )
                .size(11)
                .style(style::text_dim(pal)),
            ]
            .spacing(2),
        )
        .padding(14)
        .style(style::card_quiet(pal))
        .into()
    };

    column![
        picker_panel,
        Space::new().height(Length::Fixed(12.0)),
        role_grouped_grid(state, editor),
    ]
    .spacing(0)
    .into()
}

// ============================================================================
// Gradient picker — SV square + hue strip + RGB sliders + presets
// ============================================================================

fn gradient_picker<'a>(
    state: &'a State,
    field: &'a str,
    value: &str,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let (r, g, b) = crate::parse_hex_channels(value);
    let (h, s, v) = color_canvas::rgb_to_hsv(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
    );

    let owned = field.to_string();
    let sv_field = owned.clone();
    let sv = canvas(color_canvas::HsvSquare {
        hue: h,
        saturation: s,
        value: v,
        on_change: move |s, v| Message::SetThemeColorSv {
            field: sv_field.clone(),
            s,
            v,
        },
    })
    .width(Length::Fixed(200.0))
    .height(Length::Fixed(200.0));

    let hue_field = owned.clone();
    let hue = canvas(color_canvas::HueStrip {
        hue: h,
        on_change: move |h| Message::SetThemeColorHue {
            field: hue_field.clone(),
            h,
        },
    })
    .width(Length::Fixed(24.0))
    .height(Length::Fixed(200.0));

    let preview_color = parse_hex(value);
    let preview_chip = container(
        Space::new()
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(48.0)),
    )
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(preview_color)),
        border: iced::Border {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    let r_field = owned.clone();
    let g_field = owned.clone();
    let b_field = owned.clone();
    let r_slider = slider(0u8..=255, r, move |v| Message::SetThemeColorChannel {
        field: r_field.clone(),
        channel: ColorChannel::Red,
        value: v,
    });
    let g_slider = slider(0u8..=255, g, move |v| Message::SetThemeColorChannel {
        field: g_field.clone(),
        channel: ColorChannel::Green,
        value: v,
    });
    let b_slider = slider(0u8..=255, b, move |v| Message::SetThemeColorChannel {
        field: b_field.clone(),
        channel: ColorChannel::Blue,
        value: v,
    });

    let channel_row = |label: &'static str, current: u8, sl: iced::widget::Slider<'a, u8, Message>| {
        row![
            text(label).size(11).width(Length::Fixed(20.0)),
            sl,
            text(format!("{current:>3}"))
                .size(11)
                .width(Length::Fixed(30.0)),
        ]
        .align_y(Alignment::Center)
        .spacing(8)
    };

    let hex_input_field = owned.clone();
    let hex_row = row![
        text("HEX").size(11).width(Length::Fixed(30.0)),
        text_input("#rrggbb", value)
            .on_input(move |s| Message::SetThemeColor {
                field: hex_input_field.clone(),
                value: s,
            })
            .padding(4)
            .size(11)
            .width(Length::Fixed(110.0)),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let right_col = column![
        preview_chip,
        Space::new().height(Length::Fixed(8.0)),
        channel_row("R", r, r_slider),
        channel_row("G", g, g_slider),
        channel_row("B", b, b_slider),
        Space::new().height(Length::Fixed(8.0)),
        hex_row,
    ]
    .spacing(4);

    // Snap-to-theme presets
    let presets = [
        ("accent", pal.accent),
        ("text", pal.text),
        ("subtext", pal.subtext0),
        ("surface0", pal.surface0),
        ("surface1", pal.surface1),
        ("base", pal.base),
        ("crust", pal.crust),
        ("teal", pal.teal),
    ];
    let mut preset_row = row![text("Snap to:").size(11)]
        .spacing(6)
        .align_y(Alignment::Center);
    for (name, color) in presets.iter() {
        let preset_field = owned.clone();
        let hex = format!(
            "#{:02x}{:02x}{:02x}",
            (color.r * 255.0) as u8,
            (color.g * 255.0) as u8,
            (color.b * 255.0) as u8,
        );
        let preset_color = *color;
        let chip = button(
            Space::new()
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0)),
        )
        .padding(0)
        .style(move |_, status| iced::widget::button::Style {
            background: Some(iced::Background::Color(preset_color)),
            border: iced::Border {
                color: match status {
                    iced::widget::button::Status::Hovered => {
                        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.85)
                    }
                    _ => iced::Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: iced::Color::WHITE,
            ..Default::default()
        })
        .on_press(Message::SetThemeColor {
            field: preset_field.clone(),
            value: hex.clone(),
        });
        preset_row = preset_row.push(chip);
        preset_row = preset_row.push(text(*name).size(10));
    }

    container(
        column![
            row![
                text(format!("Editing: {field}"))
                    .size(13)
                    .style(style::text_dim(pal)),
                Space::new().width(Length::Fill),
                button(text("Done").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::ToggleThemeColorPicker(field.to_string())),
            ]
            .align_y(Alignment::Center),
            row![sv, hue, right_col]
                .spacing(12)
                .align_y(Alignment::Start),
            preset_row,
        ]
        .spacing(8),
    )
    .padding(12)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Role-grouped grid — Surfaces / Text / Accents / Slice colours.
// Each group is a section header + a flex row of swatch buttons.
// ============================================================================

fn role_grouped_grid<'a>(state: &'a State, editor: &'a ThemeEditor) -> Element<'a, Message> {
    let pal = &state.palette;
    let groups: &[(&str, &[(&str, &str)])] = &[
        (
            "Surfaces",
            &[
                ("crust", &editor.working.crust),
                ("mantle", &editor.working.mantle),
                ("base", &editor.working.base),
                ("surface0", &editor.working.surface0),
                ("surface1", &editor.working.surface1),
                ("surface2", &editor.working.surface2),
                ("overlay0", &editor.working.overlay0),
                ("overlay1", &editor.working.overlay1),
            ],
        ),
        (
            "Text",
            &[
                ("text", &editor.working.text),
                ("subtext1", &editor.working.subtext1),
                ("subtext0", &editor.working.subtext0),
            ],
        ),
        (
            "Accents",
            &[
                ("accent", &editor.working.accent),
                ("accent2", &editor.working.accent2),
                ("accent_dim", &editor.working.accent_dim),
            ],
        ),
        (
            "Slice colours",
            &[
                ("green", &editor.working.green),
                ("yellow", &editor.working.yellow),
                ("red", &editor.working.red),
                ("blue", &editor.working.blue),
                ("mauve", &editor.working.mauve),
                ("pink", &editor.working.pink),
                ("peach", &editor.working.peach),
                ("teal", &editor.working.teal),
                ("sapphire", &editor.working.sapphire),
                ("lavender", &editor.working.lavender),
            ],
        ),
    ];

    let mut col = column![text("All theme slots").size(13).style(style::text_dim(pal))]
        .spacing(8);
    for (group_name, fields) in groups.iter() {
        let mut group_col = column![text(*group_name).size(12)].spacing(4);
        let mut row_acc = row![].spacing(8).align_y(Alignment::Center);
        let mut count = 0;
        for (name, hex) in fields.iter() {
            let editing = editor.editing_field.as_deref() == Some(*name);
            row_acc = row_acc.push(grid_swatch(name, hex, editing));
            count += 1;
            if count % 2 == 0 {
                group_col = group_col.push(row_acc);
                row_acc = row![].spacing(8).align_y(Alignment::Center);
            }
        }
        if count % 2 != 0 {
            group_col = group_col.push(row_acc);
        }
        col = col.push(group_col);
    }
    container(col)
        .padding(12)
        .style(style::card_quiet(pal))
        .into()
}

fn grid_swatch<'a>(field: &'a str, hex: &'a str, editing: bool) -> Element<'a, Message> {
    let color = parse_hex(hex);
    let owned = field.to_string();
    let chip = button(
        Space::new()
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0)),
    )
    .padding(0)
    .style(make_button_style(color, editing))
    .on_press(Message::ToggleThemeColorPicker(owned));
    row![
        chip,
        text(field.to_string())
            .size(11)
            .width(Length::Fill),
        text(hex.to_string()).size(10),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::FillPortion(1))
    .into()
}

// ============================================================================
// Small helpers for the mock UI
// ============================================================================

fn clickable_band<'a>(
    field: &str,
    color: iced::Color,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let owned = field.to_string();
    button(container(body).padding([8, 12]).width(Length::Fill))
        .padding(0)
        .style(make_button_style(color, false))
        .on_press(Message::ToggleThemeColorPicker(owned))
        .width(Length::Fill)
        .into()
}

fn clickable_text_chip(
    field: &str,
    color: iced::Color,
    sample: &str,
) -> Element<'static, Message> {
    let owned = field.to_string();
    let label = format!("{sample}  ({field})");
    button(text(label).size(12).style(text_color(color)))
        .padding([6, 10])
        .style(move |_, status| iced::widget::button::Style {
            background: None,
            border: iced::Border {
                color: match status {
                    iced::widget::button::Status::Hovered => {
                        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4)
                    }
                    _ => iced::Color::TRANSPARENT,
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: color,
            ..Default::default()
        })
        .on_press(Message::ToggleThemeColorPicker(owned))
        .into()
}

fn accent_button(field: &str, color: iced::Color, label: &str) -> Element<'static, Message> {
    let owned = field.to_string();
    let label = label.to_string();
    let bg = color;
    button(text(label).size(11).style(text_color(contrast_text(bg))))
        .padding([6, 14])
        .style(move |_, status| {
            let alpha = match status {
                iced::widget::button::Status::Hovered => 0.92,
                iced::widget::button::Status::Pressed => 0.82,
                _ => 1.0,
            };
            iced::widget::button::Style {
                background: Some(iced::Background::Color(iced::Color {
                    a: alpha,
                    ..bg
                })),
                border: iced::Border {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                text_color: contrast_text(bg),
                ..Default::default()
            }
        })
        .on_press(Message::ToggleThemeColorPicker(owned))
        .into()
}

fn make_button_style(
    bg: iced::Color,
    is_editing: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_, status| {
        let border_alpha = match status {
            iced::widget::button::Status::Hovered
            | iced::widget::button::Status::Pressed => 0.85,
            _ => {
                if is_editing {
                    0.9
                } else {
                    0.18
                }
            }
        };
        iced::widget::button::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, border_alpha),
                width: if is_editing { 2.0 } else { 1.0 },
                radius: 3.0.into(),
            },
            text_color: contrast_text(bg),
            ..Default::default()
        }
    }
}

fn text_color(color: iced::Color) -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    move |_| iced::widget::text::Style { color: Some(color) }
}

/// Pick black or white text for legibility against a background
/// colour. Standard luminance threshold (0.55).
fn contrast_text(bg: iced::Color) -> iced::Color {
    let lum = 0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b;
    if lum > 0.55 {
        iced::Color::from_rgb(0.05, 0.05, 0.05)
    } else {
        iced::Color::from_rgb(0.95, 0.95, 0.95)
    }
}

fn parse_hex(s: &str) -> iced::Color {
    oxidemx_shared::theme::parse_hex_rgba(s)
        .map(|(r, g, b, _)| iced::Color::from_rgb(r as f32, g as f32, b as f32))
        .unwrap_or(iced::Color::from_rgb(1.0, 0.0, 1.0))
}
