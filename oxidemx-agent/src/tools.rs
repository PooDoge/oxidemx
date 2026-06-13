//! Agent tools bridged onto AutoAgents' `ToolT`/`ToolRuntime`.
//!
//! P0 ships `execute_command` only, guarded by the ported allowlist
//! (src/allowlist.rs). A denied command returns a structured tool
//! RESULT (not an error) so the model explains the denial — same
//! contract as the overlay. The interactive approval chip is P1; in
//! the CLI harness, off-list means denied.
//!
//! The `#[agent]` macro takes tool *expressions*, so runtime state
//! (the allowlist) rides in a process-global set once by the host
//! before any agent runs. The Conductor (P3) replaces this with
//! per-agent tool construction via manual `AgentDeriveT`.

use std::sync::OnceLock;

use autoagents::async_trait;
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents_derive::{tool, ToolInput};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

static ALLOWLIST: OnceLock<Vec<String>> = OnceLock::new();

/// Install the command allowlist for this process. Call once from
/// the host before building agents; later calls are ignored.
pub fn set_allowlist(entries: Vec<String>) {
    let _ = ALLOWLIST.set(entries);
}

fn allowlist() -> &'static [String] {
    ALLOWLIST.get().map(Vec::as_slice).unwrap_or(&[])
}

#[derive(Serialize, Deserialize, ToolInput, Debug)]
pub struct ExecuteCommandArgs {
    #[input(description = "The shell command to execute")]
    command: String,
}

#[tool(
    name = "execute_command",
    description = "Run a shell command on the user's Linux system and return its output and exit code. Only allowlisted commands will execute; anything else is denied with an explanation.",
    input = ExecuteCommandArgs,
)]
pub struct ExecuteCommand {}

#[async_trait]
impl ToolRuntime for ExecuteCommand {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let a: ExecuteCommandArgs = serde_json::from_value(args)?;
        if !crate::allowlist::is_allowlisted(&a.command, allowlist()) {
            return Ok(json!({
                "denied": true,
                "reason": "command is not on the user's allowlist; explain what you wanted to run and why",
                "command": a.command,
            }));
        }
        let (output, exit_code) = crate::allowlist::run(&a.command).await;
        Ok(json!({ "output": output, "exit_code": exit_code }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn denied_command_returns_structured_denial_not_error() {
        set_allowlist(vec!["echo".into()]);
        let out = ExecuteCommand {}
            .execute(json!({"command": "rm -rf /"}))
            .await
            .expect("denial is a result, not an error");
        assert_eq!(out["denied"], true);
        assert_eq!(out["command"], "rm -rf /");
    }

    #[tokio::test]
    async fn allowlisted_command_runs_and_reports_exit_code() {
        set_allowlist(vec!["echo".into()]);
        let out = ExecuteCommand {}
            .execute(json!({"command": "echo p0-spike"}))
            .await
            .unwrap();
        assert_eq!(out["exit_code"], 0);
        assert!(out["output"].as_str().unwrap().contains("p0-spike"));
    }

    #[test]
    fn tool_declares_schema_for_the_model() {
        let t = ExecuteCommand {};
        assert_eq!(t.name(), "execute_command");
        assert!(!t.description().is_empty());
        let schema = t.args_schema();
        assert!(schema.to_string().contains("command"));
    }
}
