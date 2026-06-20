//! Worker trait and supporting types for step execution.
//!
//! The [`Worker`] trait is the seam between the executor loop and any actual
//! AI / LLM back-end.  Tests inject a [`MockWorker`] scripted per step id.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::HarnessError;

/// A description of what a single step must accomplish.
///
/// Built by the executor from the manifest and passed to the worker.
#[derive(Clone, Debug)]
pub struct WorkerBrief {
    /// The step ID being executed.
    pub step_id: String,
    /// Human-readable title of the step.
    pub title: String,
    /// High-level goal of the overall task.
    pub goal: String,
    /// JSON object mapping each `needs` step-id to its stored output value.
    pub inputs: Value,
}

/// A single tool invocation recorded during step execution.
#[derive(Clone, Debug)]
pub struct ToolInvocation {
    /// Tool name (e.g. `"read_file"`, `"shell"`).
    pub name: String,
    /// Arguments passed to the tool.
    pub args: Value,
}

/// The result a [`Worker`] produces for one step.
#[derive(Clone, Debug)]
pub struct StepOutput {
    /// Human-readable narrative produced by the worker.
    pub text: String,
    /// Structured output to be stored and forwarded to dependent steps.
    pub output: Value,
    /// Tool invocations made during this step (for ledger accounting).
    pub tool_calls: Vec<ToolInvocation>,
    /// Optional shell command to run as a verifier: `(program, args)`.
    ///
    /// `None` means the step needs no verification (e.g. a planning step).
    pub verify_cmd: Option<(String, Vec<String>)>,
}

/// Seam for executing a single step.
///
/// Implemented by the real AI back-end in production and by [`MockWorker`] in
/// tests.
#[async_trait]
pub trait Worker: Send + Sync {
    /// Execute a step described by `brief` and return its output.
    async fn run_step(&self, brief: WorkerBrief) -> Result<StepOutput, HarnessError>;
}

// ── Test helpers ─────────────────────────────────────────────────────────────

/// A scripted [`Worker`] for use in tests.
///
/// Returns pre-configured [`StepOutput`] values keyed by `step_id`.
#[cfg(test)]
pub struct MockWorker {
    steps: std::collections::HashMap<String, StepOutput>,
}

#[cfg(test)]
impl MockWorker {
    /// Build a `MockWorker` from an iterator of `(step_id, StepOutput)` pairs.
    pub fn from_iter(
        iter: impl IntoIterator<Item = (impl Into<String>, StepOutput)>,
    ) -> Self {
        Self {
            steps: iter.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl Worker for MockWorker {
    async fn run_step(&self, brief: WorkerBrief) -> Result<StepOutput, HarnessError> {
        self.steps
            .get(&brief.step_id)
            .cloned()
            .ok_or_else(|| HarnessError::Worker(format!("no mock for step: {}", brief.step_id)))
    }
}
