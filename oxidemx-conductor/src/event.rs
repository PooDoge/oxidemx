//! Run-layer event vocabulary + the sink the supervisor emits to
//! (spec §11). This is the UI data contract: Mission Control (P4)
//! renders exactly these events; the chat sub-agent card renders them
//! filtered to one run; the CLI prints them as JSON lines.
//!
//! Adopted from kowalski's horde vocabulary (`horde.rs:453-776`) plus
//! ours (`approval_requested`, `run_cancelled`, `step_retrying`). Each
//! event is stamped by the emitter with a monotonic sequence + the
//! `run_id`; wall-clock timestamps are added at the agentd forwarding
//! boundary (AutoAgents events carry none — spec §6, verified).
//!
//! Inline handoff markdown is capped at 48 KB (the FederationRunPanel
//! contract — big artifacts are fetched by path, §11).

use serde::{Deserialize, Serialize};

/// The 48 KB inline-handoff cap (spec §11). Larger payloads are
/// truncated with a note; the full artifact lives on disk.
pub const INLINE_HANDOFF_CAP: usize = 48 * 1024;

/// A run-layer event. `#[serde(tag = "kind")]` gives stable JSON
/// (`{"kind":"task_finished", ...}`) for the D-Bus `AgentEvent(json)`
/// surface and the CLI's JSON-lines output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    /// Run accepted; the plan is about to execute.
    RunStarted {
        flow_id: String,
        run_id: String,
        /// Step ids in scheduled (topological) order, for the UI to
        /// pre-render swim-lanes.
        steps: Vec<String>,
    },
    /// A step's `needs` are satisfied; it has been handed to its agent.
    TaskAssigned { step: String, agent: String },
    /// The step's agent began executing.
    TaskStarted { step: String },
    /// Progress prose from a step (currently the assembled prompt
    /// preview / tool activity; richer streaming is a later add).
    AgentMessage { step: String, message: String },
    /// A step completed. `artifact` is the path it wrote (if any);
    /// `summary` is a short result preview.
    TaskFinished {
        step: String,
        success: bool,
        artifact: Option<String>,
        summary: String,
    },
    /// A step failed after exhausting retries.
    TaskError { step: String, error: String },
    /// A failed step is being retried (1-based attempt number).
    StepRetrying { step: String, attempt: u32 },
    /// A step was skipped — a route branch not taken, or a step made
    /// unreachable by one (so the UI can render it greyed, not failed).
    StepSkipped { step: String, reason: String },
    /// A tool call needs user approval (off-allowlist). `card` is the
    /// approval-card JSON the UI renders.
    ApprovalRequested { step: String, card: serde_json::Value },
    /// The whole run finished successfully.
    RunFinished {
        run_id: String,
        artifacts: Vec<String>,
        /// Inline handoff markdown, capped at `INLINE_HANDOFF_CAP`.
        handoff_markdown: String,
    },
    /// The run failed (a step errored terminally, or validation/setup
    /// failed before scheduling).
    RunFailed {
        run_id: String,
        reason: String,
        step: Option<String>,
    },
    /// The run was cancelled (user `cancel_run` / token fired).
    RunCancelled { run_id: String },
}

impl RunEvent {
    /// Truncate `s` to the inline cap, appending a truncation note.
    pub fn cap_inline(s: &str) -> String {
        if s.len() <= INLINE_HANDOFF_CAP {
            return s.to_string();
        }
        // Truncate on a char boundary at or below the cap.
        let mut end = INLINE_HANDOFF_CAP;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n\n…[truncated {} bytes; full artifact on disk]",
            &s[..end],
            s.len() - end
        )
    }
}

/// Where the supervisor emits run-layer events. Implementations: a
/// JSON-lines stdout sink (CLI), an mpsc forwarder (agentd → D-Bus),
/// a Vec collector (tests).
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    async fn emit(&self, event: RunEvent);
}

/// Discards every event (for runs whose progress nobody is watching).
pub struct NullSink;

#[async_trait::async_trait]
impl EventSink for NullSink {
    async fn emit(&self, _event: RunEvent) {}
}

/// Prints each event as a JSON line to stdout — the CLI's `flow run`
/// surface and the P3 exit criterion ("events on stdout").
pub struct JsonLinesSink;

#[async_trait::async_trait]
impl EventSink for JsonLinesSink {
    async fn emit(&self, event: RunEvent) {
        match serde_json::to_string(&event) {
            Ok(line) => println!("{line}"),
            Err(e) => eprintln!("{{\"kind\":\"_emit_error\",\"error\":{e:?}}}"),
        }
    }
}

/// Collects events into a shared Vec — for assertions in tests.
#[derive(Clone, Default)]
pub struct CollectingSink {
    pub events: std::sync::Arc<tokio::sync::Mutex<Vec<RunEvent>>>,
}

#[async_trait::async_trait]
impl EventSink for CollectingSink {
    async fn emit(&self, event: RunEvent) {
        self.events.lock().await.push(event);
    }
}

impl CollectingSink {
    pub async fn snapshot(&self) -> Vec<RunEvent> {
        self.events.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_with_a_kind_tag() {
        let e = RunEvent::TaskFinished {
            step: "ingest".into(),
            success: true,
            artifact: Some("debug/raw.md".into()),
            summary: "fetched 4kb".into(),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "task_finished");
        assert_eq!(json["step"], "ingest");
        assert_eq!(json["artifact"], "debug/raw.md");
    }

    #[test]
    fn cap_inline_truncates_with_note_on_char_boundary() {
        let big = "é".repeat(INLINE_HANDOFF_CAP); // 2 bytes each ⇒ over cap
        let capped = RunEvent::cap_inline(&big);
        assert!(capped.len() < big.len());
        assert!(capped.contains("truncated"));
        // Round-trips as valid UTF-8 (didn't split a char).
        assert!(capped.is_char_boundary(
            capped.find("\n\n…[truncated").unwrap()
        ));
    }

    #[test]
    fn small_payloads_pass_through_uncapped() {
        assert_eq!(RunEvent::cap_inline("hi"), "hi");
    }

    #[tokio::test]
    async fn collecting_sink_records_in_order() {
        let sink = CollectingSink::default();
        sink.emit(RunEvent::TaskStarted { step: "a".into() }).await;
        sink.emit(RunEvent::TaskStarted { step: "b".into() }).await;
        let snap = sink.snapshot().await;
        assert_eq!(snap.len(), 2);
    }
}
