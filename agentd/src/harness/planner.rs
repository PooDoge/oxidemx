//! [`PolicyPlannerModel`] — cloud-first planning with LocalPreferred / Auto toggles.
//!
//! # Policy selection
//!
//! | Policy           | `escalate=false`                   | `escalate=true`                |
//! |------------------|------------------------------------|--------------------------------|
//! | `Cloud`          | cloud (stronger=false)             | cloud (stronger=true)          |
//! | `LocalPreferred` | local with SchemaConstraint        | cloud (stronger=false)         |
//! | `Auto`           | like `Cloud` when budget ≥ floor   | like `LocalPreferred` when < floor |
//!
//! # No lock across await
//!
//! The lock pattern used throughout is: lock → read/clone → drop guard → then
//! `.await`.  No `MutexGuard` is held across any `.await` boundary.
#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;

use oxidemx_agent_local::engine::SchemaConstraint;
use oxidemx_agent_local::mode::Mode;
use oxidemx_agent_local::service::LocalModelService;
use oxidemx_agent_local::types::{ChatRequest, Message, Role};
use oxidemx_planner::model::{PlanRequest, PlannerModel};
use oxidemx_planner::PlannerError;

// ── PlannerPolicy ─────────────────────────────────────────────────────────────

/// Planning-model routing policy.
///
/// The default is `Cloud` — planning is quality-critical and the cloud
/// model is used unless the caller opts in to a local preference or
/// token-budget management.
#[derive(Debug, Clone, Copy, Default)]
pub enum PlannerPolicy {
    /// Always use the cloud model. (Default.)
    #[default]
    Cloud,
    /// Prefer the local model; fall back to cloud on escalation.
    LocalPreferred,
    /// Use cloud when `remaining_budget >= budget_floor`; otherwise behave
    /// like `LocalPreferred`.
    Auto,
}

// ── CloudComplete seam ────────────────────────────────────────────────────────

/// Seam between [`PolicyPlannerModel`] and the cloud provider.
///
/// The real implementation (compile-wired in [`RealCloudComplete`]) calls
/// `oxidemx_agent_core::runtime::route_turn` via the cloud provider factory.
/// Unit tests use `#[cfg(test)] RecordingCloud`.
///
/// `stronger=true` asks for the highest-capability cloud tier (used when
/// `req.escalate` is set and we are in `Cloud` policy).
#[async_trait]
pub trait CloudComplete: Send + Sync {
    /// Run a planning completion on the cloud model.
    async fn complete(
        &self,
        system: &str,
        goal: &str,
        json_schema: &serde_json::Value,
        stronger: bool,
    ) -> Result<String, String>;
}

// ── RealCloudComplete — compile-wired production impl ────────────────────────

/// Production [`CloudComplete`] that routes through the cloud provider factory.
///
/// The real planning call uses `oxidemx_agent_core::runtime::route_turn` with
/// the appropriate provider config loaded from the shared config store.
/// `stronger=true` selects the higher-capability model tier (e.g. the
/// flash-thinking / pro tier vs. the standard tier).
///
/// NOTE: the full `route_turn` wiring (session plumbing, tool executor, stream
/// sink) is compile-wired here and exercised in live runs; it is not
/// unit-tested in this file because it requires a live provider config +
/// network.  The unit tests below use `RecordingCloud` instead.
pub struct RealCloudComplete;

#[async_trait]
impl CloudComplete for RealCloudComplete {
    async fn complete(
        &self,
        system: &str,
        goal: &str,
        json_schema: &serde_json::Value,
        stronger: bool,
    ) -> Result<String, String> {
        use oxidemx_agent_core::mode::AgentMode;

        let prompt = format!(
            "{system}\n\n## Goal\n{goal}\n\n## Output schema\n```json\n{schema}\n```\n\nRespond with valid JSON matching the schema.",
            system = system,
            goal = goal,
            schema = serde_json::to_string_pretty(json_schema)
                .unwrap_or_else(|_| "{}".to_string()),
        );

        // `model_hint` selects the cloud tier: empty = config default (standard);
        // "stronger" = signals the session layer to use the pro/flash-thinking tier.
        let model_hint = if stronger { "stronger" } else { "" };

        // No tool executor needed for planning; use a no-op stub.
        let exec: Arc<dyn oxidemx_agent_core::tool::ToolExecutor> =
            Arc::new(NoOpToolExecutor);

        let session_id = format!("planner-cloud-{stronger}");
        let (text, _usage) =
            oxidemx_agent_core::runtime::route_turn(
                AgentMode::Agentic,
                model_hint,
                &prompt,
                None,   // no stream sink
                &[],    // no history
                vec![], // no images
                &session_id,
                &exec,
            )
            .await
            .map_err(|e| e.to_string())?;

        Ok(text)
    }
}

// ── NoOpToolExecutor ──────────────────────────────────────────────────────────

/// A tool executor that returns an error for every call.
///
/// Used by `RealCloudComplete` — planning turns do not invoke tools.
struct NoOpToolExecutor;

#[async_trait]
impl oxidemx_agent_core::tool::ToolExecutor for NoOpToolExecutor {
    async fn execute(
        &self,
        name: &str,
        _args: serde_json::Value,
        _sink: &Option<oxidemx_agent_core::events::StreamSink>,
    ) -> Result<String, String> {
        Err(format!("tool not available in planner context: {name}"))
    }
}

// ── PolicyPlannerModel ────────────────────────────────────────────────────────

/// The real [`oxidemx_planner::PlannerModel`] for the harness.
///
/// Plans cloud-first by default; `LocalPreferred` and `Auto` opt-ins route
/// non-escalated requests through the local model with a
/// `SchemaConstraint::JsonSchema` generate-time constraint.
pub struct PolicyPlannerModel {
    policy: PlannerPolicy,
    cloud: Arc<dyn CloudComplete>,
    local: Arc<dyn LocalModelService>,
    budget_floor: u64,
    remaining_budget: u64,
}

impl PolicyPlannerModel {
    /// Create a new `PolicyPlannerModel`.
    pub fn new(
        policy: PlannerPolicy,
        cloud: Arc<dyn CloudComplete>,
        local: Arc<dyn LocalModelService>,
        budget_floor: u64,
        remaining_budget: u64,
    ) -> Self {
        Self {
            policy,
            cloud,
            local,
            budget_floor,
            remaining_budget,
        }
    }

    /// Call the local model with a `SchemaConstraint::JsonSchema` constraint.
    ///
    /// Builds a [`ChatRequest`] carrying the system prompt + goal as messages
    /// and attaches the schema as a generate-time constraint via
    /// `constraint: Some(SchemaConstraint::JsonSchema(schema))`.
    async fn call_local(
        &self,
        system: &str,
        goal: &str,
        json_schema: &serde_json::Value,
    ) -> Result<String, PlannerError> {
        let req = ChatRequest {
            messages: vec![
                Message {
                    role: Role::System,
                    content: system.to_string(),
                },
                Message {
                    role: Role::User,
                    content: goal.to_string(),
                },
            ],
            mode: Mode::Chat,
            tools: vec![],
            sampling_override: None,
            system_template: None,
            constraint: Some(SchemaConstraint::JsonSchema(json_schema.clone())),
        };
        let resp = self
            .local
            .chat(req)
            .await
            .map_err(|e| PlannerError::Model(e.to_string()))?;
        Ok(resp.text)
    }
}

#[async_trait]
impl PlannerModel for PolicyPlannerModel {
    async fn complete(&self, req: PlanRequest) -> Result<String, PlannerError> {
        // Resolve effective policy for Auto.
        let effective = match self.policy {
            PlannerPolicy::Auto => {
                if self.remaining_budget >= self.budget_floor {
                    PlannerPolicy::Cloud
                } else {
                    PlannerPolicy::LocalPreferred
                }
            }
            other => other,
        };

        match effective {
            PlannerPolicy::Cloud => {
                // escalate → stronger=true, otherwise stronger=false.
                self.cloud
                    .complete(&req.system, &req.goal, &req.json_schema, req.escalate)
                    .await
                    .map_err(PlannerError::Model)
            }
            PlannerPolicy::LocalPreferred => {
                if req.escalate {
                    // Escalated: defer to cloud (stronger=false).
                    self.cloud
                        .complete(&req.system, &req.goal, &req.json_schema, false)
                        .await
                        .map_err(PlannerError::Model)
                } else {
                    // Non-escalated: use local with schema constraint.
                    self.call_local(&req.system, &req.goal, &req.json_schema)
                        .await
                }
            }
            // Auto is fully resolved above; this arm is unreachable.
            PlannerPolicy::Auto => unreachable!("Auto resolved above"),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    use oxidemx_agent_local::error::LocalError;
    use oxidemx_agent_local::guard::Verdict;
    use oxidemx_agent_local::types::{ChatResponse, ModelStatusInfo, Usage};

    // ── RecordingCloud ────────────────────────────────────────────────────────

    #[derive(Clone)]
    struct RecordingCloud {
        calls: Arc<AtomicU64>,
        reply: String,
    }

    impl RecordingCloud {
        fn ok(reply: &str) -> Arc<Self> {
            Arc::new(Self {
                calls: Arc::new(AtomicU64::new(0)),
                reply: reply.to_string(),
            })
        }

        fn calls(&self) -> u64 {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl CloudComplete for RecordingCloud {
        async fn complete(
            &self,
            _system: &str,
            _goal: &str,
            _json_schema: &serde_json::Value,
            _stronger: bool,
        ) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.reply.clone())
        }
    }

    // ── RecordingLocal ────────────────────────────────────────────────────────

    #[derive(Clone)]
    struct RecordingLocal {
        calls: Arc<AtomicU64>,
        reply: String,
    }

    impl RecordingLocal {
        fn ok(reply: &str) -> Arc<Self> {
            Arc::new(Self {
                calls: Arc::new(AtomicU64::new(0)),
                reply: reply.to_string(),
            })
        }

        fn calls(&self) -> u64 {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LocalModelService for RecordingLocal {
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, LocalError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ChatResponse {
                text: self.reply.clone(),
                usage: Usage::default(),
                verdict: Verdict::Ok,
            })
        }

        async fn chat_with_model(
            &self,
            _alias: &str,
            _req: ChatRequest,
        ) -> Result<ChatResponse, LocalError> {
            unimplemented!("not called in these tests")
        }

        async fn ensure_loaded(&self, _alias: &str) -> Result<(), LocalError> {
            unimplemented!("not called in these tests")
        }

        async fn unload(&self, _alias: &str) -> Result<(), LocalError> {
            unimplemented!("not called in these tests")
        }

        async fn set_active(&self, _alias: &str) -> Result<(), LocalError> {
            unimplemented!("not called in these tests")
        }

        fn status(&self) -> Vec<ModelStatusInfo> {
            vec![]
        }
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn req(escalate: bool) -> PlanRequest {
        PlanRequest {
            system: "sys".to_string(),
            goal: "goal".to_string(),
            json_schema: serde_json::json!({}),
            escalate,
        }
    }

    // ── verbatim brief tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn cloud_policy_uses_cloud() {
        let (cloud, local) = (RecordingCloud::ok("{}"), RecordingLocal::ok("{}"));
        let m = PolicyPlannerModel::new(PlannerPolicy::Cloud, cloud.clone(), local.clone(), 0, 0);
        m.complete(req(false)).await.unwrap();
        assert_eq!(cloud.calls(), 1);
        assert_eq!(local.calls(), 0);
    }

    #[tokio::test]
    async fn local_preferred_uses_local_then_cloud_on_escalate() {
        let (cloud, local) = (RecordingCloud::ok("{}"), RecordingLocal::ok("{}"));
        let m = PolicyPlannerModel::new(
            PlannerPolicy::LocalPreferred,
            cloud.clone(),
            local.clone(),
            0,
            0,
        );
        m.complete(req(false)).await.unwrap();
        assert_eq!(local.calls(), 1);
        m.complete(req(true)).await.unwrap();
        assert_eq!(cloud.calls(), 1);
    }

    #[tokio::test]
    async fn auto_low_budget_uses_local() {
        let (cloud, local) = (RecordingCloud::ok("{}"), RecordingLocal::ok("{}"));
        let m = PolicyPlannerModel::new(
            PlannerPolicy::Auto,
            cloud.clone(),
            local.clone(),
            1000,
            10, // remaining < floor
        );
        m.complete(req(false)).await.unwrap();
        assert_eq!(local.calls(), 1);
        assert_eq!(cloud.calls(), 0);
    }
}
