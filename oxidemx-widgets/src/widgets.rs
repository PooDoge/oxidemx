//! Small reusable iced widgets for the settings UI.
//!
//! Keeps the per-tab modules focused on layout instead of widget
//! plumbing. Each helper here takes the minimum it needs (label,
//! value, range, on_change closure) and returns an `Element`.
//!
//! Sliders aren't styled here — the app installs an
//! [`iced::Theme::custom`] in `main` derived from the active palette
//! so sliders, togglers, and default-styled buttons automatically
//! follow the accent + surface colours of whichever theme the user
//! has picked. Per-widget overrides still go through the helpers in
//! [`crate::style`].

use iced::widget::{column, row, slider, text, Space};
use iced::{Element, Length};

/// Labelled slider: "Label    [-----O-----]    0.42"
/// Range is fixed by the caller; step is auto-derived.
pub fn labeled_slider<'a, M>(
    label: &'a str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    fmt: impl Fn(f32) -> String + 'a,
    on_change: impl Fn(f32) -> M + 'a,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let display = fmt(value);
    let value_label = text(display).size(13).width(Length::Fixed(70.0));
    column![
        row![text(label).size(13), Space::new().width(Length::Fill)].spacing(8),
        row![
            slider(range, value, on_change)
                .step(step)
                .width(Length::FillPortion(4)),
            Space::new().width(Length::Fixed(8.0)),
            value_label,
        ]
        .align_y(iced::Alignment::Center),
    ]
    .spacing(4)
    .into()
}

/// Labelled u32 slider.
pub fn labeled_int_slider<'a, M>(
    label: &'a str,
    value: u32,
    range: std::ops::RangeInclusive<u32>,
    fmt: impl Fn(u32) -> String + 'a,
    on_change: impl Fn(u32) -> M + 'a,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let display = fmt(value);
    let value_label = text(display).size(13).width(Length::Fixed(70.0));
    column![
        row![text(label).size(13), Space::new().width(Length::Fill)].spacing(8),
        row![
            slider(range, value, on_change).width(Length::FillPortion(4)),
            Space::new().width(Length::Fixed(8.0)),
            value_label,
        ]
        .align_y(iced::Alignment::Center),
    ]
    .spacing(4)
    .into()
}

/// "Section header" — a slightly larger title above a control group.
pub fn section_header<'a, M: 'a>(title: &str) -> Element<'a, M> {
    text(title.to_string()).size(15).into()
}
