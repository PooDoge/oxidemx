use iced::{Element, Alignment, Padding, Theme};
use iced::widget::{row, button, text};
use crate::GtkTheme;

/// A horizontally grouped set of toggle buttons.
/// Designed to mimic AdwViewSwitcher or GTK toggle button groups.
/// Takes advantage of per-corner border radii to seamlessly join buttons.
pub fn toggle_group<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    options: Vec<(String, Message)>,
    selected_idx: usize,
) -> Element<'a, Message> {
    let mut btn_row = row![].spacing(0).align_y(Alignment::Center);

    let count = options.len();

    for (i, (label, msg)) in options.into_iter().enumerate() {
        let is_selected = i == selected_idx;

        let mut b = button(text(label))
            .on_press(msg)
            .padding(Padding::from([8, 16]));

        let radius: [f32; 4] = if count == 1 {
            [6.0, 6.0, 6.0, 6.0] // all corners
        } else if i == 0 {
            [6.0, 0.0, 0.0, 6.0] // top_left, top_right, bottom_right, bottom_left
        } else if i == count - 1 {
            [0.0, 6.0, 6.0, 0.0]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        };

        // We clone GTK to move it into the style closure
        // Since GtkTheme doesn't clone easily if it holds assets, we take what we need
        // Wait, style closures typically only borrow, but returning them from a function
        // requires `'a` or `move`. Since `gtk` is `&'a GtkTheme`, `move` inside closure
        // captures the reference `gtk` which has lifetime `'a`.
        b = b.style(move |theme: &Theme, s| {
            let mut style = if is_selected {
                gtk.button_primary(s)
            } else {
                gtk.button_secondary(s)
            };
            
            style.border.radius = iced::border::Radius::from(radius);
            
            // Adjust border widths slightly to prevent double-thick borders between segments
            // This depends on how the theme's borders are set up.
            if i != 0 {
                // If it has a left border, we might not want it overlapping
                // We'll leave it as is for default compatibility with Pop!_OS iced.
            }

            style
        });

        btn_row = btn_row.push(b);
    }

    btn_row.into()
}
