use iced::{Element, Length, Alignment};
use iced::widget::{row, column, button, text, scrollable, container};
use crate::GtkTheme;

/// A GNOME-style Carousel widget.
///
/// Allows horizontal scrolling through a collection of pages/items.
/// Displays pagination dots/buttons for navigation beneath the scrollable content.
///
/// # Example
/// ```rust,no_run
/// # use iced::widget::text;
/// # use iced_gtk_themer::GtkTheme;
/// # use iced_gtk_themer::widgets::carousel::carousel;
/// # let theme = GtkTheme::load("adwaita").unwrap();
/// let pages = vec![
///     text("Page 1").into(),
///     text("Page 2").into(),
/// ];
/// let my_carousel = carousel(&theme, pages, 0, |i| i);
/// ```
pub fn carousel<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    pages: Vec<Element<'a, Message>>,
    current_page: usize,
    on_page_select: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message> {
    let num_pages = pages.len();
    let mut page_row = row![].spacing(16).align_y(Alignment::Center);

    for page in pages.into_iter() {
        // Enforce each page to take up a significant width to mimic carousel items
        page_row = page_row.push(
            container(page)
                .width(Length::Fixed(300.0)) // Approximating page width
                .center_x(Length::Fill)
                .center_y(Length::Fill)
        );
    }

    let scroll = scrollable(page_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(4).margin(2)
        ))
        .width(Length::Fill);

    // Pagination Dots
    let mut dots_row = row![].spacing(8).align_y(Alignment::Center);
    for i in 0..num_pages {
        let is_selected = i == current_page;
        let dot_label = if is_selected { "●" } else { "○" };
        
        let mut b = button(text(dot_label).size(16))
            .padding([2, 4])
            .on_press(on_page_select(i));

        if is_selected {
            b = b.style(|_: &iced::Theme, s| gtk.button_primary(s));
        } else {
            b = b.style(|_: &iced::Theme, s| gtk.button_secondary(s));
        }

        dots_row = dots_row.push(b);
    }

    column![
        scroll,
        container(dots_row).width(Length::Fill).center_x(Length::Fill)
    ]
    .spacing(12)
    .into()
}
