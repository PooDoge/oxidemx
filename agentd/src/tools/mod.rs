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
            "read_file"       => fs::read_file(cwd, &args),
            "list_dir"        => fs::list_dir(cwd, &args),
            "search_file"     => fs::search_file(cwd, &args),
            "parse_document"  => fs::parse_document(cwd, &args),
            "execute_command" => fs::execute_command(cwd, &args).await,
            "list_system_apps" => fs::list_system_apps(),
            "google_search"   => fs::google_search(&args).await,
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

    // ── read_file ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn read_file_reads_within_cwd() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello").unwrap();
        let exec = test_executor(d.path());
        // Declared key: file_path
        let out = exec.execute("read_file", serde_json::json!({"file_path": "a.txt"}), &None).await.unwrap();
        assert!(out.contains("hello"));
    }

    #[tokio::test]
    async fn read_file_rejects_escape() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        assert!(exec.execute("read_file", serde_json::json!({"file_path": "../../etc/passwd"}), &None).await.is_err());
    }

    #[tokio::test]
    async fn read_file_missing_file_not_escapes_error() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        // A missing but in-cwd file must error with something useful, NOT "escapes".
        let err = exec.execute("read_file", serde_json::json!({"file_path": "missing.txt"}), &None).await.unwrap_err();
        assert!(!err.contains("escapes"), "missing file error should not say 'escapes': {err}");
    }

    // ── list_dir ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_dir_lists_within_cwd() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("b.txt"), "x").unwrap();
        let exec = test_executor(d.path());
        // Declared key: directory_path
        let out = exec.execute("list_dir", serde_json::json!({"directory_path": "."}), &None).await.unwrap();
        assert!(out.contains("b.txt"));
        // Output is JSON array of objects
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert!(!arr.is_empty());
        assert!(arr[0].get("name").is_some());
        assert!(arr[0].get("path").is_some());
        assert!(arr[0].get("is_dir").is_some());
        assert!(arr[0].get("size").is_some());
    }

    #[tokio::test]
    async fn list_dir_rejects_escape() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        assert!(exec.execute("list_dir", serde_json::json!({"directory_path": "../../etc"}), &None).await.is_err());
    }

    // ── search_file ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn search_file_finds_match_with_glob() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("needle.rs"), "fn main(){}").unwrap();
        std::fs::write(d.path().join("hay.txt"), "nothing").unwrap();
        let exec = test_executor(d.path());
        // Declared keys: directory + pattern  (glob, not substring)
        let out = exec.execute("search_file", serde_json::json!({"directory": ".", "pattern": "*.rs"}), &None).await.unwrap();
        assert!(out.contains("needle"), "should find needle.rs via *.rs glob");
        assert!(!out.contains("hay.txt"), "should not find hay.txt with *.rs glob");
        // Output shape: JSON array of objects
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert!(!arr.is_empty());
        assert!(arr[0].get("name").is_some());
        assert!(arr[0].get("path").is_some());
    }

    // ── execute_command ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_command_runs_in_cwd() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        let out = exec.execute("execute_command", serde_json::json!({"command": "pwd"}), &None).await.unwrap();
        assert!(out.contains(d.path().file_name().unwrap().to_str().unwrap()));
    }

    // ── parse_document ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn parse_document_reads_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("doc.txt"), "content here").unwrap();
        let exec = test_executor(d.path());
        // Declared key: source
        let out = exec.execute("parse_document", serde_json::json!({"source": "doc.txt"}), &None).await.unwrap();
        assert!(out.contains("content here"));
    }

    // ── unknown tool ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn unknown_tool_returns_err() {
        let d = tempfile::tempdir().unwrap();
        let exec = test_executor(d.path());
        assert!(exec.execute("unknown_xyz", serde_json::json!({}), &None).await.is_err());
    }
}
