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
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    std::path::Path::new(&home).join(".config/oxidemx/config.json")
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
    crate::agent_runtime::route_turn(mode, model, prompt, sink, history, image, session_id).await
}
