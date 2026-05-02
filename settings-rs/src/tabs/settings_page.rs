//! "Settings" tab — overlay-specific knobs that don't fit any of
//! the legacy device-config sections. Currently hosts the Visuals
//! and Animation sub-panels stacked. Future additions (theme
//! picker, autostart toggle, etc.) land as additional sections in
//! the same column.

use crate::widgets::section_header;
use crate::{tabs, Message, State};
use iced::widget::{button, column, container, row, rule, text, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &State) -> Element<'_, Message> {
    let header = row![
        section_header("Overlay settings"),
        Space::new().width(Length::Fill),
        button("Reset all to defaults")
            .style(button::secondary)
            .on_press(Message::ResetAll),
    ]
    .align_y(Alignment::Center);

    column![
        header,
        text(
            "Animations and visuals for the radial overlay. Edits autosave \
             — the running overlay picks them up via inotify within ~150 ms."
        )
        .size(12),
        rule::horizontal(1),
        Space::new().height(Length::Fixed(8.0)),
        section_block("Visuals", tabs::visuals::view(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block("Animation", tabs::animation::view(state)),
    ]
    .spacing(10)
    .into()
}

fn section_block<'a>(title: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![
            text(title.to_string()).size(16),
            rule::horizontal(1),
            body,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(container::bordered_box)
    .into()
}
