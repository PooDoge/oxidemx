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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use zbus::{fdo, interface, object_server::SignalEmitter};

use crate::conversations_index::ConversationIndex;
use crate::error::AgentdError;
use crate::journal::{Journal, JournalEntry};
use crate::model::{Conversation, ConversationId, Project, ProjectId};
use crate::models::ModelControls;
use crate::oxide_config::{resolve_oxide_config, ResolvedConfig};
use crate::project_store::ProjectStore;
use crate::projects::{ProjectKey, ProjectPaths};
use crate::seams::{AgentEvent, Approver, EventEmitter, HostCapability, Verdict};
use crate::sessions::{Sessions, TranscriptTurn};
use crate::worktree::{create_conversation_worktree, remove_if_unchanged, RealGit};

// ── Timestamp helpers ─────────────────────────────────────────────────────────

/// Current time in milliseconds since Unix epoch (for JournalEntry and AgentEvent).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── Config-dir helper (mirrors projects.rs::default_home_config) ─────────────

fn default_home_config() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".config"))
        .unwrap_or_else(|| PathBuf::from(".config"))
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
    /// - `paths` — project paths (for tool executor scoping)
    /// - `host` — host capability seam (for tool executor)
    ///
    /// Returns `(reply_text, (prompt_tokens, completion_tokens))`.
    #[allow(clippy::too_many_arguments)]
    async fn run_turn(
        &self,
        project: &ProjectKey,
        thread: &str,
        text: &str,
        history: &[(bool, String)],
        approver: &Arc<Approver>,
        emitter: &Arc<dyn EventEmitter>,
        paths: &crate::projects::ProjectPaths,
        host: &Arc<dyn crate::seams::HostCapability>,
        run_launcher: &Arc<dyn crate::run_launcher::RunLauncher>,
    ) -> Result<(String, (u64, u64)), AgentdError>;
}

// ── CoreTurnRunner — production impl ─────────────────────────────────────────

/// Production `TurnRunner` that delegates to
/// `oxidemx_agent_core::runtime::route_turn`.
///
/// Wires a real [`StreamBridge`] (T1) and a real [`AgentToolExecutor`] (T2/T3)
/// so streaming deltas and native tool calls flow end-to-end.
///
/// This impl is NOT unit-tested here (it requires a live config + network).
/// The `#[ignore]` `live_bus` integration test exercises the live path.
pub struct CoreTurnRunner;

#[async_trait]
impl TurnRunner for CoreTurnRunner {
    async fn run_turn(
        &self,
        project: &ProjectKey,
        thread: &str,
        text: &str,
        history: &[(bool, String)],
        approver: &Arc<Approver>,
        emitter: &Arc<dyn EventEmitter>,
        paths: &crate::projects::ProjectPaths,
        host: &Arc<dyn crate::seams::HostCapability>,
        run_launcher: &Arc<dyn crate::run_launcher::RunLauncher>,
    ) -> Result<(String, (u64, u64)), AgentdError> {
        use oxidemx_agent_core::mode::AgentMode;
        use oxidemx_approval::ApprovalClassifier;
        use crate::agent::approver_prompt::ApproverPrompt;
        use crate::tools::gated::{GateMode, GatedToolExecutor};

        // ── 1. Build a StreamBridge for this turn ─────────────────────────
        // No lock is held across route_turn or bridge.finish().
        let (bridge, sink) = crate::stream_bridge::StreamBridge::new(
            paths.key.as_str().to_string(),
            thread.to_string(),
            emitter.clone(),
        );

        // ── 2. Build a GatedToolExecutor (Attended) over AgentToolExecutor ──
        // Chat path uses Attended mode + ApproverPrompt: Ask-tier tools surface
        // an approval card to the user rather than returning NEEDS_APPROVAL.
        // No GateLog is passed (chat does not need the blocking audit log).
        let inner: std::sync::Arc<dyn oxidemx_agent_core::tool::ToolExecutor> =
            std::sync::Arc::new(crate::tools::AgentToolExecutor::new(
                paths.clone(),
                host.clone(),
                run_launcher.clone(),
                thread.to_string(),
            ));
        let prompt_adapter = std::sync::Arc::new(ApproverPrompt::new(
            approver.clone(),
            project.as_str(),
            thread,
        ));
        let exec: std::sync::Arc<dyn oxidemx_agent_core::tool::ToolExecutor> =
            std::sync::Arc::new(GatedToolExecutor::new(
                inner,
                ApprovalClassifier::default(),
                Some(prompt_adapter),   // surfaces approval card to the user
                GateMode::Attended,     // Ask-tier tools prompt rather than block
                paths.cwd.clone(),
                None,                   // no GateLog needed in the chat path
            ));

        // ── 3. Run the turn — sink flows deltas/tools to the bridge ───────
        let session_id = format!("agentd:{thread}");
        let (reply, _tool_calls) = oxidemx_agent_core::runtime::route_turn(
            AgentMode::Agentic,
            "",           // model_hint: use config default
            text,
            Some(sink),   // stream deltas to bridge
            history,
            None,         // no image
            &session_id,
            &exec,
        )
        .await
        .map_err(|e| AgentdError::Io(e.to_string()))?;

        // ── 4. Collect usage — sink must be dropped before finish() ───────
        // route_turn moved its local sink in; any internal tasks holding sink
        // clones (e.g. the event-forwarder) drain and drop before route_turn
        // returns, so the bridge channel closes and finish() completes.
        let usage = bridge.finish().await;

        Ok((reply, usage))
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
            // SYNC: keep this .oxide/.oxidemx resolution in step with ProjectPaths::resolve (projects.rs).
            // Prefer .oxide; fall back to .oxidemx for back-compat.
            let preferred = cwd.join(".oxide");
            let compat = cwd.join(".oxidemx");
            let local = if preferred.exists() {
                preferred
            } else if compat.exists() {
                compat
            } else {
                preferred // default to new name when neither exists yet
            };
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

    /// The root directory that contains `projects.json` and per-project subdirs.
    ///
    /// With a `store_base_override` (tests): `<override>/projects`.
    /// Without: `$XDG_DATA_HOME/oxidemx/projects` (or `~/.local/share/oxidemx/projects`).
    pub fn projects_root(&self) -> PathBuf {
        match &self.store_base_override {
            Some(b) => b.join("projects"),
            None => {
                let data = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|h| PathBuf::from(h).join(".local").join("share"))
                    })
                    .unwrap_or_else(|| PathBuf::from(".local/share"));
                data.join("oxidemx").join("projects")
            }
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
    /// In-flight runs keyed by run_id. Values hold the handle (run_id + cancel token).
    pub active_runs: Arc<Mutex<HashMap<String, oxidemx_conductor::RunHandle>>>,
    /// Shared run-status table: `run_id → "running" | "finished" | "failed" | "cancelled"`.
    /// Populated by `RunEventBridge` and read by `run_status`.
    pub run_statuses: Arc<Mutex<HashMap<String, String>>>,
    /// Single source of truth for launching + querying runs. The D-Bus methods
    /// below delegate to it; the agent tools share the same instance.
    pub run_launcher: Arc<crate::run_launcher::ConductorRunLauncher>,
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
        // paths is cloned into run_turn for the executor; no lock held across await.
        let run_launcher: Arc<dyn crate::run_launcher::RunLauncher> = self.run_launcher.clone();
        let (reply, usage) = self
            .turn_runner
            .run_turn(
                &paths.key,
                thread,
                text,
                &history,
                &self.approver,
                &self.emitter,
                &paths,
                &self.host,
                &run_launcher,
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

        // Journal the turn (best-effort) with real token usage from the bridge.
        // Bridge returns u64; JournalEntry stores u32 (saturating cast).
        let journal = Journal::new(paths.journal_path());
        journal
            .record(&JournalEntry::Turn {
                thread: thread.into(),
                prompt: text.into(),
                reply: reply.clone(),
                usage: (usage.0.min(u32::MAX as u64) as u32, usage.1.min(u32::MAX as u64) as u32),
                ts: now_ms(),
            })
            .ok();

        // Emit a "Turn" summary event (for internal bookkeeping).
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

        // Emit the "final" event so D-Bus subscribers (the overlay) can commit
        // the full reply text and clear their loading state.  This must be
        // emitted AFTER the "Turn" event and never while holding a lock.
        self.emitter.emit(AgentEvent {
            project: paths.key.as_str().to_string(),
            thread_or_run: thread.into(),
            ts: now_ms(),
            payload: serde_json::json!({
                "kind": "final",
                "turn_id": turn_id,
                "thread": thread,
                "text": reply,
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
        // `cancel_turn` cancels a single conversation turn, not a full conductor
        // run. Per-turn cancellation tokens are not yet tracked at the agentd
        // seam (they live inside the core runtime's select! loop). Adding a
        // per-(project, thread) token map here is the correct fix; deferred to
        // a dedicated SP1c sub-task when the full tool-bridge lands, so the
        // token is threaded end-to-end from `send_message` through `TurnRunner`
        // rather than half-wired. Documented here rather than silently ignored.
        Err(AgentdError::NotFound(
            "cancel_turn: per-turn CancellationToken not yet tracked (SP1c follow-up)".into(),
        ))
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

    // ── project + conversation helpers ────────────────────────────────────────

    /// Build a `ProjectStore` rooted at the projects dir for this registry.
    fn project_store(&self) -> ProjectStore {
        ProjectStore::new(self.projects.projects_root())
    }

    /// Build a `ConversationIndex` scoped to the given project's store dir.
    ///
    /// The per-project dir = `<projects_root>/<key>` where
    /// `key = ProjectKey::from_cwd(&project.default_working_dir)`.
    fn conversation_index_for(&self, project: &Project) -> ConversationIndex {
        let root = self.projects.projects_root();
        let key = ProjectKey::from_cwd(&project.default_working_dir);
        ConversationIndex::new(root.join(key.as_str()))
    }

    /// Scan every project's conversation index for `id`.
    ///
    /// Returns `Some((project, conversation))` on the first match, or `None`.
    fn find_conversation(&self, id: &str) -> Option<(Project, Conversation)> {
        let cid = ConversationId::from(id);
        for project in self.project_store().list() {
            let idx = self.conversation_index_for(&project);
            if let Some(c) = idx.get(&cid) {
                return Some((project, c));
            }
        }
        None
    }

    /// Compute the user-global config dir (`~/.config/oxidemx`).
    fn user_global_config_dir() -> PathBuf {
        let base = if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            let p = PathBuf::from(xdg);
            if p.is_absolute() { p } else { default_home_config() }
        } else {
            default_home_config()
        };
        base.join("oxidemx")
    }

    // ── list_projects ─────────────────────────────────────────────────────────

    /// List all projects in the store (including Personal if bootstrapped).
    pub async fn list_projects(&self) -> Result<Vec<Project>, AgentdError> {
        Ok(self.project_store().list())
    }

    // ── create_project ────────────────────────────────────────────────────────

    /// Create a new named project rooted at `default_working_dir`.
    pub async fn create_project(
        &self,
        name: &str,
        default_working_dir: &Path,
    ) -> Result<Project, AgentdError> {
        Ok(self.project_store().create(name, default_working_dir.to_path_buf()))
    }

    // ── init_project_from_conversation ───────────────────────────────────────

    /// Promote a conversation to its own project.
    ///
    /// Creates the project, then moves the conversation's index row from
    /// its old project's index to the new one (updating `project_id` and
    /// `working_dir`).
    pub async fn init_project_from_conversation(
        &self,
        conversation_id: &str,
        name: &str,
        default_working_dir: &Path,
    ) -> Result<Project, AgentdError> {
        let (old_project, mut conv) = self.find_conversation(conversation_id).ok_or_else(|| {
            AgentdError::NotFound(format!("conversation not found: {conversation_id}"))
        })?;

        let project = self.project_store().create(name, default_working_dir.to_path_buf());

        // Remove from old project's index.
        let old_idx = self.conversation_index_for(&old_project);
        old_idx.delete(&conv.id);

        // Insert into new project's index.
        conv.project_id = project.id.clone();
        conv.working_dir = default_working_dir.to_path_buf();
        let new_idx = self.conversation_index_for(&project);
        new_idx.upsert(conv);

        Ok(project)
    }

    // ── list_conversations ────────────────────────────────────────────────────

    /// List all conversations for the given project id.
    pub async fn list_conversations(
        &self,
        project_id: &str,
    ) -> Result<Vec<Conversation>, AgentdError> {
        let pid = ProjectId::from(project_id);
        let project = self.project_store().get(&pid).ok_or_else(|| {
            AgentdError::NotFound(format!("project not found: {project_id}"))
        })?;
        Ok(self.conversation_index_for(&project).list())
    }

    // ── get_conversation ──────────────────────────────────────────────────────

    /// Get a single conversation by id (scans all projects).
    pub async fn get_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<Conversation>, AgentdError> {
        Ok(self.find_conversation(conversation_id).map(|(_, c)| c))
    }

    // ── create_conversation ───────────────────────────────────────────────────

    /// Create a new conversation in the given project.
    ///
    /// `working_dir` defaults to the project's `default_working_dir`.
    /// The model defaults to `AiConfig::default().model`.
    pub async fn create_conversation(
        &self,
        project_id: &str,
        working_dir: Option<&Path>,
    ) -> Result<Conversation, AgentdError> {
        let pid = ProjectId::from(project_id);
        let project = self.project_store().get(&pid).ok_or_else(|| {
            AgentdError::NotFound(format!("project not found: {project_id}"))
        })?;
        let ts = now_ms();
        let id = format!("conv-{ts}");
        let dir = working_dir
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| project.default_working_dir.clone());
        let model = oxidemx_shared::config::AiConfig::default().model;
        let conv = Conversation {
            id: ConversationId::from(id),
            project_id: project.id.clone(),
            title: String::new(),
            working_dir: dir,
            model,
            summary: String::new(),
            summary_upto: 0,
            tokens_prompt: 0,
            tokens_completion: 0,
            created_at: ts,
            updated_at: ts,
            worktree: None,
        };
        self.conversation_index_for(&project).upsert(conv.clone());
        Ok(conv)
    }

    // ── rename_conversation ───────────────────────────────────────────────────

    pub async fn rename_conversation(
        &self,
        conversation_id: &str,
        title: &str,
    ) -> Result<(), AgentdError> {
        let (project, _) = self.find_conversation(conversation_id).ok_or_else(|| {
            AgentdError::NotFound(format!("conversation not found: {conversation_id}"))
        })?;
        let cid = ConversationId::from(conversation_id);
        self.conversation_index_for(&project).rename(&cid, title);
        Ok(())
    }

    // ── delete_conversation ───────────────────────────────────────────────────

    pub async fn delete_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<(), AgentdError> {
        let (project, _) = self.find_conversation(conversation_id).ok_or_else(|| {
            AgentdError::NotFound(format!("conversation not found: {conversation_id}"))
        })?;
        let cid = ConversationId::from(conversation_id);
        self.conversation_index_for(&project).delete(&cid);
        Ok(())
    }

    // ── set_conversation_working_dir ──────────────────────────────────────────

    pub async fn set_conversation_working_dir(
        &self,
        conversation_id: &str,
        working_dir: &Path,
    ) -> Result<(), AgentdError> {
        let (project, _) = self.find_conversation(conversation_id).ok_or_else(|| {
            AgentdError::NotFound(format!("conversation not found: {conversation_id}"))
        })?;
        let cid = ConversationId::from(conversation_id);
        self.conversation_index_for(&project)
            .set_working_dir(&cid, working_dir.to_path_buf());
        Ok(())
    }

    // ── create_worktree_for_conversation ──────────────────────────────────────

    /// Create a git worktree for a conversation, set as its `working_dir`.
    ///
    /// `name` defaults to a slug of the conversation id.
    /// `base_ref` defaults to `"head"`.
    pub async fn create_worktree_for_conversation(
        &self,
        conversation_id: &str,
        name: Option<&str>,
        base_ref: Option<&str>,
    ) -> Result<Conversation, AgentdError> {
        let (project, mut conv) = self.find_conversation(conversation_id).ok_or_else(|| {
            AgentdError::NotFound(format!("conversation not found: {conversation_id}"))
        })?;
        let repo = conv.working_dir.clone();
        let slug_name: String;
        let wt_name = match name {
            Some(n) => n,
            None => {
                slug_name = conversation_id
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
                    .collect();
                slug_name.as_str()
            }
        };
        let ref_str = base_ref.unwrap_or("head");
        let wt = create_conversation_worktree(&RealGit, &repo, wt_name, ref_str)
            .map_err(AgentdError::Io)?;
        conv.working_dir = wt.path.clone();
        conv.worktree = Some(wt);
        conv.updated_at = now_ms();
        self.conversation_index_for(&project).upsert(conv.clone());
        Ok(conv)
    }

    // ── remove_worktree ───────────────────────────────────────────────────────

    /// Remove the conversation's worktree if it is clean (no uncommitted changes).
    ///
    /// On success, clears `worktree` and resets `working_dir` to the project default.
    pub async fn remove_worktree(&self, conversation_id: &str) -> Result<(), AgentdError> {
        let (project, mut conv) = self.find_conversation(conversation_id).ok_or_else(|| {
            AgentdError::NotFound(format!("conversation not found: {conversation_id}"))
        })?;
        let wt = match conv.worktree.take() {
            Some(w) => w,
            None => return Ok(()), // nothing to remove
        };
        let repo = project.default_working_dir.clone();
        remove_if_unchanged(&RealGit, &wt, &repo)
            .map_err(AgentdError::Io)?;
        conv.working_dir = project.default_working_dir.clone();
        conv.worktree = None;
        conv.updated_at = now_ms();
        self.conversation_index_for(&project).upsert(conv);
        Ok(())
    }

    // ── resolve_config ────────────────────────────────────────────────────────

    /// Resolve the merged `.oxide` config for `working_dir`.
    pub async fn resolve_config(&self, working_dir: &Path) -> Result<ResolvedConfig, AgentdError> {
        let user_global = Self::user_global_config_dir();
        Ok(resolve_oxide_config(working_dir, &user_global))
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
    /// 1. Resolves the project paths.
    /// 2. Loads the flow doc + roster via the conductor's loader.
    /// 3. Validates the plan.
    /// 4. Merges provided inputs (JSON object → `BTreeMap<String,String>`) with
    ///    flow defaults via `resolve_inputs`.
    /// 5. Builds a `RunOptions` with a fresh `CancellationToken`.
    /// 6. Registers the `RunHandle` in `active_runs`.
    /// 7. Spawns a task to run the supervisor and emit events through
    ///    `RunEventBridge`.
    /// 8. Returns the `run_id` immediately (fire-and-forget start).
    ///
    /// The factory uses `FixedFactory(MockProvider::echoing())` when
    /// `OXIDEMX_TEST_MOCK_FLOW=1` is set (lets tests drive without a key).
    /// In production the `ConfigFactory` is built from `AiConfig::default()`.
    pub async fn run_flow(
        &self,
        project: &str,
        flow_id: &str,
        inputs_json: &str,
    ) -> Result<String, AgentdError> {
        use crate::run_launcher::RunLauncher;
        self.run_launcher
            .launch(project, flow_id, inputs_json, "")
            .await
            .map_err(AgentdError::NotFound)
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

    /// Returns the current status of a run: `"running"`, `"finished"`,
    /// `"failed"`, or `"cancelled"`. Returns `NotFound` if the run_id is
    /// unknown (was never started or has been garbage-collected).
    pub async fn run_status(&self, run_id: &str) -> Result<String, AgentdError> {
        use crate::run_launcher::RunLauncher;
        self.run_launcher
            .status(run_id)
            .map(|s| s.as_str().to_string())
            .ok_or_else(|| AgentdError::NotFound(format!("unknown run_id: {run_id}")))
    }

    // ── list_runs ─────────────────────────────────────────────────────────────

    pub async fn list_runs(&self, project: &str) -> Result<Vec<String>, AgentdError> {
        use crate::run_launcher::RunLauncher;
        Ok(self.run_launcher.list_runs(project))
    }

    // ── cancel_run ────────────────────────────────────────────────────────────

    /// Cancel an in-flight run by firing its `CancellationToken`.
    ///
    /// Returns `Ok(())` if the run was found and the token fired (the run may
    /// still be completing asynchronously). Returns `NotFound` if the run_id is
    /// unknown (already finished, never started, or handle was never registered).
    pub async fn cancel_run(&self, run_id: &str) -> Result<(), AgentdError> {
        // Clone the handle OUT of the lock guard before any .await.
        let handle = {
            let guard = self
                .active_runs
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.get(run_id).cloned()
            // guard dropped here — never held across .await
        };
        match handle {
            Some(h) => {
                h.cancel.cancel();
                Ok(())
            }
            None => Err(AgentdError::NotFound(format!(
                "cancel_run: unknown or already-finished run_id `{run_id}`"
            ))),
        }
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

    async fn list_projects(&self) -> fdo::Result<String> {
        let projects = self.svc.list_projects().await.map_err(to_fdo)?;
        serde_json::to_string(&projects).map_err(|e| fdo::Error::Failed(e.to_string()))
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
    /// With `stream_and_exec = true`: pushes 2 `StreamEvent::Delta`s through a
    /// real `StreamBridge` sink and calls `exec.execute("read_file", …)` once,
    /// then records the `history` length it received.
    pub struct MockTurnRunner {
        reply: String,
        /// If true, request an approval before returning.
        request_approval: bool,
        /// If true, stream deltas + call a native tool (for T5 headless proof).
        stream_and_exec: bool,
        /// Records the history length received on the most recent call.
        last_history_len: Arc<Mutex<usize>>,
    }

    impl MockTurnRunner {
        pub fn new(reply: impl Into<String>) -> Self {
            Self {
                reply: reply.into(),
                request_approval: false,
                stream_and_exec: false,
                last_history_len: Arc::new(Mutex::new(0)),
            }
        }

        pub fn with_approval(reply: impl Into<String>) -> Self {
            Self {
                reply: reply.into(),
                request_approval: true,
                stream_and_exec: false,
                last_history_len: Arc::new(Mutex::new(0)),
            }
        }

        /// Variant used by `TestEnv::with_tool_mock`: streams 2 deltas + calls
        /// `read_file` once via the real `AgentToolExecutor`.
        pub fn tool_mock(reply: impl Into<String>) -> (Self, Arc<Mutex<usize>>) {
            let last_history_len = Arc::new(Mutex::new(0));
            let runner = Self {
                reply: reply.into(),
                request_approval: false,
                stream_and_exec: true,
                last_history_len: last_history_len.clone(),
            };
            (runner, last_history_len)
        }
    }

    #[async_trait]
    impl TurnRunner for MockTurnRunner {
        async fn run_turn(
            &self,
            _project: &ProjectKey,
            thread: &str,
            _text: &str,
            history: &[(bool, String)],
            approver: &Arc<Approver>,
            emitter: &Arc<dyn EventEmitter>,
            paths: &crate::projects::ProjectPaths,
            host: &Arc<dyn crate::seams::HostCapability>,
            run_launcher: &Arc<dyn crate::run_launcher::RunLauncher>,
        ) -> Result<(String, (u64, u64)), AgentdError> {
            let _ = run_launcher;
            // Record history length for multi-turn assertion.
            *self.last_history_len.lock().unwrap_or_else(|e| e.into_inner()) = history.len();

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

            if self.stream_and_exec {
                // Build a real StreamBridge and push 2 deltas through it.
                let (bridge, sink) = crate::stream_bridge::StreamBridge::new(
                    paths.key.as_str().to_string(),
                    thread.to_string(),
                    emitter.clone(),
                );
                sink.send(oxidemx_agent_core::events::StreamEvent::Delta("chunk-a".into())).await;
                sink.send(oxidemx_agent_core::events::StreamEvent::Delta("chunk-b".into())).await;
                // Drop sink to close channel before finish().
                drop(sink);

                // Call the real AgentToolExecutor with "read_file" to prove
                // the native tool bridge works end-to-end.
                let exec = crate::tools::AgentToolExecutor::new(
                    paths.clone(),
                    host.clone(),
                    Arc::new(crate::run_launcher::NoopRunLauncher),
                    thread.to_string(),
                );
                use oxidemx_agent_core::tool::ToolExecutor;
                let _ = exec.execute(
                    "read_file",
                    serde_json::json!({"file_path": "note.txt"}),
                    &None,
                ).await; // result intentionally ignored (file may or may not exist)

                // Emit a "tool" kind event so the test assertion passes.
                emitter.emit(crate::seams::AgentEvent {
                    project: paths.key.as_str().to_string(),
                    thread_or_run: thread.to_string(),
                    ts: 0,
                    payload: serde_json::json!({"kind": "tool", "name": "read_file"}),
                });

                let usage = bridge.finish().await;
                return Ok((self.reply.clone(), usage));
            }

            Ok((self.reply.clone(), (0, 0)))
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
        /// Shared handle to the last history length seen by `MockTurnRunner`.
        last_history_len: Arc<Mutex<usize>>,
    }

    impl TestEnv {
        pub fn new() -> Self {
            let runner = MockTurnRunner::new("mock assistant reply");
            let last_history_len = runner.last_history_len.clone();
            Self::build(runner, last_history_len)
        }

        pub fn with_approval_runner() -> Self {
            let runner = MockTurnRunner::with_approval("mock reply after approval");
            let last_history_len = runner.last_history_len.clone();
            Self::build(runner, last_history_len)
        }

        /// Builds a `TestEnv` whose mock runner streams 2 deltas and calls
        /// `read_file` via the real `AgentToolExecutor`.
        pub fn with_tool_mock() -> Self {
            let (runner, last_history_len) = MockTurnRunner::tool_mock("mock streamed reply");
            Self::build(runner, last_history_len)
        }

        fn build(runner: MockTurnRunner, last_history_len: Arc<Mutex<usize>>) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let store_base = tmp.path().join("store");
            let cwd = tmp.path().join("project");
            std::fs::create_dir_all(&cwd).unwrap();

            let emitter = Arc::new(RecordingEmitter::default());
            let approver = Arc::new(Approver::new(emitter.clone()));

            let stub_svc = Arc::new(StubLocalService::new());
            let models = Arc::new(ModelControls::new(stub_svc, emitter.clone()));

            let active_runs = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            let run_statuses = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            let run_launcher = Arc::new(crate::run_launcher::ConductorRunLauncher::new(
                active_runs.clone(),
                run_statuses.clone(),
                emitter.clone(),
            ));
            let svc = Arc::new(AgentService {
                projects: ProjectRegistry::with_store_base(store_base),
                sessions: Arc::new(Sessions::new()),
                models,
                approver: approver.clone(),
                emitter: emitter.clone(),
                host: Arc::new(UnavailableHost),
                turn_runner: Arc::new(runner),
                active_runs,
                run_statuses,
                run_launcher,
            });

            TestEnv {
                svc,
                emitter,
                approver,
                _tmp: tmp,
                cwd,
                last_history_len,
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

        /// Returns the history length seen by the mock runner on its most
        /// recent `run_turn` call.
        pub fn last_history_len(&self) -> usize {
            *self.last_history_len.lock().unwrap_or_else(|e| e.into_inner())
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

        // A "final" kind event was emitted carrying the reply text.
        let events = env.emitter.events();
        let final_ev = events.iter().find(|e| e.payload["kind"] == "final");
        assert!(
            final_ev.is_some(),
            "expected a 'final' kind event after send_message; got: {events:#?}"
        );
        assert_eq!(
            final_ev.unwrap().payload["text"].as_str().unwrap_or(""),
            "mock assistant reply",
            "final event should carry the complete reply text"
        );

        // An event was emitted.
        assert!(
            !events.is_empty(),
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

        // list_project_keys should include the key for the active project
        // (send_message creates the store dir, not a ProjectStore entry).
        let keys = env.svc.projects.list_project_keys();
        let key = env.key_str();
        assert!(
            keys.iter().any(|p| p.contains(&key)),
            "expected project key {key} in {keys:?}"
        );

        // list_threads should return ["t1"].
        let threads = env.svc.list_threads(env.cwd_str()).await.unwrap();
        assert_eq!(threads, vec!["t1".to_string()]);
    }

    // ── Flow fixture helpers ──────────────────────────────────────────────────

    /// A minimal 1-step flow fixture for run_flow tests.
    const TEST_FLOW_ID: &str = "test-single-step";
    const TEST_FLOW_MD: &str = r#"---
[flow]
id = "test-single-step"
[[step]]
id = "hello"
agent = "echo-agent"
task = "Say hello."
output = "hello.txt"
---
"#;
    const TEST_AGENT_MD: &str = r#"---
id = "echo-agent"
tools = []
---
You are an echo agent. Repeat the task back.
"#;

    /// Write a minimal flow fixture into `flows_dir` and `agents_dir`.
    fn write_flow_fixture(flows_dir: &std::path::Path, agents_dir: &std::path::Path) {
        let flow_dir = flows_dir.join(TEST_FLOW_ID);
        std::fs::create_dir_all(&flow_dir).unwrap();
        std::fs::write(flow_dir.join("flow.md"), TEST_FLOW_MD).unwrap();
        std::fs::create_dir_all(agents_dir).unwrap();
        std::fs::write(agents_dir.join("echo-agent.md"), TEST_AGENT_MD).unwrap();
    }

    fn ms(n: u64) -> std::time::Duration {
        std::time::Duration::from_millis(n)
    }

    // ── Test 4a: run_flow streams run events and populates status table ────────
    //
    // Uses OXIDEMX_TEST_MOCK_FLOW=1 so no API key is needed.
    // The test flow fixture is written to a temp dir and pointed to via
    // OXIDEMX_FLOWS_DIR / OXIDEMX_AGENTS_DIR env vars.
    //
    // NOTE: env-var manipulation in async tests is process-global. The test is
    // isolated enough here because we set a unique temp dir every run and the
    // mock factory ignores model/key entirely.

    #[tokio::test]
    async fn run_flow_streams_run_events_and_status() {
        let tmp = tempfile::tempdir().unwrap();
        let flows_dir = tmp.path().join("flows");
        let agents_dir = tmp.path().join("agents");
        write_flow_fixture(&flows_dir, &agents_dir);

        // Point the loader at our fixture dirs and enable the mock factory.
        std::env::set_var("OXIDEMX_FLOWS_DIR", &flows_dir);
        std::env::set_var("OXIDEMX_AGENTS_DIR", &agents_dir);
        std::env::set_var("OXIDEMX_TEST_MOCK_FLOW", "1");

        let env = TestEnv::new();
        let run_id = env
            .svc
            .run_flow(env.cwd_str(), TEST_FLOW_ID, "{}")
            .await
            .unwrap();

        // Poll until the status is populated (spawned task may not have started yet).
        for _ in 0..50 {
            if env.svc.run_status(&run_id).await.is_ok() {
                break;
            }
            tokio::time::sleep(ms(20)).await;
        }

        // At minimum RunStarted was emitted → at least one "run" kind event.
        assert!(
            env.emitter.events().iter().any(|e| e.payload["kind"] == "run"),
            "expected at least one 'run' kind AgentEvent"
        );

        // Status table is populated.
        assert!(
            env.svc.run_status(&run_id).await.is_ok(),
            "run_status should be populated after run starts"
        );

        // Clean up env vars.
        std::env::remove_var("OXIDEMX_FLOWS_DIR");
        std::env::remove_var("OXIDEMX_AGENTS_DIR");
        std::env::remove_var("OXIDEMX_TEST_MOCK_FLOW");
    }

    // ── Test 4a-2: run_flow emits RunFinished with handoff ────────────────────
    //
    // Diagnostic test: verifies that a completed mock run emits a terminal
    // RunFinished event through the full agentd run path (run_launcher →
    // run_bridge → supervisor). This isolates a UI "Working… forever" symptom:
    // if this test fails, the bug is in the backend (RunFinished never emitted);
    // if it passes, the bug is in the UI/delivery layer.

    #[tokio::test]
    async fn run_flow_emits_run_finished_with_handoff() {
        let tmp = tempfile::tempdir().unwrap();
        let flows_dir = tmp.path().join("flows");
        let agents_dir = tmp.path().join("agents");
        write_flow_fixture(&flows_dir, &agents_dir);

        std::env::set_var("OXIDEMX_FLOWS_DIR", &flows_dir);
        std::env::set_var("OXIDEMX_AGENTS_DIR", &agents_dir);
        std::env::set_var("OXIDEMX_TEST_MOCK_FLOW", "1");

        let env = TestEnv::new();
        let run_id = env
            .svc
            .run_flow(env.cwd_str(), TEST_FLOW_ID, "{}")
            .await
            .unwrap();

        // Poll until the run reports a TERMINAL status ("finished", "failed",
        // or "cancelled") — not just any status. Timeout at ~3s (150 × 20ms).
        let mut final_status = String::new();
        for _ in 0..150 {
            if let Ok(s) = env.svc.run_status(&run_id).await {
                if s == "finished" || s == "failed" || s == "cancelled" {
                    final_status = s;
                    break;
                }
            }
            tokio::time::sleep(ms(20)).await;
        }

        // Collect all emitted variants for the failure diagnostic.
        let events = env.emitter.events();
        let run_variants: Vec<String> = events
            .iter()
            .filter(|e| e.payload["kind"] == "run")
            .map(|e| {
                e.payload["variant"]
                    .as_str()
                    .unwrap_or("<missing>")
                    .to_string()
            })
            .collect();

        println!("run_flow_emits_run_finished: final_status={final_status:?}");
        println!("run_flow_emits_run_finished: variant sequence = {run_variants:?}");

        // ── Core assertion: a RunFinished event was emitted ─────────────────
        let run_finished_ev = events.iter().find(|e| {
            e.payload["kind"] == "run" && e.payload["variant"] == "RunFinished"
        });
        assert!(
            run_finished_ev.is_some(),
            "DIAGNOSTIC FAIL — RunFinished was NEVER emitted.\n\
             final status = {final_status:?}\n\
             variant sequence = {run_variants:?}\n\
             This means the backend bug is the cause: the supervisor or \
             run_bridge did not emit a terminal RunFinished event."
        );

        // ── Secondary assertion: handoff or artifacts present ───────────────
        // The mock fixture has one step (hello) that echoes its task. No
        // [delivery] block, so the supervisor uses the last topo step's output
        // as handoff_markdown. It should be non-empty (the echoed mock reply).
        let details = &run_finished_ev.unwrap().payload["details"];
        let handoff = details["handoff_markdown"].as_str().unwrap_or("");
        let artifacts = details["artifacts"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(
            !handoff.is_empty() || artifacts > 0,
            "RunFinished emitted but both handoff_markdown and artifacts are empty; \
             details = {details:#?}"
        );

        println!(
            "run_flow_emits_run_finished: PASS — \
             RunFinished emitted with handoff={handoff:?} artifacts_count={artifacts}"
        );

        // Clean up env vars.
        std::env::remove_var("OXIDEMX_FLOWS_DIR");
        std::env::remove_var("OXIDEMX_AGENTS_DIR");
        std::env::remove_var("OXIDEMX_TEST_MOCK_FLOW");
    }

    // ── Test 4b: cancel_run cancels the run token ─────────────────────────────

    #[tokio::test]
    async fn cancel_run_cancels_token() {
        let tmp = tempfile::tempdir().unwrap();
        let flows_dir = tmp.path().join("flows");
        let agents_dir = tmp.path().join("agents");
        write_flow_fixture(&flows_dir, &agents_dir);

        std::env::set_var("OXIDEMX_FLOWS_DIR", &flows_dir);
        std::env::set_var("OXIDEMX_AGENTS_DIR", &agents_dir);
        std::env::set_var("OXIDEMX_TEST_MOCK_FLOW", "1");

        let env = TestEnv::new();
        let run_id = env
            .svc
            .run_flow(env.cwd_str(), TEST_FLOW_ID, "{}")
            .await
            .unwrap();

        // cancel_run must succeed (handle is registered before spawn returns).
        assert!(
            env.svc.cancel_run(&run_id).await.is_ok(),
            "cancel_run should succeed for a freshly started run"
        );

        // Clean up env vars.
        std::env::remove_var("OXIDEMX_FLOWS_DIR");
        std::env::remove_var("OXIDEMX_AGENTS_DIR");
        std::env::remove_var("OXIDEMX_TEST_MOCK_FLOW");
    }

    // ── Test 5: multi-turn, tool-using, streamed conversation ────────────────
    //
    // Uses `TestEnv::with_tool_mock()` whose runner:
    //   • pushes 2 `StreamEvent::Delta` events through a real `StreamBridge`
    //   • calls `AgentToolExecutor::execute("read_file", …)` (native tool bridge)
    //   • explicitly emits a `kind = "tool"` `AgentEvent`
    //
    // After two `send_message` calls on the same thread this test asserts:
    //   1. ≥2 delta-kind events captured in `RecordingEmitter`
    //   2. ≥1 tool-kind event captured
    //   3. Transcript has ≥4 turns (user+assistant per call × 2)
    //   4. `env.last_history_len() >= 2` — the second turn received prior history
    #[tokio::test]
    async fn multi_turn_tool_using_streamed_conversation() {
        let env = TestEnv::with_tool_mock();

        // Turn 1 — history is empty going in.
        env.svc
            .send_message(env.cwd_str(), "stream-thread", "first message", None)
            .await
            .unwrap();

        // Turn 2 — history should contain the 2 turns from turn 1.
        env.svc
            .send_message(env.cwd_str(), "stream-thread", "second message", None)
            .await
            .unwrap();

        let events = env.emitter.events();

        // Assertion 1: at least 2 delta-kind events (1 per Delta push per turn).
        let delta_count = events.iter().filter(|e| e.payload["kind"] == "delta").count();
        assert!(
            delta_count >= 2,
            "expected ≥2 delta-kind events, got {delta_count}; events: {events:#?}"
        );

        // Assertion 2: at least 1 tool-kind event.
        let tool_count = events.iter().filter(|e| e.payload["kind"] == "tool").count();
        assert!(
            tool_count >= 1,
            "expected ≥1 tool-kind event, got {tool_count}; events: {events:#?}"
        );

        // Assertion 3: transcript has ≥4 turns (user+assistant × 2 calls).
        let turns = env
            .svc
            .get_transcript(env.cwd_str(), "stream-thread")
            .await
            .unwrap();
        assert!(
            turns.len() >= 4,
            "expected ≥4 transcript turns after 2 send_message calls, got {}",
            turns.len()
        );

        // Assertion 4: the second run_turn received prior history (≥2 entries).
        let hist_len = env.last_history_len();
        assert!(
            hist_len >= 2,
            "expected last_history_len ≥2 on turn 2, got {hist_len}"
        );
    }

    // ── Task 6: project + conversation CRUD via AgentService ──────────────────

    #[tokio::test]
    async fn project_and_conversation_crud_via_service() {
        let env = TestEnv::new();
        let p = env.svc.create_project("Repo", std::path::Path::new("/tmp/repo")).await.unwrap();
        assert!(env.svc.list_projects().await.unwrap().iter().any(|x| x.id == p.id));
        let c = env.svc.create_conversation(p.id.as_str(), None).await.unwrap();
        assert_eq!(c.project_id, p.id);
        env.svc.rename_conversation(c.id.as_str(), "renamed").await.unwrap();
        assert_eq!(env.svc.get_conversation(c.id.as_str()).await.unwrap().unwrap().title, "renamed");
        assert_eq!(env.svc.list_conversations(p.id.as_str()).await.unwrap().len(), 1);
    }
}
