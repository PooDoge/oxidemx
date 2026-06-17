//! Thread chip strip: recent conversation chips + "+ New" on the
//! left, agent-mode pill + Flash/Pro indicator right-aligned.

use iced::widget::{button, row, text, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::app::Message;
use crate::radial::RadialState;

/// Compact token count: `1.2k` past a thousand, else the raw number.
fn fmt_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// Rough USD cost estimate from token counts. Rates are approximate
/// Gemini per-1M-token prices (Flash vs Pro); shown with a `~` since the
/// active provider/model may differ. Good enough for a budget feel.
fn est_cost(prompt: u64, completion: u64, pro: bool) -> f64 {
    let (in_rate, out_rate) = if pro { (1.25, 10.0) } else { (0.075, 0.30) };
    (prompt as f64 / 1_000_000.0) * in_rate + (completion as f64 / 1_000_000.0) * out_rate
}

/// How many recent threads render as chips before the "…" chip
/// hands off to the full list view. Two keeps the strip inside the
/// default 484 px window next to the mode pill + flash indicator.
const MAX_CHIPS: usize = 2;

pub fn strip<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    let chip = move |label: String, active: bool, dim: bool, msg: Message| {
        button(
            text(label)
                .size(11.5)
                // A chip must never wrap into a second line — the
                // strip is one fixed-height row; long titles are
                // pre-truncated with an ellipsis.
                .wrapping(iced::widget::text::Wrapping::None)
                .color(kit.fade(
                    if active {
                        kit.accent
                    } else if dim {
                        kit.subtext0
                    } else {
                        kit.subtext1
                    },
                    1.0,
                )),
        )
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
        if label.chars().count() > 14 {
            label = label.chars().take(13).collect::<String>() + "…";
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

    // Action icons (the agent is one unified "Agentic" mode now, so the
    // old General/Menu-Setup toggle is gone): open the Command Center
    // (Mission Control), the Agents & skills config, and MCP servers.
    bar = bar.push(chip("▦".into(), false, true, Message::AiOpenCommandCenter));
    bar = bar.push(chip("⚙".into(), false, true, Message::AiOpenAgentsConfig));
    bar = bar.push(chip("🔌".into(), false, true, Message::AiOpenMcpConfig));

    // Token/cost readout for the active thread (when any usage recorded).
    let tp = state.chat().tokens_prompt;
    let tc = state.chat().tokens_completion;
    if tp + tc > 0 {
        let cost = est_cost(tp, tc, state.chat().model == crate::ai_client::PRO_MODEL);
        bar = bar.push(
            text(format!(
                "↑{} ↓{} · ~${:.4}",
                fmt_tokens(tp),
                fmt_tokens(tc),
                cost
            ))
            .size(10.0)
            .wrapping(iced::widget::text::Wrapping::None)
            .color(kit.fade(kit.subtext0, 1.0)),
        );
        bar = bar.push(Space::new().width(Length::Fixed(6.0)));
    }

    // Flash/Pro indicator, right-aligned with the sparkle.
    let is_pro = state.chat().model == crate::ai_client::PRO_MODEL;
    bar = bar.push(
        button(
            text(if is_pro { "✦ Pro" } else { "✦ Flash" })
                .size(10.5)
                .wrapping(iced::widget::text::Wrapping::None)
                .color(kit.fade(if is_pro { kit.accent } else { kit.subtext0 }, 1.0)),
        )
        .padding([4, 6])
        .style(|_, _| button::Style::default())
        .on_press(Message::AiModelToggled),
    );

    iced::widget::container(bar)
        .width(Length::Fill)
        // Anything that still doesn't fit clips at the window edge
        // instead of painting outside the body.
        .clip(true)
        .padding(iced::Padding {
            top: 10.0,
            right: 16.0,
            bottom: 4.0,
            left: 16.0,
        })
        .into()
}
