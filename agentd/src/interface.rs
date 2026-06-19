//! `AgentService` + `#[interface(name="org.oxidemx.Agent")]` — Task 6.
//!
//! `AgentService` is a plain struct whose async methods ARE the bus methods;
//! they are directly callable in tests without a live D-Bus connection.
//! The `#[interface]` impl is a thin delegation layer that translates between
//! zbus conventions (fdo::Result, SignalEmitter) and AgentService's error type.
//!
//! ## Seams
//!
//! - `TurnRunner`: injectable trait for running one conversation turn.
//!   Production impl (`CoreTurnRunner`) delegates to
//!   `oxidemx_agent_core::runtime::route_turn`. Tests use `MockTurnRunner`.
//!
//! ## Timestamp units
//!
//! - `JournalEntry.ts` — **milliseconds** (as documented in journal.rs)
//! - `AgentEvent.ts` — **milliseconds** (as documented in seams.rs)
//!
//! Both are stamped with `SystemTime::now()`.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::Value;
use zbus::{fdo, interface, object_server::SignalEmitter};

use crate::error::AgentdError;
use crate::journal::{Journal, JournalEntry};
use crate::models::ModelControls;
use crate::projects::{ProjectKey, ProjectPaths};
use crate::seams::{AgentEvent, Approver, EventEmitter, HostCapability, Verdict};
use crate::sessions::{Sessions, TranscriptTurn};

// ── Timestamp helpers ─────────────────────────────────────────────────────────

/// Current time in milliseconds since Unix epoch (for JournalEntry and AgentEvent).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── Turn-id generator (same style as runtime::new_session_id) ────────────────

fn new_turn_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("turn-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

// ── TurnRunner seam ───────────────────────────────────────────────────────────

/// Abstraction over the core turn-running logic, so tests can inject a mock
/// without needing a live LLM configuration or network.
#[async_trait]
pub trait TurnRunner: Send + Sync {
    /// Run a single conversation turn.
    ///
    /// - `project` — identifies the project (for scoping sessions, tools, etc.)
    /// - `thread` — the conversation thread id
    /// - `text` — the user's message text
    /// - `history` — prior turns for this thread as `(is_user, text)` pairs
    /// - `approver` — gate for tool-call approvals (may be awaited inside)
    /// - `emitter` — side-channel for streaming events
    ///
    /// Returns the assistant's reply text.
    async fn run_turn(
        &self,
        project: &ProjectKey,
        thread: &str,
        text: &str,
        history: &[(bool, String)],
        approver: &Arc<Approver>,
        emitter: &Arc<dyn EventEmitter>,
    ) -> Result<String, AgentdError>;
}

// ── CoreTurnRunner — production impl ─────────────────────────────────────────

/// Production `TurnRunner` that delegates to
/// `oxidemx_agent_core::runtime::route_turn`.
///
/// This impl is NOT unit-tested in this task (it requires a live config +
/// network). It is accepted as a thin wiring shim — the same discipline used
/// for the feature-gated mistral engine.
pub struct CoreTurnRunner;

#[async_trait]
impl TurnRunner for CoreTurnRunner {
    async fn run_turn(
        &self,
        _project: &ProjectKey,
        thread: &str,
        text: &str,
        history: &[(bool, String)],
        _approver: &Arc<Approver>,
        _emitter: &Arc<dyn EventEmitter>,
    ) -> Result<String, AgentdError> {
        use oxidemx_agent_core::mode::AgentMode;
        use oxidemx_agent_core::tool::ToolExecutor;
        use std::sync::Arc;

        // A no-op tool executor for agentd's turn path.
        // Tool execution in the full agentic loop will be wired in a later
        // task (SP1c) when the full tool bridge is in place.
        struct NoopExec;
        #[async_trait]
        impl ToolExecutor for NoopExec {
            async fn execute(
                &self,
                _name: &str,
                _args: Value,
                _sink: &Option<oxidemx_agent_core::events::StreamSink>,
            ) -> Result<String, String> {
                Ok(serde_json::json!({}).to_string())
            }
        }

        let exec: Arc<dyn ToolExecutor> = Arc::new(NoopExec);
        let session_id = format!("agentd:{thread}");
        let (reply, _) = oxidemx_agent_core::runtime::route_turn(
            AgentMode::Agentic,
            "", // model_hint: use config default
            text,
            None,        // no stream sink
            history,     // C2: feed prior transcript history to the model
            None,        // no image
            &session_id,
            &exec,
        )
        .await
        .map_err(|e| AgentdError::Io(e.to_string()))?;

        Ok(reply)
    }
}

// ── ProjectRegistry ───────────────────────────────────────────────────────────

/// A simple registry that resolves projects and provides a store base.
///
/// In production the store base follows `$XDG_DATA_HOME`; tests inject a
/// temp dir via `AgentService::for_test`.
pub struct ProjectRegistry {
    /// Override the data root so tests write under a temp dir instead of
    /// `~/.local/share`.
    store_base_override: Option<PathBuf>,
}

impl ProjectRegistry {
    pub fn new() -> Self {
        Self {
            store_base_override: None,
        }
    }

    pub fn with_store_base(base: impl Into<PathBuf>) -> Self {
        Self {
            store_base_override: Some(base.into()),
        }
    }

    /// Resolve paths for the project rooted at `cwd`.
    pub fn resolve(&self, cwd: &Path) -> ProjectPaths {
        if let Some(base) = &self.store_base_override {
            let key = ProjectKey::from_cwd(cwd);
            let store = base.join("projects").join(key.as_str());
            let local = cwd.join(".oxidemx");
            // Construct directly since ProjectPaths fields are public.
            ProjectPaths {
                key,
                cwd: cwd.to_path_buf(),
                store,
                local,
            }
        } else {
            ProjectPaths::resolve(cwd)
        }
    }

    /// List all project keys in the store base (directory names under `projects/`).
    pub fn list_project_keys(&self) -> Vec<String> {
        let base = match &self.store_base_override {
            Some(b) => b.join("projects"),
            None => {
                let data_dir = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|h| PathBuf::from(h).join(".local").join("share"))
                    })
                    .unwrap_or_else(|| PathBuf::from(".local/share"));
                data_dir.join("oxidemx").join("projects")
            }
        };

        let mut keys = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        keys.push(name.to_string());
                    }
                }
            }
        }
        keys
    }
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── AgentService ──────────────────────────────────────────────────────────────

/// The service struct — holds all state and provides async methods that ARE
/// the bus methods. Directly callable in unit tests without a live D-Bus
/// connection.
pub struct AgentService {
    pub projects: ProjectRegistry,
    pub sessions: Arc<Sessions>,
    pub models: Arc<ModelControls>,
    pub approver: Arc<Approver>,
    pub emitter: Arc<dyn EventEmitter>,
    pub host: Arc<dyn HostCapability>,
    pub turn_runner: Arc<dyn TurnRunner>,
    // SP1c: run_statuses removed (I3) — run_flow not yet wired so no runs
    // are ever tracked. run_status returns an honest "not yet wired" error
    // matching run_flow/cancel_run.
}

impl AgentService {
    /// Production constructor.
    pub fn new(
        projects: ProjectRegistry,
        sessions: Arc<Sessions>,
        models: Arc<ModelControls>,
        approver: Arc<Approver>,
        emitter: Arc<dyn EventEmitter>,
        host: Arc<dyn HostCapability>,
    ) -> Self {
        Self {
            projects,
            sessions,
            models,
            approver,
            emitter,
            host,
            turn_runner: Arc::new(CoreTurnRunner),
        }
    }

    // ── send_message ──────────────────────────────────────────────────────────

    /// Send a message to a project's conversation thread.
    ///
    /// Flow:
    /// 1. Resolve project paths.
    /// 2. Ensure session exists.
    /// 3. Read prior transcript history (before appending the new user turn).
    /// 4. Append user turn to the transcript.
    /// 5. Run the turn through `TurnRunner` with history for context.
    /// 6. Append assistant turn to the transcript.
    /// 7. Record a `JournalEntry::Turn`.
    /// 8. Emit an `AgentEvent`.
    /// 9. Return a new turn id.
    pub async fn send_message(
        &self,
        project: &str,
        thread: &str,
        text: &str,
        _model_hint: Option<&str>,
    ) -> Result<String, AgentdError> {
        let cwd = PathBuf::from(project);
        let paths = self.projects.resolve(&cwd);

        // Ensure session entry exists.
        self.sessions
            .session_manager()
            .session(&paths.key, thread);

        // Get/create transcript store.
        let ts = self
            .sessions
            .transcripts(&paths.key, paths.transcripts_dir());

        // C2: Read prior transcript history BEFORE appending the new user turn,
        // so history contains only prior context (not the current message).
        let prior_turns = ts.read(thread)?;
        let history: Vec<(bool, String)> = prior_turns
            .iter()
            .map(|t| (t.role == "user", t.text.clone()))
            .collect();

        // Append user turn.
        let ts_ms = now_ms();
        ts.append(
            thread,
            &TranscriptTurn {
                role: "user".into(),
                text: text.into(),
                ts: ts_ms,
            },
        )?;

        // Run the turn, passing prior history for multi-turn context.
        let reply = self
            .turn_runner
            .run_turn(
                &paths.key,
                thread,
                text,
                &history,
                &self.approver,
                &self.emitter,
            )
            .await?;

        // Append assistant turn.
        ts.append(
            thread,
            &TranscriptTurn {
                role: "assistant".into(),
                text: reply.clone(),
                ts: now_ms(),
            },
        )?;

        // Journal the turn (best-effort).
        let journal = Journal::new(paths.journal_path());
        journal
            .record(&JournalEntry::Turn {
                thread: thread.into(),
                prompt: text.into(),
                reply: reply.clone(),
                usage: (0, 0), // usage not tracked at this seam
                ts: now_ms(),
            })
            .ok();

        // Emit an agent event.
        let turn_id = new_turn_id();
        let preview = reply.chars().take(120).collect::<String>();
        self.emitter.emit(AgentEvent {
            project: paths.key.as_str().to_string(),
            thread_or_run: thread.into(),
            ts: now_ms(),
            payload: serde_json::json!({
                "kind": "Turn",
                "turn_id": turn_id,
                "thread": thread,
                "reply_preview": preview,
            }),
        });

        Ok(turn_id)
    }

    // ── cancel_turn ───────────────────────────────────────────────────────────

    /// Cancel an in-flight turn for `thread` in `project`.
    ///
    /// Delegates to the session cancellation token. The actual cancel
    /// is driven by the core runtime's select! loop.
    pub async fn cancel_turn(
        &self,
        _project: &str,
        _thread: &str,
    ) -> Result<(), AgentdError> {
        // The cancel token lives in the core runtime's SESSIONS static,
        // accessible via `oxidemx_agent_core::runtime::SESSIONS`.
        // We do not have a direct reference here without pulling in the full
        // oxidemx-agent stack. This is noted as "not yet wired" — it needs
        // SP1c's full tool-bridge task before it can be completed cleanly.
        Err(AgentdError::NotFound("cancel_turn: not yet wired (SP1c)".into()))
    }

    // ── get_transcript ────────────────────────────────────────────────────────

    pub async fn get_transcript(
        &self,
        project: &str,
        thread: &str,
    ) -> Result<Vec<TranscriptTurn>, AgentdError> {
        let cwd = PathBuf::from(project);
        let paths = self.projects.resolve(&cwd);
        let ts = self
            .sessions
            .transcripts(&paths.key, paths.transcripts_dir());
        ts.read(thread)
    }

    // ── list_threads ──────────────────────────────────────────────────────────

    pub async fn list_threads(&self, project: &str) -> Result<Vec<String>, AgentdError> {
        let cwd = PathBuf::from(project);
        let paths = self.projects.resolve(&cwd);
        let ts = self
            .sessions
            .transcripts(&paths.key, paths.transcripts_dir());
        ts.list_threads()
    }

    // ── list_projects ─────────────────────────────────────────────────────────

    /// List all known project key strings in the store.
    pub async fn list_projects(&self) -> Result<Vec<String>, AgentdError> {
        Ok(self.projects.list_project_keys())
    }

    // ── respond_approval ──────────────────────────────────────────────────────

    /// Deliver a verdict for a pending approval request.
    ///
    /// Also records the decision in the project journal (best-effort).
    pub async fn respond_approval(
        &self,
        project: &str,
        request_id: &str,
        allow: bool,
        reason: Option<&str>,
    ) -> Result<(), AgentdError> {
        let verdict = if allow {
            Verdict::Allow
        } else {
            Verdict::Deny(reason.unwrap_or("denied").to_string())
        };
        self.approver.respond(request_id, verdict);

        // Journal the approval decision (best-effort).
        let cwd = PathBuf::from(project);
        let paths = self.projects.resolve(&cwd);
        let journal = Journal::new(paths.journal_path());
        journal
            .record(&JournalEntry::Approval {
                request_id: request_id.into(),
                tool: String::new(), // tool name not available at this seam
                verdict: if allow { "allow".into() } else { "deny".into() },
                reason: reason.map(str::to_string),
                ts: now_ms(),
            })
            .ok();

        Ok(())
    }

    // ── optimize_prompt ───────────────────────────────────────────────────────

    pub async fn optimize_prompt(
        &self,
        _project: &str,
        draft: &str,
    ) -> Result<String, AgentdError> {
        oxidemx_agent_core::runtime::optimize_prompt("", draft)
            .await
            .map_err(|e| AgentdError::Io(e.to_string()))
    }

    // ── run_flow ──────────────────────────────────────────────────────────────

    /// Launch a conductor flow scoped to the project's `runs_dir`.
    ///
    /// Requires the conductor's flows root and the flow id. This is a minimal
    /// wiring: a full integration needs the flow-event bridge (emitting run
    /// events back through the `EventEmitter`). That wiring is deferred to
    /// SP1c so we avoid a large compilation-time dependency here.
    pub async fn run_flow(
        &self,
        _project: &str,
        _flow_id: &str,
        _inputs_json: &str,
    ) -> Result<String, AgentdError> {
        // Not yet wired: conductor run_flow requires a full ProviderFactory
        // (needs config + key) and an EventSink. The seam will be completed
        // in SP1c when the D-Bus EventSink bridge (Task 8) is in place.
        Err(AgentdError::NotFound(
            "run_flow: not yet wired (SP1c + Task 8 EventSink bridge needed)".into(),
        ))
    }

    // ── list_flows ────────────────────────────────────────────────────────────

    /// List available flow ids under the project's flows root (or the global
    /// default).
    pub async fn list_flows(&self, _project: &str) -> Result<Vec<String>, AgentdError> {
        let flows_root = oxidemx_conductor::loader::default_flows_root();
        Ok(oxidemx_conductor::loader::list_flows(&flows_root))
    }

    // ── validate_flow ─────────────────────────────────────────────────────────

    pub async fn validate_flow(
        &self,
        _project: &str,
        flow_id: &str,
    ) -> Result<String, AgentdError> {
        let flows_root = oxidemx_conductor::loader::default_flows_root();
        let agents_root = oxidemx_conductor::loader::default_agents_root();
        match oxidemx_conductor::loader::load_flow(&flows_root, &agents_root, flow_id) {
            Ok((doc, roster)) => {
                match oxidemx_conductor::plan::validate(
                    &doc,
                    &roster,
                    oxidemx_conductor::KNOWN_TOOLS,
                ) {
                    Ok(_) => Ok("valid".into()),
                    Err(errs) => {
                        let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
                        Ok(format!("invalid: {}", msgs.join("; ")))
                    }
                }
            }
            Err(e) => Err(AgentdError::NotFound(e.to_string())),
        }
    }

    // ── run_status ────────────────────────────────────────────────────────────

    /// I3: run_flow is not yet wired (SP1c), so no runs are ever tracked.
    /// Returns the same honest "not yet wired" error as run_flow and cancel_run
    /// instead of a misleading NotFound for a specific run_id.
    pub async fn run_status(&self, _run_id: &str) -> Result<String, AgentdError> {
        Err(AgentdError::NotFound(
            "run_status: not yet wired (SP1c + Task 8 EventSink bridge needed)".into(),
        ))
    }

    // ── list_runs ─────────────────────────────────────────────────────────────

    pub async fn list_runs(&self, project: &str) -> Result<Vec<String>, AgentdError> {
        let cwd = PathBuf::from(project);
        let paths = self.projects.resolve(&cwd);
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
        Ok(ids)
    }

    // ── cancel_run ────────────────────────────────────────────────────────────

    pub async fn cancel_run(&self, run_id: &str) -> Result<(), AgentdError> {
        // Not yet wired: conductor cancel requires the RunHandle's CancellationToken.
        // RunHandle management will be in SP1c.
        Err(AgentdError::NotFound(format!(
            "cancel_run({run_id}): not yet wired (SP1c)"
        )))
    }

    // ── list_agents ───────────────────────────────────────────────────────────

    pub async fn list_agents(&self, _project: &str) -> Result<Vec<String>, AgentdError> {
        let agents_root = oxidemx_conductor::loader::default_agents_root();
        match oxidemx_conductor::loader::load_roster(&agents_root) {
            Ok(roster) => Ok(roster.ids().map(str::to_string).collect()),
            Err(e) => Err(AgentdError::NotFound(e.to_string())),
        }
    }

    // ── Model controls ────────────────────────────────────────────────────────

    pub async fn load_model(&self, alias: &str) -> Result<(), AgentdError> {
        self.models.load(alias).await
    }

    pub async fn unload_model(&self, alias: &str) -> Result<(), AgentdError> {
        self.models.unload(alias).await
    }

    pub async fn set_active_model(&self, alias: &str) -> Result<(), AgentdError> {
        self.models.set_active(alias).await
    }

    pub async fn list_models(&self) -> Result<String, AgentdError> {
        let infos = self.models.list();
        serde_json::to_string(&infos).map_err(|e| AgentdError::Io(e.to_string()))
    }
}

// ── D-Bus interface ───────────────────────────────────────────────────────────

/// A thin wrapper around `AgentService` that satisfies zbus's `#[interface]`
/// requirements (single impl block, `&self` methods, fdo::Result return types).
pub struct AgentInterface {
    pub svc: Arc<AgentService>,
}

impl AgentInterface {
    pub fn new(svc: Arc<AgentService>) -> Self {
        Self { svc }
    }
}

/// Map an `AgentdError` to a zbus `fdo::Error`.
fn to_fdo(e: AgentdError) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

#[interface(name = "org.oxidemx.Agent")]
impl AgentInterface {
    // ── Chat methods ──────────────────────────────────────────────────────────

    async fn send_message(
        &self,
        project: &str,
        thread: &str,
        text: &str,
        model_hint: &str,
    ) -> fdo::Result<String> {
        let hint = if model_hint.is_empty() {
            None
        } else {
            Some(model_hint)
        };
        self.svc
            .send_message(project, thread, text, hint)
            .await
            .map_err(to_fdo)
    }

    async fn cancel_turn(&self, project: &str, thread: &str) -> fdo::Result<()> {
        self.svc.cancel_turn(project, thread).await.map_err(to_fdo)
    }

    async fn get_transcript(&self, project: &str, thread: &str) -> fdo::Result<String> {
        let turns = self
            .svc
            .get_transcript(project, thread)
            .await
            .map_err(to_fdo)?;
        serde_json::to_string(&turns).map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn list_threads(&self, project: &str) -> fdo::Result<Vec<String>> {
        self.svc.list_threads(project).await.map_err(to_fdo)
    }

    async fn list_projects(&self) -> fdo::Result<Vec<String>> {
        self.svc.list_projects().await.map_err(to_fdo)
    }

    async fn respond_approval(
        &self,
        project: &str,
        request_id: &str,
        allow: bool,
        reason: &str,
    ) -> fdo::Result<()> {
        let r = if reason.is_empty() { None } else { Some(reason) };
        self.svc
            .respond_approval(project, request_id, allow, r)
            .await
            .map_err(to_fdo)
    }

    async fn optimize_prompt(&self, project: &str, draft: &str) -> fdo::Result<String> {
        self.svc
            .optimize_prompt(project, draft)
            .await
            .map_err(to_fdo)
    }

    // ── Flow methods ──────────────────────────────────────────────────────────

    async fn run_flow(
        &self,
        project: &str,
        flow_id: &str,
        inputs_json: &str,
    ) -> fdo::Result<String> {
        self.svc
            .run_flow(project, flow_id, inputs_json)
            .await
            .map_err(to_fdo)
    }

    async fn list_flows(&self, project: &str) -> fdo::Result<Vec<String>> {
        self.svc.list_flows(project).await.map_err(to_fdo)
    }

    async fn validate_flow(&self, project: &str, flow_id: &str) -> fdo::Result<String> {
        self.svc
            .validate_flow(project, flow_id)
            .await
            .map_err(to_fdo)
    }

    async fn run_status(&self, run_id: &str) -> fdo::Result<String> {
        self.svc.run_status(run_id).await.map_err(to_fdo)
    }

    async fn list_runs(&self, project: &str) -> fdo::Result<Vec<String>> {
        self.svc.list_runs(project).await.map_err(to_fdo)
    }

    async fn cancel_run(&self, run_id: &str) -> fdo::Result<()> {
        self.svc.cancel_run(run_id).await.map_err(to_fdo)
    }

    async fn list_agents(&self, project: &str) -> fdo::Result<Vec<String>> {
        self.svc.list_agents(project).await.map_err(to_fdo)
    }

    // ── Model controls ────────────────────────────────────────────────────────

    async fn load_model(&self, alias: &str) -> fdo::Result<()> {
        self.svc.load_model(alias).await.map_err(to_fdo)
    }

    async fn unload_model(&self, alias: &str) -> fdo::Result<()> {
        self.svc.unload_model(alias).await.map_err(to_fdo)
    }

    async fn set_active_model(&self, alias: &str) -> fdo::Result<()> {
        self.svc.set_active_model(alias).await.map_err(to_fdo)
    }

    async fn list_models(&self) -> fdo::Result<String> {
        self.svc.list_models().await.map_err(to_fdo)
    }

    // ── Signals ───────────────────────────────────────────────────────────────

    /// Fired whenever an agent event (turn started, turn done, tool call, etc.)
    /// occurs. `payload` is a JSON string.
    #[zbus(signal)]
    pub async fn event(
        emitter: &SignalEmitter<'_>,
        project: String,
        thread_or_run: String,
        ts: u64,
        payload: String,
    ) -> zbus::Result<()>;

    /// Fired when the agent runtime is waiting for human approval of a tool
    /// call. `card` is a JSON string describing the tool and its args.
    #[zbus(signal)]
    pub async fn approval_requested(
        emitter: &SignalEmitter<'_>,
        project: String,
        thread: String,
        request_id: String,
        card: String,
    ) -> zbus::Result<()>;

    /// Fired after every model lifecycle change (load / unload / set_active).
    /// `status` is a JSON string (alias + state).
    #[zbus(signal)]
    pub async fn model_status_changed(
        emitter: &SignalEmitter<'_>,
        alias: String,
        status: String,
    ) -> zbus::Result<()>;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seams::{RecordingEmitter, UnavailableHost};
    use async_trait::async_trait;
    use oxidemx_agent_local::error::LocalError;
    use oxidemx_agent_local::types::{ChatRequest, ChatResponse, ModelState, ModelStatusInfo, Usage};
    use oxidemx_agent_local::Verdict as LocalVerdict;
    use std::sync::{Arc, Mutex};

    // ── StubLocalService (from models.rs tests, replicated here) ─────────────

    struct StubLocalService {
        loaded: Mutex<Vec<String>>,
    }

    impl StubLocalService {
        fn new() -> Self {
            Self {
                loaded: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl oxidemx_agent_local::LocalModelService for StubLocalService {
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, LocalError> {
            Ok(ChatResponse {
                text: String::new(),
                usage: Usage::default(),
                verdict: LocalVerdict::Ok,
            })
        }
        async fn chat_with_model(
            &self,
            _alias: &str,
            _req: ChatRequest,
        ) -> Result<ChatResponse, LocalError> {
            Ok(ChatResponse {
                text: String::new(),
                usage: Usage::default(),
                verdict: LocalVerdict::Ok,
            })
        }
        async fn ensure_loaded(&self, alias: &str) -> Result<(), LocalError> {
            let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            if !g.contains(&alias.to_string()) {
                g.push(alias.to_string());
            }
            Ok(())
        }
        async fn unload(&self, alias: &str) -> Result<(), LocalError> {
            let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            g.retain(|a| a != alias);
            Ok(())
        }
        async fn set_active(&self, _alias: &str) -> Result<(), LocalError> {
            Ok(())
        }
        fn status(&self) -> Vec<ModelStatusInfo> {
            let g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            g.iter()
                .map(|alias| ModelStatusInfo {
                    alias: alias.clone(),
                    state: ModelState::Ready,
                    last_used: None,
                })
                .collect()
        }
    }

    // ── MockTurnRunner ────────────────────────────────────────────────────────

    /// A configurable mock TurnRunner for tests.
    ///
    /// Default behaviour: returns a fixed canned reply.
    /// With `request_approval = true`: calls `approver.request(...)` once
    /// before returning the reply, so the test can drive `respond_approval`.
    pub struct MockTurnRunner {
        reply: String,
        /// If true, request an approval before returning.
        request_approval: bool,
    }

    impl MockTurnRunner {
        pub fn new(reply: impl Into<String>) -> Self {
            Self {
                reply: reply.into(),
                request_approval: false,
            }
        }

        pub fn with_approval(reply: impl Into<String>) -> Self {
            Self {
                reply: reply.into(),
                request_approval: true,
            }
        }
    }

    #[async_trait]
    impl TurnRunner for MockTurnRunner {
        async fn run_turn(
            &self,
            _project: &ProjectKey,
            thread: &str,
            _text: &str,
            _history: &[(bool, String)],
            approver: &Arc<Approver>,
            _emitter: &Arc<dyn EventEmitter>,
        ) -> Result<String, AgentdError> {
            if self.request_approval {
                approver
                    .request(
                        "test-project",
                        thread,
                        serde_json::json!({"tool": "mock_tool", "args": {}}),
                    )
                    .await;
                // After approval (or denial), we still return our canned reply.
            }
            Ok(self.reply.clone())
        }
    }

    // ── TestEnv ───────────────────────────────────────────────────────────────

    /// Test harness: builds an `AgentService` with mock wiring + temp dirs.
    pub struct TestEnv {
        pub svc: Arc<AgentService>,
        pub emitter: Arc<RecordingEmitter>,
        pub approver: Arc<Approver>,
        _tmp: tempfile::TempDir, // kept alive; dropped last
        cwd: PathBuf,
    }

    impl TestEnv {
        pub fn new() -> Self {
            Self::with_runner(MockTurnRunner::new("mock assistant reply"))
        }

        pub fn with_approval_runner() -> Self {
            Self::with_runner(MockTurnRunner::with_approval("mock reply after approval"))
        }

        fn with_runner(runner: MockTurnRunner) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let store_base = tmp.path().join("store");
            let cwd = tmp.path().join("project");
            std::fs::create_dir_all(&cwd).unwrap();

            let emitter = Arc::new(RecordingEmitter::default());
            let approver = Arc::new(Approver::new(emitter.clone()));

            let stub_svc = Arc::new(StubLocalService::new());
            let models = Arc::new(ModelControls::new(stub_svc, emitter.clone()));

            let svc = Arc::new(AgentService {
                projects: ProjectRegistry::with_store_base(store_base),
                sessions: Arc::new(Sessions::new()),
                models,
                approver: approver.clone(),
                emitter: emitter.clone(),
                host: Arc::new(UnavailableHost),
                turn_runner: Arc::new(runner),
            });

            TestEnv {
                svc,
                emitter,
                approver,
                _tmp: tmp,
                cwd,
            }
        }

        pub fn cwd(&self) -> &Path {
            &self.cwd
        }

        pub fn cwd_str(&self) -> &str {
            self.cwd.to_str().unwrap()
        }

        pub fn key_str(&self) -> String {
            ProjectKey::from_cwd(&self.cwd).as_str().to_string()
        }
    }

    // ── Test 1: send_message appends transcript, emits, and journals ──────────

    #[tokio::test]
    async fn send_message_appends_transcript_and_emits_and_journals() {
        let env = TestEnv::new();
        let turn_id = env
            .svc
            .send_message(env.cwd_str(), "t1", "hello", None)
            .await
            .unwrap();
        assert!(!turn_id.is_empty(), "turn_id should not be empty");

        // Transcript has user + assistant turns.
        let turns = env
            .svc
            .get_transcript(env.cwd_str(), "t1")
            .await
            .unwrap();
        assert!(
            turns.len() >= 2,
            "expected at least 2 turns, got {}",
            turns.len()
        );
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[1].role, "assistant");

        // An event was emitted.
        assert!(
            !env.emitter.events().is_empty(),
            "expected at least one emitted event"
        );

        // Journal contains a Turn entry.
        let paths = env
            .svc
            .projects
            .resolve(env.cwd());
        let journal_text = std::fs::read_to_string(paths.journal_path()).unwrap();
        assert!(
            journal_text.contains("\"kind\":\"Turn\""),
            "journal should contain a Turn entry: {journal_text}"
        );
    }

    // ── Test 2: respond_approval round-trips and journals ─────────────────────

    #[tokio::test]
    async fn respond_approval_round_trips_and_journals() {
        let env = TestEnv::with_approval_runner();
        let cwd_str = env.cwd_str().to_string();
        let svc = env.svc.clone();
        let approver = env.approver.clone();

        // Spawn send_message — it will block inside the mock runner waiting
        // for approval.
        let svc2 = svc.clone();
        let cwd2 = cwd_str.clone();
        let handle = tokio::spawn(async move {
            svc2.send_message(&cwd2, "t-approval", "do the thing", None)
                .await
        });

        // Wait a bit for the runner to park on the approval request.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        // Discover the pending request id.
        let pending = approver.pending_ids();
        assert!(!pending.is_empty(), "expected a pending approval");
        let req_id = pending[0].clone();

        // Deny it via respond_approval.
        svc.respond_approval(&cwd_str, &req_id, false, Some("test denial"))
            .await
            .unwrap();

        // send_message should complete now.
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "send_message should succeed after denial");

        // Journal should contain an Approval entry.
        let paths = svc.projects.resolve(env.cwd());
        let journal_text = std::fs::read_to_string(paths.journal_path()).unwrap();
        assert!(
            journal_text.contains("\"kind\":\"Approval\""),
            "journal should contain an Approval entry: {journal_text}"
        );
    }

    // ── Test 3: list_projects and list_threads reflect activity ───────────────

    #[tokio::test]
    async fn list_projects_and_threads_reflect_activity() {
        let env = TestEnv::new();
        env.svc
            .send_message(env.cwd_str(), "t1", "hi", None)
            .await
            .unwrap();

        // list_projects should include the key for the active project.
        let projects = env.svc.list_projects().await.unwrap();
        let key = env.key_str();
        assert!(
            projects.iter().any(|p| p.contains(&key)),
            "expected project key {key} in {projects:?}"
        );

        // list_threads should return ["t1"].
        let threads = env.svc.list_threads(env.cwd_str()).await.unwrap();
        assert_eq!(threads, vec!["t1".to_string()]);
    }
}
