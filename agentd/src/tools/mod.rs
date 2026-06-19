//! Native tool executor for agentd.
//!
//! [`AgentToolExecutor`] implements [`ToolExecutor`] by dispatching to the
//! seven native tool bodies in [`fs`].  All filesystem tools are
//! cwd-scoped and escape-guarded; `list_system_apps` and `google_search`
//! operate at host scope.
#![forbid(unsafe_code)]

mod fs;

use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;

use oxidemx_agent_core::tool::ToolExecutor;
use oxidemx_agent_core::events::StreamSink;
use crate::projects::ProjectPaths;
use crate::seams::HostCapability;

// ── AgentToolExecutor ─────────────────────────────────────────────────────────

/// Concrete [`ToolExecutor`] for agentd.
///
/// Stores the project paths (used for cwd-scoped tools) and a
/// [`HostCapability`] seam (needed for Task 3 features; stored now so the
/// constructor signature is stable).
pub struct AgentToolExecutor {
    paths: ProjectPaths,
    #[allow(dead_code)]
    host: Arc<dyn HostCapability>,
}

impl AgentToolExecutor {
    pub fn new(paths: ProjectPaths, host: Arc<dyn HostCapability>) -> Self {
        Self { paths, host }
    }
}

#[async_trait]
impl ToolExecutor for AgentToolExecutor {
    async fn execute(
        &self,
        name: &str,
        args: Value,
        _sink: &Option<StreamSink>,
    ) -> Result<String, String> {
        let cwd = &self.paths.cwd;
        match name {
            "read_file" => fs::read_file(cwd, &args),
            "list_dir" => fs::list_dir(cwd, &args),
            "search_file" => fs::search_file(cwd, &args),
            "parse_document" => fs::parse_document(cwd, &args),
            "execute_command" => fs::execute_command(cwd, &args).await,
            "list_system_apps" => fs::list_system_apps(),
            "google_search" => fs::google_search(&args).await,
            other => Err(format!("unknown tool: {other}")),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::projects::ProjectPaths;
    use crate::seams::UnavailableHost;

    fn test_executor(cwd: &std::path::Path) -> AgentToolExecutor {
        AgentToolExecutor::new(
            ProjectPaths::resolve(cwd),
            Arc::new(UnavailableHost),
        )
    }

    #[tokio::test]
    async fn read_file_reads_within_cwd() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello").unwrap();
        let exec = test_executor(d.path());
        let out = exec.execute("read_file", serde_json::json!({"path":"a.txt"}), &None).await.unwrap();
        assert!(out.contains("hello"));
    }

    #[tokio::test]
    async fn read_file_rejects_escape() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        assert!(exec.execute("read_file", serde_json::json!({"path":"../../etc/passwd"}), &None).await.is_err());
    }

    #[tokio::test]
    async fn execute_command_runs_in_cwd() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        let out = exec.execute("execute_command", serde_json::json!({"command":"pwd"}), &None).await.unwrap();
        assert!(out.contains(d.path().file_name().unwrap().to_str().unwrap()));
    }

    #[tokio::test]
    async fn list_dir_lists_within_cwd() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("b.txt"), "x").unwrap();
        let exec = test_executor(d.path());
        let out = exec.execute("list_dir", serde_json::json!({"path":"."}), &None).await.unwrap();
        assert!(out.contains("b.txt"));
    }

    #[tokio::test]
    async fn list_dir_rejects_escape() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        assert!(exec.execute("list_dir", serde_json::json!({"path":"../../etc"}), &None).await.is_err());
    }

    #[tokio::test]
    async fn search_file_finds_match() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("needle.rs"), "fn main(){}").unwrap();
        std::fs::write(d.path().join("hay.txt"), "nothing").unwrap();
        let exec = test_executor(d.path());
        let out = exec.execute("search_file", serde_json::json!({"path":".","pattern":"needle"}), &None).await.unwrap();
        assert!(out.contains("needle"));
        assert!(!out.contains("hay.txt"));
    }

    #[tokio::test]
    async fn parse_document_reads_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("doc.txt"), "content here").unwrap();
        let exec = test_executor(d.path());
        let out = exec.execute("parse_document", serde_json::json!({"path":"doc.txt"}), &None).await.unwrap();
        assert!(out.contains("content here"));
    }

    #[tokio::test]
    async fn unknown_tool_returns_err() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        assert!(exec.execute("unknown_xyz", serde_json::json!({}), &None).await.is_err());
    }
}
