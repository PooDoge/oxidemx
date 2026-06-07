use iced::{Element, Length, Alignment};
use iced::widget::{container, column};

/// A container that constraints its child to a maximum width and centers it,
/// similar to AdwClamp in Libadwaita.
pub fn clamp<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    max_width: f32,
) -> Element<'a, Message> {
    container(
        column![content.into()]
            .max_width(max_width)
            .width(Length::Fill)
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .into()
}
