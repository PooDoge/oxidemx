# Truthful & Visible Background Runs — Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the agent's `run_flow` tool actually launch a conductor run and add truthful `run_status`/`list_runs` tools, so the agent stops confabulating run status — via a `RunLauncher` seam, with a system-prompt grounding rule.

**Architecture:** `AgentService` already has real `run_flow`/`run_status`/`list_runs` methods. Extract their bodies into a `ConductorRunLauncher` behind a `RunLauncher` trait (single source of truth — `AgentService` delegates to it, no split-brain). Thread `Arc<dyn RunLauncher>` into `AgentToolExecutor` (via the `TurnRunner::run_turn` signature, the same way `host` is threaded). Replace the `run_flow` tool stub + add `run_status`/`list_runs` tools that call the launcher. Add a structural truthfulness rule to the system prompt.

**Tech Stack:** Rust, tokio, async-trait, zbus 5 (tokio executor), oxidemx-conductor, serde_json.

**Scope:** Backend only (headless-testable). The overlay activity-UI (floating run bubbles) is a **separate plan** — it needs the visual-companion design first (spec §5/§8).

## Global Constraints

- Follow repo `CLAUDE.md` (binding): Rule 0 field-standard naming; Rule 1 grounded narration; Rule 2 `cargo fmt`/`clippy` clean, seam traits for mockable units, no gold-plating.
- **Build host-side** from repo root `oxidemx-phase1/` with a dedicated target dir so it never clobbers the distrobox/overlay `target/`: every `cargo` command below is prefixed `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`. agentd + oxidemx-agent-core build + test on the host with no `-devel` libs (verified: clean build 29s, 83 tests pass). distrobox is only needed for the GTK overlay (the separate activity-UI plan).
- Naming (locked, spec + `docs/research/`): `agentd` = Gateway; the seam is **`RunLauncher`** (not "Manager"/"Service"). Truthfulness is **structural** (a tool returns ground truth), never exhortation.
- The `run_statuses` map stores `String` values: `"running" | "finished" | "failed" | "cancelled"`. Do not invent new status strings.
- Never hold a `Mutex` guard across an `.await` (existing invariant in `interface.rs`).
- Every task: `cargo build -p agentd` and `cargo test -p agentd` clean before commit.

---

### Task 1: `RunLauncher` trait + `RunStatus` enum + `NoopRunLauncher`

**Files:**
- Create: `agentd/src/run_launcher.rs`
- Modify: `agentd/src/main.rs` (add `mod run_launcher;` — place beside the other `mod` lines)

**Interfaces:**
- Produces:
  - `pub enum RunStatus { Running, Finished, Failed, Cancelled }` with `pub fn as_str(&self) -> &'static str` and `pub fn from_status_str(s: &str) -> Option<RunStatus>`.
  - `#[async_trait] pub trait RunLauncher: Send + Sync { async fn launch(&self, project: &str, flow_id: &str, inputs_json: &str) -> Result<String, String>; fn status(&self, run_id: &str) -> Option<RunStatus>; fn list_runs(&self, project: &str) -> Vec<String>; }`
  - `pub struct NoopRunLauncher;` implementing `RunLauncher` (used by tests + the harness worker until flows launch there).

- [ ] **Step 1: Write the failing test**

Create `agentd/src/run_launcher.rs` with only the tests first:

```rust
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
    async fn launch(&self, project: &str, flow_id: &str, inputs_json: &str)
        -> Result<String, String>;

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
    async fn launch(&self, _project: &str, _flow_id: &str, _inputs_json: &str)
        -> Result<String, String> {
        Err("run launching is not available in this context".into())
    }
    fn status(&self, _run_id: &str) -> Option<RunStatus> { None }
    fn list_runs(&self, _project: &str) -> Vec<String> { Vec::new() }
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
```

- [ ] **Step 2: Run the test to verify it fails (module not declared yet)**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_launcher 2>&1 | tail -20`
Expected: FAIL — `cargo` can't find the module / `run_launcher` tests not compiled (module not registered).

- [ ] **Step 3: Register the module**

In `agentd/src/main.rs`, add alongside the existing `mod` declarations:

```rust
mod run_launcher;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_launcher 2>&1 | tail -20`
Expected: PASS — `run_status_str_roundtrips` + `noop_launcher_fails_honestly_and_queries_empty` ok.

- [ ] **Step 5: Commit**

```bash
git add agentd/src/run_launcher.rs agentd/src/main.rs
git commit -m "feat(agentd): RunLauncher seam + RunStatus enum + NoopRunLauncher"
```

---

### Task 2: `ConductorRunLauncher` — the real impl (moved bodies)

**Files:**
- Modify: `agentd/src/run_launcher.rs` (add the struct + impl + tests)

**Interfaces:**
- Consumes: `RunStatus`, `RunLauncher` (Task 1); `crate::run_bridge::RunEventBridge`; `crate::projects::ProjectPaths`; `crate::seams::EventEmitter`; `oxidemx_conductor` (loader/plan/supervisor/RunHandle); `oxidemx_shared::config::AiConfig`.
- Produces: `pub struct ConductorRunLauncher { active_runs, run_statuses, emitter }` with `pub fn new(active_runs: Arc<Mutex<HashMap<String, oxidemx_conductor::RunHandle>>>, run_statuses: Arc<Mutex<HashMap<String, String>>>, emitter: Arc<dyn EventEmitter>) -> Self`. Implements `RunLauncher`. `launch` returns `run_id` like `"run-N"`.

- [ ] **Step 1: Write the failing tests** (append to `agentd/src/run_launcher.rs` `tests` module)

```rust
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    // A no-op emitter for launcher unit tests.
    struct SilentEmitter;
    #[async_trait]
    impl crate::seams::EventEmitter for SilentEmitter {
        async fn emit(&self, _project: &str, _thread: &str, _kind: &str, _payload: serde_json::Value) {}
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
```

> Note: the `EventEmitter` trait signature above must match the real one in `agentd/src/seams.rs`. Before writing the impl, open `seams.rs` and copy the exact `EventEmitter` method signature into `SilentEmitter` (adjust `emit`'s params/async to match). If it differs, fix the test stub to match — do not change the real trait.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_launcher 2>&1 | tail -20`
Expected: FAIL — `ConductorRunLauncher` not found.

- [ ] **Step 3: Implement `ConductorRunLauncher`** (add to `agentd/src/run_launcher.rs`, above the tests)

This MOVES the bodies currently in `AgentService::run_flow` / `run_status` / `list_runs` (`agentd/src/interface.rs`). Copy them verbatim, swapping `self.projects.resolve(&cwd)` → `crate::projects::ProjectPaths::resolve(&cwd)` and `self.active_runs`/`self.run_statuses`/`self.emitter` → the struct's fields. `AgentdError` returns become `String` (use `.map_err(|e| e.to_string())` / `format!`).

```rust
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

use crate::projects::ProjectPaths;
use crate::run_bridge::RunEventBridge;
use crate::seams::EventEmitter;

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
        -> Result<String, String> {
        let cwd = PathBuf::from(project);
        let paths = ProjectPaths::resolve(&cwd);

        // 1. Load flow doc + roster.
        let flows_root = oxidemx_conductor::loader::default_flows_root();
        let agents_root = oxidemx_conductor::loader::default_agents_root();
        let (doc, roster) =
            oxidemx_conductor::loader::load_flow(&flows_root, &agents_root, flow_id)
                .map_err(|e| e.to_string())?;

        // 2. Validate plan.
        let plan = oxidemx_conductor::plan::validate(&doc, &roster, oxidemx_conductor::KNOWN_TOOLS)
            .map_err(|errs| {
                let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
                format!("invalid flow: {}", msgs.join("; "))
            })?;

        // 3. Parse inputs JSON → BTreeMap<String, String>.
        let provided: BTreeMap<String, String> =
            if inputs_json.trim().is_empty() || inputs_json.trim() == "{}" {
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

        // 4. Resolve inputs.
        let inputs = oxidemx_conductor::supervisor::resolve_inputs(&plan, &provided)
            .map_err(|missing| format!("missing required inputs: {}", missing.join(", ")))?;

        // 5. run_id + workdir.
        let run_id = {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(1);
            format!("run-{}", NEXT.fetch_add(1, Ordering::Relaxed))
        };
        let workdir = paths.runs_dir().join(&run_id);
        std::fs::create_dir_all(&workdir).map_err(|e| e.to_string())?;

        // 6. Provider factory (mock under OXIDEMX_TEST_MOCK_FLOW).
        let factory: Arc<dyn oxidemx_conductor::supervisor::ProviderFactory> =
            if std::env::var_os("OXIDEMX_TEST_MOCK_FLOW").is_some() {
                Arc::new(oxidemx_conductor::supervisor::FixedFactory(
                    oxidemx_conductor::mock::MockProvider::echoing(),
                ))
            } else {
                let ai_cfg = oxidemx_shared::config::AiConfig::default();
                let api_key = ai_cfg
                    .provider
                    .key_env()
                    .and_then(|env| std::env::var(env).ok())
                    .unwrap_or_default();
                Arc::new(oxidemx_conductor::supervisor::ConfigFactory {
                    provider: ai_cfg.provider,
                    api_key,
                })
            };

        // 7. Cancel token + RunOptions.
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

        // 8. Register handle.
        let handle = oxidemx_conductor::RunHandle { run_id: run_id.clone(), cancel: cancel.clone() };
        {
            let mut guard = self.active_runs.lock().unwrap_or_else(|e| e.into_inner());
            guard.insert(run_id.clone(), handle);
        }

        // 9. Spawn supervisor; bridge streams events + populates run_statuses.
        let bridge = Arc::new(RunEventBridge::new(project, self.emitter.clone(), self.run_statuses.clone()));
        let active_runs = self.active_runs.clone();
        let rid = run_id.clone();
        tokio::spawn(async move {
            oxidemx_conductor::supervisor::run_flow(&plan, opts, bridge).await;
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
```

> Before implementing, open `agentd/src/interface.rs` `run_flow` (≈ lines 547–672), `run_status` (≈ 715–724), and `list_runs` (≈ 728–740) and confirm the bodies match what's transcribed above (the source may have drifted). Transcribe the CURRENT source, not this snapshot, where they differ. Confirm `ProjectPaths::resolve(&Path)` and `paths.runs_dir()` exist (used in `tools/mod.rs` + `interface.rs`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_launcher 2>&1 | tail -20`
Expected: PASS — `status_reads_ground_truth_from_table`, `launch_unknown_flow_errors`, plus Task 1's tests.

- [ ] **Step 5: Commit**

```bash
git add agentd/src/run_launcher.rs
git commit -m "feat(agentd): ConductorRunLauncher (real RunLauncher over the conductor)"
```

---

### Task 3: `AgentService` delegates to `ConductorRunLauncher` (no split-brain)

**Files:**
- Modify: `agentd/src/interface.rs` (`AgentService` struct + `new` + `run_flow`/`run_status`/`list_runs` bodies)

**Interfaces:**
- Consumes: `ConductorRunLauncher`, `RunLauncher`, `RunStatus` (Tasks 1–2).
- Produces: `AgentService` gains `pub run_launcher: Arc<ConductorRunLauncher>`; its `run_flow`/`run_status`/`list_runs` become thin delegators. Behaviour (and existing tests) unchanged.

- [ ] **Step 1: Run the existing run_flow/status tests to capture the green baseline**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_flow 2>&1 | tail -20; cargo test -p agentd run_status 2>&1 | tail -10`
Expected: PASS (records the baseline these delegators must preserve). Note the test names that exercise these methods.

- [ ] **Step 2: Add the `run_launcher` field + build it in `new`**

In `agentd/src/interface.rs`, add to the `AgentService` struct (after `run_statuses`):

```rust
    /// Single source of truth for launching + querying runs. The D-Bus methods
    /// below delegate to it; the agent tools share the same instance.
    pub run_launcher: Arc<crate::run_launcher::ConductorRunLauncher>,
```

In `AgentService::new`, after `run_statuses` is created, build the launcher from the same Arcs and store it. Replace the tail of `new` so the Arcs are named before the struct literal:

```rust
        let active_runs = Arc::new(Mutex::new(HashMap::new()));
        let run_statuses = Arc::new(Mutex::new(HashMap::new()));
        let run_launcher = Arc::new(crate::run_launcher::ConductorRunLauncher::new(
            active_runs.clone(),
            run_statuses.clone(),
            emitter.clone(),
        ));
        Self {
            projects,
            sessions,
            models,
            approver,
            emitter,
            host,
            turn_runner: Arc::new(CoreTurnRunner),
            active_runs,
            run_statuses,
            run_launcher,
        }
```

> If other `AgentService { … }` struct literals exist (e.g. a test constructor near `interface.rs:1224`), add `run_launcher` there too — build it from that literal's `active_runs`/`run_statuses`/`emitter`. Search the file for `run_statuses:` to find them all.

- [ ] **Step 3: Replace the three method bodies with delegation**

`run_flow`:

```rust
    pub async fn run_flow(&self, project: &str, flow_id: &str, inputs_json: &str)
        -> Result<String, AgentdError> {
        use crate::run_launcher::RunLauncher;
        self.run_launcher
            .launch(project, flow_id, inputs_json)
            .await
            .map_err(AgentdError::NotFound)
    }
```

`run_status` (preserve the existing "unknown run_id" `NotFound` contract):

```rust
    pub async fn run_status(&self, run_id: &str) -> Result<String, AgentdError> {
        use crate::run_launcher::RunLauncher;
        self.run_launcher
            .status(run_id)
            .map(|s| s.as_str().to_string())
            .ok_or_else(|| AgentdError::NotFound(format!("unknown run_id: {run_id}")))
    }
```

`list_runs`:

```rust
    pub async fn list_runs(&self, project: &str) -> Result<Vec<String>, AgentdError> {
        use crate::run_launcher::RunLauncher;
        Ok(self.run_launcher.list_runs(project))
    }
```

Delete the now-moved imports if they became unused (`BTreeMap`, `CancellationToken`, `RunEventBridge`) only if the compiler flags them unused — they may still be used elsewhere in `interface.rs`. Let `cargo build` warnings guide removal.

- [ ] **Step 4: Verify the baseline tests still pass (regression gate)**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_flow 2>&1 | tail -20; cargo test -p agentd run_status 2>&1 | tail -10`
Expected: PASS — same tests as Step 1, now exercising the delegators. (The `run_flow` mock-launch test uses `OXIDEMX_TEST_MOCK_FLOW`.)

- [ ] **Step 5: Commit**

```bash
git add agentd/src/interface.rs
git commit -m "refactor(agentd): AgentService.run_flow/status/list_runs delegate to RunLauncher"
```

---

### Task 4: Thread `Arc<dyn RunLauncher>` into `AgentToolExecutor` + `run_turn`

**Files:**
- Modify: `agentd/src/tools/mod.rs` (`AgentToolExecutor` struct + `new`)
- Modify: `agentd/src/interface.rs` (`TurnRunner::run_turn` trait sig + `CoreTurnRunner` impl + the mock impl ≈ line 1107 + the `send_message` call site that invokes `run_turn`)
- Modify: `agentd/src/harness/worker.rs` (the `AgentToolExecutor::new` call ≈ line 64)
- Modify: `agentd/src/tools/agent.rs` (the test `AgentToolExecutor::new` ≈ line 497)

**Interfaces:**
- Consumes: `RunLauncher`, `NoopRunLauncher` (Task 1).
- Produces: `AgentToolExecutor` gains `pub(crate) run_launcher: Arc<dyn RunLauncher>`; `new(paths, host, run_launcher)`. `TurnRunner::run_turn` gains a final `run_launcher: &Arc<dyn crate::run_launcher::RunLauncher>` parameter.

- [ ] **Step 1: Add the field + constructor param (with a mock-launcher dispatch test)**

In `agentd/src/tools/mod.rs`, extend the struct + `new`:

```rust
pub struct AgentToolExecutor {
    pub(crate) paths: ProjectPaths,
    pub(crate) host: Arc<dyn HostCapability>,
    pub(crate) run_launcher: Arc<dyn crate::run_launcher::RunLauncher>,
}

impl AgentToolExecutor {
    pub fn new(
        paths: ProjectPaths,
        host: Arc<dyn HostCapability>,
        run_launcher: Arc<dyn crate::run_launcher::RunLauncher>,
    ) -> Self {
        Self { paths, host, run_launcher }
    }
}
```

Update the test helper `test_executor` (`tools/mod.rs` ≈ line 94) to pass `Arc::new(crate::run_launcher::NoopRunLauncher)`.

- [ ] **Step 2: Add the failing dispatch test** (in `tools/mod.rs` tests)

```rust
    #[tokio::test]
    async fn run_status_tool_reports_ground_truth() {
        use oxidemx_agent_core::tool::ToolExecutor;
        use crate::run_launcher::{RunLauncher, RunStatus};
        use async_trait::async_trait;

        struct FakeLauncher;
        #[async_trait]
        impl RunLauncher for FakeLauncher {
            async fn launch(&self, _p: &str, _f: &str, _i: &str) -> Result<String, String> {
                Ok("run-42".into())
            }
            fn status(&self, run_id: &str) -> Option<RunStatus> {
                (run_id == "run-42").then_some(RunStatus::Running)
            }
            fn list_runs(&self, _p: &str) -> Vec<String> { vec!["run-42".into()] }
        }

        let dir = tempfile::tempdir().unwrap();
        let exec = AgentToolExecutor::new(
            ProjectPaths::resolve(dir.path()),
            Arc::new(UnavailableHost),
            Arc::new(FakeLauncher),
        );
        let out = exec
            .execute("run_status", serde_json::json!({"run_id": "run-42"}), &None)
            .await
            .unwrap();
        assert!(out.contains("running"), "got: {out}");
        let unknown = exec
            .execute("run_status", serde_json::json!({"run_id": "run-9"}), &None)
            .await
            .unwrap();
        assert!(unknown.to_lowercase().contains("no") || unknown.contains("unknown"), "got: {unknown}");
    }
```

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_status_tool_reports_ground_truth 2>&1 | tail -20`
Expected: FAIL — `unknown tool: run_status` (the tool is added in Task 5). This test stays red until Task 5; that is expected. (Do not delete it — Task 5 turns it green.)

> Right-sizing note: this test belongs to Task 5's behaviour but is placed here so Task 4's constructor wiring is exercised. If your reviewer prefers, move it into Task 5 Step 1. Either way it must be green by end of Task 5.

- [ ] **Step 3: Thread `run_launcher` through `run_turn` + all construction sites**

(a) `agentd/src/interface.rs` — add the parameter to the `TurnRunner` trait method (after `host`):

```rust
        host: &Arc<dyn crate::seams::HostCapability>,
        run_launcher: &Arc<dyn crate::run_launcher::RunLauncher>,
    ) -> Result<(String, (u64, u64)), AgentdError>;
```

(b) `CoreTurnRunner::run_turn` (≈ line 104) — add the same param, and pass it into the executor (≈ line 133):

```rust
        std::sync::Arc::new(crate::tools::AgentToolExecutor::new(
            paths.clone(),
            host.clone(),
            run_launcher.clone(),
        ));
```

(c) The mock `TurnRunner` impl (≈ line 1107) — add the param and ignore it (`let _ = run_launcher;`).

(d) The `send_message` body that calls `self.turn_runner.run_turn(...)` — pass `&(self.run_launcher.clone() as Arc<dyn crate::run_launcher::RunLauncher>)`. (Find the `run_turn(` call in `send_message`; add the argument in the same order as the trait.)

(e) `agentd/src/harness/worker.rs` (≈ line 64) — pass `Arc::new(crate::run_launcher::NoopRunLauncher)` as the third arg (the harness does not launch flows from the model in this slice; marked for SP2d).

(f) `agentd/src/tools/agent.rs` test constructor (≈ line 497) — pass `Arc::new(crate::run_launcher::NoopRunLauncher)`.

- [ ] **Step 4: Build (expect Task-5 test still red, everything else green)**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build -p agentd 2>&1 | tail -20 && cargo test -p agentd 2>&1 | grep -E "test result|error\[|run_status_tool" | tail -20`
Expected: `cargo build` clean; full suite green EXCEPT `run_status_tool_reports_ground_truth` (still failing on `unknown tool: run_status` — Task 5 fixes it).

- [ ] **Step 5: Commit**

```bash
git add agentd/src/tools/mod.rs agentd/src/interface.rs agentd/src/harness/worker.rs agentd/src/tools/agent.rs
git commit -m "feat(agentd): thread RunLauncher into AgentToolExecutor via run_turn"
```

---

### Task 5: Real `run_flow` tool + `run_status`/`list_runs` tools

**Files:**
- Modify: `agentd/src/tools/agent.rs` (replace `run_flow` body; add `run_status` + `list_runs` fns)
- Modify: `agentd/src/tools/mod.rs` (dispatch the three tools through `self.run_launcher`)
- Modify: `oxidemx-agent-core/src/mode.rs` (add `run_status` + `list_runs` tool declarations in `tools()`)

**Interfaces:**
- Consumes: `AgentToolExecutor.run_launcher` (Task 4); `RunLauncher` trait.
- Produces: tool bodies `run_flow(launcher, paths, args)`, `run_status(launcher, args)`, `list_runs(launcher, paths, args)` returning `Result<String, String>`.

- [ ] **Step 1: The dispatch test from Task 4 is the failing test** — confirm it's red

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_status_tool_reports_ground_truth 2>&1 | tail -10`
Expected: FAIL — `unknown tool: run_status`.

- [ ] **Step 2: Replace the `run_flow` stub + add the two query tools** (`agentd/src/tools/agent.rs`)

Replace the entire `run_flow` fn (≈ lines 184–206) with a launcher-backed version, and add the two query fns. The `project` is the cwd string (`paths.cwd`):

```rust
use crate::run_launcher::RunLauncher;

/// `run_flow` — declared keys: `flow_id`, `inputs_json` (optional).
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
```

Remove the stale module doc-comment about the `run_flow` "delegated to Task 4" stub (top of `agent.rs`, ≈ lines 17–27) — it no longer applies.

- [ ] **Step 3: Dispatch the three tools** (`agentd/src/tools/mod.rs`, in the `match name`)

Replace the `"run_flow"` arm and add two arms:

```rust
            "run_flow"     => agent::run_flow(&self.run_launcher, &self.paths, &args).await,
            "run_status"   => agent::run_status(&self.run_launcher, &args).await,
            "list_runs"    => agent::list_runs(&self.run_launcher, &self.paths, &args).await,
```

- [ ] **Step 4: Declare the two new tools** (`oxidemx-agent-core/src/mode.rs`, in `tools()` ≈ after the `run_flow` decl at line ~222)

```rust
        json!({
            "type": "function",
            "name": "run_status",
            "description": "Check the live status of a background flow run by its run_id (returned by run_flow). Returns the real status: running, finished, failed, or cancelled. ALWAYS call this before telling the user whether a run is done — never guess or claim a status you have not queried.",
            "parameters": {
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "The run id returned by run_flow, e.g. \"run-3\"" }
                },
                "required": ["run_id"]
            }
        }),
        json!({
            "type": "function",
            "name": "list_runs",
            "description": "List the ids of background flow runs for the current project. Use to find a run_id when the user refers to a run without giving its id.",
            "parameters": { "type": "object", "properties": {} }
        }),
```

> Also update the `run_flow` tool description (line ~203): drop the `mock` property from `parameters.properties` (and any `"mock"` mention) since `run_flow` no longer takes it. Leave `flow_id` + `inputs_json`.

- [ ] **Step 5: Run the tool tests to verify they pass**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd run_status_tool_reports_ground_truth 2>&1 | tail -10 && cargo test -p agentd 2>&1 | grep -E "test result|error\[" | tail -10`
Expected: PASS — `run_status_tool_reports_ground_truth` green; full suite green.

- [ ] **Step 6: Commit**

```bash
git add agentd/src/tools/agent.rs agentd/src/tools/mod.rs oxidemx-agent-core/src/mode.rs
git commit -m "feat(agentd): real run_flow tool + run_status/list_runs tools (ground-truth status)"
```

---

### Task 6: Truthfulness system-prompt rule (grounded narration)

**Files:**
- Modify: `oxidemx-agent-core/src/mode.rs` (`system_instruction_base`, ≈ line 349)

**Interfaces:**
- Consumes: nothing new.
- Produces: the assembled base system instruction contains the grounded-narration rule.

- [ ] **Step 1: Write the failing test** (in `oxidemx-agent-core/src/mode.rs` tests; add a `tests` module if none, else append)

```rust
    #[test]
    fn system_prompt_states_grounded_narration_rule() {
        // Build the base instruction the same way production does. `Mode`/`self`
        // here must match how `system_instruction_base` is invoked elsewhere in
        // this file's tests — copy that construction.
        let s = test_mode().system_instruction_base();
        assert!(s.contains("only from a tool result"), "missing grounding clause");
        assert!(s.contains("Narrate your actions"), "missing action/outcome clause");
        assert!(s.to_lowercase().contains("looking is not acting"), "missing looking-is-not-acting");
    }
```

> `test_mode()` is a placeholder for however `Mode` is constructed in this file's existing tests — search `mode.rs` for an existing call to `system_instruction_base(` or `system_instruction_async(` in tests and reuse that exact constructor. If there is no existing test constructor, build the minimal `Mode` value the method needs (check the `impl` block the method lives in).

- [ ] **Step 2: Run the test to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agent-core system_prompt_states_grounded_narration_rule 2>&1 | tail -20`
Expected: FAIL — the phrases are absent.

- [ ] **Step 3: Add the TRUTHFULNESS section to `system_instruction_base`**

Inside `system_instruction_base` (`oxidemx-agent-core/src/mode.rs` ≈ line 350), append a new section to the `base` string (after the FLOWS section, before MEMORY RULES — keep the existing `\n\n` section style):

```rust
             TRUTHFULNESS\n\
             State the status or result of a run, task, file, test, or command ONLY from a \
             tool result you received in THIS turn. If you do not have that result, call the \
             tool (e.g. run_status for a flow) or say you have not checked — never guess or \
             invent a status. Narrate your actions (\"I launched the flow, run-3\"), never \
             unobserved outcomes (\"it finished\", \"it's still running\") without a tool \
             result. Looking is not acting: read, search, and check freely.\n\n\
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agent-core system_prompt_states_grounded_narration_rule 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Full build + suite + clippy, then commit**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build -p agentd 2>&1 | tail -5 && cargo test -p agentd 2>&1 | grep "test result" && cargo test -p oxidemx-agent-core 2>&1 | grep "test result" && cargo clippy -p agentd -p oxidemx-agent-core 2>&1 | tail -5`
Expected: build clean, all suites green, clippy clean.

```bash
git add oxidemx-agent-core/src/mode.rs
git commit -m "feat(agent-core): grounded-narration truthfulness rule in system prompt"
```

---

## Post-plan: live verification (manual, after all tasks)

Rebuild + restart agentd, then in the overlay chat: ask it to run a flow → it returns a real `run-N`; ask "status?" → it calls `run_status` and reports the real status (running/finished/failed), no confabulation; ask about an unknown run → it says it can't find it rather than inventing. Commands:

```bash
CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build -p agentd --release
pkill -x oxidemx-agentd; RUST_LOG=info,oxidemx_agent_core=debug nohup /tmp/oxidemx-host-target/release/oxidemx-agentd > /tmp/oxidemx-agentd.log 2>&1 &
```

---

## Self-Review

**1. Spec coverage** (`docs/superpowers/specs/2026-06-20-truthful-visible-runs-design.md`):
- §3 `RunLauncher` seam + adapter → Tasks 1–2. ✅
- §3 real `run_flow` + `run_status`/`list_runs` tools → Task 5. ✅
- §3 threaded into `AgentToolExecutor` → Task 4. ✅
- §4 system-prompt grounded-narration rule → Task 6. ✅
- §4 `ResponseGuard` extension → explicitly a follow-on in the spec; NOT in this backend plan (tracked). ✅
- §5 activity UI → separate plan (out of scope here, per spec §8). ✅
- §2 truthfulness invariant → embodied by Tasks 5 (tool coverage) + 6 (prompt). ✅
- §7 testing (mock RunLauncher; run_flow returns real id; unknown→no fabrication) → Tasks 2,4,5. ✅

**2. Placeholder scan:** Two intentional "match the current source" notes (Task 2 body transcription, Task 6 `test_mode()` constructor) — these are accuracy guards against source drift, not placeholders; each names exactly what to confirm and where. No TBD/TODO-in-code, no "add error handling", all code blocks complete.

**3. Type consistency:** `RunLauncher`/`RunStatus`/`NoopRunLauncher`/`ConductorRunLauncher` consistent across tasks. `AgentToolExecutor::new(paths, host, run_launcher)` consistent (Tasks 4–5 + all 5 construction sites enumerated). `run_turn(... host, run_launcher)` param order consistent (trait + CoreTurnRunner + mock + send_message). `status(run_id) -> Option<RunStatus>` and the `"running"/"finished"/"failed"/"cancelled"` strings consistent (Tasks 1–3, 5).
