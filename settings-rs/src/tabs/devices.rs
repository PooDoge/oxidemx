//! "Devices" tab — paired Logitech devices + status. Real device
//! probing (HID++ via the daemon's GetDeviceName / GetBatteryStatus
//! D-Bus methods) is a follow-up; for now the tab shows a static
//! MX Master 4 card sourced from the same daemon-D-Bus path the
//! legacy UI used.
//!
//! The card is purely informational right now: connected state,
//! transport type, battery (placeholder until daemon exposes it
//! to the new shared schema). Adding edit knobs (rename, unpair)
//! lands once the daemon's device-list method is wired into
//! juhradial-shared.

use crate::widgets::section_header;
use crate::{style, Message, State};
use iced::widget::{column, container, row, rule, text, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let name = state
        .daemon
        .device_name
        .clone()
        .unwrap_or_else(|| "MX Master 4".to_string());
    let (battery_pct, status) = match state.daemon.battery {
        Some((p, _)) => (Some(p as u32), DeviceStatus::Connected),
        None => match state.battery.map(|b| b.percent) {
            Some(p) => (Some(p as u32), DeviceStatus::Connected),
            None => (None, DeviceStatus::Disconnected),
        },
    };

    column![
        section_header("Devices"),
        text(
            "Paired Logitech devices + battery + transport status. Live \
             values come from the daemon over D-Bus; UPower is used as a \
             fallback when the daemon battery isn't reported."
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        device_card(state, &name, "Logi Bolt USB", "Logitech", status, battery_pct),
    ]
    .spacing(10)
    .into()
}

#[derive(Debug, Clone, Copy)]
enum DeviceStatus {
    Connected,
    Disconnected,
}

fn device_card<'a>(
    state: &'a State,
    name: &str,
    transport: &str,
    vendor: &str,
    status: DeviceStatus,
    battery_percent: Option<u32>,
) -> Element<'a, Message> {
    let pal = &state.palette;

    let dot_color = match status {
        DeviceStatus::Connected => pal.green,
        DeviceStatus::Disconnected => pal.red,
    };
    let status_label = match status {
        DeviceStatus::Connected => "Connected",
        DeviceStatus::Disconnected => "Not connected",
    };

    let battery_text = match battery_percent {
        Some(p) => format!("{p}%"),
        None => "—".into(),
    };

    container(
        column![
            row![
                container(text("MX").size(11))
                    .padding([4, 8])
                    .style(style::chip(pal)),
                column![
                    text(name.to_string()).size(15),
                    text(vendor.to_string())
                        .size(11)
                        .style(style::text_dim(pal)),
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                row![
                    container(
                        Space::new()
                            .width(Length::Fixed(8.0))
                            .height(Length::Fixed(8.0))
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
                    text(status_label.to_string())
                        .size(11)
                        .style(style::text_dim(pal)),
                ]
                .align_y(Alignment::Center)
                .spacing(6),
            ]
            .align_y(Alignment::Center)
            .spacing(12),
            rule::horizontal(1).style(style::rule_style(pal)),
            stat_row(state, "Transport", transport),
            stat_row(state, "Battery", &battery_text),
            stat_row(state, "Firmware", "—"),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

fn stat_row<'a>(state: &'a State, label: &str, value: &str) -> Element<'a, Message> {
    let pal = &state.palette;
    row![
        text(label.to_string())
            .size(12)
            .style(style::text_dim(pal))
            .width(Length::Fixed(140.0)),
        text(value.to_string()).size(12),
    ]
    .align_y(Alignment::Center)
    .into()
}
