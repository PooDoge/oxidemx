use iced::{Element, Length, Alignment, Padding};
use iced::widget::{row, column, text, container, Space};

/// A row with a title, optional subtitle, and an optional trailing widget.
/// Designed to mimic AdwActionRow from Libadwaita.
pub fn action_row<'a, Message: Clone + 'a>(
    title: &'a str,
    subtitle: Option<&'a str>,
    trailing: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut text_col = column![text(title).size(16)].spacing(4);
    
    if let Some(sub) = subtitle {
        text_col = text_col.push(
            text(sub).size(13).style(move |theme: &iced::Theme| {
                let mut c = theme.palette().text;
                c.a = 0.7; // subtle subtitle
                iced::widget::text::Style { color: Some(c),
                    ..Default::default() }
            })
        );
    }
    
    let mut row_content = row![
        text_col.width(Length::Fill)
    ]
    .align_y(Alignment::Center)
    .spacing(12)
    .padding(Padding::from(16));

    if let Some(t) = trailing {
        row_content = row_content.push(t);
    }

    container(row_content).into()
}
