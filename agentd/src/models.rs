//! Local-model controls bridge.
//!
//! [`ModelControls`] wraps [`oxidemx_agent_local::LocalModelService`] and emits
//! a `model_status` [`AgentEvent`] after each lifecycle change (load / unload /
//! set_active).  It is the seam between the raw local-LLM service and agentd's
//! D-Bus interface (Task 6) and event bus (Task 8).
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oxidemx_agent_local::types::ModelStatusInfo;
use oxidemx_agent_local::LocalModelService;

use crate::error::AgentdError;
use crate::seams::{AgentEvent, EventEmitter};

// ── helpers ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Convert a [`LocalError`] to an [`AgentdError`].
fn local_err(e: oxidemx_agent_local::error::LocalError) -> AgentdError {
    AgentdError::NotFound(e.to_string())
}

// ── ModelControls ─────────────────────────────────────────────────────────────

/// Thin bridge over [`LocalModelService`] that emits `model_status` events.
///
/// Each lifecycle call (`load`, `unload`, `set_active`) delegates to the
/// wrapped service then fires a fire-and-forget [`AgentEvent`] so the host
/// environment (and Task 8 D-Bus broadcaster) can react without polling.
pub struct ModelControls {
    svc: Arc<dyn LocalModelService>,
    emitter: Arc<dyn EventEmitter>,
}

impl ModelControls {
    /// Create a new `ModelControls` bridge.
    pub fn new(svc: Arc<dyn LocalModelService>, emitter: Arc<dyn EventEmitter>) -> Self {
        Self { svc, emitter }
    }

    /// Emit a `model_status` event for `alias` after a lifecycle change.
    ///
    /// The `state` field carries the serialised [`ModelState`] for the alias, or
    /// `"unknown"` if the alias is not in the snapshot (shouldn't happen in
    /// practice but avoids a panic on edge cases).
    fn emit_status(&self, alias: &str) {
        let state_str = self
            .svc
            .status()
            .into_iter()
            .find(|s| s.alias == alias)
            .map(|s| serde_json::to_value(&s.state).unwrap_or(serde_json::Value::Null))
            .unwrap_or_else(|| serde_json::Value::String("unknown".into()));

        self.emitter.emit(AgentEvent {
            project: String::new(),
            thread_or_run: alias.to_string(),
            ts: now_ms(),
            payload: serde_json::json!({
                "kind": "model_status",
                "alias": alias,
                "state": state_str,
            }),
        });
    }

    /// Ensure the named model is loaded and ready, then emit a status event.
    pub async fn load(&self, alias: &str) -> Result<(), AgentdError> {
        self.svc.ensure_loaded(alias).await.map_err(local_err)?;
        self.emit_status(alias);
        Ok(())
    }

    /// Unload the named model, then emit a status event.
    pub async fn unload(&self, alias: &str) -> Result<(), AgentdError> {
        self.svc.unload(alias).await.map_err(local_err)?;
        self.emit_status(alias);
        Ok(())
    }

    /// Set the named model as the active default, then emit a status event.
    pub async fn set_active(&self, alias: &str) -> Result<(), AgentdError> {
        self.svc.set_active(alias).await.map_err(local_err)?;
        self.emit_status(alias);
        Ok(())
    }

    /// Return a snapshot of every registered model's current state.
    ///
    /// Non-blocking: delegates directly to [`LocalModelService::status`].
    pub fn list(&self) -> Vec<ModelStatusInfo> {
        self.svc.status()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seams::RecordingEmitter;
    use async_trait::async_trait;
    use oxidemx_agent_local::error::LocalError;
    use oxidemx_agent_local::types::{ChatRequest, ChatResponse, ModelState, ModelStatusInfo, Usage};
    use oxidemx_agent_local::Verdict as LocalVerdict;
    use std::sync::Mutex;

    // ── StubLocalService ──────────────────────────────────────────────────────

    /// In-test stub for [`LocalModelService`].
    ///
    /// Records `ensure_loaded` / `unload` calls and returns a `Ready` snapshot
    /// for any alias that has been loaded and not yet unloaded.
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
    impl LocalModelService for StubLocalService {
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
            let mut guard = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            if !guard.contains(&alias.to_string()) {
                guard.push(alias.to_string());
            }
            Ok(())
        }

        async fn unload(&self, alias: &str) -> Result<(), LocalError> {
            let mut guard = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            guard.retain(|a| a != alias);
            Ok(())
        }

        async fn set_active(&self, _alias: &str) -> Result<(), LocalError> {
            Ok(())
        }

        fn status(&self) -> Vec<ModelStatusInfo> {
            let guard = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .iter()
                .map(|alias| ModelStatusInfo {
                    alias: alias.clone(),
                    state: ModelState::Ready,
                    last_used: None,
                })
                .collect()
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn load_emits_status_event() {
        let svc = Arc::new(StubLocalService::new());
        let em = Arc::new(RecordingEmitter::default());
        let mc = ModelControls::new(svc, em.clone());
        mc.load("q").await.unwrap();
        assert!(em.events().iter().any(|e| e.payload["kind"] == "model_status"
            && e.payload["alias"] == "q"));
        assert!(mc.list().iter().any(|s| s.alias == "q"));
    }

    #[tokio::test]
    async fn unload_emits_status_event() {
        let svc = Arc::new(StubLocalService::new());
        let em = Arc::new(RecordingEmitter::default());
        let mc = ModelControls::new(svc, em.clone());
        mc.load("m1").await.unwrap();
        mc.unload("m1").await.unwrap();
        // Two events: one for load, one for unload.
        let evs = em.events();
        assert_eq!(evs.len(), 2);
        assert!(evs.iter().all(|e| e.payload["kind"] == "model_status"));
        // After unload the list should be empty.
        assert!(mc.list().is_empty());
    }

    #[tokio::test]
    async fn set_active_emits_status_event() {
        let svc = Arc::new(StubLocalService::new());
        let em = Arc::new(RecordingEmitter::default());
        let mc = ModelControls::new(svc, em.clone());
        mc.load("m2").await.unwrap();
        mc.set_active("m2").await.unwrap();
        assert_eq!(em.events().len(), 2);
    }

    #[tokio::test]
    async fn list_reflects_loaded_models() {
        let svc = Arc::new(StubLocalService::new());
        let em = Arc::new(RecordingEmitter::default());
        let mc = ModelControls::new(svc, em.clone());
        assert!(mc.list().is_empty());
        mc.load("alpha").await.unwrap();
        mc.load("beta").await.unwrap();
        let names: Vec<_> = mc.list().iter().map(|s| s.alias.clone()).collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
    }
}
