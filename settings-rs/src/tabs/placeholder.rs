//! Placeholder for tabs not yet implemented.

use juhradial_widgets::widgets::section_header;
use crate::{Message, State};
use juhradial_widgets::style;
use iced::widget::{column, container, rule, text, Space};
use iced::{Element, Length};

pub fn view<'a>(state: &'a State, title: &str, body: &str) -> Element<'a, Message> {
    let pal = &state.palette;
    container(
        column![
            section_header(title),
            text(body.to_string()).size(13).style(style::text_dim(pal)),
            Space::new().height(Length::Fixed(8.0)),
            rule::horizontal(1).style(style::rule_style(pal)),
            text("This section will land in a future update.")
                .size(11)
                .style(style::text_faint(pal)),
        ]
        .spacing(12),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}
