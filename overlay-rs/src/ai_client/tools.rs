//! Local agent tools: the declarations sent to the model and the
//! executors that satisfy `requires_action` rounds on this machine.

use serde_json::json;
use tokio::sync::mpsc;

use super::{
    collect_output_text, get_config_path, load_api_key, post_interaction, AgentCardData,
    PendingQuestion, StreamEvent, StreamSink, CONFIG_CHANGED_TX, DEFAULT_MODEL, QUESTION_TX,
};

/// Human label for a tool the agent is about to run. Generic
/// fallback per tool — the executors emit more specific labels once
/// they've parsed their arguments (e.g. "Running brightnessctl…").
pub(super) fn activity_for_tool(name: &str) -> &'static str {
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

/// Declarations for the local agent tools (`execute_command`,
/// `schedule_task`, `memory`), shared by both modes so general chat
/// and the settings customizer expose identical machine-side
/// capabilities.
pub(super) fn agent_tool_declarations() -> Vec<serde_json::Value> {
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
            "description": "Persist and recall small facts about the user across conversations. `save` needs text (and optionally scope); `delete`/`pin`/`unpin` need id; `list` returns all entries as JSON; `search` needs query and returns the 5 most relevant entries; `consolidate` merges duplicates and distils stale entries (use when the user asks to tidy/optimise memories). Unpinned memories expire after 90 days of disuse; pinned ones are kept until deleted.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["save", "list", "delete", "pin", "unpin", "search", "consolidate"],
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
pub(super) async fn execute_local_tool(
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
                let default_bytes = include_str!("../../../oxidemx-shared/default-config.json");
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
        "search" => {
            let query = args["query"]
                .as_str()
                .or_else(|| args["text"].as_str())
                .ok_or("query argument required for search")?;
            Ok(serde_json::to_string(&crate::agent::memory::search(query))?)
        }
        "consolidate" => {
            send_activity(sink, "Consolidating memories…".to_string()).await;
            consolidate_memories().await
        }
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

/// Consolidation ("dreaming"): one nested flash-tier interaction
/// rewrites the unpinned store — merge duplicates, resolve
/// contradictions (newest wins), generalise clusters, expire
/// time-bound leftovers. `agent::memory::apply_consolidation` owns
/// the safety rails (pinned excluded, full id ledger required,
/// shrink floor, archive tombstones, .bak backup); a plan that
/// fails validation leaves the store untouched.
pub async fn consolidate_memories() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let input = crate::agent::memory::consolidation_input();
    if input.len() < 6 {
        return Ok("Memory store is small and tidy; nothing to consolidate.".to_string());
    }
    let api_key = load_api_key()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let entries_json = serde_json::to_string(&input)?;
    let req = json!({
        "model": "gemini-2.5-flash",
        "input": format!(
            "You are a memory consolidator for a desktop assistant. Below is a JSON array of \
             saved memory entries (id, text, scope, timestamps). Rewrite the store:\n\
             1. MERGE duplicates/near-duplicates into one entry (keep the most specific wording).\n\
             2. Resolve contradictions by keeping the newest fact.\n\
             3. GENERALISE clusters of related entries into one durable fact only when no \
             decision-relevant detail is lost.\n\
             4. EXPIRE entries that are clearly time-bound and past their window.\n\
             5. Keep everything else untouched. Trimming 10-30% is normal; never more than half.\n\
             Reply with ONLY a JSON object, no prose, no code fences:\n\
             {{\"ledger\": {{\"<every input id>\": \"keep|merged|superseded|expired\"}}, \
             \"entries\": [{{\"text\": \"...\", \"scope\": \"...\"}}]}}\n\
             where entries are ONLY the new merged/generalised facts (not the kept ones).\n\n\
             {entries_json}"
        ),
        "store": false,
    });
    let body = post_interaction(&client, &api_key, &req).await?;
    let text = collect_output_text(&body);
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let plan: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|e| format!("consolidation plan was not valid JSON: {e}"))?;
    crate::agent::memory::apply_consolidation(&plan).map_err(Into::into)
}

/// Boot-time hook: run a consolidation pass when the store is due
/// (size or age trigger) — app start is the closest thing a desktop
/// overlay has to idle time, and it never collides with an active
/// conversation. Failures are logged and ignored; the store's rails
/// guarantee nothing is lost.
pub async fn auto_consolidate_if_due() {
    if !crate::agent::memory::consolidation_due() {
        return;
    }
    match consolidate_memories().await {
        Ok(summary) => tracing::info!("memory auto-consolidation: {summary}"),
        Err(e) => tracing::warn!("memory auto-consolidation skipped: {e}"),
    }
}
