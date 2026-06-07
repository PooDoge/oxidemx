// ANCHOR: all
use iced::{Element, Length};
use iced::widget::{container, text};

// ANCHOR: list_item_struct
pub struct ListItem<'a, Message> {
    content: Element<'a, Message>,
    padding: f32,
    width: Length,
}
// ANCHOR_END: list_item_struct

// ANCHOR: from
impl<'a, Message> From<String> for ListItem<'a, Message>
where
    Message: 'a,
{
    fn from(s: String) -> Self {
        Self {
            content: text(s).into(),
            padding: 10.0,
            width: Length::Fill,
        }
    }
}
// ANCHOR_END: from

impl<'a, Message> ListItem<'a, Message> {
    // ANCHOR: builder
    pub fn padding(mut self, amount: f32) -> Self {
        self.padding = amount;
        self
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }
    // ANCHOR_END: builder
}

impl<'a, Message> From<ListItem<'a, Message>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(item: ListItem<'a, Message>) -> Self {
        container(item.content)
            .padding(item.padding)
            .width(item.width)
            .into()
    }
}
// ANCHOR_END: all
