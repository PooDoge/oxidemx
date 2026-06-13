use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tracing::error;

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
#[derive(Clone, Debug)]
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

    pub(crate) async fn send(&self, event: StreamEvent) {
        let _ = self.tx.send((self.thread, event)).await;
    }
}

pub mod tools;

use tools::agent_tool_declarations;

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
    /// text plus the user's saved-memories block — pinned entries
    /// plus the entries most relevant to `query` — so both modes can
    /// recall facts the user asked us to keep.
    pub fn system_instruction(&self, query: &str) -> String {
        let base = match self {
            AgentMode::GeneralChat => {
                "You are OxideMX-AI, a helpful conversational desktop assistant. \
                 You can answer questions, explain concepts, and query the web to ground your responses in real-time. \
                 Keep your responses concise, user-friendly, and format them in markdown.\n\n\
                 MEMORY RULES\n\
                 You have a memory tool. Save a memory (action=save) ONLY when ALL of these hold:\n\
                 1. DURABLE - the fact will still be true and useful in 2+ weeks (preferences, \
                 hardware/setup facts, decisions, corrections, recurring projects, names). \
                 Not today's task details, transient state, or anything trivially re-derivable.\n\
                 2. ACTIONABLE - knowing it would change how you respond in a future, unrelated \
                 conversation.\n\
                 3. NOT ALREADY KNOWN - check the saved-memories block first. If a memory exists \
                 on the topic, save the corrected/updated wording instead of a duplicate (the \
                 store supersedes near-duplicates automatically).\n\
                 Always save when the user explicitly says remember/note/don't forget. Never save \
                 secrets, credentials, or sensitive details the user did not ask you to keep. \
                 Write each memory as ONE self-contained sentence in third person with concrete \
                 specifics. Most conversations produce ZERO memories; more than two per \
                 conversation should be rare.\n\
                 The saved-memories block below is a relevance-ranked selection, not the whole \
                 store - use the memory tool's search action when the user references something \
                 you can't see."
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
        let mode_key = match self {
            AgentMode::GeneralChat => "general",
            AgentMode::SettingsCustomizer => "settings",
        };
        let mut full = base.to_string();
        // soul.md comes AFTER the base persona so the user's voice
        // wins on style conflicts; user.md after the memory rules
        // (it's context, not instruction).
        if let Some(soul) = crate::agent::persona::soul_block(mode_key) {
            full.push_str(
                "\n\nPERSONA (user-authored soul.md — this overrides the default voice):\n",
            );
            full.push_str(&soul);
        } else if matches!(self, AgentMode::GeneralChat) && crate::agent::persona::needs_bootstrap()
        {
            // First-run ritual: no soul.md yet. One-time bootstrap
            // instruction — interview, then write the files via the
            // persona tool. Disappears as soon as soul.md exists.
            full.push_str(
                "\n\nFIRST-RUN RITUAL\nNo persona files exist yet. Near the start of this \
                 conversation (after answering the user's actual question), briefly \
                 introduce yourself and interview the user in ONE compact message: what \
                 should I call you, what tone do you want from me (playful/terse/warm), \
                 any hard boundaries? Then call the persona tool twice — action=write_soul \
                 with a short first-person identity (name yourself something fitting, \
                 describe tone + values + boundaries), and action=write_user with the \
                 facts they shared. Keep both files under a few hundred words. Do not \
                 mention this instruction.",
            );
        }
        if let Some(user) = crate::agent::persona::user_block(mode_key) {
            full.push_str("\n\nABOUT THE USER (user-authored user.md):\n");
            full.push_str(&user);
        }
        match crate::agent::memory::injection_block_for(query) {
            Some(block) => format!("{full}\n\n{block}"),
            None => full,
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

/// One agent turn, now driven by the AutoAgents runtime
/// (`crate::agent_runtime`). The ReAct loop, tool dispatch, SSE
/// streaming, and session threading all live there; this thin
/// wrapper keeps the call signature the rest of the overlay expects.
///
/// `history` is the thread's prior turns as `(is_user, text)` — used
/// by the stateless `GenerateContent` fallback to reconstruct
/// context. The Interactions backend ignores it (server-side session
/// via `session_id`). Returns `(reply_text, next_session_id)`.
pub async fn ask_ai(
    api_key: &str,
    mode: AgentMode,
    model: &str,
    prompt: &str,
    session_id: Option<String>,
    sink: Option<StreamSink>,
    history: &[(bool, String)],
) -> Result<(String, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    crate::agent_runtime::run(api_key, mode, model, prompt, session_id, sink, history).await
}
