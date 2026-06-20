use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tracing::error;

// =============================================================================
// GLOBAL CHANNELS FOR ASYNC TOOL-TO-UI COMMUNICATION
// =============================================================================

// Pure-data event/card types now live in oxidemx-agent-core.
pub use oxidemx_agent_core::events::{
    AgentCardData, FlowStep, PendingQuestion, StreamEvent, StreamEventTx, StreamSink,
};

/// Channel to send pending multiple choice questions to the UI event loop.
pub static QUESTION_TX: Lazy<Mutex<Option<mpsc::Sender<PendingQuestion>>>> =
    Lazy::new(|| Mutex::new(None));

/// Channel to notify the UI loop of configuration changes made by the agent.
pub static CONFIG_CHANGED_TX: Lazy<Mutex<Option<mpsc::Sender<String>>>> =
    Lazy::new(|| Mutex::new(None));

/// Channel to push (thread_idx, StreamEvent) into the UI loop.
/// Registered by app.rs's stream subscription at boot.
pub static STREAM_TX: Lazy<Mutex<Option<StreamEventTx>>> = Lazy::new(|| Mutex::new(None));

/// Build a `StreamSink` for `thread` from the globally-registered channel,
/// if the subscription has installed one. Replaces the former
/// `StreamSink::for_thread(thread)` associated function (which referenced
/// the global `STREAM_TX` and could not move to the UI-free core crate).
pub fn stream_sink_for_thread(thread: usize) -> Option<StreamSink> {
    STREAM_TX
        .lock()
        .unwrap()
        .clone()
        .map(|tx| StreamSink { thread, tx })
}

pub mod tools;

// AgentMode + system-instruction assembly now live in core.
pub use oxidemx_agent_core::mode::AgentMode;
// list_flows delegates to core::mode::scan_flows — palette uses this.
pub use oxidemx_agent_core::mode::list_flows;

// =============================================================================
// API KEY & CONFIG PATH RESOLVERS
// =============================================================================

fn get_config_path() -> std::path::PathBuf {
    // Same XDG-aware resolution as agentd_project() — no hardcoded user.
    let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return std::path::PathBuf::from(xdg).join("oxidemx/config.json");
        }
        std::env::var("HOME").unwrap_or_else(|_| "/tmp/oxidemx-home".to_string())
    } else {
        std::env::var("HOME").unwrap_or_else(|_| "/tmp/oxidemx-home".to_string())
    };
    std::path::Path::new(&base).join(".config/oxidemx/config.json")
}

pub fn load_api_key() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    oxidemx_agent_core::api_key::load_api_key()
}

// =============================================================================
// NESTED-CALL HELPERS (heartbeat / consolidation / grounded search)
// =============================================================================
// The agent loop itself now lives in `crate::agent_runtime`; these
// raw one-shot helpers remain for tool-less nested interactions
// (heartbeat tick, memory consolidation, grounded web search).

const INTERACTIONS_URL: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
/// Default + fallback model; the chat toolbar can switch threads to
/// `PRO_MODEL` for harder prompts.
pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";
pub const PRO_MODEL: &str = "gemini-2.5-pro";

/// POST one Interactions-API request and return the parsed body,
/// surfacing the API's own error message on non-2xx (it names
/// quota/key/safety problems precisely).
async fn post_interaction(
    client: &reqwest::Client,
    api_key: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let res = client
        .post(INTERACTIONS_URL)
        .header("x-goog-api-key", api_key)
        .json(body)
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        let error_text = res.text().await.unwrap_or_default();
        error!(%status, "Interactions API returned error: {}", error_text);
        let detail = serde_json::from_str::<serde_json::Value>(&error_text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(String::from))
            .unwrap_or(error_text);
        return Err(format!("API error ({status}): {detail}").into());
    }
    Ok(res.json().await?)
}

/// Concatenated text of every `model_output` step.
fn collect_output_text(body: &serde_json::Value) -> String {
    let mut out = String::new();
    for step in body["steps"].as_array().into_iter().flatten() {
        if step["type"] == "model_output" {
            for content in step["content"].as_array().into_iter().flatten() {
                if content["type"] == "text" {
                    if let Some(t) = content["text"].as_str() {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(t);
                    }
                }
            }
        }
    }
    out
}

/// Everything one nested-call round yields. `id`/`status`/`calls`
/// are parsed for completeness but the surviving callers (heartbeat,
/// consolidation, grounded search) only read `text`.
#[derive(Debug, Default)]
#[allow(dead_code)]
struct RoundOutcome {
    id: Option<String>,
    status: String,
    text: String,
    /// `(call_id, name, arguments)` for every function_call step.
    calls: Vec<(String, String, serde_json::Value)>,
}

/// One request round as a single blocking JSON exchange. Used when
/// no sink is attached and as the fallback when SSE parsing fails.
async fn blocking_round(
    client: &reqwest::Client,
    api_key: &str,
    req_body: &serde_json::Value,
) -> Result<RoundOutcome, Box<dyn std::error::Error + Send + Sync>> {
    let body = post_interaction(client, api_key, req_body).await?;
    let mut out = RoundOutcome {
        id: body["id"].as_str().map(String::from),
        status: body["status"].as_str().unwrap_or_default().to_string(),
        text: collect_output_text(&body),
        calls: Vec::new(),
    };
    for step in body["steps"].as_array().into_iter().flatten() {
        if step["type"] == "function_call" && step["status"] == "waiting" {
            out.calls.push((
                step["id"].as_str().unwrap_or_default().to_string(),
                step["name"].as_str().unwrap_or_default().to_string(),
                step.get("arguments").cloned().unwrap_or(json!({})),
            ));
        }
    }
    Ok(out)
}

/// One headless heartbeat turn (see agent/heartbeat.rs for the
/// contract). Tool-less single round: persona + memories +
/// checklist in, plain text out. Returns `None` when the agent
/// answered HEARTBEAT_OK (nothing needs attention) or when no
/// checklist exists; `Some(alert)` otherwise.
pub async fn run_heartbeat() -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(checklist) = crate::agent::heartbeat::checklist() else {
        return Ok(None);
    };
    let api_key = load_api_key()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()?;

    let mut system = String::from(
        "You are OxideMX-AI on a periodic background heartbeat tick. You have NO tools \
         this turn. Review the user's checklist below against the current date/time and \
         your knowledge of the user. If NOTHING needs their attention right now, reply \
         with exactly HEARTBEAT_OK and nothing else. Otherwise reply with ONE short \
         notification-sized message (no markdown headers) describing only what needs \
         attention.",
    );
    if let Some(soul) = crate::agent::persona::soul_block("general") {
        system.push_str("\n\nPERSONA:\n");
        system.push_str(&soul);
    }
    if let Some(user) = crate::agent::persona::user_block("general") {
        system.push_str("\n\nABOUT THE USER:\n");
        system.push_str(&user);
    }
    if let Some(mem) = crate::agent::memory::injection_block_for(&checklist) {
        system.push_str("\n\n");
        system.push_str(&mem);
    }
    system.push_str("\n\nHEARTBEAT CHECKLIST (user-authored heartbeat.md):\n");
    system.push_str(&checklist);

    // Give the model the wall clock — it has no other way to judge
    // time-conditional checklist lines.
    let now = std::process::Command::new("date")
        .arg("+%A %Y-%m-%d %H:%M %Z")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let req = json!({
        "model": "gemini-2.5-flash",
        "system_instruction": system,
        "input": format!("Heartbeat tick at {now}."),
        "store": false,
    });
    let outcome = blocking_round(&client, &api_key, &req).await?;
    let text = outcome.text.trim().to_string();
    if text.is_empty() || text == "HEARTBEAT_OK" || text.starts_with("HEARTBEAT_OK") {
        return Ok(None);
    }
    Ok(Some(text))
}

/// One agent turn, driven by the AutoAgents runtime
/// (`crate::agent_runtime`). The provider (Gemini / OpenAI / Anthropic
/// / Ollama / Claude Code) is chosen from config; keys resolve per
/// provider inside the runtime. `model` is the thread's flash/pro hint
/// (honoured only for Gemini). `history` (the thread's prior turns) is
/// shipped to the model as conversational context. Returns
/// `(reply_text, None)` — sessions are gone; the tuple shape is kept
/// for call-site stability.
pub async fn ask_ai(
    mode: AgentMode,
    model: &str,
    prompt: &str,
    sink: Option<StreamSink>,
    history: &[(bool, String)],
    image: Option<(String, Vec<u8>)>,
    session_id: &str,
) -> Result<(String, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    oxidemx_agent_core::runtime::route_turn(
        mode,
        model,
        prompt,
        sink,
        history,
        image,
        session_id,
        &crate::agent::tool_exec::executor(),
    )
    .await
}

// =============================================================================
// AGENTD REMOTE SEND PATH
// =============================================================================

/// The stable project identifier used when the overlay talks to agentd.
///
/// The overlay is a personal-assistant shell, not a coding project — there is
/// no meaningful "cwd". We use `~/.config/oxidemx` as the project root so
/// agentd stores the overlay's transcript alongside its other config artefacts.
/// This is consistent across restarts and unique to the user's identity (no
/// collision with real code projects that happen to share a cwd).
pub fn agentd_project() -> String {
    // Prefer XDG_CONFIG_HOME, fall back to $HOME/.config, last resort /tmp/oxidemx.
    // Never hardcode a specific user path.
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return format!("{xdg}/oxidemx");
        }
    }
    let base = std::env::var("HOME").unwrap_or_else(|_| "/tmp/oxidemx-home".to_string());
    format!("{base}/.config/oxidemx")
}

/// Send a prompt to agentd over D-Bus and return immediately.
///
/// The reply arrives asynchronously via the `event` D-Bus signal, which the
/// `agent_events` subscription (see `overlay-rs/src/app/agent_events.rs`)
/// demuxes into the overlay's existing iced `Message`s.  The function itself
/// returns `Ok(turn_id)` once agentd acknowledges the send — it does NOT wait
/// for the full reply.
///
/// `thread` is the overlay thread's session id (same string the in-proc path
/// calls `session_id`).  `model_hint` is forwarded as-is; agentd may ignore
/// it if the active model is already pinned.
pub async fn ask_ai_remote(
    thread: &str,
    text: &str,
    model_hint: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use oxidemx_agent_proxy::AgentProxy;

    let conn = zbus::connection::Builder::session()?.build().await?;
    let proxy = AgentProxy::new(&conn).await?;
    let project = agentd_project();
    let turn_id = proxy
        .send_message(&project, thread, text, model_hint)
        .await?;
    Ok(turn_id)
}

/// Attempt to cancel the in-flight agentd turn for `thread`.
///
/// `cancel_turn` is not yet wired in agentd (Task 6 placeholder only).
/// We handle the error gracefully: return an `Err` whose message the caller
/// can surface in the UI as an activity label.  No crash, no panic.
pub async fn cancel_agentd_turn(
    thread: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use oxidemx_agent_proxy::AgentProxy;

    let conn = zbus::connection::Builder::session()?.build().await?;
    let proxy = AgentProxy::new(&conn).await?;
    let project = agentd_project();
    proxy.cancel_turn(&project, thread).await?;
    Ok(())
}

/// Cancel a conductor run by `run_id` via the agentd D-Bus proxy.
///
/// Non-fatal: the caller ignores the error and waits for the `RunCancelled`
/// event from agentd to confirm.
pub async fn cancel_agentd_run(
    run_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use oxidemx_agent_proxy::AgentProxy;

    let conn = zbus::connection::Builder::session()?.build().await?;
    let proxy = AgentProxy::new(&conn).await?;
    proxy.cancel_run(run_id).await?;
    Ok(())
}

/// Launch a conductor flow by `flow_id` and return the new run id.
///
/// `inputs_json` is passed as `"{}"` (empty inputs) for retries from the UI.
pub async fn run_agentd_flow(
    flow_id: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use oxidemx_agent_proxy::AgentProxy;

    let conn = zbus::connection::Builder::session()?.build().await?;
    let proxy = AgentProxy::new(&conn).await?;
    let project = agentd_project();
    let run_id = proxy.run_flow(&project, flow_id, "{}").await?;
    Ok(run_id)
}

/// Fetch the transcript for `thread` from agentd and return it as a list of
/// `(is_user, text)` pairs suitable for the overlay's `ChatMessage` history.
///
/// Returns an empty vec on any error (agentd down, thread not found, parse
/// failure) — callers treat an empty result as "no remote history yet".
pub async fn fetch_agentd_transcript(thread: &str) -> Vec<(bool, String)> {
    use oxidemx_agent_proxy::AgentProxy;

    let conn = match zbus::connection::Builder::session().map(|b| b.build()) {
        Ok(f) => match f.await {
            Ok(c) => c,
            Err(_) => return vec![],
        },
        Err(_) => return vec![],
    };
    let proxy = match AgentProxy::new(&conn).await {
        Ok(p) => p,
        Err(_) => return vec![],
    };
    let project = agentd_project();
    let json = match proxy.get_transcript(&project, thread).await {
        Ok(j) => j,
        Err(_) => return vec![],
    };
    // agentd returns JSON-encoded Vec<TranscriptTurn> where each turn has
    // {"role":"user"|"assistant", "text":"...", "ts": <u64>}.
    let turns: Vec<serde_json::Value> = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    turns
        .into_iter()
        .filter_map(|t| {
            let role = t.get("role")?.as_str()?;
            let text = t.get("text")?.as_str()?.to_string();
            let is_user = role == "user";
            Some((is_user, text))
        })
        .collect()
}

/// List thread ids known to agentd for the overlay project.
///
/// Returns an empty vec on any error.
/// Unused now; will be consumed by a thread-picker UI in Task 8.
#[allow(dead_code)]
pub async fn list_agentd_threads() -> Vec<String> {
    use oxidemx_agent_proxy::AgentProxy;

    let conn = match zbus::connection::Builder::session()
        .map(|b| b.build())
    {
        Ok(f) => match f.await {
            Ok(c) => c,
            Err(_) => return vec![],
        },
        Err(_) => return vec![],
    };
    let proxy = match AgentProxy::new(&conn).await {
        Ok(p) => p,
        Err(_) => return vec![],
    };
    let project = agentd_project();
    proxy.list_threads(&project).await.unwrap_or_default()
}
