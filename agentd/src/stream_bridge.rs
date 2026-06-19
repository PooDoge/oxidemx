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
                        let payload = serde_json::to_value(&card_data).unwrap_or_else(|_| {
                            json!({ "kind": "card", "error": "serialize_failed" })
                        });
                        emitter.emit(build_event(&project, &thread, {
                            let mut p = json!({ "kind": "card" });
                            if let (Some(obj), serde_json::Value::Object(card_obj)) =
                                (p.as_object_mut(), payload)
                            {
                                obj.extend(card_obj);
                            }
                            p
                        }));
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
        self.drain.await.unwrap_or((0, 0))
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
}
