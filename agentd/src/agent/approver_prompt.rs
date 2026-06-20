//! [`ApproverPrompt`] — bridges the SP1b [`Approver`] seam to the
//! [`ApprovalPrompt`] trait used by [`GatedToolExecutor`].
//!
//! # No lock across await
//!
//! This adapter holds no `Mutex`. It builds a card, calls
//! `Approver::request` (which parks on a oneshot internally), maps the
//! [`Verdict`], and returns. The `Approver` manages its own pending map and
//! drops its guards before awaiting — no lock crosses an `.await` boundary
//! in this crate.
#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;

use crate::seams::{Approver, Verdict};
use crate::tools::gated::ApprovalPrompt;

/// Adapter: implements [`ApprovalPrompt`] by delegating to the SP1b
/// [`Approver`] gate.
///
/// On `confirm`, builds a small JSON card `{"tool":…,"reason":…}`, calls
/// `approver.request(project, thread, card)`, and maps the returned
/// [`Verdict`]:
///
/// - `Verdict::Allow | Verdict::Always` → `true` (proceed)
/// - `Verdict::Deny(_) | Verdict::Edit(_)` → `false` (block)
pub struct ApproverPrompt {
    approver: Arc<Approver>,
    project: String,
    thread: String,
}

impl ApproverPrompt {
    /// Construct an `ApproverPrompt`.
    ///
    /// - `approver` — the shared [`Approver`] instance (from the host seams).
    /// - `project`  — project identifier forwarded to approval events.
    /// - `thread`   — thread/run identifier forwarded to approval events.
    pub fn new(approver: Arc<Approver>, project: impl Into<String>, thread: impl Into<String>) -> Self {
        Self {
            approver,
            project: project.into(),
            thread: thread.into(),
        }
    }
}

#[async_trait]
impl ApprovalPrompt for ApproverPrompt {
    async fn confirm(&self, tool: &str, reason: &str) -> bool {
        let card = serde_json::json!({
            "tool": tool,
            "reason": reason,
        });
        // No lock is held here. `Approver::request` manages its own pending
        // map, drops the guard before awaiting the oneshot.
        let verdict = self.approver.request(&self.project, &self.thread, card).await;
        matches!(verdict, Verdict::Allow | Verdict::Always)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::seams::{Approver, NullEmitter, Verdict};

    #[tokio::test]
    async fn approver_prompt_maps_verdict_to_bool() {
        let emitter = Arc::new(NullEmitter);
        let approver = Arc::new(Approver::new(emitter));

        // ── Allow → true ─────────────────────────────────────────────────
        {
            let ap2 = approver.clone();
            let prompt = ApproverPrompt::new(ap2, "proj", "thread-1");
            let h = tokio::spawn(async move {
                prompt.confirm("execute_command", "git push --force is risky").await
            });
            // Give the spawned task time to register its oneshot in the
            // Approver's pending map before we query pending_ids().
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let ids = approver.pending_ids();
            assert_eq!(ids.len(), 1, "expected 1 pending approval");
            approver.respond(&ids[0], Verdict::Allow);
            assert!(h.await.unwrap(), "Allow should map to true");
        }

        // ── Deny → false ──────────────────────────────────────────────────
        {
            let ap2 = approver.clone();
            let prompt = ApproverPrompt::new(ap2, "proj", "thread-2");
            let h = tokio::spawn(async move {
                prompt.confirm("execute_command", "some risky command").await
            });
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let ids = approver.pending_ids();
            assert_eq!(ids.len(), 1, "expected 1 pending approval");
            approver.respond(&ids[0], Verdict::Deny("not allowed".into()));
            assert!(!h.await.unwrap(), "Deny should map to false");
        }

        // ── Always → true ─────────────────────────────────────────────────
        {
            let ap2 = approver.clone();
            let prompt = ApproverPrompt::new(ap2, "proj", "thread-3");
            let h = tokio::spawn(async move {
                prompt.confirm("read_file", "reading sensitive file").await
            });
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let ids = approver.pending_ids();
            assert_eq!(ids.len(), 1);
            approver.respond(&ids[0], Verdict::Always);
            assert!(h.await.unwrap(), "Always should map to true");
        }

        // ── Edit → false ───────────────────────────────────────────────────
        {
            let ap2 = approver.clone();
            let prompt = ApproverPrompt::new(ap2, "proj", "thread-4");
            let h = tokio::spawn(async move {
                prompt.confirm("execute_command", "some command for editing").await
            });
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let ids = approver.pending_ids();
            assert_eq!(ids.len(), 1);
            approver.respond(&ids[0], Verdict::Edit(serde_json::json!({})));
            assert!(!h.await.unwrap(), "Edit should map to false");
        }
    }
}
