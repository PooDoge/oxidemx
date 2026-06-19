//! Host seams: [`EventEmitter`], [`Approver`], [`HostCapability`].
//!
//! These are the three integration points between agentd's core logic and
//! the host environment (GNOME Shell overlay, D-Bus, human-in-the-loop UI).
//!
//! - **[`EventEmitter`]** — fire-and-forget side-channel for status events.
//!   Production impl (zbus) comes in Task 8; tests use [`RecordingEmitter`].
//! - **[`Approver`]** — async gate: a tool call parks on a oneshot until the
//!   host UI calls `respond`. No lock is held across the `.await`.
//! - **[`HostCapability`]** — escape hatch for host-provided capabilities
//!   (clipboard, screenshot, …). [`UnavailableHost`] returns `NotFound`.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::error::AgentdError;

// ── Verdict ───────────────────────────────────────────────────────────────────

/// The outcome of a human approval request.
#[derive(Debug, Clone)]
pub enum Verdict {
    /// Allow the tool call to proceed.
    Allow,
    /// Deny the tool call; `String` is a human-readable reason.
    Deny(String),
    /// Allow this and all future identical tool calls (remember the decision).
    Always,
    /// Allow but with an edited argument payload.
    Edit(Value),
}

// ── AgentEvent ────────────────────────────────────────────────────────────────

/// A status event emitted by the agent runtime to the host environment.
#[derive(Debug, Clone)]
pub struct AgentEvent {
    /// The project this event belongs to.
    pub project: String,
    /// The thread or conductor run ID.
    pub thread_or_run: String,
    /// Unix timestamp (milliseconds since epoch).
    pub ts: u64,
    /// Arbitrary JSON payload (event kind + data).
    pub payload: Value,
}

// ── EventEmitter ─────────────────────────────────────────────────────────────

/// Side-channel for host-environment status events.
///
/// Implementations must be `Send + Sync` so they can be stored in `Arc<dyn
/// EventEmitter>` and passed across async task boundaries.
pub trait EventEmitter: Send + Sync {
    fn emit(&self, ev: AgentEvent);
}

// ── NullEmitter ───────────────────────────────────────────────────────────────

/// Discards all events. Use in contexts where event delivery is not needed.
pub struct NullEmitter;

impl EventEmitter for NullEmitter {
    fn emit(&self, _ev: AgentEvent) {}
}

// ── RecordingEmitter ─────────────────────────────────────────────────────────

/// Records every emitted event in memory. For use in tests.
#[derive(Default)]
pub struct RecordingEmitter {
    events: Mutex<Vec<AgentEvent>>,
}

impl EventEmitter for RecordingEmitter {
    fn emit(&self, ev: AgentEvent) {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(ev);
    }
}

impl RecordingEmitter {
    /// Return a snapshot of all recorded events (test accessor).
    pub fn events(&self) -> Vec<AgentEvent> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

// ── Approver ──────────────────────────────────────────────────────────────────

/// Async human-approval gate.
///
/// `request` emits an approval event then parks on a [`oneshot`] receiver
/// until `respond` is called. Critically, **no lock is held across the
/// `.await`**: the pending map is locked only to insert/remove the sender,
/// and the guard is dropped before `receiver.await`.
pub struct Approver {
    pending: Mutex<HashMap<String, oneshot::Sender<Verdict>>>,
    emitter: Arc<dyn EventEmitter>,
    counter: AtomicU64,
}

impl Approver {
    /// Create a new `Approver` backed by the supplied [`EventEmitter`].
    pub fn new(emitter: Arc<dyn EventEmitter>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            emitter,
            counter: AtomicU64::new(0),
        }
    }

    /// Request approval for a tool call.
    ///
    /// 1. Mints a unique `request_id` (monotonic counter).
    /// 2. Registers a oneshot sender in `pending`.
    /// 3. **Drops the lock**.
    /// 4. Emits an `ApprovalRequest` event.
    /// 5. Awaits the oneshot receiver (parks until `respond` is called).
    /// 6. Returns the [`Verdict`].
    pub async fn request(&self, project: &str, thread: &str, card: Value) -> Verdict {
        // 1. Mint request_id.
        let id = self.counter.fetch_add(1, Ordering::Relaxed).to_string();

        // 2. Create oneshot + register sender.
        let (tx, rx) = oneshot::channel::<Verdict>();
        {
            let mut guard = self
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.insert(id.clone(), tx);
        } // 3. Lock released here — guard dropped before .await below.

        // 4. Emit the approval-request event.
        self.emitter.emit(AgentEvent {
            project: project.to_string(),
            thread_or_run: thread.to_string(),
            ts: now_ms(),
            payload: serde_json::json!({
                "kind": "ApprovalRequest",
                "request_id": id,
                "card": card,
            }),
        });

        // 5. Park until respond() sends a verdict (or the sender is dropped).
        rx.await.unwrap_or(Verdict::Deny("approver dropped".into()))
    }

    /// Deliver a verdict for a pending request.
    ///
    /// Removes the sender from the pending map (lock released before `send`),
    /// then fires the oneshot to wake the awaiting `request` future.
    pub fn respond(&self, request_id: &str, verdict: Verdict) {
        // Lock, remove sender, release lock, then send.
        let maybe_tx = {
            let mut guard = self
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.remove(request_id)
        }; // guard dropped here.
        if let Some(tx) = maybe_tx {
            // Ignore send error: the waiting future may have been cancelled.
            let _ = tx.send(verdict);
        }
    }

    /// Return the request IDs of all currently pending approvals.
    ///
    /// Used by tests to discover the ID minted by an in-flight `request`.
    #[cfg(test)]
    pub fn pending_ids(&self) -> Vec<String> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }
}

// ── HostCapability ────────────────────────────────────────────────────────────

/// Escape hatch for capabilities provided by the host environment (e.g.
/// clipboard read/write, screenshot, file-save dialog).
///
/// The `async fn` signature requires the `async_trait` macro because Rust
/// does not yet support async trait methods in stable without it.
#[async_trait]
pub trait HostCapability: Send + Sync {
    /// Invoke capability `cap` with JSON `args`, returning a JSON result or an
    /// [`AgentdError`].
    async fn invoke(&self, cap: &str, args: Value) -> Result<Value, AgentdError>;
}

// ── UnavailableHost ───────────────────────────────────────────────────────────

/// Stub implementation returned when no host is connected.
pub struct UnavailableHost;

#[async_trait]
impl HostCapability for UnavailableHost {
    async fn invoke(&self, _cap: &str, _args: Value) -> Result<Value, AgentdError> {
        Err(AgentdError::NotFound("no host".into()))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Current time in milliseconds since Unix epoch.
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approver_blocks_until_respond() {
        let em = Arc::new(RecordingEmitter::default());
        let ap = Arc::new(Approver::new(em.clone()));
        let ap2 = ap.clone();
        let h = tokio::spawn(async move {
            ap2.request("proj", "t1", serde_json::json!({"tool":"run"}))
                .await
        });
        // give the task time to register + emit
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let id = ap.pending_ids()[0].clone(); // test accessor
        ap.respond(&id, Verdict::Deny("no".into()));
        assert!(matches!(h.await.unwrap(), Verdict::Deny(_)));
        assert!(!em.events().is_empty()); // approval event emitted
    }
}
