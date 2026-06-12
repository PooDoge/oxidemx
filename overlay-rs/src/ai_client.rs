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

/// Human label for a tool the agent is about to run. Generic
/// fallback per tool — the executors emit more specific labels once
/// they've parsed their arguments (e.g. "Running brightnessctl…").
fn activity_for_tool(name: &str) -> &'static str {
    match name {
        "google_search" => "Searching the web…",
        "get_menu_config" => "Reading menu config…",
        "set_menu_config" => "Writing config…",
        "list_system_apps" => "Listing installed apps…",
        "ask_multiple_choice_question" => "Waiting for your choice…",
        "execute_command" => "Running command…",
        "schedule_task" => "Managing scheduled tasks…",
        "memory" => "Updating memories…",
        _ => "Running tool…",
    }
}

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

/// Declarations for the local agent tools (`execute_command`,
/// `schedule_task`, `memory`), shared by both modes so general chat
/// and the settings customizer expose identical machine-side
/// capabilities.
fn agent_tool_declarations() -> Vec<serde_json::Value> {
    vec![
        json!({
            "type": "function",
            "name": "execute_command",
            "description": "Run a shell command on the user's machine via `sh -c`, with a 10 second timeout. Commands matching the user's allowlist run immediately; anything else asks the user for confirmation first. Returns the command's output and exit code.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command line to execute"
                    }
                },
                "required": ["command"]
            }
        }),
        json!({
            "type": "function",
            "name": "schedule_task",
            "description": "Manage recurring tasks backed by systemd user timers. `create` needs name, on_calendar and command; `enable`/`disable`/`delete`/`run_now` need unit; `list` returns all OxideMX tasks as JSON.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "enable", "disable", "delete", "run_now", "list"],
                        "description": "What to do"
                    },
                    "name": {
                        "type": "string",
                        "description": "Human-readable task name (create only)"
                    },
                    "on_calendar": {
                        "type": "string",
                        "description": "systemd OnCalendar expression, e.g. 'daily' or '*-*-* 03:00:00' (create only)"
                    },
                    "command": {
                        "type": "string",
                        "description": "Shell command the task runs (create only)"
                    },
                    "unit": {
                        "type": "string",
                        "description": "Task unit base name from list/create output, e.g. 'oxidemx-task-nightly-backup'"
                    }
                },
                "required": ["action"]
            }
        }),
        json!({
            "type": "function",
            "name": "memory",
            "description": "Persist and recall small facts about the user across conversations. `save` needs text (and optionally scope); `delete`/`pin`/`unpin` need id; `list` returns all entries as JSON. Unpinned memories expire after 90 days of disuse; pinned ones are kept until deleted.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["save", "list", "delete", "pin", "unpin"],
                        "description": "What to do"
                    },
                    "text": {
                        "type": "string",
                        "description": "The fact to remember (save only)"
                    },
                    "scope": {
                        "type": "string",
                        "description": "Grouping label like 'preferences' or 'projects' (save only, defaults to 'general')"
                    },
                    "id": {
                        "type": "string",
                        "description": "Memory id from list/save output (delete/pin/unpin)"
                    }
                },
                "required": ["action"]
            }
        }),
    ]
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

/// Drain complete SSE blocks (separated by a blank line) from `buf`,
/// returning `(event, data)` pairs. Incomplete trailing data stays
/// in the buffer for the next network chunk.
fn split_sse_events(buf: &mut String) -> Vec<(String, String)> {
    let mut events = Vec::new();
    while let Some(pos) = buf.find("\n\n") {
        let block: String = buf.drain(..pos + 2).collect();
        let mut event = String::new();
        let mut data = String::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        if !event.is_empty() || !data.is_empty() {
            events.push((event, data));
        }
    }
    events
}

/// Fold one parsed SSE event into the round outcome, forwarding text
/// deltas to the sink as they arrive.
async fn apply_sse_event(
    event: &str,
    data: &str,
    out: &mut RoundOutcome,
    sink: &Option<StreamSink>,
) {
    match event {
        "interaction.created" | "interaction.completed" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(id) = v["interaction"]["id"].as_str() {
                    out.id = Some(id.to_string());
                }
                if let Some(status) = v["interaction"]["status"].as_str() {
                    out.status = status.to_string();
                }
            }
        }
        "step.start" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                let step = &v["step"];
                if step["type"] == "function_call" {
                    out.calls.push((
                        step["id"].as_str().unwrap_or_default().to_string(),
                        step["name"].as_str().unwrap_or_default().to_string(),
                        step.get("arguments").cloned().unwrap_or(json!({})),
                    ));
                }
            }
        }
        "step.delta" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if v["delta"]["type"] == "text" {
                    if let Some(t) = v["delta"]["text"].as_str() {
                        out.text.push_str(t);
                        if let Some(s) = sink {
                            s.send(StreamEvent::Delta(t.to_string())).await;
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// One request round over SSE. Hard API errors (non-2xx) propagate;
/// a stream that ends without a final status is an error the caller
/// retries via the blocking path.
async fn stream_round(
    client: &reqwest::Client,
    api_key: &str,
    req_body: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<RoundOutcome, Box<dyn std::error::Error + Send + Sync>> {
    use futures_util::StreamExt;

    let mut body = req_body.clone();
    body["stream"] = json!(true);
    let res = client
        .post(INTERACTIONS_URL)
        .header("x-goog-api-key", api_key)
        .json(&body)
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        let error_text = res.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<serde_json::Value>(&error_text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(String::from))
            .unwrap_or(error_text);
        return Err(format!("API error ({status}): {detail}").into());
    }

    let mut out = RoundOutcome::default();
    let mut buf = String::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        for (event, data) in split_sse_events(&mut buf) {
            apply_sse_event(&event, &data, &mut out, sink).await;
        }
    }
    if out.status.is_empty() {
        return Err("SSE stream ended without a final interaction status".into());
    }
    Ok(out)
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

/// Grounded web search via a NESTED, search-only interaction: the
/// Interactions API refuses to mix built-in tools with custom
/// function declarations in one request, so the settings agent
/// declares `google_search` as a custom function and this executor
/// satisfies it with a second interaction that uses Google's
/// built-in search grounding. Same API key, no third-party search
/// service.
async fn grounded_search(query: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let api_key = load_api_key()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let req = json!({
        "model": DEFAULT_MODEL,
        "input": format!(
            "Search the web and summarize current, factual information for this query. \
             Include key facts and source names. Query: {query}"
        ),
        "tools": [{ "type": "google_search" }],
        // One-shot lookup — no need to persist it server-side.
        "store": false,
    });
    let body = post_interaction(&client, &api_key, &req).await?;
    let text = collect_output_text(&body);
    if text.is_empty() {
        Ok("No search results found.".to_string())
    } else {
        Ok(text)
    }
}

// =============================================================================
// LOCAL TOOL EXECUTION ROUTER
// =============================================================================

/// Push a multiple-choice question to the chat UI via `QUESTION_TX`
/// and block the agent turn until the user picks an option. Shared
/// by the model-facing `ask_multiple_choice_question` tool and the
/// `execute_command` off-allowlist confirmation flow.
async fn ask_user_choice(
    question: String,
    options: Vec<String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let tx_opt = QUESTION_TX.lock().unwrap().clone();
    let Some(tx) = tx_opt else {
        return Err("Question channel not initialized".into());
    };
    let (resp_tx, mut resp_rx) = mpsc::channel(1);
    tx.send(PendingQuestion {
        question,
        options,
        response_tx: resp_tx,
    })
    .await?;
    match resp_rx.recv().await {
        Some(answer) => Ok(answer),
        None => Err("Response channel closed".into()),
    }
}

/// Execute one model-requested tool call. `sink` lets executors push
/// live `Activity` labels (with the actual target interpolated) and
/// structured `Card` events into the owning chat thread; the
/// returned string is what goes back to the model as the
/// `function_result`.
async fn execute_local_tool(
    name: &str,
    args: serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match name {
        "execute_command" => execute_command_tool(&args, sink).await,
        "schedule_task" => schedule_task_tool(&args, sink).await,
        "memory" => memory_tool(&args, sink).await,
        "get_menu_config" => {
            let path = get_config_path();
            if !path.exists() {
                let default_bytes = include_str!("../../oxidemx-shared/default-config.json");
                return Ok(default_bytes.to_string());
            }
            let content = tokio::fs::read_to_string(&path).await?;
            Ok(content)
        }
        "set_menu_config" => {
            let config_json = args["config_json"]
                .as_str()
                .ok_or("config_json argument missing or not a string")?;

            // Validate JSON format
            let _: serde_json::Value = serde_json::from_str(config_json)?;
            let path = get_config_path();

            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            // Notify UI
            {
                let tx_opt = CONFIG_CHANGED_TX.lock().unwrap().clone();
                if let Some(tx) = tx_opt {
                    let _ = tx.send(config_json.to_string()).await;
                }
            }

            tokio::fs::write(&path, config_json).await?;
            Ok("Configuration saved successfully".to_string())
        }
        "list_system_apps" => {
            let mut apps = Vec::new();
            let dirs = vec!["/usr/share/applications", "/usr/local/share/applications"];

            let home = std::env::var("HOME").unwrap_or_default();
            let user_apps_dir = format!("{}/.local/share/applications", home);
            let mut all_dirs = dirs;
            if !home.is_empty() {
                all_dirs.push(&user_apps_dir);
            }

            for dir_path in all_dirs {
                let path = std::path::Path::new(&dir_path);
                if !path.exists() {
                    continue;
                }
                let mut entries = tokio::fs::read_dir(path).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    if name_str.ends_with(".desktop") {
                        if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                            let mut name = None;
                            let mut exec = None;
                            let mut icon = None;
                            let mut categories = None;

                            for line in content.lines() {
                                if line.starts_with("Name=") && name.is_none() {
                                    name = Some(line.strip_prefix("Name=").unwrap().to_string());
                                } else if line.starts_with("Exec=") && exec.is_none() {
                                    exec = Some(line.strip_prefix("Exec=").unwrap().to_string());
                                } else if line.starts_with("Icon=") && icon.is_none() {
                                    icon = Some(line.strip_prefix("Icon=").unwrap().to_string());
                                } else if line.starts_with("Categories=") && categories.is_none() {
                                    categories =
                                        Some(line.strip_prefix("Categories=").unwrap().to_string());
                                }
                            }

                            if let (Some(n), Some(e)) = (name, exec) {
                                apps.push(json!({
                                    "name": n,
                                    "exec": e,
                                    "icon": icon.unwrap_or_default(),
                                    "categories": categories.unwrap_or_default()
                                }));
                            }
                        }
                    }
                }
            }
            Ok(serde_json::to_string_pretty(&apps)?)
        }
        "google_search" => {
            let query = args["query"]
                .as_str()
                .ok_or("query argument missing or not a string")?;
            grounded_search(query).await
        }
        "ask_multiple_choice_question" => {
            let question = args["question"]
                .as_str()
                .ok_or("question argument missing or not a string")?;
            let options_val = args["options"]
                .as_array()
                .ok_or("options argument missing or not an array")?;

            let mut options = Vec::new();
            for opt in options_val {
                if let Some(opt_str) = opt.as_str() {
                    options.push(opt_str.to_string());
                }
            }

            ask_user_choice(question.to_string(), options).await
        }
        other => Err(format!("Unknown tool: {}", other).into()),
    }
}

// =============================================================================
// AGENT TOOL EXECUTORS (execute_command / schedule_task / memory)
// =============================================================================

/// Send an `Activity` label to the chat thread, if streaming.
async fn send_activity(sink: &Option<StreamSink>, label: String) {
    if let Some(s) = sink {
        s.send(StreamEvent::Activity(label)).await;
    }
}

/// Send a structured agent card to the chat thread, if streaming.
async fn send_card(sink: &Option<StreamSink>, card: AgentCardData) {
    if let Some(s) = sink {
        s.send(StreamEvent::Card(card)).await;
    }
}

/// `execute_command`: allowlisted commands run straight away,
/// anything else asks the user through the chat's confirmation chip
/// first. Either way the run is surfaced as a Command card and the
/// model receives the (capped) output + exit code.
async fn execute_command_tool(
    args: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let command = args["command"]
        .as_str()
        .ok_or("command argument missing or not a string")?;
    let head = command.split_whitespace().next().unwrap_or(command);
    send_activity(sink, format!("Running {head}…")).await;

    if !crate::agent::commands::is_allowlisted(command, &crate::agent::commands::allowlist()) {
        send_activity(sink, "Waiting for your approval…".to_string()).await;
        let answer = ask_user_choice(
            format!("Run `{command}`?"),
            vec!["Run it".to_string(), "Don't run".to_string()],
        )
        .await?;
        if answer != "Run it" {
            // Tell the model plainly so it doesn't retry the same
            // command or assume it ran.
            return Ok(format!(
                "The user declined to run `{command}`. Do not run it; \
                 ask before proposing an alternative command."
            ));
        }
        send_activity(sink, format!("Running {head}…")).await;
    }

    let (output, exit_code) = crate::agent::commands::run(command).await;
    send_card(
        sink,
        AgentCardData::Command {
            command: command.to_string(),
            stdout: output.clone(),
            exit_code,
        },
    )
    .await;
    Ok(format!("exit code {exit_code}\noutput:\n{output}"))
}

/// Build the Task card payload from a `TaskInfo`.
fn task_card(info: &crate::agent::tasks::TaskInfo) -> AgentCardData {
    AgentCardData::Task {
        name: info.name.clone(),
        unit: info.unit.clone(),
        schedule: info.schedule.clone(),
        next_run: info.next_run.clone(),
        enabled: info.enabled,
    }
}

/// `schedule_task`: thin dispatcher over `agent::tasks`. Mutating
/// actions emit a Task card so the conversation shows the timer's
/// state; `list` feeds plain JSON back to the model only.
async fn schedule_task_tool(
    args: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let action = args["action"]
        .as_str()
        .ok_or("action argument missing or not a string")?;
    // `unit` is shared by every action except create/list.
    let unit = || -> Result<&str, String> {
        args["unit"]
            .as_str()
            .ok_or_else(|| format!("unit argument required for action '{action}'"))
    };

    match action {
        "create" => {
            let name = args["name"]
                .as_str()
                .ok_or("name argument required for create")?;
            let on_calendar = args["on_calendar"]
                .as_str()
                .ok_or("on_calendar argument required for create")?;
            let command = args["command"]
                .as_str()
                .ok_or("command argument required for create")?;
            send_activity(sink, "Scheduling task — writing systemd unit…".to_string()).await;
            let info = crate::agent::tasks::create(name, on_calendar, command)?;
            send_card(sink, task_card(&info)).await;
            Ok(format!("Task created: {}", serde_json::to_string(&info)?))
        }
        "enable" | "disable" => {
            let unit = unit()?;
            let enabled = action == "enable";
            send_activity(
                sink,
                format!("{} task…", if enabled { "Enabling" } else { "Disabling" }),
            )
            .await;
            crate::agent::tasks::set_enabled(unit, enabled)?;
            // Re-read so the card shows the post-change state
            // (enabled flag + refreshed next_run).
            if let Some(info) = crate::agent::tasks::list()
                .into_iter()
                .find(|t| t.unit == unit)
            {
                send_card(sink, task_card(&info)).await;
            }
            Ok(format!(
                "Task '{unit}' {}.",
                if enabled { "enabled" } else { "disabled" }
            ))
        }
        "run_now" => {
            let unit = unit()?;
            send_activity(sink, "Starting task…".to_string()).await;
            crate::agent::tasks::run_now(unit)?;
            Ok(format!("Task '{unit}' started."))
        }
        "delete" => {
            let unit = unit()?;
            send_activity(sink, "Deleting task…".to_string()).await;
            crate::agent::tasks::delete(unit)?;
            Ok(format!("Task '{unit}' deleted."))
        }
        "list" => {
            send_activity(sink, "Listing scheduled tasks…".to_string()).await;
            Ok(serde_json::to_string(&crate::agent::tasks::list())?)
        }
        other => Err(format!("Unknown schedule_task action: {other}").into()),
    }
}

/// `memory`: thin dispatcher over `agent::memory`. Only `save`
/// produces a card (the retention chip); the rest return short
/// confirmations or JSON the model folds into its reply.
async fn memory_tool(
    args: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let action = args["action"]
        .as_str()
        .ok_or("action argument missing or not a string")?;
    let id = || -> Result<&str, String> {
        args["id"]
            .as_str()
            .ok_or_else(|| format!("id argument required for action '{action}'"))
    };

    match action {
        "save" => {
            let text = args["text"]
                .as_str()
                .ok_or("text argument required for save")?;
            let scope = args["scope"].as_str().unwrap_or("general");
            send_activity(sink, "Saving memory…".to_string()).await;
            let entry = crate::agent::memory::save_entry(text, scope);
            send_card(
                sink,
                AgentCardData::Memory {
                    id: entry.id.clone(),
                    text: entry.text.clone(),
                    // Saves are always unpinned; pinning is a
                    // separate action with its own retention.
                    retention: "auto · 90d".to_string(),
                },
            )
            .await;
            Ok(format!("Memory saved with id {}.", entry.id))
        }
        "list" => Ok(serde_json::to_string(&crate::agent::memory::load_all())?),
        "delete" => {
            let id = id()?;
            send_activity(sink, "Deleting memory…".to_string()).await;
            if crate::agent::memory::delete(id) {
                Ok(format!("Memory {id} deleted."))
            } else {
                Ok(format!("No memory with id {id}."))
            }
        }
        "pin" | "unpin" => {
            let id = id()?;
            let pinned = action == "pin";
            send_activity(
                sink,
                format!("{} memory…", if pinned { "Pinning" } else { "Unpinning" }),
            )
            .await;
            if crate::agent::memory::set_pinned(id, pinned) {
                Ok(format!(
                    "Memory {id} {} — retention is now '{}'.",
                    if pinned { "pinned" } else { "unpinned" },
                    if pinned {
                        "until changed"
                    } else {
                        "auto · 90d"
                    }
                ))
            } else {
                Ok(format!("No memory with id {id}."))
            }
        }
        other => Err(format!("Unknown memory action: {other}").into()),
    }
}

#[cfg(test)]
mod sse_tests {
    use super::split_sse_events;

    #[test]
    fn drains_complete_blocks_and_keeps_partials() {
        let mut buf = String::from(
            "event: step.delta\ndata: {\"a\":1}\n\nevent: done\ndata: [DONE]\n\nevent: partial\nda",
        );
        let events = split_sse_events(&mut buf);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], ("step.delta".into(), "{\"a\":1}".into()));
        assert_eq!(events[1], ("done".into(), "[DONE]".into()));
        assert_eq!(buf, "event: partial\nda");
    }

    #[test]
    fn partial_then_completion_across_chunks() {
        let mut buf = String::from("event: x\ndata: {\"t\":");
        assert!(split_sse_events(&mut buf).is_empty());
        buf.push_str("\"hi\"}\n\n");
        let events = split_sse_events(&mut buf);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, "{\"t\":\"hi\"}");
        assert!(buf.is_empty());
    }

    #[test]
    fn multiline_data_joined() {
        let mut buf = String::from("event: e\ndata: line1\ndata: line2\n\n");
        let events = split_sse_events(&mut buf);
        assert_eq!(events[0].1, "line1\nline2");
    }
}
