//! `subscription` wiring — daemon D-Bus stream, config watcher,
//! frame ticker, window events, AI question/stream channels — plus
//! the small chat helpers shared with `update`.

use iced::{Subscription, Task};

use super::Message;
use crate::radial::RadialState;

pub(super) fn subscription(state: &RadialState) -> Subscription<Message> {
    // Three streams merged into the same Message channel:
    //   * D-Bus listener — translates the daemon's three signal
    //     streams into OverlayEvent values.
    //   * Inotify config watcher — yields a fresh AppConfig each
    //     time `~/.config/oxidemx/config.json` is saved.
    //   * 60 Hz frame ticker — keeps animations smooth while a
    //     menu is visible. (Cheap when nothing animates because
    //     update() returns Task::none() immediately.)
    let mut subs = vec![
        Subscription::run(crate::dbus::stream).map(Message::Overlay),
        Subscription::run(crate::config::watch_stream)
            .map(|cfg| Message::ConfigReloaded(Box::new(cfg))),
        Subscription::run(ai_question_stream),
        Subscription::run(ai_stream_stream),
        iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick),
        iced::window::events().map(|(id, event)| match event {
            iced::window::Event::Opened { .. } => Message::WindowOpened(id),
            // Routed through its own message (not ToggleDismiss)
            // so update() can ignore focus loss while the chat
            // shell is up — see Message::WindowUnfocused.
            iced::window::Event::Unfocused => Message::WindowUnfocused,
            iced::window::Event::Focused => Message::WindowFocused,
            iced::window::Event::Resized(size) => Message::WindowResized(size),
            _ => Message::Noop,
        }),
    ];
    // Live-data sampling only while something is on screen — a
    // closed overlay spawns no sampling processes. The subscription
    // identity restarting on open is fine: the first tick re-seeds
    // the procfs delta baselines.
    if state.is_drawable() {
        subs.push(Subscription::run(crate::sampler::stream).map(Message::WidgetSample));
    }
    Subscription::batch(subs)
}

fn ai_question_stream() -> impl futures_util::stream::Stream<Item = Message> {
    let (tx, rx) = async_channel::unbounded();

    let (q_tx, mut q_rx) = tokio::sync::mpsc::channel(10);
    *crate::ai_client::QUESTION_TX.lock().unwrap() = Some(q_tx);

    tokio::task::spawn(async move {
        while let Some(pending) = q_rx.recv().await {
            let _ = tx.send(Message::AiQuestionReceived(pending)).await;
        }
    });

    rx
}

/// Registers the global stream-event channel and forwards
/// `(thread, StreamEvent)` pairs — text deltas + tool-activity
/// labels — into the iced message loop. Same pattern as
/// `ai_question_stream`.
fn ai_stream_stream() -> impl futures_util::stream::Stream<Item = Message> {
    let (tx, rx) = async_channel::unbounded();

    let (s_tx, mut s_rx) = tokio::sync::mpsc::channel(64);
    *crate::ai_client::STREAM_TX.lock().unwrap() = Some(s_tx);

    tokio::task::spawn(async move {
        while let Some(event) = s_rx.recv().await {
            let _ = tx.send(Message::AiStream(event)).await;
        }
    });

    rx
}

/// Snap the conversation scrollable to the newest message.
pub(super) fn scroll_chat_to_end() -> Task<Message> {
    iced::widget::operation::snap_to_end(crate::chat_ui::body::CHAT_SCROLL_ID)
}

/// "2h ago"-style label for the thread list + memories view.
pub(crate) fn rel_time(ts: u64) -> String {
    if ts == 0 {
        return "earlier".to_string();
    }
    let now = crate::radial::now_secs();
    let delta = now.saturating_sub(ts);
    match delta {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", delta / 60),
        3600..=86_399 => format!("{}h ago", delta / 3600),
        _ => format!("{}d ago", delta / 86_400),
    }
}
