//! Bridges async transport calls + the SSE stream into Freya signals the
//! regions render. The streaming reducer (`Transcript`) is pure + unit-tested;
//! `AppState` wraps it in signals and spawns the async I/O.
//!
//! # Truthfulness invariant (Rule 1)
//!
//! `Transcript` is the ONLY place assistant text is assembled.
//! Assistant content is only ever appended from `delta`/`final` events —
//! never fabricated by any other path.
use std::sync::Arc;

use freya::prelude::*;
use oxide_client::{AgentEvent, Conversation, ConversationId, Project, Transport, Turn};

// ── Connection state ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnState {
    Unknown,
    Connected,
    Reconnecting,
    Unreachable,
}

// ── Pure streaming reducer ──────────────────────────────────────────────────

/// Pure streaming reducer — the single place assistant text is assembled.
///
/// `apply_user` appends a user turn and clears `live_assistant`.
/// `apply_event` appends to `live_assistant` on `delta` and commits an
/// assistant `Turn` (clearing `live_assistant`) on `final`.
/// Nothing else touches assistant text — the truthfulness invariant is
/// structural, not aspirational.
#[derive(Default, Clone, PartialEq)]
pub struct Transcript {
    pub turns: Vec<Turn>,
    /// In-flight assistant text (the streaming "bubble").  Empty between turns.
    pub live_assistant: String,
}

impl Transcript {
    /// Append a user message and reset the streaming bubble.
    pub fn apply_user(&mut self, text: String) {
        self.turns.push(Turn { role: "user".into(), text, ts: 0 });
        self.live_assistant.clear();
    }

    /// Process one SSE event.
    ///
    /// - `delta` — append `text` to `live_assistant`.
    /// - `final` — commit an assistant `Turn` (text from event or accumulated
    ///   `live_assistant`) and clear the bubble.
    /// - anything else — ignored (activity/tool/card events are not rendered in 2a).
    pub fn apply_event(&mut self, ev: &AgentEvent) {
        match ev.kind.as_str() {
            "delta" => {
                if let Some(t) = ev.text() {
                    self.live_assistant.push_str(t);
                }
            }
            "final" => {
                // ev.text() is canonical; accumulated live_assistant is intentionally
                // discarded when the final event provides its own text field.
                let text = ev
                    .text()
                    .map(str::to_string)
                    .unwrap_or_else(|| std::mem::take(&mut self.live_assistant));
                self.turns.push(Turn { role: "assistant".into(), text, ts: 0 });
                self.live_assistant.clear();
            }
            _ => {} // activity/tool/card — not rendered in 2a
        }
    }
}

// ── AppState ────────────────────────────────────────────────────────────────

/// Freya signal bridge to the agentd transport.
///
/// Must be constructed inside a Freya component (calls `use_state`).
/// Call `bootstrap` once on mount; call `open_conversation` and `send`
/// from event handlers.
#[derive(Clone)]
pub struct AppState {
    transport: Arc<dyn Transport>,
    pub projects: State<Vec<Project>>,
    pub conversations: State<Vec<Conversation>>,
    pub active: State<Option<ConversationId>>,
    pub transcript: State<Transcript>,
    pub connection: State<ConnState>,
}

impl PartialEq for AppState {
    /// Two `AppState` handles are equal when they share the same underlying
    /// signals — i.e., they were created in the same component invocation.
    fn eq(&self, other: &Self) -> bool {
        self.projects == other.projects
            && self.conversations == other.conversations
            && self.active == other.active
            && self.transcript == other.transcript
            && self.connection == other.connection
    }
}

impl AppState {
    /// Create `AppState`.  Must be called from inside a Freya component body
    /// (same restriction as `use_state`).
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            projects: use_state(Vec::new),
            conversations: use_state(Vec::new),
            active: use_state(|| None),
            transcript: use_state(Transcript::default),
            connection: use_state(|| ConnState::Unknown),
        }
    }

    /// Health-check → load projects → load personal project's conversations.
    ///
    /// Call once on component mount (e.g. from `use_future`).
    pub fn bootstrap(&self) {
        let t = self.transport.clone();
        let mut projects = self.projects;
        let mut conversations = self.conversations;
        let mut connection = self.connection;
        spawn(async move {
            match t.health().await {
                Ok(()) => connection.set(ConnState::Connected),
                Err(_) => {
                    connection.set(ConnState::Unreachable);
                    return;
                }
            }
            if let Ok(ps) = t.list_projects().await {
                projects.set(ps);
            }
            if let Ok(cs) = t.list_conversations("personal").await {
                conversations.set(cs);
            }
        });
    }

    /// Load history and subscribe to the SSE stream for `id`.
    pub fn open_conversation(&self, id: ConversationId) {
        // Synchronously set active + reset transcript BEFORE the spawn so that
        // `send()` calls during the `get_history` await window append to the
        // fresh (empty) transcript, not the previous conversation's turns.
        let mut active = self.active;
        active.set(Some(id.clone()));
        let mut transcript = self.transcript;
        transcript.set(Transcript::default());

        let t = self.transport.clone();
        let mut connection = self.connection;
        spawn(async move {
            // Fetch history, then merge so any turns `send()` appended locally
            // during the await are preserved AFTER the history turns.
            let history = t.get_history(id.as_str()).await.unwrap_or_default();
            transcript.with_mut(|mut tx| {
                let locals = std::mem::take(&mut tx.turns);
                tx.turns = history;
                tx.turns.extend(locals);
            });
            let mut events = t.subscribe(id.as_str());
            use futures_util::StreamExt;
            while let Some(ev) = events.next().await {
                match ev {
                    Ok(ev) => {
                        connection.set(ConnState::Connected);
                        transcript.with_mut(|mut tx| tx.apply_event(&ev));
                    }
                    Err(_) => {
                        connection.set(ConnState::Reconnecting);
                    }
                }
            }
        });
    }

    /// Append the user turn immediately, then fire `send_message` to the
    /// transport.  The assistant reply streams back via the open subscription.
    pub fn send(&self, text: String) {
        let Some(id) = self.active.peek().clone() else { return };
        let mut transcript = self.transcript;
        transcript.with_mut(|mut tx| tx.apply_user(text.clone()));
        let t = self.transport.clone();
        spawn(async move {
            let _ = t.send_message(id.as_str(), &text).await;
        });
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use oxide_client::{mock::MockTransport, AgentEvent};

    use super::*;

    /// delta → delta → final accumulates "pong" then commits an assistant turn.
    #[test]
    fn send_streams_delta_then_final_into_signals() {
        // Only the pure Transcript reducer is tested here — no Freya context needed.
        let _transport: Arc<dyn oxide_client::Transport> = Arc::new(MockTransport {
            events: std::sync::Mutex::new(vec![
                AgentEvent {
                    seq: 1,
                    kind: "delta".into(),
                    payload: serde_json::json!({"kind":"delta","text":"po"}),
                },
                AgentEvent {
                    seq: 2,
                    kind: "delta".into(),
                    payload: serde_json::json!({"kind":"delta","text":"ng"}),
                },
                AgentEvent {
                    seq: 3,
                    kind: "final".into(),
                    payload: serde_json::json!({"kind":"final","text":"pong"}),
                },
            ]),
            ..MockTransport::new()
        });

        let mut tx = Transcript::default();
        tx.apply_user("Reply pong".into());
        assert_eq!(tx.live_assistant, "");
        assert_eq!(tx.turns.len(), 1);
        assert_eq!(tx.turns[0].role, "user");

        tx.apply_event(&AgentEvent {
            seq: 1,
            kind: "delta".into(),
            payload: serde_json::json!({"text":"po"}),
        });
        tx.apply_event(&AgentEvent {
            seq: 2,
            kind: "delta".into(),
            payload: serde_json::json!({"text":"ng"}),
        });
        assert_eq!(tx.live_assistant, "pong");

        tx.apply_event(&AgentEvent {
            seq: 3,
            kind: "final".into(),
            payload: serde_json::json!({"text":"pong"}),
        });
        assert_eq!(tx.live_assistant, "", "live_assistant must be cleared after final");
        assert_eq!(
            tx.turns.last().unwrap().text,
            "pong",
            "final must commit a Turn with the assistant text"
        );
        assert_eq!(
            tx.turns.last().unwrap().role,
            "assistant",
            "committed Turn must have role=assistant"
        );
    }

    /// An unhealthy `MockTransport` must return `Err` from `health()` — the
    /// seam `AppState::bootstrap` relies on to transition to `Unreachable`.
    #[tokio::test]
    async fn unhealthy_transport_health_returns_err() {
        let mock = MockTransport { healthy: false, ..MockTransport::new() };
        assert!(
            mock.health().await.is_err(),
            "unhealthy mock must return Err from health()"
        );
    }

    /// apply_user clears live_assistant; a second user turn appends.
    #[test]
    fn apply_user_appends_and_clears_live() {
        let mut tx = Transcript::default();
        tx.live_assistant = "leftover".into();
        tx.apply_user("hello".into());
        assert_eq!(tx.live_assistant, "", "apply_user must clear live_assistant");
        assert_eq!(tx.turns.len(), 1);
        assert_eq!(tx.turns[0].text, "hello");
        assert_eq!(tx.turns[0].role, "user");
    }

    /// final with no preceding deltas uses its own text field (not empty live_assistant).
    #[test]
    fn final_without_deltas_commits_event_text() {
        let mut tx = Transcript::default();
        tx.apply_event(&AgentEvent {
            seq: 1,
            kind: "final".into(),
            payload: serde_json::json!({"text":"direct"}),
        });
        assert_eq!(tx.turns.last().unwrap().text, "direct");
        assert_eq!(tx.live_assistant, "");
    }

    /// final with no text field falls back to accumulated live_assistant.
    #[test]
    fn final_without_text_field_falls_back_to_live() {
        let mut tx = Transcript::default();
        tx.apply_event(&AgentEvent {
            seq: 1,
            kind: "delta".into(),
            payload: serde_json::json!({"text":"acc"}),
        });
        // Final has no "text" key — should drain live_assistant.
        tx.apply_event(&AgentEvent {
            seq: 2,
            kind: "final".into(),
            payload: serde_json::json!({}),
        });
        assert_eq!(tx.turns.last().unwrap().text, "acc");
        assert_eq!(tx.live_assistant, "");
    }
}
