//! "Gaming" tab — gaming-mode toggle + DPI cycle.
//!
//! The daemon's `gaming::GamingMode` raises pointer DPI to a
//! preset, suppresses the radial overlay, and (optionally)
//! mutes haptics so the wheel ratchet doesn't fire mid-game. We
//! expose two controls:
//!
//!   * Master toggle — calls `SetGamingMode(bool)` over D-Bus,
//!     daemon enables / disables.
//!   * Cycle DPI — calls `CycleGamingDpi()`. Daemon walks its
//!     preset list and reports back the new label.

use crate::widgets::section_header;
use crate::{style, Message, State};
use iced::widget::{button, column, container, row, rule, text, toggler, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let on = state.daemon.gaming_mode;

    let toggle_card = container(
        row![
            container(text("G").size(11).style(style::text_dim(pal)))
                .padding([3, 8])
                .style(style::chip(pal)),
            column![
                text("Gaming Mode").size(14),
                text(if on {
                    "On — DPI raised, overlay suppressed."
                } else {
                    "Off — normal pointer behaviour."
                })
                .size(11)
                .style(style::text_dim(pal)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            toggler(on)
                .on_toggle(Message::SetGamingMode)
                .style(style::toggler_style(pal)),
        ]
        .align_y(Alignment::Center)
        .spacing(12),
    )
    .padding(14)
    .style(style::card(pal));

    let dpi_card = container(
        column![
            row![
                text("Gaming DPI").size(14),
                Space::new().width(Length::Fill),
                button(text("Cycle preset").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::CycleGamingDpi),
            ]
            .align_y(Alignment::Center),
            text(
                "Walks through the preset DPI list (e.g. 1600 → 3200 → \
                 4800) on each click. Useful for hot-binding to a side \
                 button via the macro recorder.",
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(8),
    )
    .padding(14)
    .style(style::card(pal));

    column![
        section_header("Gaming"),
        text(
            "Per-game pointer + haptics overrides. Toggle gaming mode \
             when starting a session — the daemon raises DPI, suppresses \
             the radial overlay, and (optionally) mutes haptics.",
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        toggle_card,
        Space::new().height(Length::Fixed(12.0)),
        dpi_card,
    ]
    .spacing(10)
    .into()
}
