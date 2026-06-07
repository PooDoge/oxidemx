use iced::{Element, Length, Alignment};
use iced::widget::{column, container, text};
use crate::GtkTheme;
use crate::widgets::clamp::clamp;
use crate::widgets::segmented_button::segmented_button;

/// A preferences dialog layout with tabs, mimicking AdwPreferencesDialog.
/// Displays a view switcher (segmented button) at the top, and the active page content below.
pub fn preferences_dialog<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    title: &'a str,
    tabs: Vec<(String, Element<'a, Message>, Message)>,
    active_tab: usize,
) -> Element<'a, Message> {
    let mut header = column![].spacing(16).align_x(Alignment::Center);

    header = header.push(
        text(title).size(24).width(Length::Shrink)
    );

    let mut options = Vec::new();
    let mut pages = Vec::new();

    for (tab_title, page_content, msg) in tabs.into_iter() {
        options.push((tab_title, msg));
        pages.push(page_content);
    }

    if !options.is_empty() {
        header = header.push(segmented_button(gtk, options, active_tab));
    }

    let active_content = if active_tab < pages.len() {
        pages.into_iter().nth(active_tab).unwrap()
    } else {
        iced::widget::space().width(Length::Fill).height(Length::Fill).into()
    };

    let content = column![
        header,
        active_content
    ]
    .spacing(24);

    container(clamp(content, 600.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(24)
        .into()
}
