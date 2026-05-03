//! "Visuals" tab — static visual knobs (background opacity,
//! highlight intensity).

use crate::widgets::labeled_slider;
use crate::{style, Message, State, VisualField};
use iced::widget::{column, container, text};
use iced::Element;

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let v = &state.config.radial_menu.visuals;

    let bg = labeled_slider(
        "Menu background opacity",
        v.menu_background_opacity,
        0.0..=1.0,
        0.01,
        |x| format!("{:.0} %", x * 100.0),
        |x| Message::SetVisual(VisualField::MenuBackgroundOpacity, x),
    );
    let hl = labeled_slider(
        "Slice highlight intensity",
        v.slice_highlight_opacity,
        0.0..=1.0,
        0.01,
        |x| format!("{:.0} %", x * 100.0),
        |x| Message::SetVisual(VisualField::SliceHighlightOpacity, x),
    );

    container(
        column![
            text(
                "Tweak the static look of the radial menu. Animations are configured \
                 separately under the Animation tab.",
            )
            .size(12)
            .style(style::text_dim(pal)),
            container(column![bg, hl].spacing(20)).padding(12),
        ]
        .spacing(12),
    )
    .into()
}
