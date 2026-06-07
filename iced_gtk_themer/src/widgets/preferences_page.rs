use iced::{Element, Length, Padding};
use iced::widget::{column, scrollable};
use crate::widgets::clamp::clamp;

/// A preferences page layout mimicking AdwPreferencesPage.
/// It wraps content in a scrollable view and clamps it to a readable width.
/// Typically used to display a column of `preferences_group`s.
pub fn preferences_page<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let col = column![content.into()]
        .spacing(24)
        .padding(Padding {
            top: 24.0,
            bottom: 24.0,
            left: 12.0,
            right: 12.0,
        });

    let clamped = clamp(col, 600.0);

    scrollable(clamped)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
