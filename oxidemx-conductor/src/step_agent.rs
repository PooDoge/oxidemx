//! The per-step agent — one AutoAgents ReAct agent per flow step.
//!
//! A manual `AgentDeriveT` (the same shape as the overlay's proven
//! `OverlayAgent`): `description()` carries the step's resolved system
//! prompt (agent persona + any normalization directives), `name()` is
//! the step id, and `tools()` returns the bridged tools the step's
//! roster agent is granted. Routine `execute_command` gating is the
//! tool's own job (allowlist → structured `Ok` denial, so the model
//! recovers — the P1b lesson); the hook here owns only the *halting*
//! concern: a fired run `CancellationToken` aborts the turn (P1b
//! learning #4 — in the conductor, cancel is NOT future-drop, so the
//! token path must exist).

use std::sync::Arc;

use autoagents::async_trait;
use autoagents::core::agent::memory::SlidingWindowMemory;
use autoagents::core::agent::prebuilt::executor::ReActAgent;
use autoagents::core::agent::task::Task;
use autoagents::core::agent::{
    AgentBuilder, AgentDeriveT, AgentHooks, Context, DirectAgent, HookOutcome,
};
use autoagents::core::tool::ToolT;
use autoagents::llm::{LLMProvider, ToolCall};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use oxidemx_agent::tools::ExecuteCommand;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// One step's agent definition.
#[derive(Clone, Debug)]
pub struct StepAgent {
    step_id: String,
    system: String,
    tools: Vec<String>,
    cancel: CancellationToken,
}

impl StepAgent {
    pub fn new(step_id: String, system: String, tools: Vec<String>, cancel: CancellationToken) -> Self {
        Self {
            step_id,
            system,
            tools,
            cancel,
        }
    }
}

impl AgentDeriveT for StepAgent {
    type Output = String;

    fn name(&self) -> &str {
        &self.step_id
    }
    fn description(&self) -> &str {
        &self.system
    }
    fn output_schema(&self) -> Option<Value> {
        None
    }
    fn tools(&self) -> Vec<Box<dyn ToolT>> {
        self.tools
            .iter()
            .filter_map(|t| match t.as_str() {
                // The one bridged tool today; self-gates via the
                // process allowlist the supervisor installs.
                "execute_command" => Some(Box::new(ExecuteCommand {}) as Box<dyn ToolT>),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl AgentHooks for StepAgent {
    /// Abort a not-yet-started turn if the run was cancelled.
    async fn on_run_start(&self, _task: &Task, _ctx: &Context) -> HookOutcome {
        if self.cancel.is_cancelled() {
            HookOutcome::Abort
        } else {
            HookOutcome::Continue
        }
    }

    /// Abort before a tool runs if the run was cancelled mid-turn.
    /// Routine allow/deny is the tool's concern (structured denial),
    /// not a hard abort here.
    async fn on_tool_call(&self, _call: &ToolCall, _ctx: &Context) -> HookOutcome {
        if self.cancel.is_cancelled() {
            HookOutcome::Abort
        } else {
            HookOutcome::Continue
        }
    }
}

/// Build a one-shot ReAct DirectAgent for a step and run it against
/// `prompt`, racing the run against the cancellation token. Returns
/// the agent's final response text.
///
/// `max_turns` honors the flow's `max_turns` (ReAct turn cap). The
/// `select!` on cancel means a fired token stops the supervisor's wait
/// immediately; for our own providers (mock, Claude Code CLI) it also
/// aborts the in-flight call, and built-in HTTP providers at least
/// stop being awaited (their socket can't be aborted from here — the
/// documented v1 limitation, spec §7.3).
pub async fn build_and_run(
    provider: Arc<dyn LLMProvider>,
    agent: StepAgent,
    prompt: &str,
    max_turns: usize,
    cancel: &CancellationToken,
) -> Result<String, BoxError> {
    // Don't even build an agent for an already-cancelled run.
    if cancel.is_cancelled() {
        return Err("run cancelled".into());
    }
    let handle = AgentBuilder::<_, DirectAgent>::new(ReActAgent::with_max_turns(agent, max_turns))
        .llm(provider)
        .memory(Box::new(SlidingWindowMemory::new(20)))
        .build()
        .await?;

    tokio::select! {
        res = handle.agent.run(Task::new(prompt)) => res.map_err(|e| Box::new(e) as BoxError),
        _ = cancel.cancelled() => Err("run cancelled".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;

    #[tokio::test]
    async fn runs_a_step_against_the_mock_provider() {
        let cancel = CancellationToken::new();
        let agent = StepAgent::new(
            "ingest".into(),
            "You fetch pages.".into(),
            vec![],
            cancel.clone(),
        );
        let out = build_and_run(MockProvider::echoing(), agent, "Fetch https://x", 4, &cancel)
            .await
            .unwrap();
        assert!(out.contains("Fetch https://x"));
    }

    #[tokio::test]
    async fn a_cancelled_token_stops_the_run() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let agent = StepAgent::new("s".into(), "sys".into(), vec![], cancel.clone());
        let err = build_and_run(MockProvider::echoing(), agent, "do it", 4, &cancel)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cancelled"));
    }
}
