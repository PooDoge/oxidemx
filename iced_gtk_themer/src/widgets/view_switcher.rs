use iced::{Element, Length, Alignment};
use iced::widget::{row, column, button, text, scrollable, container};
use crate::GtkTheme;

/// A GNOME-style View Switcher widget.
///
/// Designed to switch between main views, presenting an icon and a label.
/// Handles horizontal scrolling when space is constrained.
///
/// # Example
/// ```rust,no_run
/// # use iced_gtk_themer::GtkTheme;
/// # use iced_gtk_themer::widgets::view_switcher::view_switcher;
/// # let theme = GtkTheme::load("adwaita").unwrap();
/// let views = vec![
///     ("Home".to_string(), "🏠".to_string(), Some(1)),
/// ];
/// let switcher = view_switcher(&theme, views, 0);
/// ```
pub fn view_switcher<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    views: Vec<(String, String, Option<Message>)>,
    selected_idx: usize,
) -> Element<'a, Message> {
    let mut switcher_row = row![].spacing(8).align_y(Alignment::Center);

    for (i, (label, icon_str, msg)) in views.into_iter().enumerate() {
        let is_selected = i == selected_idx;
        
        let content = column![
            text(icon_str).size(20),
            text(label).size(12)
        ]
        .spacing(4)
        .align_x(Alignment::Center);

        let mut b = button(content).padding([8, 16]);

        if let Some(m) = msg {
            b = b.on_press(m);
        }

        if is_selected {
            b = b.style(|_: &iced::Theme, s| gtk.button_primary(s));
        } else {
            b = b.style(|_: &iced::Theme, s| gtk.button_secondary(s));
        }

        switcher_row = switcher_row.push(b);
    }

    let scroll = scrollable(switcher_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(4)
                .margin(2)
        ))
        .width(Length::Fill);

    container(scroll)
        .width(Length::Fill)
        .padding([4, 8])
        .center_x()
        .into()
}
