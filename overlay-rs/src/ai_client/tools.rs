//! Local agent tools: the declarations sent to the model and the
//! executors that satisfy `requires_action` rounds on this machine.

use serde_json::json;
use tokio::sync::mpsc;

use super::{
    collect_output_text, get_config_path, load_api_key, post_interaction, AgentCardData, FlowStep,
    PendingQuestion, StreamEvent, StreamSink, CONFIG_CHANGED_TX, DEFAULT_MODEL, QUESTION_TX,
};

/// Declarations for the local agent tools (`execute_command`,
/// `schedule_task`, `memory`), shared by both modes so general chat
/// and the settings customizer expose identical machine-side
/// capabilities.
pub(super) fn agent_tool_declarations() -> Vec<serde_json::Value> {
    vec![
        json!({
            "type": "function",
            "name": "compose_flow",
            "description": "Author a new multi-agent flow from the user's description and save it (then it's runnable via run_flow / the Agents tab / Mission Control). You write the COMPLETE flow.md content. Format: TOML frontmatter between `---` fences, then a markdown body.\n[flow] id, name, description, version=1\n[inputs] <name> = { type=\"string\", required=true|false, default=\"...\" }  (use in tasks as {{input.<name>}})\n[defaults] model=\"gemini-2.5-flash\", approval=\"allowlist\", max_turns=8\n[[step]] id, agent (a roster id — one of: web-researcher, summarizer, extractor, writer, skeptic), needs=[ids], task=\"...\", context=[\"@artifact@\" (sole predecessor) or \"@step:<id>@\" (a named ancestor)], output=\"debug/x.md\". Optional review step: kind=\"reflect\", target=\"<step>\", critic=\"skeptic\", max_rounds=2. Optional branch: kind=\"route\", needs=[...], choices={ label=\"<step-id>\" }, prompt=\"...\".\n[delivery] root=\"ANSWER.md\", title=\"...\"\nThe tool validates after writing and returns any errors — if invalid, call it again with the corrected flow.md.",
            "parameters": {
                "type": "object",
                "properties": {
                    "flow_id": {
                        "type": "string",
                        "description": "A short id for the flow (becomes a dir slug, e.g. 'release-watch')"
                    },
                    "flow_md": {
                        "type": "string",
                        "description": "The complete flow.md content (TOML frontmatter + markdown body)"
                    }
                },
                "required": ["flow_id", "flow_md"]
            }
        }),
        json!({
            "type": "function",
            "name": "run_flow",
            "description": "Run a multi-agent OxideMX flow (a named, pre-authored pipeline of agent steps) via the conductor. Use when the user asks to run a flow by name, or for a multi-step task a flow exists for (e.g. 'research-digest' to fetch+digest+answer a URL). List available flows is out of scope — the user knows the flow id. Streams live per-step progress and returns a summary; a card with a 'Watch' button to Mission Control appears in chat.",
            "parameters": {
                "type": "object",
                "properties": {
                    "flow_id": {
                        "type": "string",
                        "description": "The flow id to run (a directory under ~/.config/oxidemx/flows/)"
                    },
                    "inputs_json": {
                        "type": "string",
                        "description": "Optional JSON object of input values keyed by name, e.g. {\"url\": \"https://…\"}. Declared flow defaults apply for anything omitted."
                    },
                    "mock": {
                        "type": "boolean",
                        "description": "Run with the deterministic mock provider (no LLM calls) for a dry run. Default false (real run)."
                    }
                },
                "required": ["flow_id"]
            }
        }),
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
        json!({
            "type": "function",
            "name": "persona",
            "description": "Read or rewrite the user-editable persona files: soul.md (your identity, tone, values — first person) and user.md (durable facts about the user). `read` returns both files. `write_soul`/`write_user` REPLACE the whole file with `content` — include everything that should remain. Use sparingly: on the first-run ritual, or when the user asks to change your personality or correct their profile.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "write_soul", "write_user"]
                    },
                    "content": {
                        "type": "string",
                        "description": "Full replacement markdown for write_* actions"
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
pub(crate) async fn execute_local_tool(
    name: &str,
    args: serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match name {
        "compose_flow" => compose_flow_tool(&args, sink).await,
        "run_flow" => run_flow_tool(&args, sink).await,
        "execute_command" => execute_command_tool(&args, sink).await,
        "schedule_task" => schedule_task_tool(&args, sink).await,
        "memory" => memory_tool(&args, sink).await,
        "persona" => persona_tool(&args, sink).await,
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

/// Sanitize a model-chosen flow id into a safe directory slug.
fn slugify_flow_id(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// `compose_flow`: write a model-authored flow.md to the flows dir and
/// validate it via the conductor, returning the result so the model can
/// fix-and-retry. Authoring (not running) — the user runs it after.
async fn compose_flow_tool(
    args: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let id = slugify_flow_id(
        args["flow_id"]
            .as_str()
            .ok_or("compose_flow: `flow_id` missing")?,
    );
    if id.is_empty() {
        return Ok("The flow_id must contain letters or numbers.".into());
    }
    let flow_md = args["flow_md"]
        .as_str()
        .ok_or("compose_flow: `flow_md` missing")?;

    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::path::Path::new(&home)
        .join(".config/oxidemx/flows")
        .join(&id);
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join("flow.md"), flow_md).await?;

    send_activity(sink, format!("Composing flow “{id}”…")).await;

    let out = tokio::process::Command::new("oxidemx-conductor")
        .args(["validate", &id])
        .output()
        .await
        .map_err(|e| format!("could not run the conductor validator: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    if out.status.success() {
        Ok(format!(
            "Flow '{id}' saved and VALID — {}. The user can run it now (run_flow / the Agents tab / Mission Control), or refine it in Settings → Agents.",
            stdout.trim()
        ))
    } else {
        Ok(format!(
            "Flow '{id}' was saved but is INVALID:\n{}\nRevise the flow.md and call compose_flow again with the corrected content.",
            stderr.trim()
        ))
    }
}

/// Record/overwrite a step's status in the running flow-card model.
fn set_flow_status(steps: &mut Vec<FlowStep>, step: &str, status: &str) {
    if let Some(s) = steps.iter_mut().find(|s| s.step == step) {
        s.status = status.to_string();
    } else {
        steps.push(FlowStep {
            step: step.to_string(),
            status: status.to_string(),
        });
    }
}

/// `run_flow`: launch a conductor flow as a subprocess and stream its
/// run-layer events (JSON lines) into the chat — live `Activity` per
/// step, a final `Flow` card, and a text summary back to the model.
///
/// Shelling the installed `oxidemx-conductor` keeps the heavy
/// conductor/toolkit out of the overlay binary and isolates the run
/// (its own LLM calls, fs writes) in a child process.
async fn run_flow_tool(
    args: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::AsyncBufReadExt;

    let flow_id = args["flow_id"]
        .as_str()
        .ok_or("run_flow: `flow_id` argument missing or not a string")?
        .to_string();
    let mock = args.get("mock").and_then(serde_json::Value::as_bool).unwrap_or(false);

    let mut cmd_args = vec!["run".to_string(), flow_id.clone()];
    if mock {
        cmd_args.push("--mock".to_string());
    }
    // Inputs arrive as a JSON-object string (Gemini's function schema
    // can't express an open-keyed object directly).
    if let Some(obj) = args
        .get("inputs_json")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .as_ref()
        .and_then(serde_json::Value::as_object)
    {
        for (k, v) in obj {
            let val = v
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string());
            cmd_args.push("--input".to_string());
            cmd_args.push(format!("{k}={val}"));
        }
    }

    send_activity(sink, format!("Running flow “{flow_id}”…")).await;

    let mut child = tokio::process::Command::new("oxidemx-conductor")
        .args(&cmd_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!("could not launch the conductor (is `oxidemx-conductor` on PATH?): {e}")
        })?;

    let stdout = child.stdout.take().ok_or("run_flow: no stdout from conductor")?;
    let mut lines = tokio::io::BufReader::new(stdout).lines();

    let mut steps: Vec<FlowStep> = Vec::new();
    let mut artifacts: Vec<String> = Vec::new();
    let mut run_id = String::new();
    let mut success = false;
    let mut fail_reason: Option<String> = None;

    while let Some(line) = lines.next_line().await? {
        let Ok(ev) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let kind = ev["kind"].as_str().unwrap_or("");
        let step = ev["step"].as_str().unwrap_or("");
        match kind {
            "run_started" => {
                run_id = ev["run_id"].as_str().unwrap_or("").to_string();
                if let Some(arr) = ev["steps"].as_array() {
                    for s in arr.iter().filter_map(serde_json::Value::as_str) {
                        set_flow_status(&mut steps, s, "pending");
                    }
                }
            }
            "task_started" => {
                set_flow_status(&mut steps, step, "running");
                send_activity(sink, format!("Flow “{flow_id}” · {step}…")).await;
            }
            "task_finished" => {
                set_flow_status(&mut steps, step, "done");
                if let Some(a) = ev["artifact"].as_str() {
                    artifacts.push(a.to_string());
                }
            }
            "task_error" => set_flow_status(&mut steps, step, "failed"),
            "step_skipped" => set_flow_status(&mut steps, step, "skipped"),
            "step_retrying" => {
                send_activity(sink, format!("Flow “{flow_id}” · retrying {step}…")).await
            }
            "run_finished" => {
                success = true;
                if let Some(arr) = ev["artifacts"].as_array() {
                    artifacts = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
            }
            "run_failed" => {
                success = false;
                fail_reason = ev["reason"].as_str().map(String::from);
            }
            _ => {}
        }
    }
    let _ = child.wait().await;

    send_card(
        sink,
        AgentCardData::Flow {
            flow_id: flow_id.clone(),
            run_id: run_id.clone(),
            success,
            steps: steps.clone(),
            artifacts: artifacts.clone(),
        },
    )
    .await;

    let done = steps.iter().filter(|s| s.status == "done").count();
    if success {
        Ok(format!(
            "Flow '{flow_id}' completed: {done}/{} steps done, {} artifact(s) [{}] (run {run_id}).",
            steps.len(),
            artifacts.len(),
            artifacts.join(", "),
        ))
    } else {
        Ok(format!(
            "Flow '{flow_id}' failed{}. {done}/{} steps finished first.",
            fail_reason.map(|r| format!(": {r}")).unwrap_or_default(),
            steps.len(),
        ))
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
        // "Always allow" suggests the first two tokens as a prefix
        // pattern ("git status", "systemctl --user") — broad enough
        // to kill repeat prompts, narrow enough not to hand over
        // the whole binary. The Claude Code approval-card detail.
        let suggest: String = command
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let always = format!("Always allow `{suggest} …`");
        let answer = ask_user_choice(
            format!("Run `{command}`?"),
            vec![
                "Run it".to_string(),
                always.clone(),
                "Don't run".to_string(),
            ],
        )
        .await?;
        if answer == always {
            if let Err(e) = crate::agent::commands::add_allowlist_entry(&suggest) {
                tracing::warn!("failed to persist allowlist entry: {e}");
            }
        } else if answer != "Run it" {
            // Tell the model plainly so it doesn't retry the same
            // command or assume it ran. If the user explains their
            // refusal in the next message, treat that as corrective
            // context.
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

/// `persona`: read/rewrite soul.md + user.md. Writes are full-file
/// replaces (capped in `agent::persona`); the files are also
/// user-editable on disk, so reads always reflect the latest text.
async fn persona_tool(
    args: &serde_json::Value,
    sink: &Option<StreamSink>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let action = args["action"]
        .as_str()
        .ok_or("action argument missing or not a string")?;
    match action {
        "read" => {
            let soul = std::fs::read_to_string(crate::agent::persona::soul_path())
                .unwrap_or_else(|_| "(soul.md does not exist yet)".to_string());
            let user = std::fs::read_to_string(crate::agent::persona::user_path())
                .unwrap_or_else(|_| "(user.md does not exist yet)".to_string());
            Ok(format!("--- soul.md ---\n{soul}\n--- user.md ---\n{user}"))
        }
        "write_soul" | "write_user" => {
            let content = args["content"]
                .as_str()
                .ok_or("content argument required for write actions")?;
            let which = if action == "write_soul" {
                "soul"
            } else {
                "user"
            };
            send_activity(sink, format!("Writing {which}.md…")).await;
            let path = crate::agent::persona::write_file(which, content)?;
            Ok(format!("{} written.", path.display()))
        }
        other => Err(format!("Unknown persona action: {other}").into()),
    }
}
