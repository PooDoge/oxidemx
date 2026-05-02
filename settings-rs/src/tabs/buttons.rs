//! "Buttons" tab — replicates the centerpiece of the legacy
//! juhradial UI: a mouse picture in the middle with button-target
//! callouts, and a right-side panel showing the radial menu's
//! current contents (Actions Ring), the Easy-Switch shortcut
//! toggle, and the per-button assignments list.
//!
//! Today the per-button mapping lives in the daemon, not in
//! `juhradial-shared`, so the assignments table is rendered as a
//! read-only stub keyed off the labels the legacy UI used. When
//! we move that mapping into the shared crate the right-hand
//! Assignments rows can become editable in place.

use crate::widgets::section_header;
use crate::{Message, State};
use iced::widget::{column, container, row, rule, text, toggler, Space};
use iced::{Alignment, Element, Length};
use juhradial_shared::{ActionKind, Slice};

pub fn view(state: &State) -> Element<'_, Message> {
    row![
        center_panel(),
        Space::new().width(Length::Fixed(16.0)),
        right_panel(state),
    ]
    .spacing(0)
    .into()
}

// ----------------------------------------------------------------------------
// Center: mouse layout placeholder + button list
// ----------------------------------------------------------------------------

fn center_panel<'a>() -> Element<'a, Message> {
    let buttons = [
        ("Middle Button", "Middle Click"),
        ("Shift Wheel Mode", "SmartShift"),
        ("Forward", "Forward"),
        ("Back", "Back"),
        ("Horizontal Scroll", "Scroll Left/Right"),
        ("Gestures", "Virtual desktops"),
        ("Show Actions Ring", "Radial Menu"),
    ];

    let mut button_col = column![text("MX Master 4 buttons").size(15)].spacing(8);
    for (label, current) in buttons {
        button_col = button_col.push(button_row(label, current));
    }

    container(
        column![
            section_header("Mouse buttons"),
            text(
                "The legacy UI shows a picture of the mouse with callouts; \
                 a high-fidelity render lands later. For now the button list \
                 below mirrors what the daemon understands."
            )
            .size(12),
            container(button_col)
                .padding(16)
                .style(container::bordered_box),
        ]
        .spacing(12),
    )
    .width(Length::FillPortion(2))
    .into()
}

fn button_row<'a>(name: &str, mapped: &str) -> Element<'a, Message> {
    row![
        container(text("·").size(13))
            .padding([2, 8])
            .style(container::bordered_box),
        column![
            text(name.to_string()).size(13),
            text(mapped.to_string()).size(11),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        text(">").size(13),
    ]
    .align_y(Alignment::Center)
    .spacing(10)
    .padding([6, 8])
    .into()
}

// ----------------------------------------------------------------------------
// Right column: Actions Ring + Easy-Switch + Button Assignments
// ----------------------------------------------------------------------------

fn right_panel(state: &State) -> Element<'_, Message> {
    container(
        column![
            actions_ring_panel(&state.config.radial_menu.slices),
            easy_switch_panel(state.config.radial_menu.easy_switch_shortcuts),
            assignments_panel(),
        ]
        .spacing(16),
    )
    .width(Length::FillPortion(1))
    .into()
}

fn actions_ring_panel(slices: &[Slice]) -> Element<'_, Message> {
    // Render a 2-column grid of the configured slices with their
    // colour key and label — matches the legacy UI's "Actions Ring"
    // card. Slices come straight from config so editing the JSON
    // reorders / renames items here live.
    let mut left = column![].spacing(6);
    let mut right = column![].spacing(6);
    for (i, slice) in slices.iter().enumerate() {
        let row = slice_row(slice);
        if i % 2 == 0 {
            left = left.push(row);
        } else {
            right = right.push(row);
        }
    }
    container(
        column![
            row![
                text("Actions Ring").size(15),
                Space::new().width(Length::Fill),
                text("Click any action to customize").size(10),
            ]
            .align_y(Alignment::Center),
            rule::horizontal(1),
            row![
                left.width(Length::FillPortion(1)),
                right.width(Length::FillPortion(1))
            ]
            .spacing(12),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(container::bordered_box)
    .into()
}

fn slice_row(slice: &Slice) -> Element<'_, Message> {
    let kind_glyph = match slice.kind {
        ActionKind::Submenu => "...",
        ActionKind::Macro => "M",
        ActionKind::EasySwitch => "ES",
        ActionKind::Settings => "*",
        _ => ">",
    };
    row![
        container(text(color_dot(&slice.color)).size(11))
            .padding([1, 6])
            .style(container::bordered_box),
        text(format!("{kind_glyph}  ")).size(11),
        text(slice.label.as_str()).size(12),
        Space::new().width(Length::Fill),
        text(">").size(11),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .padding([4, 6])
    .into()
}

fn color_dot(color_key: &str) -> String {
    if color_key.is_empty() {
        "·".into()
    } else {
        format!("●{}", &color_key[..1.min(color_key.len())])
    }
}

fn easy_switch_panel<'a>(enabled: bool) -> Element<'a, Message> {
    container(
        row![
            container(text("ES").size(11))
                .padding([3, 8])
                .style(container::bordered_box),
            column![
                text("Easy-Switch Shortcuts").size(13),
                text("Replace Emoji with Easy-Switch 1, 2, 3 submenu").size(11),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            toggler(enabled).on_toggle(Message::SetEasySwitchShortcuts),
        ]
        .align_y(Alignment::Center)
        .spacing(10)
        .padding(4),
    )
    .padding(12)
    .style(container::bordered_box)
    .into()
}

fn assignments_panel<'a>() -> Element<'a, Message> {
    let header = row![
        text("BUTTON ASSIGNMENTS").size(11),
        Space::new().width(Length::Fill),
    ];
    let entries = [
        ("Middle Button", "Middle Click"),
        ("Shift Wheel Mode", "SmartShift"),
        ("Forward", "Forward"),
        ("Horizontal Scroll", "Scroll Left/Right"),
        ("Back", "Back"),
        ("Gestures", "Virtual desktops"),
        ("Show Actions Ring", "Radial Menu"),
    ];
    let mut col = column![header, rule::horizontal(1)].spacing(8);
    for (name, mapped) in entries {
        col = col.push(assignment_row(name, mapped));
    }
    container(col).padding(14).style(container::bordered_box).into()
}

fn assignment_row<'a>(name: &str, mapped: &str) -> Element<'a, Message> {
    row![
        container(text(">").size(11))
            .padding([2, 8])
            .style(container::bordered_box),
        column![
            text(name.to_string()).size(12),
            text(mapped.to_string()).size(10),
        ]
        .spacing(1),
        Space::new().width(Length::Fill),
        text(">").size(11),
    ]
    .align_y(Alignment::Center)
    .spacing(10)
    .padding([4, 6])
    .into()
}
