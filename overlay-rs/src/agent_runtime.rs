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
use oxidemx_agent::session::{ProviderFingerprint, SessionStore};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Process-global session store (Phase 2). Each chat thread maps to one
/// [`oxidemx_agent::session::Session`] keyed by its stable `session_id`, so a
/// thread's provider instance is built once and reused across turns. Held as
/// a static (matching the overlay's other UI↔backend bridges in `ai_client`)
/// so the detached turn task and the STOP/delete handlers reach the same map.
pub static SESSIONS: once_cell::sync::Lazy<oxidemx_agent::session::SessionManager> =
    once_cell::sync::Lazy::new(oxidemx_agent::session::SessionManager::new);

/// Mint a fresh, process-stable session id for a chat thread that doesn't yet
/// have one. Monotonic within a run; stable once assigned (persisted on the
/// thread). `Math.random`/uuid avoided — a counter is enough for a key.
pub fn new_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("chat-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

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
fn resolve_provider(model_hint: &str) -> Result<(AiProvider, String, String, String), BoxError> {
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
        // Optional key: local servers like mistral.rs may run authed. Use a
        // stored key if present; otherwise empty (the factory sends the
        // keyless EMPTY placeholder).
        oxidemx_agent::keys::provider_key(provider).unwrap_or_default()
    };
    // Local OpenAI-compatible endpoint (used only by the MistralRs provider;
    // ignored by the others in the factory).
    let endpoint = ai.local_endpoint.clone();
    Ok((provider, model, key, endpoint))
}

/// One agent turn for `session_id` (Phase 2). Returns
/// `(reply_text, Some(session_id))` — the id is threaded back so the caller
/// persists it on the chat thread.
///
/// The session owns the provider instance (built once, reused across turns —
/// see [`oxidemx_agent::session`]) and a per-turn cancellation token. History
/// still ships client-side: `history` is the thread's prior turns as
/// `(is_user, text)`, seeded into the executor's memory each turn (all
/// backends in use are stateless / history-shipping).
pub async fn run(
    mode: AgentMode,
    model_hint: &str,
    prompt: &str,
    sink: Option<StreamSink>,
    history: &[(bool, String)],
    image: Option<(String, Vec<u8>)>,
    session_id: &str,
) -> Result<(String, Option<String>), BoxError> {
    let (provider, model, key, endpoint) = resolve_provider(model_hint)?;

    // The session is the isolation unit: one provider instance per
    // conversation, reused across turns. The fingerprint rebuilds it only
    // when the resolved config changes (settings edit / per-thread model
    // switch).
    let session = SESSIONS.session(session_id);
    let fingerprint = ProviderFingerprint {
        provider,
        model: model.clone(),
        key: key.clone(),
        endpoint: endpoint.clone(),
    };
    // One cancellation token for the whole turn (spans retries). STOP /
    // thread-delete fire it; we `select!` the model round against it below.
    let cancel = session.begin_turn();

    // Hybrid lexical+semantic memory recall (falls back to lexical
    // internally if embeddings are unavailable). Computed once; the
    // agent (system + tools) is cheap to clone per retry attempt.
    let system = mode.system_instruction_async(prompt).await;
    let tools = build_tools(mode, &sink);
    let streaming = provider.supports_streaming_tools();
    let debug = std::env::var_os("OXIDEMX_AGENT_DEBUG").is_some();

    // Retry transient provider failures (rate limits, 5xx, timeouts,
    // connection resets) with exponential backoff. Non-transient errors
    // (auth, bad request) fail fast so the user sees the real problem.
    const MAX_ATTEMPTS: u32 = 3;
    let mut attempt = 0u32;
    loop {
        attempt += 1;

        let agent = OverlayAgent {
            system: system.clone(),
            tools: tools.clone(),
            sink: sink.clone(),
        };
        // Built once for the session, reused across turns and retries.
        let llm: Arc<dyn LLMProvider> = session
            .provider(fingerprint.clone())
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

        // Seed an attached image as the latest user turn so the model
        // SEES it (vision) alongside the text prompt. Providers that
        // support image parts (Gemini) include it; others ignore it.
        if let Some((mime, bytes)) = &image {
            use autoagents::llm::chat::ImageMime;
            let im = match mime.as_str() {
                "image/jpeg" | "image/jpg" => ImageMime::JPEG,
                "image/gif" => ImageMime::GIF,
                "image/webp" => ImageMime::WEBP,
                _ => ImageMime::PNG,
            };
            let _ = memory
                .remember(&ChatMessage {
                    role: ChatRole::User,
                    message_type: MessageType::Image((im, bytes.clone())),
                    content: String::new(),
                })
                .await;
        }

        // max_turns 30: a chat agent may chain several tools in one turn
        // (read a file, run a command, search, run a flow…); the ReAct
        // default of 10 runs out and the model fabricates "no result".
        let built = AgentBuilder::<_, DirectAgent>::new(ReActAgent::with_max_turns(agent, 30))
            .llm(llm)
            .memory(Box::new(memory))
            .stream(streaming)
            .build()
            .await;
        let mut handle = match built {
            Ok(h) => h,
            Err(e) => {
                let msg = e.to_string();
                if attempt < MAX_ATTEMPTS && is_retryable(&msg) {
                    backoff(&sink, attempt, MAX_ATTEMPTS).await;
                    continue;
                }
                return Err(Box::new(e) as BoxError);
            }
        };

        // Either path returns Ok(reply) or Err(message) for this attempt,
        // raced against the session's cancel token (STOP / thread-delete).
        // Dropping `turn` on cancel drops the in-flight request and unblocks
        // a tool parked on an approval await.
        let turn = async {
            if streaming {
            use futures_util::StreamExt;
            forward_stream(handle.subscribe_events(), sink.clone());
            match handle.agent.run_stream(Task::new(prompt)).await {
                Ok(mut out) => {
                    let mut reply = String::new();
                    let mut last_err: Option<String> = None;
                    while let Some(item) = out.next().await {
                        match item {
                            Ok(s) => {
                                if debug {
                                    eprintln!("[stream item] {s:?}");
                                }
                                if !s.is_empty() {
                                    reply = s;
                                }
                            }
                            Err(e) => {
                                if debug {
                                    eprintln!("[stream ERR] {e}");
                                }
                                last_err = Some(e.to_string());
                            }
                        }
                    }
                    match (reply.is_empty(), last_err) {
                        (true, Some(e)) => Err(e),
                        _ => Ok(reply),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
            } else {
                drain_events(handle.subscribe_events());
                handle
                    .agent
                    .run(Task::new(prompt))
                    .await
                    .map_err(|e| e.to_string())
            }
        };
        let outcome: Result<String, String> = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err("stopped".to_string()),
            res = turn => res,
        };

        match outcome {
            Ok(reply) => return Ok((reply, Some(session_id.to_string()))),
            Err(msg) => {
                if attempt < MAX_ATTEMPTS && is_retryable(&msg) {
                    backoff(&sink, attempt, MAX_ATTEMPTS).await;
                    continue;
                }
                return Err(msg.into());
            }
        }
    }
}

/// Whether an error message looks like a transient/retryable provider
/// failure (rate limit, 5xx, timeout, connection) rather than a hard
/// error (auth, bad request, quota exhausted).
fn is_retryable(msg: &str) -> bool {
    let m = msg.to_lowercase();
    const TRANSIENT: &[&str] = &[
        "429",
        "rate limit",
        "rate-limit",
        "ratelimit",
        "overloaded",
        "503",
        "502",
        "504",
        "500 internal",
        "unavailable",
        "temporarily",
        "timeout",
        "timed out",
        "connection",
        "reset by peer",
        "broken pipe",
        "dns",
    ];
    TRANSIENT.iter().any(|p| m.contains(p))
}

/// Exponential backoff (0.5s, 1s, 2s) between attempts, with a
/// user-visible "Retrying" activity.
async fn backoff(sink: &Option<StreamSink>, attempt: u32, max: u32) {
    if let Some(s) = sink {
        s.send(StreamEvent::Activity(format!(
            "Connection hiccup — retrying ({attempt}/{max})…"
        )))
        .await;
    }
    let ms = 500u64 * 2u64.pow(attempt - 1);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// One-shot summarization via a direct provider `chat()` (no agent loop,
/// no tools) — rolls older turns into a thread summary so long
/// conversations stay within a bounded context. Merges with the prior
/// summary when present. Returns `None` on any failure (the caller just
/// keeps the existing summary).
pub async fn summarize(model_hint: &str, prior: &str, msgs: &[(bool, String)]) -> Option<String> {
    if msgs.is_empty() {
        return None;
    }
    let (provider, model, key, endpoint) = resolve_provider(model_hint).ok()?;
    let llm = oxidemx_agent::factory::provider_from_config_with_endpoint(
        provider, &model, &key, Some(&endpoint),
    )
    .ok()?;

    let convo = msgs
        .iter()
        .map(|(u, t)| format!("{}: {}", if *u { "User" } else { "Assistant" }, t))
        .collect::<Vec<_>>()
        .join("\n");
    let mut prompt = if prior.is_empty() {
        String::from("Summarize this conversation excerpt into concise notes.\n\n")
    } else {
        format!(
            "Update this running summary with the new exchange below, keeping it concise.\n\n\
             EXISTING SUMMARY:\n{prior}\n\n"
        )
    };
    prompt.push_str(
        "Preserve facts, names, decisions, preferences, and unresolved threads. \
         Return ONLY the summary, no preamble.\n\nCONVERSATION:\n",
    );
    prompt.push_str(&convo);

    let msg = ChatMessage {
        role: ChatRole::User,
        message_type: MessageType::Text,
        content: prompt,
    };
    let resp = llm.chat(&[msg], None).await.ok()?;
    resp.text()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A lean, single-shot chat — direct `provider.chat()` with no agent loop and
/// no tools, with a caller-supplied system prompt. This is the capability
/// surface for SIMPLE prompts and PROMPT OPTIMIZATION — the role a fast local
/// SLM (mistral.rs) is best suited to. It keeps token cost minimal versus the
/// full Agentic system prompt + toolset (which is heavy/OOM-prone on a small
/// local model). Routes through the session so the provider instance is
/// reused (warm connection pool); pass a dedicated lightweight `session_id`.
pub async fn simple_chat(
    model_hint: &str,
    system: Option<&str>,
    prompt: &str,
    session_id: &str,
) -> Result<String, BoxError> {
    let (provider, model, key, endpoint) = resolve_provider(model_hint)?;
    let session = SESSIONS.session(session_id);
    let llm = session
        .provider(ProviderFingerprint {
            provider,
            model,
            key,
            endpoint,
        })
        .map_err(|e| Box::new(e) as BoxError)?;

    let mut msgs = Vec::new();
    if let Some(sys) = system.map(str::trim).filter(|s| !s.is_empty()) {
        msgs.push(ChatMessage {
            role: ChatRole::System,
            message_type: MessageType::Text,
            content: sys.to_string(),
        });
    }
    msgs.push(ChatMessage {
        role: ChatRole::User,
        message_type: MessageType::Text,
        content: prompt.to_string(),
    });

    let resp = llm
        .chat(&msgs, None)
        .await
        .map_err(|e| Box::new(e) as BoxError)?;
    Ok(resp.text().map(|s| s.trim().to_string()).unwrap_or_default())
}

/// Prompt-optimization capability: rewrite a rough user prompt into a clear,
/// specific, well-scoped one (intent preserved, no answer, no commentary).
/// Built on [`simple_chat`] — suited to the local SLM. Uses a dedicated
/// `prompt-optimizer` session so it never disturbs a chat thread's provider.
pub async fn optimize_prompt(model_hint: &str, draft: &str) -> Result<String, BoxError> {
    const SYS: &str = "You are a prompt optimizer. Rewrite the user's prompt to be clear, \
        specific, and well-scoped for an AI assistant. Preserve their intent. Do NOT answer \
        the prompt, ask questions, or add commentary — output ONLY the improved prompt.";
    simple_chat(model_hint, Some(SYS), draft, "prompt-optimizer").await
}

/// Forward the executor's text-delta StreamChunks to the chat thread as
/// `StreamEvent::Delta` (live token rendering). Only used for providers
/// that support streaming-with-tools.
fn forward_stream<S>(mut rx: S, sink: Option<StreamSink>)
where
    S: futures_util::Stream<Item = autoagents::protocol::Event> + Send + Unpin + 'static,
{
    use autoagents::protocol::{Event, StreamChunk};
    use futures_util::StreamExt;
    let debug = std::env::var_os("OXIDEMX_AGENT_DEBUG").is_some();
    tokio::spawn(async move {
        // Gemini reports usageMetadata cumulatively WITHIN a round (the
        // count grows across chunks), so summing every Usage event would
        // over-count. Track the latest per round and flush it once on
        // the round's Done — that sums correctly across multi-round
        // (tool-using) turns.
        let mut pending_usage: Option<(u32, u32)> = None;
        macro_rules! flush_usage {
            () => {
                if let Some((p, c)) = pending_usage.take() {
                    if let Some(s) = &sink {
                        s.send(StreamEvent::Usage {
                            prompt: p,
                            completion: c,
                        })
                        .await;
                    }
                }
            };
        }
        while let Some(ev) = rx.next().await {
            if debug {
                eprintln!("[event] {ev:?}");
            }
            match ev {
                Event::StreamChunk {
                    chunk: StreamChunk::Text(t),
                    ..
                } if !t.is_empty() => {
                    if let Some(s) = &sink {
                        s.send(StreamEvent::Delta(t)).await;
                    }
                }
                Event::StreamChunk {
                    chunk: StreamChunk::Usage(u),
                    ..
                } => {
                    pending_usage = Some((u.prompt_tokens, u.completion_tokens));
                }
                Event::StreamChunk {
                    chunk: StreamChunk::Done { .. },
                    ..
                } => {
                    flush_usage!();
                }
                _ => {}
            }
        }
        // Safety net if the stream ended without a trailing Done.
        flush_usage!();
    });
}

/// Drain the executor event stream so it never backpressures the run
/// (non-streaming path; activity/cards reach the UI via the sink).
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

#[cfg(test)]
mod tests {
    /// Live summarization smoke (needs a Gemini key). Ignored by default
    /// so the normal test run stays offline; run with `--ignored`.
    /// Live vision smoke: a 1×1 PNG must round-trip through the image
    /// plumbing to Gemini and come back with a (non-error) reply —
    /// proving the provider accepted the inline image. Needs a key.
    #[tokio::test]
    #[ignore]
    async fn image_vision_live() {
        // A real PNG asset from the repo (set OXIDEMX_TEST_IMG to override).
        let path = std::env::var("OXIDEMX_TEST_IMG")
            .unwrap_or_else(|_| "assets/flow-indicator.png".to_string());
        let png = std::fs::read(&path).expect("read test image");
        let out = super::run(
            crate::ai_client::AgentMode::Agentic,
            "gemini-2.5-flash",
            "Describe this image in one short sentence.",
            None,
            &[],
            Some(("image/png".to_string(), png)),
            "test-vision",
        )
        .await;
        eprintln!("VISION => {out:?}");
        let (reply, _) = out.expect("vision run should succeed");
        assert!(!reply.trim().is_empty(), "expected a non-empty reply");
    }

    #[tokio::test]
    #[ignore]
    async fn summarize_live() {
        let msgs = vec![
            (true, "I'm building a Rust overlay with iced.".to_string()),
            (
                false,
                "Nice — iced 0.14 with wgpu is a solid choice.".to_string(),
            ),
            (
                true,
                "Remember I deploy on Bazzite via /usr/local/bin.".to_string(),
            ),
        ];
        let out = super::summarize("gemini-2.5-flash", "", &msgs).await;
        eprintln!("SUMMARY => {out:?}");
        assert!(out.is_some(), "expected a summary");
        assert!(!out.unwrap().trim().is_empty());
    }
}
