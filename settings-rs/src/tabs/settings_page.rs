//! "Settings" tab — overlay-specific knobs that don't fit any of
//! the legacy device-config sections. Hosts the Theme picker,
//! Visuals, and Animation sub-panels stacked.

use crate::palette::theme_catalogue;
use crate::widgets::section_header;
use crate::{style, tabs, Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, Space};
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

    column![
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
    .spacing(10)
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
