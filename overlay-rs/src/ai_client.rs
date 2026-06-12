use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{error, info};

// =============================================================================
// GLOBAL CHANNELS FOR ASYNC TOOL-TO-UI COMMUNICATION
// =============================================================================

#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub question: String,
    pub options: Vec<String>,
    pub response_tx: mpsc::Sender<String>,
}

/// Channel to send pending multiple choice questions to the UI event loop.
pub static QUESTION_TX: Lazy<Mutex<Option<mpsc::Sender<PendingQuestion>>>> =
    Lazy::new(|| Mutex::new(None));

/// Channel to notify the UI loop of configuration changes made by the agent.
pub static CONFIG_CHANGED_TX: Lazy<Mutex<Option<mpsc::Sender<String>>>> =
    Lazy::new(|| Mutex::new(None));

/// Live progress events for an in-flight agent turn, tagged with the
/// chat-thread index that issued the request so late events file
/// into the right conversation.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of the model's text reply, in order.
    Delta(String),
    /// What the agent is doing right now ("Searching the web…",
    /// "Scheduling task — writing systemd unit…"). Dynamic so tool
    /// executors can interpolate the target into the label.
    Activity(String),
    /// A structured agent-feature card to append to the
    /// conversation (command executed / task scheduled / memory
    /// saved). Rendered by `chat_ui::cards` and persisted on the
    /// owning `ChatMessage`.
    Card(AgentCardData),
}

/// Payload for the three agent-feature card types. Serialized into
/// `ai-chats.json` as part of `ChatMessage`, so every field is
/// plain data (chips/buttons are derived in the view).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentCardData {
    /// `execute_command` ran. `stdout` is trimmed to the card cap.
    Command {
        command: String,
        stdout: String,
        exit_code: i32,
    },
    /// `schedule_task` created/changed a systemd user timer.
    Task {
        name: String,
        /// Unit base name ("oxidemx-task-<slug>").
        unit: String,
        /// OnCalendar expression as written to the timer.
        schedule: String,
        /// Human "next run" from `systemctl --user list-timers`,
        /// `None` when the timer is disabled.
        next_run: Option<String>,
        enabled: bool,
    },
    /// `memory` saved an entry.
    Memory {
        id: String,
        text: String,
        /// "until changed" (pinned) or "auto · 90d" (unpinned).
        retention: String,
    },
}

/// Channel to push (thread_idx, StreamEvent) into the UI loop.
/// Registered by app.rs's stream subscription at boot.
/// Channel type for thread-tagged stream events flowing into the
/// iced subscription.
pub type StreamEventTx = mpsc::Sender<(usize, StreamEvent)>;

pub static STREAM_TX: Lazy<Mutex<Option<StreamEventTx>>> = Lazy::new(|| Mutex::new(None));

/// Per-request handle for forwarding stream events. Cheap to clone.
#[derive(Clone)]
pub struct StreamSink {
    pub thread: usize,
    pub tx: mpsc::Sender<(usize, StreamEvent)>,
}

impl StreamSink {
    /// Build a sink for `thread` from the globally-registered
    /// channel, if the subscription has installed one.
    pub fn for_thread(thread: usize) -> Option<StreamSink> {
        STREAM_TX
            .lock()
            .unwrap()
            .clone()
            .map(|tx| StreamSink { thread, tx })
    }

    async fn send(&self, event: StreamEvent) {
        let _ = self.tx.send((self.thread, event)).await;
    }
}

mod sse;
mod tools;

use sse::stream_round;
use tools::{activity_for_tool, agent_tool_declarations, execute_local_tool};

// =============================================================================
// API KEY & CONFIG PATH RESOLVERS
// =============================================================================

fn get_config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    std::path::Path::new(&home).join(".config/oxidemx/config.json")
}

pub fn load_api_key() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    let path = std::path::Path::new(&home).join(".config/oxidemx/gemini.key");
    if path.exists() {
        let key = std::fs::read_to_string(path)?;
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    let path_legacy = std::path::Path::new(&home).join(".config/juhradial/gemini.key");
    if path_legacy.exists() {
        let key = std::fs::read_to_string(path_legacy)?;
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    Err("Gemini API key not found. Please set GEMINI_API_KEY or save it in ~/.config/oxidemx/gemini.key".into())
}

// =============================================================================
// AGENT MODES & PROMPT DEFS
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    GeneralChat,
    SettingsCustomizer,
}

impl AgentMode {
    /// Short label for the chat shell's mode pills.
    pub fn label(&self) -> &'static str {
        match self {
            AgentMode::GeneralChat => "General",
            AgentMode::SettingsCustomizer => "Menu Setup",
        }
    }

    /// Number of tools armed for this mode — surfaced in the chat
    /// header's status line ("· N tools armed").
    pub fn tool_count(&self) -> usize {
        self.tools().len()
    }

    /// Full system instruction for this mode: the static persona
    /// text plus the user's saved-memories block (when any exist),
    /// so both modes can recall facts the user asked us to keep.
    pub fn system_instruction(&self) -> String {
        let base = match self {
            AgentMode::GeneralChat => {
                "You are OxideMX-AI, a helpful conversational desktop assistant. \
                 You can answer questions, explain concepts, and query the web to ground your responses in real-time. \
                 Keep your responses concise, user-friendly, and format them in markdown."
            }
            AgentMode::SettingsCustomizer => {
                "You are the OxideMX Settings Customizer. You specialize in configuring \
                 the OxideMX circular radial menu overlay, mouse remapping shortcuts, visual themes, and animation curves. \
                 You can read and modify the active layout config. When generating themes, layouts, or list recommendations, \
                 you can output structures in JSON matching the specified schemas. \
                 If you need to make changes, call the set_menu_config tool. \
                 If you have questions with multiple choice options, call the ask_multiple_choice_question tool. \
                 Slice `icon` fields MUST be icon names that actually exist: either a standard \
                 Adwaita/freedesktop symbolic name (e.g. utilities-terminal-symbolic, \
                 text-editor-symbolic, applications-engineering-symbolic, folder-symbolic, \
                 system-run-symbolic, applications-games-symbolic, web-browser-symbolic, \
                 audio-volume-high-symbolic, camera-photo-symbolic, preferences-system-symbolic) \
                 or an Icon= value taken from list_system_apps output. NEVER invent icon names — \
                 a nonexistent name renders as a blank placeholder. When binding launchers, prefer \
                 calling list_system_apps and reusing each app's real exec and icon. \
                 Keep your text replies clean, direct, and focused on layout modification."
            }
        };
        match crate::agent::memory::injection_block() {
            Some(block) => format!("{base}\n\n{block}"),
            None => base.to_string(),
        }
    }

    /// Tool declarations for the Interactions API. NOTE: the API
    /// rejects requests mixing built-in tools (`{"type":
    /// "google_search"}`) with custom function declarations
    /// ("cannot be combined in the same request"). Both modes carry
    /// custom functions now (the agent tools below), so BOTH declare
    /// `google_search` as a CUSTOM function whose executor runs a
    /// nested, search-only Interactions call (see `grounded_search`).
    /// Same API key, no third-party service.
    pub fn tools(&self) -> Vec<serde_json::Value> {
        let search_fn = json!({
            "type": "function",
            "name": "google_search",
            "description": "Search the web with Google for real-time information. Returns a grounded, sourced summary of current facts for the query.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query terms"
                    }
                },
                "required": ["query"]
            }
        });

        match self {
            // General chat gets web search + the local agent tools.
            AgentMode::GeneralChat => {
                let mut tools = vec![search_fn];
                tools.extend(agent_tool_declarations());
                tools
            }
            AgentMode::SettingsCustomizer => {
                let mut tools = vec![
                    json!({
                        "type": "function",
                        "name": "get_menu_config",
                        "description": "Retrieve the current OxideMX radial menu layout, animation curves, and mouse button configuration.",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        }
                    }),
                    json!({
                        "type": "function",
                        "name": "set_menu_config",
                        "description": "Overwrite the current OxideMX radial menu configuration with a new JSON setup. Use this to save changes to themes, layout slices, custom pages, or animation speeds.",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "config_json": {
                                    "type": "string",
                                    "description": "The complete new configuration JSON string"
                                }
                            },
                            "required": ["config_json"]
                        }
                    }),
                    json!({
                        "type": "function",
                        "name": "list_system_apps",
                        "description": "Scan the host system's desktop directories to list installed applications, commands, and icons. Helpful for recommending executables to bind to custom slices.",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        }
                    }),
                    search_fn,
                    json!({
                        "type": "function",
                        "name": "ask_multiple_choice_question",
                        "description": "Ask the user a clarifying multiple-choice question. Used when there are multiple valid options or parameters to clarify.",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "question": {
                                    "type": "string",
                                    "description": "The question text to present"
                                },
                                "options": {
                                    "type": "array",
                                    "items": {
                                        "type": "string"
                                    },
                                    "description": "The list of choices/options the user can click"
                                }
                            },
                            "required": ["question", "options"]
                        }
                    }),
                ];
                tools.extend(agent_tool_declarations());
                tools
            }
        }
    }
}

// =============================================================================
// MAIN ASYNC API CLIENT FUNCTION (AGENT LOOP)
// =============================================================================

/// Hard cap on model⇄tool round-trips within one `ask_ai` call so a
/// confused model can't loop the agent forever.
const MAX_TOOL_ROUNDS: usize = 8;

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

/// Everything one request/response round yields, whether it came in
/// over SSE or as a single JSON body.
#[derive(Debug, Default)]
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

/// One agent turn against the Gemini **Interactions API**
/// (`v1beta/interactions`). Server-side conversation state: pass
/// the `session_id` returned by the previous turn as
/// `previous_interaction_id` and the API replays the full context —
/// no client-side history shipping.
///
/// With a `sink`, responses stream over SSE — text deltas and
/// tool-activity labels are forwarded live; if SSE parsing ever
/// fails mid-round, the round transparently retries as a blocking
/// call (the UI just sees the text arrive at once). Function calls
/// surface as `requires_action`; each is executed locally and fed
/// back as a `function_result` input until the model answers with
/// plain text.
///
/// History note: the original Antigravity-era client targeted this
/// API at `v1beta2` (404) and was temporarily ported to stateless
/// `generateContent`; this is the proper `v1beta` transport.
pub async fn ask_ai(
    api_key: &str,
    mode: AgentMode,
    model: &str,
    prompt: &str,
    mut session_id: Option<String>,
    sink: Option<StreamSink>,
) -> Result<(String, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let tools = mode.tools();
    let mut input: serde_json::Value = json!(prompt);
    let mut full_text = String::new();

    if let Some(s) = &sink {
        s.send(StreamEvent::Activity("Thinking…".to_string())).await;
    }

    for round in 0..MAX_TOOL_ROUNDS {
        let mut req_body = json!({
            "model": model,
            "input": input,
            "tools": tools,
            "system_instruction": mode.system_instruction(),
        });
        if let Some(prev) = &session_id {
            req_body["previous_interaction_id"] = json!(prev);
        }

        info!(round, model, prev = ?session_id, "Sending Interactions API request");
        let outcome = if sink.is_some() {
            match stream_round(&client, api_key, &req_body, &sink).await {
                Ok(o) => o,
                Err(e) if e.to_string().starts_with("API error") => return Err(e),
                Err(e) => {
                    // SSE hiccup — retry the round as a plain JSON
                    // exchange so the turn still completes.
                    tracing::warn!(error = %e, "SSE round failed; falling back to blocking call");
                    blocking_round(&client, api_key, &req_body).await?
                }
            }
        } else {
            blocking_round(&client, api_key, &req_body).await?
        };

        info!(status = %outcome.status, id = ?outcome.id, "Interaction response");
        session_id = outcome.id.or(session_id);
        if !outcome.text.is_empty() {
            if !full_text.is_empty() {
                full_text.push('\n');
            }
            full_text.push_str(&outcome.text);
        }

        match outcome.status.as_str() {
            "completed" => {
                if full_text.is_empty() {
                    return Err("Model returned an empty reply".into());
                }
                return Ok((full_text, session_id));
            }
            "requires_action" => {
                let Some((call_id, name, args)) = outcome.calls.into_iter().next() else {
                    return Err("requires_action with no pending function call".into());
                };
                info!("Executing local tool '{}' (call_id={})", name, call_id);
                if let Some(s) = &sink {
                    s.send(StreamEvent::Activity(activity_for_tool(&name).to_string()))
                        .await;
                }
                let result_text = match execute_local_tool(&name, args, &sink).await {
                    Ok(t) => t,
                    // Feed tool failures back to the model instead
                    // of aborting the turn — it can usually recover
                    // or explain.
                    Err(e) => format!("Tool error: {e}"),
                };
                if let Some(s) = &sink {
                    s.send(StreamEvent::Activity("Thinking…".to_string())).await;
                }
                input = json!({
                    "type": "function_result",
                    "call_id": call_id,
                    "name": name,
                    "result": [{ "type": "text", "text": result_text }],
                });
            }
            "failed" | "cancelled" | "incomplete" | "budget_exceeded" => {
                return Err(format!("Interaction ended with status '{}'", outcome.status).into());
            }
            other => {
                return Err(format!("Unrecognized interaction status: {other}").into());
            }
        }
    }

    Err(format!("Agent exceeded {MAX_TOOL_ROUNDS} tool rounds without a final answer").into())
}
