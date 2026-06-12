//! Thread chip strip: recent conversation chips + "+ New" on the
//! left, agent-mode pill + Flash/Pro indicator right-aligned.

use iced::widget::{button, row, text, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::app::Message;
use crate::radial::RadialState;

/// How many recent threads render as chips before the "…" chip
/// hands off to the full list view.
const MAX_CHIPS: usize = 3;

pub fn strip<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    let chip = move |label: String, active: bool, dim: bool, msg: Message| {
        button(text(label).size(11.5).color(kit.fade(
            if active {
                kit.accent
            } else if dim {
                kit.subtext0
            } else {
                kit.subtext1
            },
            1.0,
        )))
        .padding([5, 12])
        .style(move |_, _status| button::Style {
            background: Some(iced::Background::Color(if active {
                kit.fade(kit.accent, 0.12)
            } else if dim {
                iced::Color::TRANSPARENT
            } else {
                kit.fade(kit.surface0, 1.0)
            })),
            border: iced::border::Border {
                color: if active {
                    kit.fade(kit.accent, 0.4)
                } else if dim {
                    kit.fade(kit.surface1, 1.0)
                } else {
                    iced::Color::TRANSPARENT
                },
                width: 1.0,
                radius: 999.0.into(),
            },
            ..Default::default()
        })
        .on_press(msg)
    };

    let mut bar = row![].spacing(6).align_y(Alignment::Center);

    // Newest non-empty threads first.
    let recent: Vec<usize> = state
        .ai_threads
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, t)| !t.history.is_empty())
        .map(|(i, _)| i)
        .collect();
    for &idx in recent.iter().take(MAX_CHIPS) {
        let t = &state.ai_threads[idx];
        let mut label = if t.title.is_empty() {
            "Untitled".to_string()
        } else {
            t.title.clone()
        };
        if label.chars().count() > 18 {
            label = label.chars().take(17).collect::<String>() + "…";
        }
        bar = bar.push(chip(
            label,
            idx == state.ai_active && !state.ai_show_threads,
            false,
            Message::AiSelectThread(idx),
        ));
    }
    bar = bar.push(chip("+ New".to_string(), false, true, Message::AiNewChat));
    if recent.len() > MAX_CHIPS {
        bar = bar.push(chip(
            "…".to_string(),
            state.ai_show_threads,
            true,
            Message::AiToggleThreads,
        ));
    }

    bar = bar.push(Space::new().width(Length::Fill));

    // Agent-mode pill (tap cycles Menu Setup ↔ General). The
    // redesign's strip only shows the flash indicator, but the two
    // tool configurations are a real functional switch — keep it
    // reachable as a compact pill.
    let mode = state.chat().mode;
    let next_mode = match mode {
        crate::ai_client::AgentMode::SettingsCustomizer => crate::ai_client::AgentMode::GeneralChat,
        crate::ai_client::AgentMode::GeneralChat => crate::ai_client::AgentMode::SettingsCustomizer,
    };
    bar = bar.push(chip(
        mode.label().to_string(),
        false,
        true,
        Message::AiModeSelected(next_mode),
    ));

    // Flash/Pro indicator, right-aligned with the sparkle.
    let is_pro = state.chat().model == crate::ai_client::PRO_MODEL;
    bar = bar.push(
        button(
            text(if is_pro {
                "✦ Pro mode"
            } else {
                "✦ Flash mode"
            })
            .size(10.5)
            .color(kit.fade(if is_pro { kit.accent } else { kit.subtext0 }, 1.0)),
        )
        .padding([4, 6])
        .style(|_, _| button::Style::default())
        .on_press(Message::AiModelToggled),
    );

    iced::widget::container(bar)
        .width(Length::Fill)
        .padding(iced::Padding {
            top: 10.0,
            right: 16.0,
            bottom: 4.0,
            left: 16.0,
        })
        .into()
}
