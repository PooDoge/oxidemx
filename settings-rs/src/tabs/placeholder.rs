//! Placeholder for tabs we haven't implemented yet — keeps the
//! sidebar shape matching the legacy UI without forcing us to ship
//! every section at once.

use crate::widgets::section_header;
use crate::Message;
use iced::widget::{column, container, text};
use iced::Element;

pub fn view<'a>(title: &str, body: &str) -> Element<'a, Message> {
    container(
        column![
            section_header(title),
            text(body.to_string()).size(13),
            text("This section will land in a future update.").size(11),
        ]
        .spacing(12),
    )
    .into()
}
