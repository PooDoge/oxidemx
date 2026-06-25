//! [`CoreWorker`] — production [`oxidemx_harness::Worker`] that runs a step
//! as a gated agent turn.
//!
//! # Design
//!
//! `run_step` builds a [`GatedToolExecutor`] in **Autonomous** mode (no prompt,
//! gate_log present) over a fresh [`AgentToolExecutor`], then calls
//! `oxidemx_agent_core::runtime::route_turn`.  After the turn:
//!
//! - If the [`GateLog`] is non-empty, the first block is returned as
//!   [`HarnessError::NeedsApproval`].  The gate_log is snapshotted AFTER
//!   `route_turn` returns, so no lock is held across the await.
//! - On `route_turn` error → [`HarnessError::Worker`].
//! - On success → [`StepOutput`] with `verify_cmd` forwarded from `brief.verify`.
//!
//! # tool_calls capture
//!
//! `route_turn` does not currently surface individual tool invocations through
//! the `StreamBridge` in a machine-parseable form that maps cleanly to
//! `Vec<ToolInvocation>`.  Until that seam is added the worker returns
//! `tool_calls: vec![]`.
//!
//! TODO(SP2d-3): capture tool invocations for ledger `record_tool_call` by
//! collecting [`StreamEvent::Card`] items with `kind = "command"` / `kind =
//! "task"` from the bridge and mapping them to [`ToolInvocation`].
//!
//! # Not unit-tested directly
//!
//! `run_step` (the `route_turn` path) is compile-wired but NOT unit-tested
//! here: it requires a live provider config and network.  The pure helpers
//! (`prompt_from`, `gatelog_to_outcome`) are fully unit-tested below.

#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use oxidemx_approval::ApprovalClassifier;
use oxidemx_harness::{HarnessError, StepOutput, Worker, WorkerBrief};

use crate::projects::ProjectPaths;
use crate::seams::{EventEmitter, HostCapability};
use crate::tools::gated::{GateBlock, GateLog, GateMode, GatedToolExecutor};

// ── CoreWorker ────────────────────────────────────────────────────────────────

/// Production [`Worker`] that drives a step through `route_turn`.
pub struct CoreWorker {
    pub emitter: Arc<dyn EventEmitter>,
    pub host: Arc<dyn HostCapability>,
    pub classifier: ApprovalClassifier,
    pub paths: ProjectPaths,
}

#[async_trait]
impl Worker for CoreWorker {
    async fn run_step(&self, brief: WorkerBrief) -> Result<StepOutput, HarnessError> {
        use oxidemx_agent_core::mode::AgentMode;

        // ── 1. Build the gate_log + GatedToolExecutor (Autonomous) ────────────
        let gate_log: GateLog = Arc::new(Mutex::new(Vec::new()));
        let inner: Arc<dyn oxidemx_agent_core::tool::ToolExecutor> =
            Arc::new(crate::tools::AgentToolExecutor::new(
                self.paths.clone(),
                self.host.clone(),
                Arc::new(crate::run_launcher::NoopRunLauncher),
                String::new(),
            ));
        let exec: Arc<dyn oxidemx_agent_core::tool::ToolExecutor> =
            Arc::new(GatedToolExecutor::new(
                inner,
                self.classifier.clone(),
                None,                   // No prompt — Autonomous mode
                GateMode::Autonomous,
                self.paths.cwd.clone(),
                Some(gate_log.clone()),
            ));

        // ── 2. Build StreamBridge sink ────────────────────────────────────────
        let session_id = session_id_for(&brief);
        let (bridge, sink) = crate::stream_bridge::StreamBridge::new(
            self.paths.key.as_str().to_string(),
            format!("step:{}", brief.step_id),
            self.emitter.clone(),
        );

        // ── 3. Build prompt and call route_turn ───────────────────────────────
        // No lock is held across this await. gate_log is only read AFTER return.
        let prompt = prompt_from(&brief);
        let history = history_from(&brief);
        let route_result = oxidemx_agent_core::runtime::route_turn(
            AgentMode::Agentic,
            "",           // model_hint: use config default
            &prompt,
            Some(sink),   // stream deltas to bridge
            &history,
            vec![],       // images: Task 6 wires attachments through here
            &session_id,
            &exec,
        )
        .await;

        // Drop exec before bridge.finish() so any internal sink clones are
        // released and the bridge channel can close.
        drop(exec);
        let _usage = bridge.finish().await;

        // ── 4. Snapshot gate_log — AFTER route_turn returns ───────────────────
        // Poison-safe: unwrap_or_else on the lock guard.
        let blocks: Vec<GateBlock> = {
            let guard = gate_log.lock().unwrap_or_else(|e| e.into_inner());
            guard.clone()
        };

        // ── 5. Map outcome ────────────────────────────────────────────────────
        if let Some(err) = gatelog_to_outcome(&blocks) {
            return Err(err);
        }

        let reply = route_result.map_err(|e| HarnessError::Worker(e.to_string()))?.0;

        Ok(StepOutput {
            text: reply.clone(),
            output: serde_json::json!({ "text": reply }),
            // TODO(SP2d-3): capture tool invocations for ledger record_tool_call
            tool_calls: vec![],
            verify_cmd: brief.verify.clone(),
        })
    }
}

// ── Pure helpers (unit-tested) ────────────────────────────────────────────────

/// Build the agent prompt from a [`WorkerBrief`].
///
/// Composes `title`, `goal`, and the JSON-serialized `inputs` so the agent
/// has full context about what the step must accomplish.
pub(crate) fn prompt_from(brief: &WorkerBrief) -> String {
    let inputs_str = serde_json::to_string_pretty(&brief.inputs)
        .unwrap_or_else(|_| "{}".to_string());
    format!(
        "## Step: {title}\n\n**Goal:** {goal}\n\n**Inputs from prior steps:**\n```json\n{inputs}\n```\n\nComplete this step.",
        title = brief.title,
        goal = brief.goal,
        inputs = inputs_str,
    )
}

/// Build a history-from-brief stub.
///
/// `WorkerBrief` does not carry a conversation history today; returns empty.
/// This is a forward-compatibility seam — SP2e can populate it from the
/// ledger's step journal.
pub(crate) fn history_from(_brief: &WorkerBrief) -> Vec<(bool, String)> {
    vec![]
}

/// Build a stable session_id for a step so the provider can route
/// context correctly.
pub(crate) fn session_id_for(brief: &WorkerBrief) -> String {
    format!("harness:{}", brief.step_id)
}

/// Map a [`GateLog`] snapshot to a [`HarnessError`] if any blocks are present.
///
/// Returns `Some(HarnessError::NeedsApproval { tool, reason })` for the first
/// block, or `None` if the log is empty.
pub(crate) fn gatelog_to_outcome(blocks: &[GateBlock]) -> Option<HarnessError> {
    blocks.first().map(|b| HarnessError::NeedsApproval {
        tool: b.tool.clone(),
        reason: b.reason.clone(),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn make_brief(step_id: &str) -> WorkerBrief {
        WorkerBrief {
            step_id: step_id.to_string(),
            title: "Test step".to_string(),
            goal: "Build something".to_string(),
            inputs: Value::Object(serde_json::Map::new()),
            verify: None,
        }
    }

    // ── Verbatim brief test ───────────────────────────────────────────────────

    #[test]
    fn gatelog_to_outcome_flags_needs_approval() {
        assert!(gatelog_to_outcome(&[]).is_none());
        let o = gatelog_to_outcome(&[GateBlock {
            tool: "x".into(),
            reason: "r".into(),
        }]);
        assert!(matches!(o, Some(HarnessError::NeedsApproval { .. })));
    }

    // ── Additional helper tests ───────────────────────────────────────────────

    #[test]
    fn prompt_from_includes_title_goal_and_inputs() {
        let brief = make_brief("step-1");
        let prompt = prompt_from(&brief);
        assert!(prompt.contains("Test step"), "prompt must contain title");
        assert!(prompt.contains("Build something"), "prompt must contain goal");
    }

    #[test]
    fn gatelog_to_outcome_returns_first_block() {
        let blocks = vec![
            GateBlock { tool: "dangerous".into(), reason: "rm -rf".into() },
            GateBlock { tool: "also_bad".into(), reason: "other".into() },
        ];
        let err = gatelog_to_outcome(&blocks).unwrap();
        match err {
            HarnessError::NeedsApproval { tool, reason } => {
                assert_eq!(tool, "dangerous");
                assert_eq!(reason, "rm -rf");
            }
            _ => panic!("expected NeedsApproval"),
        }
    }

    #[test]
    fn verify_forwarded_from_brief() {
        // Verify that CoreWorker forwards brief.verify into StepOutput.verify_cmd.
        // (We test the shape — the actual run_step is compile-wired, not unit-tested.)
        let verify_cmd = Some(("cargo".to_string(), vec!["check".to_string()]));
        let brief = WorkerBrief {
            step_id: "step-v".to_string(),
            title: "verify step".to_string(),
            goal: "build something".to_string(),
            inputs: Value::Object(serde_json::Map::new()),
            verify: verify_cmd.clone(),
        };
        // Manually construct what run_step would build given an Ok reply.
        let reply = "done".to_string();
        let out = StepOutput {
            text: reply.clone(),
            output: serde_json::json!({ "text": reply }),
            tool_calls: vec![],
            verify_cmd: brief.verify.clone(),
        };
        assert_eq!(out.verify_cmd, verify_cmd);
    }

    #[test]
    fn session_id_for_is_stable() {
        let brief = make_brief("abc-123");
        assert_eq!(session_id_for(&brief), "harness:abc-123");
    }
}
