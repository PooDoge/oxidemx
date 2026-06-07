use iced::{Element, Length, Padding};
use iced::widget::{column, container, scrollable, button, text};
use crate::GtkTheme;

/// A sidebar widget that acts as a navigation panel.
/// It displays a vertical list of selectable items.
pub struct Sidebar<'a, Message> {
    gtk: &'a GtkTheme,
    options: Vec<(String, Message)>,
    selected_idx: usize,
    width: Length,
}

impl<'a, Message: Clone + 'static> Sidebar<'a, Message> {
    /// Creates a new sidebar with the given navigation options.
    pub fn new(gtk: &'a GtkTheme, options: Vec<(String, Message)>, selected_idx: usize) -> Self {
        Self {
            gtk,
            options,
            selected_idx,
            width: Length::Fixed(200.0),
        }
    }

    /// Sets the width of the sidebar. Default is `Length::Fixed(200.0)`.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Converts the `Sidebar` into an `Element`.
    pub fn into_element(self) -> Element<'a, Message> {
        let mut col = column![].spacing(4).width(Length::Fill);

        for (i, (label, msg)) in self.options.into_iter().enumerate() {
            let is_selected = i == self.selected_idx;

            let mut b = button(text(label))
                .on_press(msg)
                .width(Length::Fill)
                .padding(Padding::from([8, 12]));

            if is_selected {
                b = b.style(|_: &iced::Theme, s| self.gtk.button_primary(s));
            } else {
                // Secondary button for unselected items, or ghost button
                b = b.style(|_: &iced::Theme, s| {
                    let mut style = self.gtk.button_secondary(s);
                    // Make unselected look more like flat list items when not hovered
                    if s == iced::widget::button::Status::Active {
                        style.background = Some(iced::Background::Color(iced::Color::TRANSPARENT));
                        style.border.color = iced::Color::TRANSPARENT;
                    }
                    style
                });
            }

            col = col.push(b);
        }

        let scroll_content = scrollable(col)
            .height(Length::Fill);

        container(scroll_content)
            .width(self.width)
            .height(Length::Fill)
            .padding(8)
            .style(move |_: &iced::Theme| {
                let bg = self.gtk.colors
                    .get("sidebar_bg_color")
                    .or_else(|| self.gtk.colors.get("window_bg_color"))
                    .cloned()
                    .unwrap_or(iced::Color::from_rgb(0.95, 0.95, 0.95));

                let border_color = self.gtk.colors
                    .get("border_color")
                    .cloned()
                    .unwrap_or(iced::Color::from_rgb(0.8, 0.8, 0.8));

                let mut style = container::background(bg);
                // Standard GTK sidebars usually have a right border to separate from content
                style.border = iced::Border {
                    color: border_color,
                    width: 0.0, // Since standard iced doesn't do individual borders well, we rely on parent containers or just subtle background differences
                    radius: 0.0.into(),
                };
                style
            })
            .into()
    }
}

impl<'a, Message: Clone + 'static> From<Sidebar<'a, Message>> for Element<'a, Message> {
    fn from(sidebar: Sidebar<'a, Message>) -> Self {
        sidebar.into_element()
    }
}

/// Helper function to create a new `Sidebar`.
pub fn sidebar<'a, Message: Clone + 'static>(
    gtk: &'a GtkTheme,
    options: Vec<(String, Message)>,
    selected_idx: usize,
) -> Sidebar<'a, Message> {
    Sidebar::new(gtk, options, selected_idx)
}
