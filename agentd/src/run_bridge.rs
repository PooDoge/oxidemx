//! `RunEventBridge`: forwards conductor `RunEvent`s to agentd's `EventEmitter`
//! and maintains a shared run-status table.
//!
//! ## Design
//!
//! `RunEventBridge` implements `oxidemx_conductor::EventSink`. Each `RunEvent`
//! is mapped to an `AgentEvent` whose `payload.kind == "run"` and whose other
//! fields carry the variant name and structured data. The mapping is lossy in
//! the sense that all conductor variants collapse into one JSON schema, but the
//! variant name is preserved in `payload.variant` so consumers can dispatch.
//!
//! ## Lock discipline
//!
//! The `statuses` `Mutex` is **never** held across an `.await`. The pattern
//! throughout `emit` is: lock → update → drop guard → emit (async). This
//! satisfies the task brief's "no lock across await" requirement and avoids
//! potential deadlocks with the async runtime.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use oxidemx_conductor::{EventSink, RunEvent};

use crate::seams::{AgentEvent, EventEmitter};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── RunEventBridge ────────────────────────────────────────────────────────────

/// Bridges conductor `RunEvent`s onto agentd's `EventEmitter`.
///
/// Also maintains a shared `statuses` table (`run_id → status string`) that
/// `AgentService::run_status` reads. The table is updated synchronously
/// (before the async `emit` call) so that any `run_status` call issued
/// immediately after a status-changing event sees the new value.
pub struct RunEventBridge {
    /// The agentd project string (cwd path). Used to populate `AgentEvent.project`.
    pub project: String,
    /// Emitter forwarding events to the D-Bus signal (or a recorder in tests).
    pub emitter: Arc<dyn EventEmitter>,
    /// Shared run-status table: `run_id → "running" | "finished" | "failed" | "cancelled"`.
    pub statuses: Arc<Mutex<HashMap<String, String>>>,
}

impl RunEventBridge {
    pub fn new(
        project: impl Into<String>,
        emitter: Arc<dyn EventEmitter>,
        statuses: Arc<Mutex<HashMap<String, String>>>,
    ) -> Self {
        Self {
            project: project.into(),
            emitter,
            statuses,
        }
    }

    /// Update the status table (lock → update → drop).
    fn set_status(&self, run_id: &str, status: &str) {
        let mut guard = self.statuses.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(run_id.to_string(), status.to_string());
        // guard dropped here — NEVER held across an .await
    }

    /// Emit an `AgentEvent` with `payload.kind = "run"` plus the given fields.
    fn do_emit(&self, run_id: impl Into<String>, variant: &str, details: serde_json::Value) {
        let run_id = run_id.into();
        let payload = serde_json::json!({
            "kind": "run",
            "variant": variant,
            "run_id": run_id,
            "details": details,
        });
        self.emitter.emit(AgentEvent {
            project: self.project.clone(),
            thread_or_run: run_id,
            ts: now_ms(),
            payload,
        });
    }
}

#[async_trait]
impl EventSink for RunEventBridge {
    async fn emit(&self, event: RunEvent) {
        match &event {
            RunEvent::RunStarted { flow_id, run_id, steps } => {
                // Update status FIRST, then emit (lock never held across await).
                self.set_status(run_id, "running");
                self.do_emit(run_id, "RunStarted", serde_json::json!({
                    "flow_id": flow_id,
                    "steps": steps,
                }));
            }
            RunEvent::TaskAssigned { step, agent } => {
                self.do_emit("", "TaskAssigned", serde_json::json!({
                    "step": step,
                    "agent": agent,
                }));
            }
            RunEvent::TaskStarted { step } => {
                self.do_emit("", "TaskStarted", serde_json::json!({
                    "step": step,
                }));
            }
            RunEvent::AgentMessage { step, message } => {
                self.do_emit("", "AgentMessage", serde_json::json!({
                    "step": step,
                    "message": message,
                }));
            }
            RunEvent::TaskFinished { step, success, artifact, summary } => {
                self.do_emit("", "TaskFinished", serde_json::json!({
                    "step": step,
                    "success": success,
                    "artifact": artifact,
                    "summary": summary,
                }));
            }
            RunEvent::TaskError { step, error } => {
                self.do_emit("", "TaskError", serde_json::json!({
                    "step": step,
                    "error": error,
                }));
            }
            RunEvent::StepRetrying { step, attempt } => {
                self.do_emit("", "StepRetrying", serde_json::json!({
                    "step": step,
                    "attempt": attempt,
                }));
            }
            RunEvent::StepSkipped { step, reason } => {
                self.do_emit("", "StepSkipped", serde_json::json!({
                    "step": step,
                    "reason": reason,
                }));
            }
            RunEvent::ApprovalRequested { step, card } => {
                self.do_emit("", "ApprovalRequested", serde_json::json!({
                    "step": step,
                    "card": card,
                }));
            }
            RunEvent::RunFinished { run_id, artifacts, handoff_markdown } => {
                self.set_status(run_id, "finished");
                self.do_emit(run_id, "RunFinished", serde_json::json!({
                    "artifacts": artifacts,
                    "handoff_markdown": handoff_markdown,
                }));
            }
            RunEvent::RunFailed { run_id, reason, step } => {
                self.set_status(run_id, "failed");
                self.do_emit(run_id, "RunFailed", serde_json::json!({
                    "reason": reason,
                    "step": step,
                }));
            }
            RunEvent::RunCancelled { run_id } => {
                self.set_status(run_id, "cancelled");
                self.do_emit(run_id, "RunCancelled", serde_json::json!({}));
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seams::RecordingEmitter;

    fn make_bridge() -> (RunEventBridge, Arc<RecordingEmitter>, Arc<Mutex<HashMap<String, String>>>) {
        let emitter = Arc::new(RecordingEmitter::default());
        let statuses: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let bridge = RunEventBridge::new("test-project", emitter.clone(), statuses.clone());
        (bridge, emitter, statuses)
    }

    #[tokio::test]
    async fn run_started_sets_running_status_and_emits_run_event() {
        let (bridge, emitter, statuses) = make_bridge();
        bridge.emit(RunEvent::RunStarted {
            flow_id: "my-flow".into(),
            run_id: "run-1".into(),
            steps: vec!["step-a".into()],
        }).await;

        let guard = statuses.lock().unwrap();
        assert_eq!(guard.get("run-1").map(|s| s.as_str()), Some("running"));
        drop(guard);

        let events = emitter.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["kind"], "run");
        assert_eq!(events[0].payload["variant"], "RunStarted");
        assert_eq!(events[0].payload["run_id"], "run-1");
        assert_eq!(events[0].payload["details"]["flow_id"], "my-flow");
    }

    #[tokio::test]
    async fn run_finished_sets_finished_status() {
        let (bridge, _emitter, statuses) = make_bridge();
        bridge.emit(RunEvent::RunFinished {
            run_id: "run-2".into(),
            artifacts: vec![],
            handoff_markdown: String::new(),
        }).await;

        let guard = statuses.lock().unwrap();
        assert_eq!(guard.get("run-2").map(|s| s.as_str()), Some("finished"));
    }

    #[tokio::test]
    async fn run_failed_sets_failed_status() {
        let (bridge, _emitter, statuses) = make_bridge();
        bridge.emit(RunEvent::RunFailed {
            run_id: "run-3".into(),
            reason: "oops".into(),
            step: None,
        }).await;

        let guard = statuses.lock().unwrap();
        assert_eq!(guard.get("run-3").map(|s| s.as_str()), Some("failed"));
    }

    #[tokio::test]
    async fn run_cancelled_sets_cancelled_status() {
        let (bridge, _emitter, statuses) = make_bridge();
        bridge.emit(RunEvent::RunCancelled { run_id: "run-4".into() }).await;

        let guard = statuses.lock().unwrap();
        assert_eq!(guard.get("run-4").map(|s| s.as_str()), Some("cancelled"));
    }

    #[tokio::test]
    async fn non_terminal_events_emit_without_status_update() {
        let (bridge, emitter, statuses) = make_bridge();
        bridge.emit(RunEvent::TaskAssigned { step: "s1".into(), agent: "ag".into() }).await;
        bridge.emit(RunEvent::TaskStarted { step: "s1".into() }).await;
        bridge.emit(RunEvent::AgentMessage { step: "s1".into(), message: "hi".into() }).await;
        bridge.emit(RunEvent::TaskFinished {
            step: "s1".into(),
            success: true,
            artifact: None,
            summary: "done".into(),
        }).await;
        bridge.emit(RunEvent::TaskError { step: "s1".into(), error: "boom".into() }).await;
        bridge.emit(RunEvent::StepRetrying { step: "s1".into(), attempt: 1 }).await;
        bridge.emit(RunEvent::StepSkipped { step: "s1".into(), reason: "skipped".into() }).await;

        // No status written (no run_id).
        let guard = statuses.lock().unwrap();
        assert!(guard.is_empty(), "non-terminal events should not update status table");
        drop(guard);

        // But all events were emitted.
        assert_eq!(emitter.events().len(), 7);
        for ev in emitter.events() {
            assert_eq!(ev.payload["kind"], "run");
        }
    }
}
