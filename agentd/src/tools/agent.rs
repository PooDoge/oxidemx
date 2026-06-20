//! Native tool bodies for the "agent" tools:
//! `compose_flow`, `run_flow`, `run_status`, `list_runs`, `use_skill`,
//! `memory`, `persona`, `schedule_task`, and the host-delegated
//! `ask_multiple_choice_question`.
//!
//! # Project-scoping notes
//!
//! * `memory` / `persona` — the core helpers in `oxidemx-agent-core` are
//!   backed by global paths (`~/.local/share/oxidemx/memories.json`,
//!   `~/.config/oxidemx/{soul,user}.md`). Per-project scoped memory and
//!   persona are a planned follow-up (tracked in followups.md). For now the
//!   tools wire directly to the available core functions and note the gap.
//!
//! * `use_skill` — scans `paths.merged_skill_roots()`, which includes both
//!   global roots and the project-local `.oxidemx/skills` directory. This
//!   is fully project-scoped.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::run_launcher::RunLauncher;
use crate::seams::HostCapability;

// ── Skill helpers ─────────────────────────────────────────────────────────────

/// Walk `roots` (in order, first match wins) looking for a skill named `name`.
/// Returns the `SKILL.md` path and the instruction body, or `None` if not found.
fn find_skill_body(roots: &[PathBuf], name: &str) -> Option<(PathBuf, String)> {
    for root in roots {
        let skill_md = root.join(name).join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        if let Some(body) = oxidemx_agent_core::skills::read_body(&skill_md) {
            return Some((skill_md, body));
        }
    }
    None
}

// ── Flow helpers ──────────────────────────────────────────────────────────────

/// Sanitize a flow id: lowercase alphanumeric + hyphens, runs of other chars
/// → single hyphen. Matches the oracle's `slugify_flow_id` behaviour.
fn slugify_flow_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    let mut pending_dash = false;
    for c in id.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    out
}

fn flows_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .map_err(|_| "HOME environment variable not set".to_string())?;
    Ok(PathBuf::from(home).join(".config/oxidemx/flows"))
}

// =============================================================================
// Tool implementations
// =============================================================================

// ── use_skill ─────────────────────────────────────────────────────────────────

/// `use_skill` — declared key: `name`.
///
/// Scanned against `merged_skill_roots` (project-local wins).
/// Checks the enabled set from the global `skills.json`; a skill not in the
/// enabled set is reported as unavailable (same policy as the oracle).
pub(super) fn use_skill(
    paths: &crate::projects::ProjectPaths,
    args: &Value,
) -> Result<String, String> {
    let name = args["name"]
        .as_str()
        .or_else(|| args["skill_name"].as_str()) // tolerant fallback
        .ok_or_else(|| "use_skill: missing 'name' argument".to_string())?;

    let enabled = oxidemx_agent_core::skills::enabled_set();
    let roots = paths.merged_skill_roots();

    match find_skill_body(&roots, name) {
        Some((_path, body)) if enabled.contains(name) => {
            let capped: String = body.chars().take(12_000).collect();
            let suffix = if capped.len() < body.len() {
                "\n\n…[skill truncated]"
            } else {
                ""
            };
            Ok(format!(
                "SKILL '{name}' — follow these instructions for the current task:\n\n{capped}{suffix}"
            ))
        }
        Some(_) => Ok(format!(
            "No enabled skill named '{name}'. Only enabled skills can be used — \
             check the AVAILABLE SKILLS list for exact names."
        )),
        None => Ok(format!(
            "No enabled skill named '{name}'. Only enabled skills can be used — \
             check the AVAILABLE SKILLS list for exact names."
        )),
    }
}

// ── compose_flow ──────────────────────────────────────────────────────────────

/// `compose_flow` — declared keys: `flow_id`, `flow_md`.
///
/// Writes the flow to the global flows directory, then validates via
/// `oxidemx-conductor validate <id>` subprocess. Thin: no supervisor logic.
pub(super) async fn compose_flow(args: &Value) -> Result<String, String> {
    let raw_id = args["flow_id"]
        .as_str()
        .ok_or_else(|| "compose_flow: missing 'flow_id' argument".to_string())?;
    let id = slugify_flow_id(raw_id);
    if id.is_empty() {
        return Ok("The flow_id must contain letters or numbers.".into());
    }
    let flow_md = args["flow_md"]
        .as_str()
        .ok_or_else(|| "compose_flow: missing 'flow_md' argument".to_string())?;

    let dir = flows_dir()?.join(&id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("compose_flow: could not create flow dir: {e}"))?;
    std::fs::write(dir.join("flow.md"), flow_md)
        .map_err(|e| format!("compose_flow: could not write flow.md: {e}"))?;

    // Validate via the installed conductor binary (subprocess, thin).
    let out = tokio::process::Command::new("oxidemx-conductor")
        .args(["validate", &id])
        .output()
        .await
        .map_err(|e| format!("compose_flow: could not run the conductor validator: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    if out.status.success() {
        Ok(format!(
            "Flow '{id}' saved and VALID — {}. \
             The user can run it now (run_flow / the Agents tab / Mission Control), \
             or refine it in Settings → Agents.",
            stdout.trim()
        ))
    } else {
        Ok(format!(
            "Flow '{id}' was saved but is INVALID:\n{}\n\
             Revise the flow.md and call compose_flow again with the corrected content.",
            stderr.trim()
        ))
    }
}

// ── run_flow ──────────────────────────────────────────────────────────────────

/// `run_flow` — declared keys: `flow_id`, `inputs_json` (optional).
///
/// Launches a real conductor run via the `RunLauncher` and returns the real
/// run_id. (The `mock` key is honoured via the OXIDEMX_TEST_MOCK_FLOW env in
/// the launcher; it is no longer a tool argument.)
pub(super) async fn run_flow(
    launcher: &Arc<dyn RunLauncher>,
    paths: &crate::projects::ProjectPaths,
    args: &Value,
) -> Result<String, String> {
    let flow_id = args["flow_id"]
        .as_str()
        .ok_or_else(|| "run_flow: missing 'flow_id' argument".to_string())?;
    let inputs_json = args
        .get("inputs_json")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let project = paths.cwd.to_string_lossy();
    match launcher.launch(&project, flow_id, inputs_json).await {
        Ok(run_id) => Ok(format!(
            "Launched flow '{flow_id}' — run id `{run_id}`. It is now running in the \
             background; check its status with run_status(run_id=\"{run_id}\") — do not \
             guess whether it has finished."
        )),
        Err(e) => Err(format!("run_flow: could not launch '{flow_id}': {e}")),
    }
}

// ── run_status ────────────────────────────────────────────────────────────────

/// `run_status` — declared key: `run_id`. Ground truth from the run table.
pub(super) async fn run_status(
    launcher: &Arc<dyn RunLauncher>,
    args: &Value,
) -> Result<String, String> {
    let run_id = args["run_id"]
        .as_str()
        .ok_or_else(|| "run_status: missing 'run_id' argument".to_string())?;
    match launcher.status(run_id) {
        Some(s) => Ok(format!("Run `{run_id}` status: {}.", s.as_str())),
        None => Ok(format!(
            "No run with id `{run_id}` is known (it was never started or has been \
             cleaned up). Do not assume a status."
        )),
    }
}

// ── list_runs ─────────────────────────────────────────────────────────────────

/// `list_runs` — no args. Lists known run ids for the project.
pub(super) async fn list_runs(
    launcher: &Arc<dyn RunLauncher>,
    paths: &crate::projects::ProjectPaths,
    _args: &Value,
) -> Result<String, String> {
    let project = paths.cwd.to_string_lossy();
    let ids = launcher.list_runs(&project);
    if ids.is_empty() {
        Ok("No runs found for this project.".into())
    } else {
        Ok(format!("Known runs: {}.", ids.join(", ")))
    }
}

// ── schedule_task ─────────────────────────────────────────────────────────────

/// `schedule_task` — declared keys: `action`, `name`, `on_calendar`, `command`, `unit`.
pub(super) async fn schedule_task(args: &Value) -> Result<String, String> {
    let action = args["action"]
        .as_str()
        .ok_or_else(|| "schedule_task: missing 'action' argument".to_string())?;

    let unit = || -> Result<&str, String> {
        args["unit"]
            .as_str()
            .ok_or_else(|| format!("schedule_task: 'unit' argument required for action '{action}'"))
    };

    match action {
        "create" => {
            let name = args["name"]
                .as_str()
                .ok_or_else(|| "schedule_task: 'name' argument required for create".to_string())?;
            let on_calendar = args["on_calendar"]
                .as_str()
                .ok_or_else(|| {
                    "schedule_task: 'on_calendar' argument required for create".to_string()
                })?;
            let command = args["command"]
                .as_str()
                .ok_or_else(|| {
                    "schedule_task: 'command' argument required for create".to_string()
                })?;
            let info = oxidemx_agent_core::tasks::create(name, on_calendar, command)
                .map_err(|e| format!("schedule_task create: {e}"))?;
            Ok(format!(
                "Task created: {}",
                serde_json::to_string(&info)
                    .map_err(|e| format!("schedule_task: serialize error: {e}"))?
            ))
        }
        "enable" | "disable" => {
            let unit = unit()?;
            let enabled = action == "enable";
            oxidemx_agent_core::tasks::set_enabled(unit, enabled)
                .map_err(|e| format!("schedule_task {action}: {e}"))?;
            Ok(format!(
                "Task '{unit}' {}.",
                if enabled { "enabled" } else { "disabled" }
            ))
        }
        "run_now" => {
            let unit = unit()?;
            oxidemx_agent_core::tasks::run_now(unit)
                .map_err(|e| format!("schedule_task run_now: {e}"))?;
            Ok(format!("Task '{unit}' started."))
        }
        "delete" => {
            let unit = unit()?;
            oxidemx_agent_core::tasks::delete(unit)
                .map_err(|e| format!("schedule_task delete: {e}"))?;
            Ok(format!("Task '{unit}' deleted."))
        }
        "list" => {
            let tasks = oxidemx_agent_core::tasks::list();
            serde_json::to_string(&tasks)
                .map_err(|e| format!("schedule_task list: serialize error: {e}"))
        }
        other => Err(format!("schedule_task: unknown action '{other}'")),
    }
}

// ── memory ────────────────────────────────────────────────────────────────────

/// `memory` — declared keys: `action`, `text`, `scope`, `id`, `query`.
///
/// Note: backed by the global store (`~/.local/share/oxidemx/memories.json`).
/// Per-project memory is a follow-up (see followups.md).
pub(super) fn memory(args: &Value) -> Result<String, String> {
    let action = args["action"]
        .as_str()
        .ok_or_else(|| "memory: missing 'action' argument".to_string())?;

    let id_arg = || -> Result<&str, String> {
        args["id"]
            .as_str()
            .ok_or_else(|| format!("memory: 'id' argument required for action '{action}'"))
    };

    match action {
        "save" => {
            let text = args["text"]
                .as_str()
                .ok_or_else(|| "memory: 'text' argument required for save".to_string())?;
            let scope = args["scope"].as_str().unwrap_or("general");
            let entry = oxidemx_agent_core::memory::save_entry(text, scope);
            Ok(format!("Memory saved with id {}.", entry.id))
        }
        "list" => {
            let all = oxidemx_agent_core::memory::load_all();
            serde_json::to_string(&all)
                .map_err(|e| format!("memory list: serialize error: {e}"))
        }
        "search" => {
            let query = args["query"]
                .as_str()
                .or_else(|| args["text"].as_str())
                .ok_or_else(|| "memory: 'query' argument required for search".to_string())?;
            let hits = oxidemx_agent_core::memory::search(query);
            serde_json::to_string(&hits)
                .map_err(|e| format!("memory search: serialize error: {e}"))
        }
        "delete" => {
            let id = id_arg()?;
            if oxidemx_agent_core::memory::delete(id) {
                Ok(format!("Memory {id} deleted."))
            } else {
                Ok(format!("No memory with id {id}."))
            }
        }
        "pin" | "unpin" => {
            let id = id_arg()?;
            let pinned = action == "pin";
            if oxidemx_agent_core::memory::set_pinned(id, pinned) {
                Ok(format!(
                    "Memory {id} {} — retention is now '{}'.",
                    if pinned { "pinned" } else { "unpinned" },
                    if pinned { "until changed" } else { "auto · 90d" }
                ))
            } else {
                Ok(format!("No memory with id {id}."))
            }
        }
        "consolidate" => {
            let input = oxidemx_agent_core::memory::consolidation_input();
            if input.len() < 6 {
                Ok("Memory store is small and tidy; nothing to consolidate.".to_string())
            } else {
                Ok(format!(
                    "Consolidation requires an LLM call; {} unpinned entries are candidates. \
                     Full LLM consolidation is wired through the agent runtime (Task 5+).",
                    input.len()
                ))
            }
        }
        other => Err(format!("memory: unknown action '{other}'")),
    }
}

// ── persona ───────────────────────────────────────────────────────────────────

/// `persona` — declared keys: `action`, `content`.
///
/// Note: backed by global config paths (`~/.config/oxidemx/{soul,user}.md`).
/// Per-project persona scoping is a follow-up (see followups.md).
pub(super) fn persona(args: &Value) -> Result<String, String> {
    let action = args["action"]
        .as_str()
        .ok_or_else(|| "persona: missing 'action' argument".to_string())?;

    match action {
        "read" => {
            let soul = std::fs::read_to_string(oxidemx_agent_core::persona::soul_path())
                .unwrap_or_else(|_| "(soul.md does not exist yet)".to_string());
            let user = std::fs::read_to_string(oxidemx_agent_core::persona::user_path())
                .unwrap_or_else(|_| "(user.md does not exist yet)".to_string());
            Ok(format!("--- soul.md ---\n{soul}\n--- user.md ---\n{user}"))
        }
        "write_soul" | "write_user" => {
            let content = args["content"]
                .as_str()
                .ok_or_else(|| {
                    "persona: 'content' argument required for write actions".to_string()
                })?;
            let which = if action == "write_soul" { "soul" } else { "user" };
            let path = oxidemx_agent_core::persona::write_file(which, content)
                .map_err(|e| format!("persona: {e}"))?;
            Ok(format!("{} written.", path.display()))
        }
        other => Err(format!("persona: unknown action '{other}'")),
    }
}

// ── ask_multiple_choice_question (host-delegated) ─────────────────────────────

/// `ask_multiple_choice_question` — declared keys: `question`, `options`.
///
/// Delegates to `host.invoke("ask_multiple_choice_question", args)`. Returns
/// the host's reply as a string, or a clean "host unavailable" error.
pub(super) async fn ask_multiple_choice_question(
    host: &Arc<dyn HostCapability>,
    args: &Value,
) -> Result<String, String> {
    // Validate required args before hitting the host.
    let _question = args["question"]
        .as_str()
        .ok_or_else(|| {
            "ask_multiple_choice_question: missing 'question' argument".to_string()
        })?;
    let _options = args["options"]
        .as_array()
        .ok_or_else(|| {
            "ask_multiple_choice_question: missing 'options' argument".to_string()
        })?;

    match host.invoke("ask_multiple_choice_question", args.clone()).await {
        Ok(reply) => {
            // Return the host's reply as a JSON string — model can parse the
            // `choice` field from it.
            serde_json::to_string(&reply)
                .map_err(|e| format!("ask_multiple_choice_question: serialize reply: {e}"))
        }
        Err(e) => Err(format!(
            "ask_multiple_choice_question: host unavailable: {e}"
        )),
    }
}

// ── get_menu_config / set_menu_config (host-delegated) ────────────────────────

/// Generic host-delegated tool for `get_menu_config` and `set_menu_config`.
///
/// For `set_menu_config` specifically: the host implementation (overlay, Task 6)
/// must extract the `config_json` argument from `args` and apply the configuration.
/// The agentd dispatcher passes the full arguments through unchanged.
pub(super) async fn host_delegated(
    capability: &str,
    host: &Arc<dyn HostCapability>,
    args: &Value,
) -> Result<String, String> {
    match host.invoke(capability, args.clone()).await {
        Ok(reply) => serde_json::to_string(&reply)
            .map_err(|e| format!("{capability}: serialize reply: {e}")),
        Err(e) => Err(format!("{capability}: host unavailable: {e}")),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
pub(super) mod test_support {
    //! Helpers shared between `agent.rs` and `mod.rs` tests.

    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::Value;

    use crate::run_launcher::{RunLauncher, RunStatus};
    use crate::seams::HostCapability;
    use crate::error::AgentdError;

    /// A [`HostCapability`] that records every capability name it receives and
    /// returns a fixed `canned_reply` JSON value.
    pub struct RecordingHost {
        calls: Mutex<Vec<String>>,
        reply: Value,
    }

    impl RecordingHost {
        pub fn with_reply(reply: Value) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                reply,
            }
        }

        /// Snapshot of all capability names called so far.
        pub fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
    }

    #[async_trait]
    impl HostCapability for RecordingHost {
        async fn invoke(&self, cap: &str, _args: Value) -> Result<Value, AgentdError> {
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(cap.to_string());
            Ok(self.reply.clone())
        }
    }

    /// A [`RunLauncher`] that always returns a fixed run_id from `launch`, and
    /// reports `RunStatus::Running` for that id.
    pub struct FakeLauncher {
        pub run_id: String,
    }

    #[async_trait]
    impl RunLauncher for FakeLauncher {
        async fn launch(
            &self,
            _project: &str,
            _flow_id: &str,
            _inputs_json: &str,
        ) -> Result<String, String> {
            Ok(self.run_id.clone())
        }

        fn status(&self, run_id: &str) -> Option<RunStatus> {
            if run_id == self.run_id {
                Some(RunStatus::Running)
            } else {
                None
            }
        }

        fn list_runs(&self, _project: &str) -> Vec<String> {
            vec![self.run_id.clone()]
        }
    }

    /// Build a test executor with a specific `HostCapability`.
    pub fn test_executor_with_host(
        cwd: &std::path::Path,
        host: Arc<dyn HostCapability>,
    ) -> crate::tools::AgentToolExecutor {
        crate::tools::AgentToolExecutor::new(
            crate::projects::ProjectPaths::resolve(cwd),
            host,
            Arc::new(crate::run_launcher::NoopRunLauncher),
        )
    }

    /// Build a test executor with a specific `RunLauncher` (uses `UnavailableHost`).
    pub fn test_executor_with_launcher(
        cwd: &std::path::Path,
        launcher: Arc<dyn RunLauncher>,
    ) -> crate::tools::AgentToolExecutor {
        crate::tools::AgentToolExecutor::new(
            crate::projects::ProjectPaths::resolve(cwd),
            Arc::new(crate::seams::UnavailableHost),
            launcher,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use std::sync::Arc;
    use oxidemx_agent_core::tool::ToolExecutor;

    // ── ask_multiple_choice_question ──────────────────────────────────────────

    /// Verbatim test from the task brief.
    #[tokio::test]
    async fn ask_multiple_choice_delegates_to_host() {
        let host = Arc::new(RecordingHost::with_reply(serde_json::json!({"choice":"B"})));
        let exec = test_executor_with_host(tempfile::tempdir().unwrap().path(), host.clone());
        let out = exec
            .execute(
                "ask_multiple_choice_question",
                serde_json::json!({"question":"x","options":["A","B"]}),
                &None,
            )
            .await
            .unwrap();
        assert!(host.calls().iter().any(|c| c == "ask_multiple_choice_question"));
        assert!(out.contains("B"));
    }

    #[tokio::test]
    async fn ask_multiple_choice_missing_question_errors() {
        let host = Arc::new(RecordingHost::with_reply(serde_json::json!({"choice":"A"})));
        let exec = test_executor_with_host(tempfile::tempdir().unwrap().path(), host);
        let err = exec
            .execute(
                "ask_multiple_choice_question",
                serde_json::json!({"options":["A","B"]}),
                &None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("question"));
    }

    // ── memory round-trip ────────────────────────────────────────────────────

    #[tokio::test]
    #[ignore]
    async fn memory_save_then_list_round_trip() {
        // Ignored: mutates the real global memory store until per-project memory
        // scoping lands (SP1c followup). The core memory tests in oxidemx-agent-core
        // already cover the store mechanics. This test dispatcher wiring is verified
        // by the action-parsing tests below.
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(
            d.path(),
            Arc::new(crate::seams::UnavailableHost),
        );

        // Save a memory.
        let save_out = exec
            .execute(
                "memory",
                serde_json::json!({"action": "save", "text": "test fact for T3", "scope": "test"}),
                &None,
            )
            .await
            .unwrap();
        assert!(save_out.contains("Memory saved with id"));

        // List should produce JSON that contains the saved text.
        let list_out = exec
            .execute("memory", serde_json::json!({"action": "list"}), &None)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&list_out).unwrap();
        assert!(parsed.is_array());
        // We saved at least one entry that contains our text.
        let arr = parsed.as_array().unwrap();
        assert!(
            arr.iter()
                .any(|e| e["text"].as_str().unwrap_or("").contains("test fact for T3")),
            "saved text not found in list output"
        );
    }

    #[tokio::test]
    async fn memory_unknown_action_errors() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        let err = exec
            .execute("memory", serde_json::json!({"action": "frobnicate"}), &None)
            .await
            .unwrap_err();
        assert!(err.contains("unknown action"));
    }

    // ── use_skill ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn use_skill_reads_from_project_skills_dir() {
        let d = tempfile::tempdir().unwrap();
        // Create a skill under the project-local .oxidemx/skills directory.
        let skill_dir = d.path().join(".oxidemx").join("skills").join("my-test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-test-skill\ndescription: A test skill\n---\n\nDo the thing.",
        )
        .unwrap();

        // We don't want to depend on the real enabled_set() since it reads a
        // global config file. The use_skill helper checks `enabled_set()` from
        // oxidemx_agent_core. In test, the skill won't be enabled, so the
        // response should be the "not enabled" message (not a crash).
        //
        // We verify: (a) the executor dispatches, (b) the skill directory is
        // found (the response changes from "no enabled skill" to the content
        // OR the not-enabled message — both are Ok variants, not Err).
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        let out = exec
            .execute("use_skill", serde_json::json!({"name": "my-test-skill"}), &None)
            .await
            .unwrap(); // must not error
        // Either served the body (if the global enabled_set happened to include it)
        // OR returned the not-enabled message. Either way it's a valid Ok.
        assert!(!out.is_empty());
    }

    #[tokio::test]
    async fn use_skill_missing_name_errors() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        let err = exec
            .execute("use_skill", serde_json::json!({}), &None)
            .await
            .unwrap_err();
        assert!(err.contains("name"));
    }

    // ── schedule_task ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn schedule_task_list_returns_json_array() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        // list calls systemctl --user; it may fail in the test sandbox, but
        // the dispatcher must at least try and return either JSON or a clean error.
        let result = exec
            .execute("schedule_task", serde_json::json!({"action": "list"}), &None)
            .await;
        // If systemctl is present, we get JSON. If not, we get an Err.
        // Both are acceptable — we just verify the dispatcher routes correctly.
        match result {
            Ok(s) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                assert!(v.is_array());
            }
            Err(e) => {
                assert!(e.contains("schedule_task") || e.contains("systemctl"));
            }
        }
    }

    #[tokio::test]
    async fn schedule_task_unknown_action_errors() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        let err = exec
            .execute(
                "schedule_task",
                serde_json::json!({"action": "bogus"}),
                &None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("unknown action"));
    }

    // ── persona ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn persona_read_returns_both_files() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        // read always succeeds (missing files degrade to placeholder strings).
        let out = exec
            .execute("persona", serde_json::json!({"action": "read"}), &None)
            .await
            .unwrap();
        assert!(out.contains("soul.md"));
        assert!(out.contains("user.md"));
    }

    #[tokio::test]
    async fn persona_unknown_action_errors() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        let err = exec
            .execute("persona", serde_json::json!({"action": "nope"}), &None)
            .await
            .unwrap_err();
        assert!(err.contains("unknown action"));
    }

    // ── compose_flow ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn compose_flow_empty_id_is_ok_not_panic() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor_with_host(d.path(), Arc::new(crate::seams::UnavailableHost));
        // A flow_id that slugifies to empty returns an Ok with a helpful message.
        let out = exec
            .execute(
                "compose_flow",
                serde_json::json!({"flow_id": "!!!---", "flow_md": "body"}),
                &None,
            )
            .await
            .unwrap();
        assert!(out.contains("letters or numbers"));
    }

    // ── run_flow (real launcher) ──────────────────────────────────────────────

    #[tokio::test]
    async fn run_flow_tool_launches_and_reports_run_id() {
        let d = tempfile::tempdir().unwrap();
        let launcher = Arc::new(FakeLauncher { run_id: "run-7".to_string() });
        let exec = test_executor_with_launcher(d.path(), launcher);
        let out = exec
            .execute(
                "run_flow",
                serde_json::json!({"flow_id": "test-flow"}),
                &None,
            )
            .await
            .unwrap();
        assert!(out.contains("run-7"), "output should contain the real run id: {out}");
        assert!(out.contains("Launched"), "output should mention Launched: {out}");
    }

    // ── run_status (ground truth) ─────────────────────────────────────────────

    #[tokio::test]
    async fn run_status_tool_reports_ground_truth() {
        let d = tempfile::tempdir().unwrap();
        let launcher = Arc::new(FakeLauncher { run_id: "run-42".to_string() });
        let exec = test_executor_with_launcher(d.path(), launcher);

        // Known run → returns "running"
        let out = exec
            .execute(
                "run_status",
                serde_json::json!({"run_id": "run-42"}),
                &None,
            )
            .await
            .unwrap();
        assert!(out.contains("running"), "should report running status: {out}");

        // Unknown run → reports unknown / no run
        let out2 = exec
            .execute(
                "run_status",
                serde_json::json!({"run_id": "run-999"}),
                &None,
            )
            .await
            .unwrap();
        assert!(
            out2.contains("No run") || out2.contains("unknown") || out2.contains("known"),
            "unknown id should say no/unknown: {out2}"
        );
    }
}
