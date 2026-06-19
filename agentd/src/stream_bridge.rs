//! `StreamBridge` — forwards [`oxidemx_agent_core::events::StreamSink`] events
//! onto agentd's [`EventEmitter`] and captures token usage for the journal.
//!
//! # Lifecycle
//!
//! 1. [`StreamBridge::new`] creates an internal mpsc channel, constructs a
//!    [`StreamSink`] from the sender, spawns a drain task, and returns
//!    `(bridge, sink)`.
//! 2. The caller hands `sink` to `route_turn`; the turn sends [`StreamEvent`]s
//!    into the sink as they arrive.
//! 3. When the turn is done the caller drops `sink` (closing the channel).
//! 4. [`StreamBridge::finish`] awaits the drain task and returns accumulated
//!    `(prompt, completion)` token counts.
//!
//! # Concurrency rules
//!
//! No lock is held across any `.await`. The drain task owns the receiver and
//! accumulates usage counters without shared state.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oxidemx_agent_core::events::{StreamEvent, StreamSink};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::seams::{AgentEvent, EventEmitter};

// ── StreamBridge ─────────────────────────────────────────────────────────────

/// Handle returned by [`StreamBridge::new`].
///
/// Call [`finish`](StreamBridge::finish) after dropping the paired
/// [`StreamSink`] to collect accumulated token usage.
pub struct StreamBridge {
    /// Drain task handle. Awaited in `finish`.
    drain: JoinHandle<(u64, u64)>,
}

impl StreamBridge {
    /// Create a new bridge.
    ///
    /// Returns `(bridge, sink)`:
    /// - `bridge` — call [`finish`](StreamBridge::finish) when the turn ends.
    /// - `sink` — pass to `route_turn`; dropping it signals end-of-stream.
    pub fn new(
        project: String,
        thread: String,
        emitter: Arc<dyn EventEmitter>,
    ) -> (StreamBridge, StreamSink) {
        // Buffer of 128 is plenty for burst token streams; backpressure if
        // the drain falls behind (unlikely — emit is sync).
        let (tx, mut rx) = mpsc::channel::<(usize, StreamEvent)>(128);

        // Build a StreamSink from the sender. thread index 0 — not meaningful
        // in the agentd context (we only have one turn-sink active at a time).
        let sink = StreamSink { thread: 0, tx };

        let drain: JoinHandle<(u64, u64)> = tokio::spawn(async move {
            let mut prompt_total: u64 = 0;
            let mut completion_total: u64 = 0;

            // Drain until the channel is closed (sink dropped).
            while let Some((_thread_idx, event)) = rx.recv().await {
                match event {
                    StreamEvent::Delta(text) => {
                        emitter.emit(build_event(&project, &thread, json!({
                            "kind": "delta",
                            "text": text,
                        })));
                    }
                    StreamEvent::Activity(msg) => {
                        emitter.emit(build_event(&project, &thread, json!({
                            "kind": "activity",
                            "text": msg,
                        })));
                    }
                    StreamEvent::Card(card_data) => {
                        // Serialize the card; serde uses the #[serde(tag="kind")] layout.
                        let card_val = serde_json::to_value(&card_data).unwrap_or_else(|_| {
                            json!({ "kind": "command", "error": "serialize_failed" })
                        });
                        // Extract the inner kind ("command", "task", "memory", "flow").
                        let inner_kind = card_val
                            .get("kind")
                            .and_then(|k| k.as_str())
                            .unwrap_or("command");
                        // Map inner kind to outer kind: "flow" → "flow", anything else → "tool".
                        let outer_kind = if inner_kind == "flow" { "flow" } else { "tool" };
                        // Build payload with outer kind and nested card data (no stomp).
                        let payload = json!({
                            "kind": outer_kind,
                            "card": card_val,
                        });
                        emitter.emit(build_event(&project, &thread, payload));
                    }
                    StreamEvent::Usage { prompt, completion } => {
                        // Accumulate — do NOT emit as an event.
                        prompt_total += u64::from(prompt);
                        completion_total += u64::from(completion);
                    }
                }
            }

            (prompt_total, completion_total)
        });

        (StreamBridge { drain }, sink)
    }

    /// Signal end-of-stream and collect accumulated token usage.
    ///
    /// The caller must have dropped the paired [`StreamSink`] (or every clone
    /// of it) before calling this; otherwise the drain task will never finish
    /// and this future will park indefinitely.
    ///
    /// Returns `(prompt_tokens, completion_tokens)`.
    pub async fn finish(self) -> (u64, u64) {
        // The channel is already closed (caller dropped the sink). The drain
        // task will exit its recv loop and return the accumulated counts.
        match self.drain.await {
            Ok(counts) => counts,
            Err(e) => {
                warn!("drain task panicked: {}", e);
                (0, 0)
            }
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn build_event(project: &str, thread: &str, payload: serde_json::Value) -> AgentEvent {
    AgentEvent {
        project: project.to_string(),
        thread_or_run: thread.to_string(),
        ts: now_ms(),
        payload,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bridge_forwards_deltas_and_captures_usage() {
        use oxidemx_agent_core::events::StreamEvent;
        let em = std::sync::Arc::new(crate::seams::RecordingEmitter::default());
        let (bridge, sink) = StreamBridge::new("proj".into(), "t1".into(), em.clone());
        sink.send(StreamEvent::Delta("Hel".into())).await;
        sink.send(StreamEvent::Delta("lo".into())).await;
        sink.send(StreamEvent::Usage { prompt: 10, completion: 3 }).await;
        drop(sink);
        let (p, c) = bridge.finish().await;
        assert_eq!((p, c), (10, 3));
        let evs = em.events();
        let deltas: Vec<_> = evs.iter().filter(|e| e.payload["kind"] == "delta").collect();
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].payload["text"], "Hel"); // ordered
        assert!(evs.iter().all(|e| e.payload["kind"] != "usage")); // usage not an event
    }

    #[tokio::test]
    async fn bridge_forwards_activity() {
        use oxidemx_agent_core::events::StreamEvent;
        let em = std::sync::Arc::new(crate::seams::RecordingEmitter::default());
        let (bridge, sink) = StreamBridge::new("proj".into(), "t1".into(), em.clone());
        sink.send(StreamEvent::Activity("working".into())).await;
        drop(sink);
        bridge.finish().await;
        let evs = em.events();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].payload["kind"], "activity");
        assert_eq!(evs[0].payload["text"], "working");
    }

    #[tokio::test]
    async fn bridge_maps_card_by_inner_kind() {
        use oxidemx_agent_core::events::{StreamEvent, AgentCardData};
        let em = std::sync::Arc::new(crate::seams::RecordingEmitter::default());
        let (bridge, sink) = StreamBridge::new("proj".into(), "t1".into(), em.clone());
        // Test with a Flow card (inner kind "flow" → outer kind "flow").
        let card = AgentCardData::Flow {
            flow_id: "f1".into(),
            run_id: "r1".into(),
            success: true,
            steps: vec![],
            artifacts: vec![],
        };
        sink.send(StreamEvent::Card(card)).await;
        drop(sink);
        bridge.finish().await;
        let evs = em.events();
        assert_eq!(evs.len(), 1);
        let outer_kind = &evs[0].payload["kind"];
        assert_eq!(outer_kind, "flow", "Flow card should map to outer kind 'flow'");
        assert!(evs[0].payload["card"].is_object(), "Card data should be nested under 'card' key");
        assert_eq!(evs[0].payload["card"]["kind"], "flow", "Inner card kind should be preserved");
    }

    #[tokio::test]
    async fn bridge_maps_command_card_to_tool() {
        use oxidemx_agent_core::events::{StreamEvent, AgentCardData};
        let em = std::sync::Arc::new(crate::seams::RecordingEmitter::default());
        let (bridge, sink) = StreamBridge::new("proj".into(), "t1".into(), em.clone());
        // Test with a Command card (inner kind "command" → outer kind "tool").
        let card = AgentCardData::Command {
            command: "echo hello".into(),
            stdout: "hello".into(),
            exit_code: 0,
        };
        sink.send(StreamEvent::Card(card)).await;
        drop(sink);
        bridge.finish().await;
        let evs = em.events();
        assert_eq!(evs.len(), 1);
        let outer_kind = &evs[0].payload["kind"];
        assert_eq!(outer_kind, "tool", "Command card should map to outer kind 'tool'");
        assert!(evs[0].payload["card"].is_object(), "Card data should be nested under 'card' key");
        assert_eq!(evs[0].payload["card"]["kind"], "command", "Inner card kind should be preserved");
        assert_eq!(evs[0].payload["card"]["command"], "echo hello", "Card data should not be stomped");
    }
}
