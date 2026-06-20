//! The `RunLauncher` seam: launch conductor flows + query run status as ground
//! truth. `ConductorRunLauncher` (Task 2) is the real impl over the conductor;
//! `AgentService` delegates to it (Task 3); the agent tools call it (Task 5).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::projects::ProjectPaths;
use crate::run_bridge::RunEventBridge;
use crate::seams::EventEmitter;

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

/// Real `RunLauncher` over the conductor supervisor. Holds clones of the
/// `AgentService` run-state Arcs so it is the single source of truth for both
/// the D-Bus methods (via delegation, Task 3) and the agent tools (Task 5).
pub struct ConductorRunLauncher {
    active_runs: Arc<Mutex<HashMap<String, oxidemx_conductor::RunHandle>>>,
    run_statuses: Arc<Mutex<HashMap<String, String>>>,
    emitter: Arc<dyn EventEmitter>,
}

impl ConductorRunLauncher {
    pub fn new(
        active_runs: Arc<Mutex<HashMap<String, oxidemx_conductor::RunHandle>>>,
        run_statuses: Arc<Mutex<HashMap<String, String>>>,
        emitter: Arc<dyn EventEmitter>,
    ) -> Self {
        Self { active_runs, run_statuses, emitter }
    }
}

#[async_trait]
impl RunLauncher for ConductorRunLauncher {
    async fn launch(&self, project: &str, flow_id: &str, inputs_json: &str)
        -> Result<String, String>
    {
        let cwd = PathBuf::from(project);
        let paths = ProjectPaths::resolve(&cwd);

        // ── 1. Load flow doc + roster ─────────────────────────────────────
        let flows_root = oxidemx_conductor::loader::default_flows_root();
        let agents_root = oxidemx_conductor::loader::default_agents_root();
        let (doc, roster) =
            oxidemx_conductor::loader::load_flow(&flows_root, &agents_root, flow_id)
                .map_err(|e| e.to_string())?;

        // ── 2. Validate plan ──────────────────────────────────────────────
        let plan =
            oxidemx_conductor::plan::validate(&doc, &roster, oxidemx_conductor::KNOWN_TOOLS)
                .map_err(|errs| {
                    let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
                    format!("invalid flow: {}", msgs.join("; "))
                })?;

        // ── 3. Parse inputs JSON → BTreeMap<String, String> ──────────────
        let provided: BTreeMap<String, String> = if inputs_json.trim().is_empty()
            || inputs_json.trim() == "{}"
        {
            BTreeMap::new()
        } else {
            let v: serde_json::Value = serde_json::from_str(inputs_json)
                .map_err(|e| format!("inputs_json parse: {e}"))?;
            match v {
                serde_json::Value::Object(map) => map
                    .into_iter()
                    .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_string()))
                    .collect(),
                _ => BTreeMap::new(),
            }
        };

        // ── 4. Resolve inputs (merge with defaults, check required) ───────
        let inputs =
            oxidemx_conductor::supervisor::resolve_inputs(&plan, &provided)
                .map_err(|missing| {
                    format!("missing required inputs: {}", missing.join(", "))
                })?;

        // ── 5. Build run_id + workdir ─────────────────────────────────────
        let run_id = {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(1);
            format!("run-{}", NEXT.fetch_add(1, Ordering::Relaxed))
        };
        let workdir = paths.runs_dir().join(&run_id);
        std::fs::create_dir_all(&workdir).map_err(|e| e.to_string())?;

        // ── 6. Build the provider factory ─────────────────────────────────
        let factory: Arc<dyn oxidemx_conductor::supervisor::ProviderFactory> =
            if std::env::var_os("OXIDEMX_TEST_MOCK_FLOW").is_some() {
                Arc::new(oxidemx_conductor::supervisor::FixedFactory(
                    oxidemx_conductor::mock::MockProvider::echoing(),
                ))
            } else {
                let ai_cfg = oxidemx_shared::config::AiConfig::default();
                // Resolve the key the SAME file-first way the chat path does
                // (`oxidemx_agent::keys::provider_key` reads the stored
                // `~/.config/oxidemx/<provider>.key`), falling back to the
                // provider's key env var. The original moved body read ONLY the
                // env var, so flows failed instantly when the key was stored in
                // a file (the normal case) rather than the environment.
                let api_key = oxidemx_agent::keys::provider_key(ai_cfg.provider)
                    .or_else(|| {
                        ai_cfg.provider.key_env().and_then(|env| std::env::var(env).ok())
                    })
                    .unwrap_or_default();
                Arc::new(oxidemx_conductor::supervisor::ConfigFactory {
                    provider: ai_cfg.provider,
                    api_key,
                })
            };

        // ── 7. Build cancel token + RunOptions ────────────────────────────
        let cancel = CancellationToken::new();
        let opts = oxidemx_conductor::supervisor::RunOptions {
            run_id: run_id.clone(),
            inputs,
            workdir,
            roster,
            factory,
            cancel: cancel.clone(),
            approval: oxidemx_conductor::approval::ApprovalPolicy::Autonomous,
            allowlist: vec![],
        };

        // ── 8. Register handle ────────────────────────────────────────────
        let handle = oxidemx_conductor::RunHandle {
            run_id: run_id.clone(),
            cancel: cancel.clone(),
        };
        {
            let mut guard = self.active_runs.lock().unwrap_or_else(|e| e.into_inner());
            guard.insert(run_id.clone(), handle);
            // guard dropped here — NEVER held across .await
        }

        // ── 9. Spawn the supervisor ───────────────────────────────────────
        let bridge = Arc::new(RunEventBridge::new(
            project,
            self.emitter.clone(),
            self.run_statuses.clone(),
        ));
        let active_runs = self.active_runs.clone();
        let rid = run_id.clone();
        tokio::spawn(async move {
            oxidemx_conductor::supervisor::run_flow(&plan, opts, bridge).await;
            // Remove the handle once the run completes (success, fail, or cancel).
            let mut guard = active_runs.lock().unwrap_or_else(|e| e.into_inner());
            guard.remove(&rid);
        });

        Ok(run_id)
    }

    fn status(&self, run_id: &str) -> Option<RunStatus> {
        let guard = self.run_statuses.lock().unwrap_or_else(|e| e.into_inner());
        guard.get(run_id).and_then(|s| RunStatus::from_status_str(s))
    }

    fn list_runs(&self, project: &str) -> Vec<String> {
        let paths = ProjectPaths::resolve(&PathBuf::from(project));
        let runs_dir = paths.runs_dir();
        let mut ids = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&runs_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        ids.push(name.to_string());
                    }
                }
            }
        }
        ids.sort();
        ids
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

    // ── ConductorRunLauncher tests ────────────────────────────────────────────

    // A no-op emitter for launcher unit tests.
    struct SilentEmitter;
    impl crate::seams::EventEmitter for SilentEmitter {
        fn emit(&self, _ev: crate::seams::AgentEvent) {}
    }

    fn launcher_with_statuses(seed: &[(&str, &str)]) -> ConductorRunLauncher {
        let mut m = HashMap::new();
        for (k, v) in seed { m.insert((*k).to_string(), (*v).to_string()); }
        ConductorRunLauncher::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(m)),
            Arc::new(SilentEmitter),
        )
    }

    #[test]
    fn status_reads_ground_truth_from_table() {
        let l = launcher_with_statuses(&[("run-7", "finished")]);
        assert_eq!(l.status("run-7"), Some(RunStatus::Finished));
        assert_eq!(l.status("run-404"), None); // unknown → None, never a guess
    }

    #[tokio::test]
    async fn launch_unknown_flow_errors() {
        let l = launcher_with_statuses(&[]);
        // A flow id that does not exist on disk must surface a real error,
        // never a fake "launched".
        let err = l.launch("/tmp/nonexistent-project", "definitely-not-a-real-flow", "{}")
            .await
            .unwrap_err();
        assert!(!err.is_empty());
    }
}
