use iced::{Element, Length, Padding};
use iced::widget::{column, text, container};
use crate::GtkTheme;

/// A grouping container for ActionRows, mimicking AdwPreferencesGroup.
pub fn preferences_group<'a, Message: Clone + 'a>(
    _gtk: &GtkTheme,
    title: Option<&'a str>,
    rows: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = column![].spacing(12).width(Length::Fill);

    if let Some(t) = title {
        content = content.push(
            container(text(t).size(13).style(move |theme: &iced::Theme| {
                let mut c = theme.palette().text;
                c.a = 0.7;
                iced::widget::text::Style { color: Some(c),
                    ..Default::default() }
            }))
            .padding(Padding { top: 12.0, bottom: 4.0, left: 16.0, right: 16.0 })
        );
    }

    let mut group = column![].width(Length::Fill);
    
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            // Divider
            group = group.push(
                container(iced::widget::space().width(Length::Fill).height(Length::Fixed(1.0)))
                    .style(|theme: &iced::Theme| {
                        let mut c = theme.palette().text;
                        c.a = 0.1;
                        container::background(c)
                    })
            );
        }
        group = group.push(row);
    }

    let card = container(group)
        .style(|theme: &iced::Theme| {
            let mut style = container::background(theme.extended_palette().background.weak.color);
            style.border = iced::Border {
                color: iced::Color::TRANSPARENT,
                width: 0.0,
                radius: 12.0.into(),
            };
            style
        });

    content = content.push(card);

    container(content).into()
}
