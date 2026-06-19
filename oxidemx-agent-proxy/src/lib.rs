//! D-Bus client proxy for org.oxidemx.Agent (light: zbus + serde only).
//!
//! Provides a ready-made `#[proxy]` trait that mirrors `agentd/src/interface.rs`.
//! All methods and signals are documented at their source in the server impl.

#![forbid(unsafe_code)]

use zbus::proxy;

/// D-Bus proxy client for `org.oxidemx.Agent`.
///
/// Mirrors the full method + signal set from Task 6 (agentd interface.rs).
/// Use this to call agent methods and subscribe to signals from SP1c clients
/// (overlay-rs, settings, mission-control).
#[proxy(
    interface = "org.oxidemx.Agent",
    default_service = "org.oxidemx.Agent",
    default_path = "/org/oxidemx/Agent"
)]
pub trait Agent {
    // ── Chat methods ──────────────────────────────────────────────────────────

    /// Send a message to a project's conversation thread.
    ///
    /// Returns a new turn id.
    async fn send_message(
        &self,
        project: &str,
        thread: &str,
        text: &str,
        model_hint: &str,
    ) -> zbus::Result<String>;

    /// Cancel an in-flight turn for `thread` in `project`.
    async fn cancel_turn(&self, project: &str, thread: &str) -> zbus::Result<()>;

    /// Get the transcript (conversation history) for a thread.
    ///
    /// Returns a JSON-encoded `Vec<TranscriptTurn>` string.
    async fn get_transcript(&self, project: &str, thread: &str) -> zbus::Result<String>;

    /// List all threads for a project.
    async fn list_threads(&self, project: &str) -> zbus::Result<Vec<String>>;

    /// List all known projects in the store.
    async fn list_projects(&self) -> zbus::Result<Vec<String>>;

    /// Deliver a verdict for a pending approval request.
    async fn respond_approval(
        &self,
        project: &str,
        request_id: &str,
        allow: bool,
        reason: &str,
    ) -> zbus::Result<()>;

    /// Optimize a prompt draft.
    ///
    /// Returns the optimized prompt string.
    async fn optimize_prompt(&self, project: &str, draft: &str) -> zbus::Result<String>;

    // ── Flow methods ──────────────────────────────────────────────────────────

    /// Launch a conductor flow scoped to the project's runs directory.
    ///
    /// - `flow_id`: identifies the flow
    /// - `inputs_json`: JSON-encoded inputs
    ///
    /// Returns a run id.
    async fn run_flow(
        &self,
        project: &str,
        flow_id: &str,
        inputs_json: &str,
    ) -> zbus::Result<String>;

    /// List available flow ids.
    async fn list_flows(&self, project: &str) -> zbus::Result<Vec<String>>;

    /// Validate a flow by id.
    ///
    /// Returns "valid" or "invalid: <errors>".
    async fn validate_flow(&self, project: &str, flow_id: &str) -> zbus::Result<String>;

    /// Get the status of a run.
    ///
    /// Returns a status string.
    async fn run_status(&self, run_id: &str) -> zbus::Result<String>;

    /// List all run ids for a project.
    async fn list_runs(&self, project: &str) -> zbus::Result<Vec<String>>;

    /// Cancel a flow run.
    async fn cancel_run(&self, run_id: &str) -> zbus::Result<()>;

    /// List available agent ids.
    async fn list_agents(&self, project: &str) -> zbus::Result<Vec<String>>;

    // ── Model controls ────────────────────────────────────────────────────────

    /// Load a model by alias.
    async fn load_model(&self, alias: &str) -> zbus::Result<()>;

    /// Unload a model by alias.
    async fn unload_model(&self, alias: &str) -> zbus::Result<()>;

    /// Set the active model.
    async fn set_active_model(&self, alias: &str) -> zbus::Result<()>;

    /// List all model status info.
    ///
    /// Returns a JSON-encoded string.
    async fn list_models(&self) -> zbus::Result<String>;

    // ── Signals ───────────────────────────────────────────────────────────────

    /// Fired whenever an agent event occurs.
    ///
    /// `payload` is a JSON string describing the event.
    #[zbus(signal)]
    async fn event(
        &self,
        project: String,
        thread_or_run: String,
        ts: u64,
        payload: String,
    ) -> zbus::Result<()>;

    /// Fired when the agent runtime is waiting for human approval.
    ///
    /// `card` is a JSON string describing the tool and its arguments.
    #[zbus(signal)]
    async fn approval_requested(
        &self,
        project: String,
        thread: String,
        request_id: String,
        card: String,
    ) -> zbus::Result<()>;

    /// Fired after every model lifecycle change.
    ///
    /// `status` is a JSON string (alias + state).
    #[zbus(signal)]
    async fn model_status_changed(&self, alias: String, status: String) -> zbus::Result<()>;
}
