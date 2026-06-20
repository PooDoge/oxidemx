//! Preventive tool-gate for the autonomous agent.
//!
//! [`GatedToolExecutor`] wraps an inner [`ToolExecutor`] and classifies every
//! tool call BEFORE delegating. A denied or unapproved call never reaches the
//! inner executor.
//!
//! # Gate logic
//!
//! ```text
//! classify(tool, args, cwd)
//!   ├─ AutoAllow | AutoAllowIfReversible → delegate to inner
//!   ├─ AutoDeny                          → Err("tool denied: …")
//!   └─ Ask
//!         ├─ Attended + Some(prompt) → confirm()
//!         │       ├─ true  → delegate to inner
//!         │       └─ false → Err("denied by user: …")
//!         └─ Autonomous | no prompt   → Err("NEEDS_APPROVAL: …")
//! ```

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use oxidemx_agent_core::events::StreamSink;
use oxidemx_agent_core::tool::ToolExecutor;
use oxidemx_approval::{ApprovalClassifier, Tier};

// ── Public types ──────────────────────────────────────────────────────────────

/// A record of a blocked tool call.
///
/// Pushed into a [`GateLog`] whenever the gate blocks a call (AutoDeny,
/// Ask→NEEDS_APPROVAL, or Ask→denied-by-user). Used by the Worker to detect
/// that a tool was blocked and route to the approval flow.
#[derive(Debug, Clone)]
pub struct GateBlock {
    /// The tool name that was blocked.
    pub tool: String,
    /// The classifier's human-readable reason for the block.
    pub reason: String,
}

/// Shared audit log of blocked tool calls.
///
/// The Worker passes `Some(Arc::clone(&log))` when constructing a
/// [`GatedToolExecutor`]; chat-path callers pass `None`.
pub type GateLog = Arc<Mutex<Vec<GateBlock>>>;

/// Helper: push a block record to the log if one is present.
///
/// The guard is acquired, the record pushed, and the guard dropped before
/// returning — it is never held across an `.await`.
fn record_block(log: &Option<GateLog>, tool: &str, reason: &str) {
    if let Some(log) = log {
        let mut guard = log.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(GateBlock { tool: tool.to_string(), reason: reason.to_string() });
        // guard dropped here — no await follows in this helper.
    }
}

/// Whether a human is present to respond to [`Tier::Ask`] prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateMode {
    /// A human is at the keyboard; [`Tier::Ask`] calls block for confirmation.
    Attended,
    /// No human interaction; [`Tier::Ask`] calls return `NEEDS_APPROVAL`.
    Autonomous,
}

/// Callback seam that surfaces a confirmation dialog or prompt to the user.
///
/// The real implementation (wired in SP2d-2) delegates to the SP1b `Approver`.
/// Test code provides [`OkPrompt`]/[`DenyPrompt`] mocks.
#[async_trait]
pub trait ApprovalPrompt: Send + Sync {
    /// Ask the user whether to proceed with `tool`. Returns `true` to allow.
    async fn confirm(&self, tool: &str, reason: &str) -> bool;
}

/// Wraps an inner [`ToolExecutor`] with a preventive risk-tier gate.
///
/// Every `execute` call is classified first; only allowed calls are forwarded
/// to `inner`. No lock is held across any `.await`.
pub struct GatedToolExecutor {
    inner: Arc<dyn ToolExecutor>,
    classifier: ApprovalClassifier,
    prompt: Option<Arc<dyn ApprovalPrompt>>,
    mode: GateMode,
    cwd: PathBuf,
    gate_log: Option<GateLog>,
}

impl GatedToolExecutor {
    /// Construct a new gated executor.
    ///
    /// - `inner`      — the real executor to delegate allowed calls to.
    /// - `classifier` — risk classifier (use [`ApprovalClassifier::default()`]).
    /// - `prompt`     — optional confirmation UI; required in [`GateMode::Attended`].
    /// - `mode`       — [`GateMode::Attended`] or [`GateMode::Autonomous`].
    /// - `cwd`        — agent working directory (used by the path-escape checks).
    /// - `gate_log`   — optional audit log; the Worker passes `Some(log)` to
    ///   detect blocked calls and route to the approval flow.
    ///   Chat callers pass `None`.
    pub fn new(
        inner: Arc<dyn ToolExecutor>,
        classifier: ApprovalClassifier,
        prompt: Option<Arc<dyn ApprovalPrompt>>,
        mode: GateMode,
        cwd: PathBuf,
        gate_log: Option<GateLog>,
    ) -> Self {
        Self { inner, classifier, prompt, mode, cwd, gate_log }
    }
}

#[async_trait]
impl ToolExecutor for GatedToolExecutor {
    async fn execute(
        &self,
        name: &str,
        args: Value,
        sink: &Option<StreamSink>,
    ) -> Result<String, String> {
        // THE INVARIANT: classify first; a denied/unapproved tool must NEVER
        // reach self.inner.
        let d = self.classifier.classify(name, &args, &self.cwd);

        match d.tier {
            // ── Safe: delegate immediately ─────────────────────────────────
            Tier::AutoAllow | Tier::AutoAllowIfReversible => {
                self.inner.execute(name, args, sink).await
            }

            // ── Hard deny: never delegate ──────────────────────────────────
            Tier::AutoDeny => {
                record_block(&self.gate_log, name, &d.reason);
                Err(format!("tool denied: {}", d.reason))
            }

            // ── Requires approval ──────────────────────────────────────────
            Tier::Ask => match (self.mode, self.prompt.as_ref()) {
                (GateMode::Attended, Some(prompt)) => {
                    // No lock is introduced here; `d` is on the stack, the
                    // struct holds no Mutex. Safe to .await directly.
                    if prompt.confirm(name, &d.reason).await {
                        self.inner.execute(name, args, sink).await
                    } else {
                        record_block(&self.gate_log, name, &d.reason);
                        Err(format!("denied by user: {}", d.reason))
                    }
                }
                _ => {
                    // Autonomous mode, or no prompt available.
                    record_block(&self.gate_log, name, &d.reason);
                    Err(format!("NEEDS_APPROVAL: {name}: {}", d.reason))
                }
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // ── Mocks ─────────────────────────────────────────────────────────────────

    /// Records the name of each tool call forwarded to it, then returns `Ok`.
    ///
    /// Uses a poison-safe `Mutex` — the lock is held only for the Vec push,
    /// never across an `.await`.
    #[derive(Default)]
    struct RecordingExecutor {
        calls: Mutex<Vec<String>>,
    }

    impl RecordingExecutor {
        /// Return a snapshot of all recorded call names.
        pub fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
    }

    #[async_trait]
    impl ToolExecutor for RecordingExecutor {
        async fn execute(
            &self,
            name: &str,
            _args: Value,
            _sink: &Option<StreamSink>,
        ) -> Result<String, String> {
            // Lock, push, unlock — no await inside the critical section.
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(name.to_string());
            Ok("ok".into())
        }
    }

    /// Always confirms (returns `true`).
    struct OkPrompt;

    #[async_trait]
    impl ApprovalPrompt for OkPrompt {
        async fn confirm(&self, _tool: &str, _reason: &str) -> bool {
            true
        }
    }

    /// Always denies (returns `false`).
    struct DenyPrompt;

    #[async_trait]
    impl ApprovalPrompt for DenyPrompt {
        async fn confirm(&self, _tool: &str, _reason: &str) -> bool {
            false
        }
    }

    // ── Brief tests (verbatim) ────────────────────────────────────────────────

    #[tokio::test]
    async fn auto_allow_delegates() {
        let rec = Arc::new(RecordingExecutor::default());
        let g = GatedToolExecutor::new(
            rec.clone(),
            ApprovalClassifier::default(),
            None,
            GateMode::Autonomous,
            tempfile::tempdir().unwrap().path().into(),
            None,
        );
        let r = g
            .execute("read_file", serde_json::json!({"file_path":"x"}), &None)
            .await;
        assert!(r.is_ok());
        assert_eq!(rec.calls(), vec!["read_file".to_string()]); // delegated
    }

    #[tokio::test]
    async fn auto_deny_never_delegates() {
        let rec = Arc::new(RecordingExecutor::default());
        let g = GatedToolExecutor::new(
            rec.clone(),
            ApprovalClassifier::default(),
            None,
            GateMode::Autonomous,
            tempfile::tempdir().unwrap().path().into(),
            None,
        );
        let r = g
            .execute(
                "execute_command",
                serde_json::json!({"command":"git push --force"}),
                &None,
            )
            .await;
        assert!(r.unwrap_err().contains("denied"));
        assert!(rec.calls().is_empty()); // NEVER ran
    }

    #[tokio::test]
    async fn ask_autonomous_returns_needs_approval_marker_no_delegate() {
        let rec = Arc::new(RecordingExecutor::default());
        let g = GatedToolExecutor::new(
            rec.clone(),
            ApprovalClassifier::default(),
            None,
            GateMode::Autonomous,
            tempfile::tempdir().unwrap().path().into(),
            None,
        );
        // a commit is Ask tier
        let r = g
            .execute(
                "execute_command",
                serde_json::json!({"command":"git commit -m x"}),
                &None,
            )
            .await;
        assert!(r.unwrap_err().contains("NEEDS_APPROVAL"));
        assert!(rec.calls().is_empty());
    }

    #[tokio::test]
    async fn ask_attended_approves_then_delegates() {
        let rec = Arc::new(RecordingExecutor::default());
        let g = GatedToolExecutor::new(
            rec.clone(),
            ApprovalClassifier::default(),
            Some(Arc::new(OkPrompt)),
            GateMode::Attended,
            tempfile::tempdir().unwrap().path().into(),
            None,
        );
        let r = g
            .execute(
                "execute_command",
                serde_json::json!({"command":"git commit -m x"}),
                &None,
            )
            .await;
        assert!(r.is_ok());
        assert_eq!(rec.calls(), vec!["execute_command".to_string()]);
    }

    #[tokio::test]
    async fn ask_attended_denies_no_delegate() {
        let rec = Arc::new(RecordingExecutor::default());
        let g = GatedToolExecutor::new(
            rec.clone(),
            ApprovalClassifier::default(),
            Some(Arc::new(DenyPrompt)),
            GateMode::Attended,
            tempfile::tempdir().unwrap().path().into(),
            None,
        );
        let r = g
            .execute(
                "execute_command",
                serde_json::json!({"command":"git commit -m x"}),
                &None,
            )
            .await;
        assert!(r.is_err());
        assert!(rec.calls().is_empty());
    }

    #[tokio::test]
    async fn ask_attended_no_prompt_returns_needs_approval_no_delegate() {
        let rec = Arc::new(RecordingExecutor::default());
        let g = GatedToolExecutor::new(
            rec.clone(),
            ApprovalClassifier::default(),
            None,                       // Attended mode but NO prompt wired
            GateMode::Attended,
            tempfile::tempdir().unwrap().path().into(),
            None,
        );
        let r = g
            .execute(
                "execute_command",
                serde_json::json!({"command":"git commit -m x"}),
                &None,
            )
            .await;
        assert!(r.unwrap_err().contains("NEEDS_APPROVAL")); // fails closed
        assert!(rec.calls().is_empty());                     // inner NEVER ran
    }

    // ── Task 2 tests (verbatim from brief) ───────────────────────────────────

    #[tokio::test]
    async fn gate_records_block_in_log_on_deny() {
        let log: GateLog = Arc::new(Mutex::new(Vec::new()));
        let rec = Arc::new(RecordingExecutor::default());
        let g = GatedToolExecutor::new(rec.clone(), ApprovalClassifier::default(), None,
            GateMode::Autonomous, tempfile::tempdir().unwrap().path().into(), Some(log.clone()));
        let _ = g.execute("execute_command", serde_json::json!({"command":"git push --force"}), &None).await;
        assert!(rec.calls().is_empty());                         // gate held
        assert_eq!(log.lock().unwrap_or_else(|e| e.into_inner()).len(), 1);  // recorded
    }
}
