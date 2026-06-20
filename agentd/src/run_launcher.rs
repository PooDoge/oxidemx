//! The `RunLauncher` seam: launch conductor flows + query run status as ground
//! truth. `ConductorRunLauncher` (Task 2) is the real impl over the conductor;
//! `AgentService` delegates to it (Task 3); the agent tools call it (Task 5).
#![forbid(unsafe_code)]

use async_trait::async_trait;

/// Lifecycle status of a conductor run. Mirrors the `run_statuses` table strings
/// exactly (`running`/`finished`/`failed`/`cancelled`) — typed so callers cannot
/// invent a status the system never produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Finished,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Finished => "finished",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_status_str(s: &str) -> Option<RunStatus> {
        match s {
            "running" => Some(RunStatus::Running),
            "finished" => Some(RunStatus::Finished),
            "failed" => Some(RunStatus::Failed),
            "cancelled" => Some(RunStatus::Cancelled),
            _ => None,
        }
    }
}

/// Launch + query conductor flow runs. The single seam the agent tools use, so
/// run status is always ground truth (never the model's guess).
#[async_trait]
pub trait RunLauncher: Send + Sync {
    /// Launch `flow_id` for `project` with optional `inputs_json`; returns the
    /// real `run_id`. Errors are the real failure (bad flow, missing inputs).
    async fn launch(
        &self,
        project: &str,
        flow_id: &str,
        inputs_json: &str,
    ) -> Result<String, String>;

    /// Current status of `run_id`, or `None` if unknown (never started / GC'd).
    fn status(&self, run_id: &str) -> Option<RunStatus>;

    /// All run ids known for `project` (from the runs directory).
    fn list_runs(&self, project: &str) -> Vec<String>;
}

/// No-op launcher for contexts that do not launch flows (tests, the harness
/// worker until SP2d flows launch there). Launch fails honestly; queries empty.
pub struct NoopRunLauncher;

#[async_trait]
impl RunLauncher for NoopRunLauncher {
    async fn launch(
        &self,
        _project: &str,
        _flow_id: &str,
        _inputs_json: &str,
    ) -> Result<String, String> {
        Err("run launching is not available in this context".into())
    }
    fn status(&self, _run_id: &str) -> Option<RunStatus> {
        None
    }
    fn list_runs(&self, _project: &str) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_status_str_roundtrips() {
        for s in ["running", "finished", "failed", "cancelled"] {
            assert_eq!(RunStatus::from_status_str(s).unwrap().as_str(), s);
        }
        assert_eq!(RunStatus::from_status_str("bogus"), None);
    }

    #[tokio::test]
    async fn noop_launcher_fails_honestly_and_queries_empty() {
        let l = NoopRunLauncher;
        assert!(l.launch("/tmp/p", "flow", "{}").await.is_err());
        assert_eq!(l.status("run-1"), None);
        assert!(l.list_runs("/tmp/p").is_empty());
    }
}
