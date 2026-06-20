//! Header furniture: title + model/tools status line on the left
//! (offset past the canvas-drawn page puck), action buttons on the
//! right (Memories / Scheduled tasks / New chat / Close). The puck
//! and the drag pill are painted by `chat_shell::CapsPainter`;
//! empty header surface drags the window via the canvas hit test.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Element, Length};

use super::{tokens, Kit};
use crate::app::{ChatView, Message};
use crate::chat_shell::{EDGE_PAD, HEADER_H};
use crate::radial::RadialState;

/// App logo for the standalone chat window's header (left slot). Decoded once —
/// `Handle` is Arc-backed so `.clone()` per render is cheap (vs re-reading +
/// re-hashing the 670 KB PNG every frame).
static LOGO_HANDLE: std::sync::LazyLock<iced::widget::image::Handle> =
    std::sync::LazyLock::new(|| {
        iced::widget::image::Handle::from_bytes(
            include_bytes!("../../../assets/oxidemx.png").to_vec(),
        )
    });

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
        super::widgets::status_dot(kit, kit.green, true),
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

    // View switcher — one shared `segment` builder per view.
    let seg = |view: ChatView, name: &'static str, label: &'static str| {
        super::widgets::segment(kit, name, label, active == view, Message::AiShowView(view))
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
    let close_btn = super::widgets::round_icon_button(
        "close",
        11.0,
        32.0,
        16.0,
        kit.fade(kit.text, 1.0),
        kit.fade(kit.crust, 1.0),
        Message::ToggleDismiss,
    );

    // In chat_window_mode: show the app logo in the left slot (the puck
    // is not drawn); otherwise reserve space for the canvas-drawn puck.
    let left_slot: iced::Element<'a, Message> = if state.chat_window_mode {
        container(
            iced::widget::image(LOGO_HANDLE.clone())
                .width(Length::Fixed(26.0))
                .height(Length::Fixed(26.0)),
        )
        .padding(iced::Padding { left: 14.0, ..Default::default() })
        .into()
    } else {
        Space::new().width(Length::Fixed(14.0 + 32.0 + 10.0)).into()
    };

    // In chat_window_mode the WM frame closes the window; omit close_btn.
    let mut bar_children: Vec<iced::Element<'a, Message>> = vec![
        left_slot,
        title_block.into(),
        Space::new().width(Length::Fill).into(),
        switcher.into(),
    ];
    if !state.chat_window_mode {
        bar_children.push(close_btn.into());
    }
    // Fill so the spacer right-aligns the tabs in the chat window; keep the
    // overlay header's original Shrink so its layout is byte-for-byte unchanged.
    let bar = iced::widget::Row::from_vec(bar_children)
        .width(if state.chat_window_mode {
            Length::Fill
        } else {
            Length::Shrink
        })
        .spacing(6)
        .align_y(Alignment::Center);

    let bar_c = if state.chat_window_mode {
        let a = kit.accent;
        let s = kit.surface0;
        Some(iced::Color {
            r: a.r * 0.45 + s.r * 0.55,
            g: a.g * 0.45 + s.g * 0.55,
            b: a.b * 0.45 + s.b * 0.55,
            a: 1.0,
        })
    } else {
        None
    };

    let outer = container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(EDGE_PAD + HEADER_H))
        .padding(iced::Padding {
            top: EDGE_PAD,
            right: EDGE_PAD + 8.0,
            bottom: 0.0,
            left: EDGE_PAD,
        });

    if let Some(bg) = bar_c {
        outer
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(bg)),
                ..Default::default()
            })
            .into()
    } else {
        outer.into()
    }
}
