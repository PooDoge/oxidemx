//! Header furniture: title + model/tools status line on the left
//! (offset past the canvas-drawn page puck), action buttons on the
//! right (Memories / Scheduled tasks / New chat / Close). The puck
//! and the drag pill are painted by `chat_shell::CapsPainter`;
//! empty header surface drags the window via the canvas hit test.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

use super::icons::icon;
use super::{tokens, Kit};
use crate::app::{ChatView, Message};
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

    // Which of the four primary views is active (drives the switcher).
    let active = if state.ai_show_skills {
        ChatView::Skills
    } else if state.ai_show_memories {
        ChatView::Memory
    } else if state.ai_show_tasks {
        ChatView::Tasks
    } else {
        ChatView::Conversation
    };

    // One segment of the view switcher: icon + label, accent when active.
    let seg = move |view: ChatView, name: &'static str, label: &'static str| {
        let on = active == view;
        let col = kit.fade(if on { kit.accent } else { kit.subtext0 }, 1.0);
        button(
            row![
                icon(name, 15.0, col),
                text(label).size(tokens::T_LABEL).color(col),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([4, 9])
        .style(move |_, _| button::Style {
            background: on.then(|| iced::Background::Color(kit.fade(kit.accent, 0.16))),
            border: iced::border::Border::default().rounded(7.0),
            text_color: col,
            ..Default::default()
        })
        .on_press(Message::AiShowView(view))
    };

    let switcher = container(
        row![
            seg(ChatView::Conversation, "message", "Chat"),
            seg(ChatView::Skills, "flask", "Skills"),
            seg(ChatView::Memory, "memory", "Memory"),
            seg(ChatView::Tasks, "clock", "Tasks"),
        ]
        .spacing(2),
    )
    .padding(2)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(kit.fade(kit.crust, 1.0))),
        border: iced::border::Border {
            color: kit.fade(kit.surface2, 1.0),
            width: 1.0,
            radius: tokens::R_BUTTON.into(),
        },
        ..Default::default()
    });

    // Close: light filled circle with a centred × icon.
    let close_btn = button(iced::widget::center(icon(
        "close",
        11.0,
        kit.fade(kit.crust, 1.0),
    )))
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
        switcher,
        close_btn,
    ]
    .spacing(6)
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
