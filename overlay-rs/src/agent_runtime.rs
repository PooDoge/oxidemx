//! The overlay's agent loop, on AutoAgents.
//!
//! Replaces the hand-rolled `ask_ai` ReAct loop. A `ReActAgent` runs
//! against our `GeminiInteractionsProvider` (server-side sessions,
//! live SSE deltas) or the `GenerateContent` fallback, with every
//! existing overlay tool exposed verbatim.
//!
//! How the pieces map (all verified against AutoAgents 0.3.7):
//! - The agent's `description()` becomes the ReAct system message,
//!   which our provider folds into the Interactions `system_instruction`
//!   — so it carries the full mode persona + memory injection.
//! - The provider streams text deltas through its `delta_sink` even
//!   while the executor runs non-streaming; a forwarder task relays
//!   them as `StreamEvent::Delta` into the chat thread.
//! - Each tool is a thin `OverlayTool` delegating to the existing
//!   `ai_client::tools::execute_local_tool` (which still owns approval
//!   chips, activity labels, and card emission).
//! - `on_turn_start` emits the "Thinking…" activity between rounds.

use std::sync::Arc;

use async_trait::async_trait;
use autoagents::core::agent::memory::{MemoryProvider, SlidingWindowMemory};
use autoagents::core::agent::prebuilt::executor::ReActAgent;
use autoagents::core::agent::task::Task;
use autoagents::core::agent::{AgentBuilder, AgentDeriveT, AgentHooks, Context, DirectAgent};
use autoagents::core::tool::{ToolCallError, ToolRuntime, ToolT};
use autoagents::llm::chat::{ChatMessage, ChatRole, MessageType};
use autoagents::llm::LLMProvider;
use oxidemx_agent::provider::GeminiInteractionsProvider;
use oxidemx_shared::config::AiBackend;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::ai_client::{AgentMode, StreamEvent, StreamSink};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// One model-facing tool. Carries its declaration (name/description/
/// JSON schema from `AgentMode::tools`) plus the stream sink, and
/// delegates execution to the existing overlay tool dispatcher.
#[derive(Debug, Clone)]
struct OverlayTool {
    name: String,
    description: String,
    schema: Value,
    sink: Option<StreamSink>,
}

#[async_trait]
impl ToolRuntime for OverlayTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        match crate::ai_client::tools::execute_local_tool(&self.name, args, &self.sink).await {
            Ok(text) => Ok(Value::String(text)),
            Err(e) => Err(ToolCallError::RuntimeError(e)),
        }
    }
}

impl ToolT for OverlayTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn args_schema(&self) -> Value {
        self.schema.clone()
    }
}

/// The overlay agent: a system prompt, a tool set, and the sink the
/// "Thinking…" hook writes to.
#[derive(Debug, Clone)]
struct OverlayAgent {
    system: String,
    tools: Vec<OverlayTool>,
    sink: Option<StreamSink>,
}

impl AgentDeriveT for OverlayAgent {
    type Output = String;

    fn name(&self) -> &str {
        "overlay_agent"
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
            .map(|t| Box::new(t.clone()) as Box<dyn ToolT>)
            .collect()
    }
}

#[async_trait]
impl AgentHooks for OverlayAgent {
    async fn on_turn_start(&self, _turn_index: usize, _ctx: &Context) {
        if let Some(s) = &self.sink {
            s.send(StreamEvent::Activity("Thinking…".to_string())).await;
        }
    }
}

/// Build the tool set for a mode from its declarations.
fn build_tools(mode: AgentMode, sink: &Option<StreamSink>) -> Vec<OverlayTool> {
    mode.tools()
        .into_iter()
        .filter_map(|decl| {
            let name = decl.get("name")?.as_str()?.to_string();
            let description = decl
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let schema = decl
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
            Some(OverlayTool {
                name,
                description,
                schema,
                sink: sink.clone(),
            })
        })
        .collect()
}

/// Which backend the agent runtime should use this turn.
fn configured_backend() -> AiBackend {
    oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai.backend)
        .unwrap_or_default()
}

/// One agent turn. Returns `(reply_text, next_session_id)`.
///
/// `history` is the thread's prior turns as `(is_user, text)`, used
/// only by the stateless `GenerateContent` fallback to reconstruct
/// context; the Interactions backend relies on `session_id` instead.
pub async fn run(
    api_key: &str,
    mode: AgentMode,
    model: &str,
    prompt: &str,
    session_id: Option<String>,
    sink: Option<StreamSink>,
    history: &[(bool, String)],
) -> Result<(String, Option<String>), BoxError> {
    let system = mode.system_instruction(prompt);
    let tools = build_tools(mode, &sink);
    let agent = OverlayAgent {
        system,
        tools,
        sink: sink.clone(),
    };

    match configured_backend() {
        AiBackend::Interactions => run_interactions(agent, api_key, model, prompt, session_id, sink).await,
        AiBackend::GenerateContent => {
            run_generate_content(agent, api_key, model, prompt, history).await
        }
    }
}

/// Interactions backend: server-side session + live SSE deltas.
async fn run_interactions(
    agent: OverlayAgent,
    api_key: &str,
    model: &str,
    prompt: &str,
    session_id: Option<String>,
    sink: Option<StreamSink>,
) -> Result<(String, Option<String>), BoxError> {
    // Provider streams text deltas through this channel; a forwarder
    // relays them into the chat thread as they arrive.
    let (dtx, mut drx) = mpsc::channel::<String>(64);
    let provider = GeminiInteractionsProvider::new(api_key, model).with_delta_sink(dtx);
    provider.seed_session(session_id).await;

    let fwd_sink = sink.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(text) = drx.recv().await {
            if let Some(s) = &fwd_sink {
                s.send(StreamEvent::Delta(text)).await;
            }
        }
    });

    let llm: Arc<dyn LLMProvider> = provider.clone();
    let mut handle = AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(agent))
        .llm(llm)
        .memory(Box::new(SlidingWindowMemory::new(20)))
        .build()
        .await?;
    drain_events(handle.subscribe_events());

    let reply: String = handle.agent.run(Task::new(prompt)).await?;
    let next_session = provider.session().await;

    // Close the delta channel (drop the provider's sender) and let
    // the forwarder finish draining anything already queued.
    drop(handle);
    drop(provider);
    let _ = forwarder.await;

    Ok((reply, next_session))
}

/// GenerateContent fallback: stateless, ships history via memory.
/// No live deltas (the upstream backend doesn't use our sink) — the
/// reply arrives at once. Documented degraded mode.
async fn run_generate_content(
    agent: OverlayAgent,
    api_key: &str,
    model: &str,
    prompt: &str,
    history: &[(bool, String)],
) -> Result<(String, Option<String>), BoxError> {
    let llm = oxidemx_agent::factory::provider_from_config(AiBackend::GenerateContent, model, api_key)
        .map_err(|e| Box::new(e) as BoxError)?;

    let mut memory = SlidingWindowMemory::new(40);
    for (is_user, text) in history {
        let role = if *is_user {
            ChatRole::User
        } else {
            ChatRole::Assistant
        };
        let _ = memory
            .remember(&ChatMessage {
                role,
                message_type: MessageType::Text,
                content: text.clone(),
            })
            .await;
    }

    let mut handle = AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(agent))
        .llm(llm)
        .memory(Box::new(memory))
        .build()
        .await?;
    drain_events(handle.subscribe_events());

    let reply: String = handle.agent.run(Task::new(prompt)).await?;
    Ok((reply, None))
}

/// Drain the executor event stream so it never backpressures the
/// run. Activity/cards reach the UI through the tools + hooks, not
/// this stream, so the events are discarded here.
fn drain_events<S>(mut rx: S)
where
    S: futures_util::Stream + Send + Unpin + 'static,
{
    use futures_util::StreamExt;
    tokio::spawn(async move { while rx.next().await.is_some() {} });
}
