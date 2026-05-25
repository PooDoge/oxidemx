//! "Visuals" tab — static visual knobs (background opacity,
//! highlight intensity).

use juhradial_widgets::widgets::labeled_slider;
use crate::{Message, State, VisualField};
use juhradial_widgets::style;
use iced::widget::{column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Length};

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
    let center_size = labeled_slider(
        "Centre label size",
        v.center_label_size,
        0.0..=24.0,
        0.5,
        |x| {
            if x <= 0.5 {
                "off".to_string()
            } else {
                format!("{x:.0} px")
            }
        },
        |x| Message::SetVisual(VisualField::CenterLabelSize, x),
    );

    let font_input = text_input(
        "System default (e.g. \"Inter\", \"Geist Mono\")",
        v.font_family.as_str(),
    )
    .on_input(Message::SetFontFamily)
    .padding(8)
    .size(12)
    .width(Length::Fill);

    let font_row = column![
        text("Font family").size(13),
        text(
            "Override the font used for the centre label and other rendered \
             text. Leave blank to use the system default. Family must be \
             installed on the system."
        )
        .size(11)
        .style(style::text_dim(pal)),
        font_input,
    ]
    .spacing(6);

    container(
        column![
            text(
                "Tweak the static look of the radial menu. Animations are configured \
                 separately under the Animation tab.",
            )
            .size(12)
            .style(style::text_dim(pal)),
            container(column![bg, hl, center_size, font_row].spacing(20)).padding(12),
        ]
        .spacing(12),
    )
    .into()
}

// `row`/`Alignment`/`Space` are imported for callers that grow this
// view later; suppress unused warning so a fresh build stays clean.
#[allow(dead_code)]
fn _imports_link() -> (
    iced::widget::Space,
    iced::Alignment,
    iced::widget::Row<'static, Message>,
) {
    (Space::new(), Alignment::Start, row![])
}
