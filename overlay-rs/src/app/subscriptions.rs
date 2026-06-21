//! `subscription` wiring — daemon D-Bus stream, config watcher,
//! frame ticker, window events, AI question/stream channels — plus
//! the small chat helpers shared with `update`.

use iced::{Subscription, Task};

use super::Message;
use crate::radial::RadialState;

pub(super) fn subscription(state: &RadialState) -> Subscription<Message> {
    // Streams merged into the same Message channel:
    //   * D-Bus listener — translates the daemon's three signal
    //     streams into OverlayEvent values.
    //   * Inotify config watcher — yields a fresh AppConfig each
    //     time `~/.config/oxidemx/config.json` is saved.
    //   * 60 Hz frame ticker — keeps animations smooth while a
    //     menu is visible. (Cheap when nothing animates because
    //     update() returns Task::none() immediately.)
    let mut subs = vec![
        Subscription::run(crate::config::watch_stream)
            .map(|cfg| Message::ConfigReloaded(Box::new(cfg))),
        Subscription::run(ai_question_stream),
        Subscription::run(ai_stream_stream),
        // Widget-host worker: spawned once (stable fn identity keeps
        // the subscription alive across rebuilds); yields
        // Message::WidgetHost scene/failure/registry events.
        Subscription::run(crate::widget_host::stream),
        iced::window::events().map(|(id, event)| match event {
            iced::window::Event::Opened { .. } => Message::WindowOpened(id),
            // Routed through its own message (not ToggleDismiss)
            // so update() can ignore focus loss while the chat
            // shell is up — see Message::WindowUnfocused.
            iced::window::Event::Unfocused => Message::WindowUnfocused,
            iced::window::Event::Focused => Message::WindowFocused,
            iced::window::Event::Resized(size) => Message::WindowResized(size),
            iced::window::Event::FileDropped(path) => Message::AiFileDropped(path),
            iced::window::Event::CloseRequested => Message::WindowCloseRequested(id),
            _ => Message::Noop,
        }),
    ];
    // 60 Hz frame ticker. The radial overlay keeps it always-on (cheap at its
    // small fixed size). The fullscreen chat window only ticks while something is
    // actually animating (thinking / awaiting a choice / an active scroll tween) —
    // a full-window redraw every 16 ms otherwise saturates the main thread and
    // makes typing laggy (the user types while idle, when nothing needs animating).
    let runs_live = state.chat_window_mode
        && state.activity.clusters.iter().any(|c| {
            matches!(c.status, crate::activity::ClusterStatus::Running)
                || c.finished_at.is_some_and(|t| t.elapsed().as_millis() < 7000)
        });
    let needs_tick = !state.chat_window_mode
        || state.ai_loading
        || state.ai_pending_question.is_some()
        || state.ai_scroll_active
        || runs_live;
    if needs_tick {
        subs.push(iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick));
    }
    // The daemon listener requests `org.oxidemx.overlay` + registers AgentHost
    // (dbus.rs). The standalone chat window must NOT run it — otherwise it steals
    // the radial overlay's bus name and the daemon can no longer reach the real
    // overlay. The chat ignores radial show/hide triggers anyway.
    if !state.chat_window_mode {
        subs.push(Subscription::run(crate::dbus::stream).map(Message::Overlay));
    }
    // In chat-window mode, forward Present requests from the D-Bus single-instance
    // service into the iced message loop so the window can try to gain focus.
    if state.chat_window_mode {
        subs.push(Subscription::run(present_stream));
    }
    // agentd event subscriber — active only when the use_agentd flag is on.
    // The D-Bus connection is only opened when agentd routing is actually
    // enabled, keeping the default in-proc path free of any agentd D-Bus churn.
    if state.use_agentd {
        subs.push(Subscription::run(crate::app::agent_events::stream));
    }
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

/// Forward Present requests from the single-instance D-Bus service into the
/// iced message loop. Drains the receiver stashed by the binary before launch.
fn present_stream() -> impl futures_util::stream::Stream<Item = Message> {
    let (tx, rx) = async_channel::unbounded();
    if let Some(mut present_rx) =
        crate::chat_window::single_instance::take_present_receiver()
    {
        tokio::task::spawn(async move {
            while present_rx.recv().await.is_some() {
                let _ = tx.send(Message::PresentWindow).await;
            }
        });
    }
    rx
}

/// Snap the conversation scrollable to the newest message.
pub(super) fn scroll_chat_to_end() -> Task<Message> {
    iced::widget::operation::snap_to_end(crate::chat_ui::body::CHAT_SCROLL_ID)
}

/// Snap the conversation scrollable to a specific relative offset
/// (`y` in [0, 1]; 0 = top, 1 = bottom). Drives the eased "↓ Latest"
/// scroll one frame at a time. Unlike a wheel scroll, this operation
/// mutates the offset directly and does NOT re-fire `on_scroll`, so it
/// never echoes back as a `Message::AiChatScrolled`.
pub(super) fn scroll_chat_to(y: f32) -> Task<Message> {
    iced::widget::operation::snap_to(
        crate::chat_ui::body::CHAT_SCROLL_ID,
        iced::widget::scrollable::RelativeOffset { x: 0.0, y },
    )
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
