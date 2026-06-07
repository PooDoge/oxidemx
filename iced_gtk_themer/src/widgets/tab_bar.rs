use iced::{Element, Length};
use iced::widget::{row, button, text, scrollable, container};
use crate::GtkTheme;

/// A GNOME-style Tab Bar widget.
///
/// Wraps tabs in a horizontally scrollable container, gracefully handling overflow
/// which reflects standard GNOME design patterns.
///
/// # Example
/// ```rust,no_run
/// # use iced::widget::text;
/// # use iced_gtk_themer::GtkTheme;
/// # use iced_gtk_themer::widgets::tab_bar::tab_bar;
/// # let theme = GtkTheme::load("adwaita").unwrap();
/// let tabs = vec![
///     ("Tab 1".to_string(), Some(1)),
///     ("Tab 2".to_string(), Some(2)),
/// ];
/// let bar = tab_bar(&theme, tabs, 0);
/// ```
pub fn tab_bar<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    tabs: Vec<(String, Option<Message>)>,
    selected_idx: usize,
) -> Element<'a, Message> {
    let mut tab_row = row![].spacing(4);

    for (i, (label, msg)) in tabs.into_iter().enumerate() {
        let is_selected = i == selected_idx;
        let mut b = button(text(label))
            .padding([8, 16]);
            
        if let Some(m) = msg {
            b = b.on_press(m);
        }

        if is_selected {
            b = b.style(|_: &iced::Theme, s| gtk.button_primary(s));
        } else {
            b = b.style(|_: &iced::Theme, s| gtk.button_secondary(s));
        }

        tab_row = tab_row.push(b);
    }

    let scroll = scrollable(tab_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(4)
                .margin(2)
        ))
        .width(Length::Fill);

    container(scroll)
        .width(Length::Fill)
        .padding([4, 8])
        .into()
}
