use iced::{Element, Length, Alignment, Padding};
use iced::widget::{row, button};
use crate::GtkTheme;

/// A styled segmented button control. Mimics Libcosmic and Libadwaita segmented buttons.
/// Note: Since Iced does not support per-corner border radii currently, this acts
/// structurally like a segmented button but utilizes rounded inner borders if needed,
/// or just displays side-by-side buttons natively.
pub fn segmented_button<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    options: Vec<(String, Message)>,
    selected_idx: usize,
) -> Element<'a, Message> {
    let mut btn_row = row![].spacing(0).align_y(Alignment::Center);

    for (i, (label, msg)) in options.into_iter().enumerate() {
        let is_selected = i == selected_idx;
        
        let mut b = button(iced::widget::text(label))
            .on_press(msg)
            .padding(Padding::from([8, 16]));
            
        if is_selected {
            b = b.style(|_: &iced::Theme, s| gtk.button_primary(s));
        } else {
            b = b.style(|_: &iced::Theme, s| gtk.button_secondary(s));
        }
        
        btn_row = btn_row.push(b);
    }

    // Wrap the row in a container that has rounded borders to simulate a segmented control wrapper
    iced::widget::container(btn_row)
        .style(|theme: &iced::Theme| {
            let mut style = iced::widget::container::background(iced::Color::TRANSPARENT);
            style.border = iced::Border {
                color: theme.extended_palette().background.strong.color,
                width: 1.0,
                radius: 12.0.into(),
            };
            style
        })
        .into()
}
