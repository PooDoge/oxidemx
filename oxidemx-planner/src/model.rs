//! The `PlannerModel` trait — a seam between the planner logic and the
//! underlying language model.  The real implementation (SP2c) wires
//! `oxidemx-agent-local`'s mistral.rs engine here; tests use
//! `MockPlannerModel`.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::PlannerError;

/// A single request sent to the planning model.
#[derive(Debug, Clone)]
pub struct PlanRequest {
    /// System-level instructions or corrective notes fed back on retry.
    pub system: String,
    /// The goal the model must turn into a step plan.
    pub goal: String,
    /// The JSON Schema that the model's response **must** conform to.
    ///
    /// The real implementation passes this to the model as a generate-time
    /// `SchemaConstraint::JsonSchema`; mocks just record it.
    pub json_schema: Value,
    /// When `true`, the implementation should route to the cloud model
    /// (escalation path).
    pub escalate: bool,
}

/// Seam between the schema-gated planner and the language model backend.
///
/// The real implementation lives in `agentd` (SP2c); this crate only
/// depends on the trait so it stays test-friendly with a mock.
#[async_trait]
pub trait PlannerModel: Send + Sync {
    /// Send a planning request and return the raw text completion.
    async fn complete(&self, req: PlanRequest) -> Result<String, PlannerError>;
}

// ──────────────────────────────────────────────────────────────────────────────
// Test double
// ──────────────────────────────────────────────────────────────────────────────

/// A scripted mock that pops pre-canned replies one at a time.
///
/// Constructed with [`MockPlannerModel::scripted`].  Each call to
/// [`PlannerModel::complete`] pops the next reply from the front of the
/// queue.  Requests are recorded so tests can assert that the schema and
/// escalate flag were forwarded correctly.
#[cfg(test)]
pub(crate) struct MockPlannerModel {
    replies: std::sync::Mutex<std::collections::VecDeque<String>>,
    /// All requests received, in order.
    pub recorded: std::sync::Mutex<Vec<PlanRequest>>,
}

#[cfg(test)]
impl MockPlannerModel {
    /// Build a mock that will return `replies` in order (first in, first out).
    pub(crate) fn scripted(replies: Vec<String>) -> Self {
        Self {
            replies: std::sync::Mutex::new(replies.into_iter().collect()),
            recorded: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl PlannerModel for MockPlannerModel {
    async fn complete(&self, req: PlanRequest) -> Result<String, PlannerError> {
        self.recorded
            .lock()
            .unwrap()
            .push(req.clone());
        let mut queue = self.replies.lock().unwrap();
        queue
            .pop_front()
            .ok_or_else(|| PlannerError::Model("MockPlannerModel: no more scripted replies".into()))
    }
}
