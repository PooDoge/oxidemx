use iced::{Element, Length, Padding};
use iced::widget::{column, container, rule};
use crate::GtkTheme;

/// A Boxed List widget that mimics the GTK style of grouping items inside a rounded box
/// with separators between them.
///
/// Use `BoxedList::new(gtk)` to create a new list, then use `.push()` to add elements.
pub struct BoxedList<'a, Message> {
    gtk: &'a GtkTheme,
    children: Vec<Element<'a, Message>>,
    item_padding: Padding,
}

impl<'a, Message: Clone + 'static> BoxedList<'a, Message> {
    /// Creates a new, empty BoxedList.
    pub fn new(gtk: &'a GtkTheme) -> Self {
        Self {
            gtk,
            children: Vec::new(),
            item_padding: Padding::from(0),
        }
    }

    /// Adds an item to the list.
    pub fn push(mut self, child: impl Into<Element<'a, Message>>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Sets the padding for each item in the list. Default is 0.
    pub fn item_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.item_padding = padding.into();
        self
    }

    /// Converts the `BoxedList` into an `Element`.
    pub fn into_element(self) -> Element<'a, Message> {
        let count = self.children.len();
        let mut col = column::with_capacity((2 * count).saturating_sub(1));

        for (i, child) in self.children.into_iter().enumerate() {
            if i > 0 {
                // Separator
                let separator_color = self.gtk.colors
                    .get("border_color")
                    .cloned()
                    .unwrap_or(iced::Color::from_rgb(0.8, 0.8, 0.8));

                col = col.push(
                    rule::horizontal(1).style(move |theme| {
                        let mut style = rule::Style { color: iced::Color::TRANSPARENT, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true };
                        style.color = separator_color;
                        style.radius = 0.0.into();
                        style.fill_mode = rule::FillMode::Full;
                        style
                    })
                );
            }

            col = col.push(
                container(child)
                    .width(Length::Fill)
                    .padding(self.item_padding)
            );
        }

        container(col)
            .width(Length::Fill)
            .style(|_: &iced::Theme| self.gtk.container_card())
            .into()
    }
}

impl<'a, Message: Clone + 'static> From<BoxedList<'a, Message>> for Element<'a, Message> {
    fn from(list: BoxedList<'a, Message>) -> Self {
        list.into_element()
    }
}

/// Helper function to create a new `BoxedList`.
pub fn boxed_list<'a, Message: Clone + 'static>(gtk: &'a GtkTheme) -> BoxedList<'a, Message> {
    BoxedList::new(gtk)
}
