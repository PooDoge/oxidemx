use iced::{Element, Length, Alignment, Padding, Theme};
use iced::widget::{row, column, text, container, button};
use crate::GtkTheme;

/// A row with a title, optional subtitle, and a spin control (numeric input).
/// Mimics AdwSpinRow from Libadwaita.
pub fn spin_row<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    title: &'a str,
    subtitle: Option<&'a str>,
    value: f64,
    on_change: impl Fn(f64) -> Message + 'a,
    step: f64,
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

    let decrease_msg = on_change(value - step);
    let increase_msg = on_change(value + step);

    let mut btn_minus = button(text("-").width(Length::Fill).align_x(iced::alignment::Horizontal::Center))
        .on_press(decrease_msg)
        .width(Length::Fixed(32.0))
        .padding(Padding::from([8, 0]));
        
    btn_minus = btn_minus.style(move |theme: &Theme, s| {
        let mut style = gtk.button_secondary(s);
        style.border.radius = iced::border::Radius::from([6.0, 0.0, 0.0, 6.0]);
        style
    });

    let val_text = container(text(format!("{}", value)).align_x(iced::alignment::Horizontal::Center))
        .width(Length::Fixed(48.0))
        .padding(Padding::from([8, 0]))
        .style(move |theme: &Theme| {
            let s = gtk.button_secondary(iced::widget::button::Status::Active);
            iced::widget::container::Style {
                background: s.background,
                text_color: Some(s.text_color),
                border: s.border,
                ..Default::default()
            }
        });

    let mut btn_plus = button(text("+").width(Length::Fill).align_x(iced::alignment::Horizontal::Center))
        .on_press(increase_msg)
        .width(Length::Fixed(32.0))
        .padding(Padding::from([8, 0]));

    btn_plus = btn_plus.style(move |theme: &Theme, s| {
        let mut style = gtk.button_secondary(s);
        style.border.radius = iced::border::Radius::from([0.0, 6.0, 6.0, 0.0]);
        style
    });

    let spin_control = row![btn_minus, val_text, btn_plus].spacing(0).align_y(Alignment::Center);

    let row_content = row![
        text_col.width(Length::Fill),
        spin_control
    ]
    .align_y(Alignment::Center)
    .spacing(12)
    .padding(Padding::from(16));

    container(row_content).into()
}
