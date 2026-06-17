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
    /// A chunk of the model's text reply, in order. Not produced in
    /// the current non-streaming runtime (the reply arrives complete
    /// via `AiResponseReceived`); the variant + its UI scaffolding are
    /// kept so token streaming can be re-added without rewiring.
    #[allow(dead_code)]
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
    /// `run_flow` ran a conductor flow. Live per-step progress streams
    /// as `Activity` while it runs; this card is the final summary,
    /// with a "Watch" chip that opens Mission Control on the flow.
    Flow {
        flow_id: String,
        run_id: String,
        success: bool,
        steps: Vec<FlowStep>,
        artifacts: Vec<String>,
    },
}

/// One step's terminal status inside a `Flow` card.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlowStep {
    pub step: String,
    /// `pending` | `running` | `done` | `failed` | `skipped`.
    pub status: String,
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

/// A markdown list of the user's available flows (id · name —
/// description), scanned from `~/.config/oxidemx/flows/<id>/flow.md`.
/// `None` when there are no flows. Lightweight frontmatter parse — no
/// conductor dependency (the overlay shells the conductor to run them).
/// Scan `~/.config/oxidemx/flows/<id>/flow.md`, returning `(id, name,
/// description)` for each, sorted by id. Lightweight frontmatter parse —
/// no conductor dependency (the overlay shells the conductor to run them).
fn scan_flows() -> Vec<(String, String, String)> {
    let Ok(home) = std::env::var("HOME") else {
        return Vec::new();
    };
    let dir = std::path::Path::new(&home).join(".config/oxidemx/flows");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, String, String)> = Vec::new();
    for e in rd.flatten() {
        let md = e.path().join("flow.md");
        let Ok(src) = std::fs::read_to_string(&md) else {
            continue;
        };
        // Only scan the frontmatter (between the first two `---` fences).
        let front = src.split("---").nth(1).unwrap_or(&src);
        let field = |key: &str| -> Option<String> {
            front.lines().find_map(|l| {
                let l = l.trim();
                l.strip_prefix(key)
                    .and_then(|r| r.trim().strip_prefix('='))
                    .map(|v| v.trim().trim_matches('"').to_string())
                    .filter(|v| !v.is_empty())
            })
        };
        let id = field("id").unwrap_or_else(|| e.file_name().to_string_lossy().to_string());
        let name = field("name").unwrap_or_else(|| id.clone());
        let desc = field("description").unwrap_or_default();
        entries.push((id, name, desc));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// `(id, name)` of the user's flows — for the slash palette.
pub fn list_flows() -> Vec<(String, String)> {
    scan_flows()
        .into_iter()
        .map(|(id, name, _)| (id, name))
        .collect()
}

fn available_flows_block() -> Option<String> {
    let entries = scan_flows();
    if entries.is_empty() {
        return None;
    }
    let mut block = String::from(
        "AVAILABLE FLOWS (the user's pre-authored pipelines — run by id with run_flow; \
         offer these when relevant, and list them if asked what you can do):\n",
    );
    for (id, name, desc) in entries {
        block.push_str(&format!("- `{id}` — {name}: {desc}\n"));
    }
    Some(block)
}

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
    /// One unified agentic assistant: conversation + web search,
    /// memory/persona, shell, task scheduling, radial-menu config, and
    /// multi-agent flows. Replaces the old General / Menu Setup split
    /// (old persisted thread modes deserialize here via the aliases).
    #[serde(alias = "general_chat", alias = "settings_customizer")]
    Agentic,
}

impl AgentMode {
    /// Short label for the chat shell.
    pub fn label(&self) -> &'static str {
        "Agentic"
    }

    /// Number of tools armed for this mode — surfaced in the chat
    /// header's status line ("· N tools armed").
    pub fn tool_count(&self) -> usize {
        self.tools().len()
    }

    /// Persona half of the system instruction (base prompt + soul.md
    /// + first-run ritual + user.md), WITHOUT the memory block.
    fn system_instruction_base(&self) -> String {
        let base = "You are OxideMX-AI, the user's agentic desktop assistant. You hold a \
             natural conversation AND act on the machine through your tools — answer questions, \
             query the web for real-time facts, run shell commands, schedule recurring tasks, \
             remember durable facts, configure the OxideMX radial menu, and launch multi-agent \
             flows. Reach for a tool whenever it gets a better, grounded result; otherwise just \
             reply. Keep responses concise and format them in markdown.\n\n\
             RADIAL MENU CONFIG\n\
             You can read/modify the active layout via get_menu_config / set_menu_config. When \
             editing, slice `icon` fields MUST be icon names that actually exist: a standard \
             Adwaita/freedesktop symbolic name (e.g. utilities-terminal-symbolic, \
             system-run-symbolic, web-browser-symbolic, preferences-system-symbolic) or an Icon= \
             value from list_system_apps. NEVER invent icon names (a nonexistent name renders \
             blank); prefer list_system_apps for real exec + icon when binding launchers. Use \
             ask_multiple_choice_question when options need clarifying.\n\n\
             FILES\n\
             You can read the filesystem: read_file (text), list_dir, search_file (by pattern), and \
             parse_document (PDF/DOCX/XLSX/HTML — rich formats). Flow runs write their artifacts to \
             ~/.local/share/oxidemx/runs/<flow>-<timestamp>/ (the final answer is usually ANSWER.md, \
             intermediates under debug/) — read them there when the user asks about a flow's output.\n\n\
             FLOWS\n\
             For multi-step work a pre-authored flow covers, call run_flow with its id. When the \
             user wants a NEW repeatable pipeline ('every morning fetch X, digest it, …'), author \
             it with compose_flow (you write the flow.md; it validates and tells you any errors to \
             fix). run_flow streams live progress and returns a summary.\n\n\
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
             you can't see.";
        // Persona files stay keyed "general" — the single agent inherits
        // the existing soul.md/user.md, no migration needed.
        let mode_key = "general";
        let mut full = base.to_string();
        // Make the agent AWARE of the user's actual flows (ids + what
        // they do) so it can run/recommend them by name without the
        // user knowing exact ids — and answer "what can you do".
        if let Some(flows) = available_flows_block() {
            full.push_str("\n\n");
            full.push_str(&flows);
        }
        // soul.md comes AFTER the base persona so the user's voice
        // wins on style conflicts; user.md after the memory rules
        // (it's context, not instruction).
        if let Some(soul) = crate::agent::persona::soul_block(mode_key) {
            full.push_str(
                "\n\nPERSONA (user-authored soul.md — this overrides the default voice):\n",
            );
            full.push_str(&soul);
        } else if crate::agent::persona::needs_bootstrap() {
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
        full
    }

    /// System instruction with **hybrid lexical + semantic** memory
    /// recall — the interactive chat path. Falls back to lexical
    /// internally if embeddings are unavailable (see
    /// `memory::injection_block_for_async`).
    pub async fn system_instruction_async(&self, query: &str) -> String {
        let base = self.system_instruction_base();
        match crate::agent::memory::injection_block_for_async(query).await {
            Some(block) => format!("{base}\n\n{block}"),
            None => base,
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

        // One agent → the union of every capability.
        {
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
            // + run_flow/execute_command/schedule_task/memory/persona
            // (search_fn is already in the vec above).
            tools.extend(agent_tool_declarations());
            tools
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
) -> Result<(String, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    crate::agent_runtime::run(mode, model, prompt, sink, history).await
}
