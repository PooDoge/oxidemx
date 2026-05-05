//! "Settings" tab — overlay-specific knobs that don't fit any of
//! the legacy device-config sections. Hosts the Theme picker,
//! Visuals, and Animation sub-panels stacked.

use crate::palette::theme_catalogue;
use crate::widgets::section_header;
use crate::{style, tabs, Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, text_input, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let header = row![
        section_header("Overlay settings"),
        Space::new().width(Length::Fill),
        button(text("Reset all to defaults").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::ResetAll),
    ]
    .align_y(Alignment::Center);

    column![
        header,
        text(
            "Theme, visuals, and animations for the radial overlay + this \
             settings window. Edits autosave; the running overlay picks them \
             up via inotify within ~150 ms."
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        section_block(state, "Theme", theme_picker(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "Visuals", tabs::visuals::view(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "Animation", tabs::animation::view(state)),
    ]
    .spacing(10)
    .into()
}

fn section_block<'a>(
    state: &'a State,
    title: &str,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let pal = &state.palette;
    container(
        column![
            text(title.to_string()).size(16),
            rule::horizontal(1).style(style::rule_style(pal)),
            body,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Theme picker
// ============================================================================

fn theme_picker(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let entries = theme_catalogue();
    let current_slug = state.config.theme.as_str().to_string();
    let options: Vec<ThemeChoice> = entries
        .iter()
        .map(|(slug, name)| ThemeChoice {
            slug: slug.clone(),
            display: name.clone(),
        })
        .collect();
    let selected = options.iter().find(|c| c.slug == current_slug).cloned();

    let picker = pick_list(options, selected, |choice: ThemeChoice| {
        Message::SetTheme(choice.slug)
    })
    .style(style::pick_list_style(pal))
    .text_size(13);

    let preview = swatch_row(pal);

    let customise_btn = button(
        text(if state.theme_editor.is_some() {
            "Close customiser"
        } else {
            "Customise…"
        })
        .size(11),
    )
    .style(style::btn_secondary(pal))
    .on_press(Message::ToggleThemeCustomiser);

    let mut col = column![
        text(
            "Pick a theme. Re-styles every settings widget instantly; the \
             radial overlay reads this field on next show too."
        )
        .size(12)
        .style(style::text_dim(pal)),
        row![
            text("Theme").size(13),
            Space::new().width(Length::Fixed(16.0)),
            picker,
            Space::new().width(Length::Fill),
            customise_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(8),
        row![
            text("Palette preview")
                .size(11)
                .style(style::text_faint(pal)),
            Space::new().width(Length::Fixed(8.0)),
            preview,
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    ]
    .spacing(10);

    if let Some(editor) = state.theme_editor.as_ref() {
        col = col.push(palette_editor(state, editor));
    }
    col.into()
}

// ============================================================================
// Custom palette editor
// ============================================================================

fn palette_editor<'a>(
    state: &'a State,
    editor: &'a crate::ThemeEditor,
) -> Element<'a, Message> {
    let pal = &state.palette;

    // 24 colour fields, three columns × eight rows.
    let fields: &[(&str, &str)] = &[
        // base + surface stack
        ("crust", &editor.working.crust),
        ("mantle", &editor.working.mantle),
        ("base", &editor.working.base),
        ("surface0", &editor.working.surface0),
        ("surface1", &editor.working.surface1),
        ("surface2", &editor.working.surface2),
        ("overlay0", &editor.working.overlay0),
        ("overlay1", &editor.working.overlay1),
        // text
        ("text", &editor.working.text),
        ("subtext1", &editor.working.subtext1),
        ("subtext0", &editor.working.subtext0),
        // accents
        ("accent", &editor.working.accent),
        ("accent2", &editor.working.accent2),
        ("accent_dim", &editor.working.accent_dim),
        // slice colours
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

    let mut grid = column![].spacing(4);
    let mut row_acc: Vec<Element<Message>> = Vec::new();
    for (i, (name, value)) in fields.iter().enumerate() {
        row_acc.push(color_row(*name, value));
        if (i + 1) % 2 == 0 || i + 1 == fields.len() {
            let mut r = row![].spacing(8);
            for el in row_acc.drain(..) {
                r = r.push(el);
            }
            grid = grid.push(r);
        }
    }

    let save_row = row![
        text_input("Theme name (e.g. \"midnight\")", &editor.slug)
            .on_input(Message::SetCustomThemeName)
            .padding(6)
            .size(12)
            .width(Length::Fill),
        button(text("Save as user theme").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::SaveCustomTheme),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(
        column![
            text("Palette editor")
                .size(13)
                .style(style::text_dim(pal)),
            text(
                "Each field is a #rrggbb hex. The active settings window \
                 re-skins live as you type. \"Save as user theme\" writes \
                 it to ~/.local/share/juhradial/themes/<name>.json and \
                 switches the picker to it."
            )
            .size(11)
            .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            grid,
            rule::horizontal(1).style(style::rule_style(pal)),
            save_row,
        ]
        .spacing(8),
    )
    .padding(12)
    .style(style::card_quiet(pal))
    .into()
}

fn color_row<'a>(field: &'a str, value: &'a str) -> Element<'a, Message> {
    let owned_field = field.to_string();
    let swatch_color = juhradial_shared::theme::parse_hex_rgba(value)
        .map(|(r, g, b, _)| iced::Color::from_rgb(r as f32, g as f32, b as f32))
        .unwrap_or(iced::Color::from_rgb(1.0, 0.0, 1.0));
    row![
        container(
            Space::new()
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0)),
        )
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(swatch_color)),
            border: iced::Border {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.18),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }),
        text(field.to_string()).size(11).width(Length::Fixed(80.0)),
        text_input("#rrggbb", value)
            .on_input(move |v| Message::SetThemeColor {
                field: owned_field.clone(),
                value: v,
            })
            .padding(4)
            .size(11)
            .width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .spacing(6)
    .width(Length::FillPortion(1))
    .into()
}

fn swatch_row(pal: &crate::palette::Palette) -> Element<'static, Message> {
    let swatches = [
        pal.accent,
        pal.green,
        pal.yellow,
        pal.red,
        pal.blue,
        pal.mauve,
        pal.pink,
        pal.peach,
        pal.teal,
        pal.sapphire,
        pal.lavender,
    ];
    let mut row_w = row![].spacing(4);
    for c in swatches {
        row_w = row_w.push(swatch(c));
    }
    row_w.into()
}

fn swatch(color: iced::Color) -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0)),
    )
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(color)),
        border: iced::Border {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.18),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThemeChoice {
    slug: String,
    display: String,
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}
