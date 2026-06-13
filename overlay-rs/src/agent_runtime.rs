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
use oxidemx_shared::config::AiProvider;
use serde_json::Value;

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
            // The Gemini backend builds the function response from this
            // Value by parsing it as JSON — a bare `Value::String` of
            // PLAIN TEXT (execute_command output, google_search prose,
            // run_flow summary) parses to nothing and the model sees
            // `{"content": null}`. So: if the tool already returned JSON
            // text, deliver the parsed object; otherwise wrap the plain
            // text as `{"output": …}` so the model actually receives it.
            Ok(text) => Ok(serde_json::from_str::<Value>(&text)
                .unwrap_or_else(|_| serde_json::json!({ "output": text }))),
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

/// The provider + model + key for this turn, from config.json.
/// `model_hint` is the thread's model (the Gemini flash/pro toggle);
/// honoured only for Gemini, where it's meaningful — other providers
/// use their configured model.
fn resolve_provider(model_hint: &str) -> Result<(AiProvider, String, String), BoxError> {
    let ai = oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai)
        .unwrap_or_default();
    let provider = ai.provider;
    let model = if provider == AiProvider::Gemini {
        model_hint.to_string()
    } else {
        ai.model.clone()
    };
    let key = if provider.needs_key() {
        oxidemx_agent::keys::provider_key(provider).ok_or_else(|| {
            format!(
                "No API key for {}. Add one in Settings → AI.",
                provider.label()
            )
        })?
    } else {
        String::new()
    };
    Ok((provider, model, key))
}

/// One agent turn. Returns `(reply_text, None)` — sessions are gone;
/// every provider ships history via memory. The tuple shape is kept
/// for call-site stability.
///
/// `history` is the thread's prior turns as `(is_user, text)`, seeded
/// into the executor's memory so the model has conversational context
/// across turns (all providers are stateless now).
pub async fn run(
    mode: AgentMode,
    model_hint: &str,
    prompt: &str,
    sink: Option<StreamSink>,
    history: &[(bool, String)],
) -> Result<(String, Option<String>), BoxError> {
    let (provider, model, key) = resolve_provider(model_hint)?;

    // Hybrid lexical+semantic memory recall (falls back to lexical
    // internally if embeddings are unavailable).
    let system = mode.system_instruction_async(prompt).await;
    let tools = build_tools(mode, &sink);
    let agent = OverlayAgent {
        system,
        tools,
        sink: sink.clone(),
    };

    let llm: Arc<dyn LLMProvider> = oxidemx_agent::factory::provider_from_config(
        provider, &model, &key,
    )
    .map_err(|e| Box::new(e) as BoxError)?;

    // Ship the thread transcript so the model has multi-turn context.
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

    // 10 (the ReAct default) is too few for a chat agent that may chain
    // several tools in one turn (read a file, run a command, search,
    // run a flow…). At 10, a multi-tool request runs out of turns and
    // the model fabricates "no result" for tools it never reached. 30
    // gives ample headroom while still bounding runaway loops.
    let mut handle = AgentBuilder::<_, DirectAgent>::new(ReActAgent::with_max_turns(agent, 30))
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
    S::Item: std::fmt::Debug,
{
    use futures_util::StreamExt;
    let debug = std::env::var_os("OXIDEMX_AGENT_DEBUG").is_some();
    tokio::spawn(async move {
        while let Some(ev) = rx.next().await {
            if debug {
                eprintln!("[event] {ev:?}");
            }
        }
    });
}
