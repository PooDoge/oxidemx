use iced::{Element, Length, Alignment, Padding, Theme};
use iced::widget::{row, column, text, button};
use crate::GtkTheme;

/// A clickable row with a title, optional subtitle, and an optional trailing widget.
/// Designed to mimic AdwActionRow or flat list rows when used interactively.
pub fn button_row<'a, Message: Clone + 'a>(
    _gtk: &'a GtkTheme,
    title: &'a str,
    subtitle: Option<&'a str>,
    on_press: Message,
    trailing: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut text_col = column![text(title).size(16)].spacing(4);

    if let Some(sub) = subtitle {
        text_col = text_col.push(
            text(sub).size(13).style(move |theme: &Theme| {
                let mut c = theme.palette().text;
                c.a = 0.7; // subtle subtitle
                iced::widget::text::Style { color: Some(c), ..Default::default() }
            })
        );
    }

    let mut row_content = row![
        text_col.width(Length::Fill)
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    if let Some(t) = trailing {
        row_content = row_content.push(t);
    }

    let b = button(row_content)
        .on_press(on_press)
        .padding(Padding::from(16))
        .width(Length::Fill)
        .style(move |theme: &Theme, status| {
            // A flat list row style: transparent background, slight text tint on hover
            let mut style = iced::widget::button::Style::default();
            style.text_color = theme.palette().text;
            
            match status {
                iced::widget::button::Status::Hovered => {
                    let mut bg = theme.palette().text;
                    bg.a = 0.05; // faint hover tint
                    style.background = Some(bg.into());
                }
                iced::widget::button::Status::Pressed => {
                    let mut bg = theme.palette().text;
                    bg.a = 0.1; // stronger press tint
                    style.background = Some(bg.into());
                }
                _ => {
                    style.background = None;
                }
            }
            
            style
        });

    b.into()
}
