//! Thread chip strip: recent conversation chips + "+ New" on the
//! left, agent-mode pill + Flash/Pro indicator right-aligned.

use iced::widget::{row, text, Space};
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

    // The strip's conversation chips use the shared 3-state Chip
    // (`controls::pill_dim`). A chip must never wrap — long titles are
    // pre-truncated with an ellipsis before they reach here.
    let chip = move |label: String, active: bool, dim: bool, msg: Message| {
        super::widgets::pill_dim(kit, None, label, active, dim, msg)
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
    // "New" — a dim (ghost) chip with a leading plus.
    bar = bar.push(super::widgets::pill_dim(
        kit,
        Some("plus"),
        "New",
        false,
        true,
        Message::AiNewChat,
    ));
    if recent.len() > MAX_CHIPS {
        bar = bar.push(chip(
            "…".to_string(),
            state.ai_show_threads,
            true,
            Message::AiToggleThreads,
        ));
    }

    bar = bar.push(Space::new().width(Length::Fill));

    // Command Center / Agents / MCP moved into the slash palette + the
    // Command Center window (IA consolidation, P0 §06), so the strip is
    // now just conversation context: threads, New, and the model pill.

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

    // Labeled model pill (the picker across providers is P2; for now it
    // toggles Flash ↔ Pro on click).
    let is_pro = state.chat().model == crate::ai_client::PRO_MODEL;
    let model_label = if is_pro {
        "Gemini · Pro"
    } else {
        "Gemini · Flash"
    };
    bar = bar.push(super::widgets::model_pill(
        kit,
        "model",
        model_label,
        false,
        Message::AiModelToggled,
    ));

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
