//! Subscriber for the agentd `event`, `approval_requested`, and
//! `model_status_changed` D-Bus signals.
//!
//! Used only when `AiConfig::use_agentd` is `true`.  The in-proc path
//! (StreamEvent channel) remains unchanged; this subscriber runs **in
//! parallel** and feeds the SAME iced `Message`s so the rendering layer
//! is fully reused.
//!
//! # Signal → iced Message demux table
//!
//! | Signal / payload `kind` | iced Message produced |
//! |-------------------------|----------------------|
//! | `event` / `"delta"`     | `AiStream((thread_idx, StreamEvent::Delta(text)))` |
//! | `event` / `"activity"`  | `AiStream((thread_idx, StreamEvent::Activity(text)))` |
//! | `event` / `"tool"`      | `AiStream((thread_idx, StreamEvent::Card(card)))` |
//! | `event` / `"flow"`      | `AiStream((thread_idx, StreamEvent::Card(flow_card)))` |
//! | `event` / `"final"`     | `AgentdFinal(thread_idx, text)` |
//! | `approval_requested`    | `AgentdApprovalRequested { thread, request_id, card }` |
//! | `model_status_changed`  | `AgentdModelStatus(alias, status_json)` |
//!
//! # Thread mapping
//!
//! agentd's `thread_or_run` string is the same string the overlay uses as
//! `session_id`.  The subscriber resolves it to an iced thread index via a
//! shared function (a linear scan over `ai_threads` — fast enough for the
//! handful of threads a user ever has open).  When the session id is not
//! found (race: the thread was just created), the event targets thread 0.

use futures_util::stream::{Stream, StreamExt};
use tracing::{debug, warn};

use super::Message;
use oxidemx_agent_core::events::AgentCardData;

/// Entry-point used by `iced::Subscription::run`.  Connects to the session
/// bus, subscribes to the three agentd signals, and forwards them as iced
/// `Message`s via an `async_channel`.
///
/// Reconnects on disconnect with a 2 s backoff — same pattern as `dbus.rs`.
/// No lock is held across an `await` in this module.
pub fn stream() -> impl Stream<Item = Message> {
    let (tx, rx) = async_channel::unbounded::<Message>();

    tokio::task::spawn(async move {
        loop {
            match run_subscriber(tx.clone()).await {
                Ok(()) => {
                    debug!("agentd event subscriber returned ok; not reconnecting");
                    break;
                }
                Err(e) => {
                    warn!("agentd event subscriber exited: {e}; reconnecting in 2s");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });

    rx
}

async fn run_subscriber(tx: async_channel::Sender<Message>) -> zbus::Result<()> {
    use oxidemx_agent_proxy::AgentProxy;

    let conn = zbus::connection::Builder::session()?.build().await?;
    let proxy = AgentProxy::new(&conn).await?;

    let mut event_stream = proxy.receive_event().await?;
    let mut approval_stream = proxy.receive_approval_requested().await?;
    let mut model_stream = proxy.receive_model_status_changed().await?;

    let tx_ev = tx.clone();
    let tx_ap = tx.clone();
    let tx_ms = tx;

    let event_task = async move {
        while let Some(sig) = event_stream.next().await {
            if let Ok(args) = sig.args() {
                let thread_or_run = args.thread_or_run.to_string();
                let payload_str = args.payload.to_string();
                let msg = demux_event(&thread_or_run, &payload_str);
                if tx_ev.send(msg).await.is_err() {
                    return;
                }
            }
        }
    };

    let approval_task = async move {
        while let Some(sig) = approval_stream.next().await {
            if let Ok(args) = sig.args() {
                let msg = Message::AgentdApprovalRequested {
                    thread: args.thread.to_string(),
                    request_id: args.request_id.to_string(),
                    card_json: args.card.to_string(),
                };
                if tx_ap.send(msg).await.is_err() {
                    return;
                }
            }
        }
    };

    let model_task = async move {
        while let Some(sig) = model_stream.next().await {
            if let Ok(args) = sig.args() {
                let msg = Message::AgentdModelStatus(
                    args.alias.to_string(),
                    args.status.to_string(),
                );
                if tx_ms.send(msg).await.is_err() {
                    return;
                }
            }
        }
    };

    tokio::select! {
        () = event_task => {},
        () = approval_task => {},
        () = model_task => {},
    }

    Ok(())
}

// ── payload demux ─────────────────────────────────────────────────────────────

/// Parse an agentd event payload JSON string and map it to an iced `Message`.
///
/// The `thread_or_run` is the raw session-id string from D-Bus; the mapping
/// to a thread index happens inside `Message::AgentdEvent` handling in
/// `update.rs` where the `RadialState` is accessible.  We pass the raw string
/// here so the subscriber doesn't need shared state.
fn demux_event(thread_or_run: &str, payload_json: &str) -> Message {
    let val: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(v) => v,
        Err(e) => {
            warn!("agentd event: failed to parse payload JSON: {e}");
            return Message::Noop;
        }
    };

    let kind = val
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("unknown");

    match kind {
        "delta" => {
            let text = val
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            Message::AgentdEvent {
                session_id: thread_or_run.to_string(),
                inner: AgentdInner::Delta(text),
            }
        }
        "activity" => {
            let text = val
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            Message::AgentdEvent {
                session_id: thread_or_run.to_string(),
                inner: AgentdInner::Activity(text),
            }
        }
        "tool" | "flow" => {
            // The card is nested under "card" in the payload.
            let card_val = val.get("card").cloned().unwrap_or(serde_json::Value::Null);
            let card: Option<AgentCardData> = serde_json::from_value(card_val).ok();
            Message::AgentdEvent {
                session_id: thread_or_run.to_string(),
                inner: AgentdInner::Card(card),
            }
        }
        "final" => {
            let text = val
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            Message::AgentdEvent {
                session_id: thread_or_run.to_string(),
                inner: AgentdInner::Final(text),
            }
        }
        "run" => {
            // RunEventBridge emits "run" kind events for conductor flow
            // progress (RunStarted / StepStarted / StepFinished / RunFinished
            // etc.).  Parse the full payload into a RunEventView for the
            // activity dock (Task 5 handles it in update.rs).
            let d = val.get("details").cloned().unwrap_or(serde_json::Value::Null);
            let s = |v: &serde_json::Value, k: &str| {
                v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string()
            };
            let strs = |v: &serde_json::Value, k: &str| {
                v.get(k)
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            };
            let view = crate::activity::RunEventView {
                run_id: val.get("run_id").and_then(|x| x.as_str()).unwrap_or(thread_or_run).to_string(),
                conversation_id: val.get("conversation_id").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                variant: val.get("variant").and_then(|x| x.as_str()).unwrap_or("run").to_string(),
                flow_id: s(&d, "flow_id"),
                steps: strs(&d, "steps"),
                step: s(&d, "step"),
                agent: s(&d, "agent"),
                message: { let m = s(&d, "message"); if m.is_empty() { s(&d, "error") } else { m } },
                success: d.get("success").and_then(|x| x.as_bool()).unwrap_or(false),
                artifact: d.get("artifact").and_then(|x| x.as_str()).map(String::from),
                summary: s(&d, "summary"),
                artifacts: strs(&d, "artifacts"),
                handoff: s(&d, "handoff_markdown"),
            };
            Message::RunEvent(view)
        }
        other => {
            debug!("agentd event: unhandled kind '{other}' — ignoring");
            Message::Noop
        }
    }
}

#[cfg(test)]
mod run_parse_tests {
    use super::*;

    #[test]
    fn demux_run_event_carries_conversation_id() {
        let payload = serde_json::json!({
            "kind": "run", "variant": "RunFinished", "run_id": "run-1",
            "conversation_id": "chat-4",
            "details": { "artifacts": ["ANSWER.md"], "handoff_markdown": "done" }
        }).to_string();
        match demux_event("run-1", &payload) {
            Message::RunEvent(v) => {
                assert_eq!(v.conversation_id, "chat-4");
                assert_eq!(v.variant, "RunFinished");
                assert_eq!(v.handoff, "done");
            }
            _ => panic!("expected RunEvent"),
        }
    }

    #[test]
    fn parses_run_started_into_view() {
        let payload = r#"{"kind":"run","variant":"RunStarted","run_id":"run-9",
            "details":{"flow_id":"research","steps":["a","b"]}}"#;
        match demux_event("run-9", payload) {
            Message::RunEvent(v) => {
                assert_eq!(v.run_id, "run-9");
                assert_eq!(v.variant, "RunStarted");
                assert_eq!(v.flow_id, "research");
                assert_eq!(v.steps, vec!["a".to_string(), "b".to_string()]);
            }
            other => panic!("expected RunEvent, got {other:?}"),
        }
    }

    #[test]
    fn task_error_maps_error_into_message() {
        let payload = r#"{"kind":"run","variant":"TaskError","run_id":"run-9","details":{"step":"a","error":"boom"}}"#;
        match demux_event("run-9", payload) {
            Message::RunEvent(v) => { assert_eq!(v.variant, "TaskError"); assert_eq!(v.message, "boom"); }
            other => panic!("expected RunEvent, got {other:?}"),
        }
    }

    #[test]
    fn parses_task_finished_details() {
        let payload = r#"{"kind":"run","variant":"TaskFinished","run_id":"run-9",
            "details":{"step":"a","success":true,"artifact":"/tmp/o.md","summary":"ok"}}"#;
        match demux_event("run-9", payload) {
            Message::RunEvent(v) => {
                assert_eq!(v.step, "a");
                assert!(v.success);
                assert_eq!(v.artifact.as_deref(), Some("/tmp/o.md"));
                assert_eq!(v.summary, "ok");
            }
            other => panic!("expected RunEvent, got {other:?}"),
        }
    }
}

// ── AgentdInner ───────────────────────────────────────────────────────────────

/// Typed payload carried inside `Message::AgentdEvent`.  Defined here so
/// `update.rs` can pattern-match without dealing with raw JSON.
#[derive(Debug, Clone)]
pub enum AgentdInner {
    /// Streamed text chunk.
    Delta(String),
    /// Tool/activity label shown in the status area.
    Activity(String),
    /// Structured card (command / task / memory / flow).
    /// `None` means the card JSON failed to deserialize — treated as Activity.
    Card(Option<AgentCardData>),
    /// Terminal full reply text.  Signals that the turn is complete.
    Final(String),
}

// Re-export for `update.rs`.
pub use AgentdInner as Inner;

/// Resolve a D-Bus `session_id` string to the overlay's `ai_threads` index.
///
/// Scans linearly — there are at most a handful of threads at any time, so
/// `O(n)` is fine and avoids any shared state.  Returns `None` when not found
/// (caller falls back to the active thread or ignores the event).
pub fn session_to_thread_idx(
    threads: &[crate::radial::ChatThread],
    session_id: &str,
) -> Option<usize> {
    threads
        .iter()
        .position(|t| t.session_id.as_deref() == Some(session_id))
}
