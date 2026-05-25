//! "Easy-Switch" tab — paired host slots + live host-switching.
//!
//! Reads `GetEasySwitchInfo()` (slot count + current host) and
//! `GetHostNames()` (one label per slot) from the daemon over
//! D-Bus, both refreshed every 5 s by the snapshot tick. Each slot
//! becomes a clickable card; pressing one calls `SetHost(idx)` on
//! the daemon, which fires the HID++ command to bond the device
//! to that host.

use juhradial_widgets::widgets::section_header;
use crate::{Message, State};
use juhradial_widgets::style;
use iced::widget::{button, column, container, row, rule, text, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let body: Element<Message> = match state.daemon.easy_switch.as_ref() {
        Some(es) if es.slot_count > 0 => slot_grid(state, es),
        _ => container(
            text(
                "Easy-Switch info isn't available — either the daemon \
                 isn't running, the mouse hasn't connected yet, or it \
                 doesn't support multi-host switching.",
            )
            .size(12)
            .style(style::text_dim(pal)),
        )
        .padding(20)
        .style(style::card_quiet(pal))
        .into(),
    };

    column![
        section_header("Easy-Switch"),
        text(
            "Logitech multi-host pairing. The MX Master 4 keeps up to \
             three host bondings at once — click a slot to switch the \
             active host, or use the dedicated button on the underside \
             of the mouse.",
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        body,
    ]
    .spacing(10)
    .into()
}

fn slot_grid<'a>(state: &'a State, es: &'a crate::daemon::EasySwitch) -> Element<'a, Message> {
    let _pal = &state.palette;
    let mut grid = column![].spacing(8);
    for idx in 0..es.slot_count {
        let name = es
            .host_names
            .get(idx as usize)
            .cloned()
            .unwrap_or_else(|| format!("Host {}", idx + 1));
        let is_current = idx == es.current_host;
        grid = grid.push(slot_card(state, idx, &name, is_current));
    }
    grid.into()
}

fn slot_card<'a>(
    state: &'a State,
    idx: u8,
    name: &str,
    is_current: bool,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let label = if name.is_empty() {
        format!("Host {} (unbonded)", idx + 1)
    } else {
        name.to_string()
    };

    let action_label = if is_current { "Active" } else { "Switch" };
    let mut action_btn = button(text(action_label.to_string()).size(11));
    action_btn = if is_current {
        action_btn.style(style::btn_secondary(pal))
    } else {
        action_btn.style(style::btn_secondary(pal)).on_press(Message::SwitchHost(idx))
    };

    let dot_color = if is_current { pal.green } else { pal.subtext0 };

    container(
        row![
            container(
                Space::new()
                    .width(Length::Fixed(8.0))
                    .height(Length::Fixed(8.0)),
            )
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(dot_color)),
                border: iced::Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
            container(text(format!("{}", idx + 1)).size(11))
                .padding([3, 8])
                .style(style::chip(pal)),
            column![
                text(label).size(13),
                text(if is_current {
                    "Currently bonded".to_string()
                } else {
                    "Switch the device to this host".to_string()
                })
                .size(11)
                .style(style::text_dim(pal)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            action_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(12),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}
