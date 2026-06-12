//! Header furniture: title + model/tools status line on the left
//! (offset past the canvas-drawn page puck), action buttons on the
//! right (Memories / Scheduled tasks / New chat / Close). The puck
//! and the drag pill are painted by `chat_shell::CapsPainter`;
//! empty header surface drags the window via the canvas hit test.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::app::Message;
use crate::chat_shell::{EDGE_PAD, HEADER_H};
use crate::radial::RadialState;

pub fn view<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    // "claude-haiku · 3 tools armed"-style status line: live model
    // short-name + armed tool count for the active mode.
    let model = state.chat().model.clone();
    let model_short = model
        .strip_prefix("gemini-")
        .map(|m| format!("gemini {m}"))
        .unwrap_or(model);
    let tool_count = state.chat().mode.tool_count();

    let status = row![
        container(Space::new())
            .width(Length::Fixed(6.0))
            .height(Length::Fixed(6.0))
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(kit.fade(kit.green, 1.0))),
                border: iced::border::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        text(format!("{model_short} · {tool_count} tools armed"))
            .size(10.5)
            .color(kit.fade(kit.subtext0, 1.0)),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    let title_block = column![
        text("AI Assistant")
            .size(14.5)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..Default::default()
            })
            .color(kit.fade(kit.text, 1.0)),
        status,
    ]
    .spacing(1);

    let icon_btn = move |glyph: &'static str, tip_active: bool, msg: Message| {
        button(
            text(glyph)
                .size(19)
                .color(kit.fade(if tip_active { kit.accent } else { kit.subtext0 }, 1.0))
                .align_x(iced::alignment::Horizontal::Center),
        )
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .padding(0)
        .style(move |_, _status| button::Style {
            background: if tip_active {
                Some(iced::Background::Color(kit.fade(kit.accent, 0.13)))
            } else {
                None
            },
            border: iced::border::Border {
                color: if tip_active {
                    kit.fade(kit.accent, 0.33)
                } else {
                    iced::Color::TRANSPARENT
                },
                width: 1.0,
                radius: 9.0.into(),
            },
            ..Default::default()
        })
        .on_press(msg)
    };

    // Close: light filled circle with a dark ×, per the design.
    let close_btn = button(
        text("✕")
            .size(16)
            .color(kit.fade(kit.crust, 1.0))
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fixed(32.0))
    .height(Length::Fixed(32.0))
    .padding(0)
    .style(move |_, _status| button::Style {
        background: Some(iced::Background::Color(kit.fade(kit.text, 1.0))),
        border: iced::border::Border {
            radius: 16.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .on_press(Message::ToggleDismiss);

    let bar = row![
        // Left slot for the canvas-drawn 32 px puck: 14 px header
        // padding + 32 px puck + 10 px gap.
        Space::new().width(Length::Fixed(14.0 + 32.0 + 10.0)),
        title_block,
        Space::new().width(Length::Fill),
        icon_btn("✱", state.ai_show_memories, Message::AiToggleMemories),
        icon_btn("◔", state.ai_show_tasks, Message::AiToggleTasks),
        icon_btn("＋", false, Message::AiNewChat),
        close_btn,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(EDGE_PAD + HEADER_H))
        .padding(iced::Padding {
            top: EDGE_PAD,
            right: EDGE_PAD + 8.0,
            bottom: 0.0,
            left: EDGE_PAD,
        })
        .into()
}
