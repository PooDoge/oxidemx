//! Internal `InferenceEngine` seam — the boundary between the session manager
//! (Task 7) and the real mistral.rs backend (Task 9).
//!
//! Nothing outside this crate should depend on this module; it is `pub(crate)`.

use async_trait::async_trait;
use oxidemx_shared::config::{ModelSpec, SamplingConfig};
use serde_json::Value;

use crate::error::LocalError;
use crate::types::{Message, Usage};

// ── Request / Reply ───────────────────────────────────────────────────────────

/// Everything needed for a single inference call.
#[allow(dead_code)]
/// consumed by LocalModelManager (next task); allow until then
#[derive(Debug, Clone)]
pub(crate) struct EngineRequest {
    /// Conversation history to pass to the model.
    pub messages: Vec<Message>,
    /// Sampling parameters for this request.
    pub sampling: SamplingConfig,
    /// Tool definitions forwarded to the model (empty when not in tool-use mode).
    pub tools: Vec<Value>,
}

/// The inference result returned by an engine.
#[allow(dead_code)]
/// consumed by LocalModelManager (next task); allow until then
#[derive(Debug, Clone)]
pub(crate) struct EngineReply {
    /// Raw text produced by the model.
    pub text: String,
    /// Token-usage accounting for this call.
    pub usage: Usage,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Internal boundary between the session manager and the inference backend.
///
/// The session manager (Task 7) depends **only** on this trait, which means
/// it is fully unit-testable via [`MockEngine`] without bringing in the
/// mistral.rs native engine.
#[allow(dead_code)]
/// consumed by LocalModelManager (next task); allow until then
#[async_trait]
pub(crate) trait InferenceEngine: Send + Sync {
    /// Load (or verify already loaded) the model described by `spec`.
    async fn load(&self, spec: &ModelSpec) -> Result<(), LocalError>;

    /// Unload the model registered under `alias`, releasing its memory.
    async fn unload(&self, alias: &str) -> Result<(), LocalError>;

    /// Run one inference turn against the model registered under `alias`.
    async fn generate(&self, alias: &str, req: &EngineRequest) -> Result<EngineReply, LocalError>;
}

// ── MockEngine (test-only) ────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::sync::Mutex;

    use oxidemx_shared::config::{Capabilities, ModelSource};

    /// A scripted, record-keeping engine for unit tests.
    ///
    /// * `loaded()` — returns every alias passed to `load` in call order.
    /// * `unloaded()` — returns every alias passed to `unload` in call order.
    /// * `with_reply(text)` — sets the text returned by the next `generate` call.
    /// * `fail_next_load()` — makes the next `load` call return `LoadFailed`.
    pub(crate) struct MockEngine {
        loaded_log: Mutex<Vec<String>>,
        unloaded_log: Mutex<Vec<String>>,
        scripted_reply: Mutex<String>,
        fail_next_load: Mutex<bool>,
    }

    impl MockEngine {
        /// Create a new `MockEngine` with an empty reply (use `with_reply` to set it).
        pub(crate) fn new() -> Self {
            Self {
                loaded_log: Mutex::new(Vec::new()),
                unloaded_log: Mutex::new(Vec::new()),
                scripted_reply: Mutex::new(String::new()),
                fail_next_load: Mutex::new(false),
            }
        }

        /// Set the text that the mock will return from the next `generate` call.
        /// Returns `self` for chaining.
        pub(crate) fn with_reply(self, text: impl Into<String>) -> Self {
            *self.scripted_reply.lock().unwrap() = text.into();
            self
        }

        /// Tell the mock to fail the next `load` call with `LocalError::LoadFailed`.
        pub(crate) fn fail_next_load(&self) {
            *self.fail_next_load.lock().unwrap() = true;
        }

        /// Return a copy of the list of aliases that were successfully loaded,
        /// in call order.
        pub(crate) fn loaded(&self) -> Vec<String> {
            self.loaded_log.lock().unwrap().clone()
        }

        /// Return a copy of the list of aliases that were unloaded, in call order.
        pub(crate) fn unloaded(&self) -> Vec<String> {
            self.unloaded_log.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl InferenceEngine for MockEngine {
        async fn load(&self, spec: &ModelSpec) -> Result<(), LocalError> {
            let should_fail = {
                let mut flag = self.fail_next_load.lock().unwrap();
                let v = *flag;
                *flag = false;
                v
            };
            if should_fail {
                return Err(LocalError::LoadFailed {
                    alias: spec.alias.clone(),
                    reason: "mock-injected failure".to_string(),
                });
            }
            self.loaded_log.lock().unwrap().push(spec.alias.clone());
            Ok(())
        }

        async fn unload(&self, alias: &str) -> Result<(), LocalError> {
            self.unloaded_log.lock().unwrap().push(alias.to_string());
            Ok(())
        }

        async fn generate(
            &self,
            _alias: &str,
            _req: &EngineRequest,
        ) -> Result<EngineReply, LocalError> {
            let text = self.scripted_reply.lock().unwrap().clone();
            Ok(EngineReply {
                text,
                usage: Usage::default(),
            })
        }
    }

    /// Build a minimal valid [`ModelSpec`] for use in unit tests.
    ///
    /// Uses `ModelSource::Gguf` so no network access is implied.
    pub(crate) fn test_spec(alias: &str) -> ModelSpec {
        ModelSpec {
            alias: alias.to_string(),
            source: ModelSource::Gguf {
                dir: "/tmp/models".to_string(),
                files: vec!["model.gguf".to_string()],
            },
            capabilities: Capabilities::empty(),
            sampling: SamplingConfig::default(),
            isq: None,
            keep_resident: false,
            ctx_window: None,
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    mod tests {
        use super::*;

        #[tokio::test]
        async fn mock_engine_records_and_replies() {
            let m = MockEngine::new().with_reply("pong");
            let spec = test_spec("q");
            m.load(&spec).await.unwrap();
            let r = m
                .generate(
                    "q",
                    &EngineRequest {
                        messages: vec![],
                        sampling: Default::default(),
                        tools: vec![],
                    },
                )
                .await
                .unwrap();
            assert_eq!(r.text, "pong");
            assert_eq!(m.loaded(), vec!["q".to_string()]);
        }

        #[tokio::test]
        async fn mock_engine_fail_next_load() {
            let m = MockEngine::new();
            let spec = test_spec("fail-me");
            m.fail_next_load();
            let err = m.load(&spec).await.unwrap_err();
            assert!(matches!(err, LocalError::LoadFailed { .. }));
            // The alias should NOT appear in the loaded log.
            assert!(m.loaded().is_empty());
        }

        #[tokio::test]
        async fn mock_engine_records_unload() {
            let m = MockEngine::new();
            m.unload("some-model").await.unwrap();
            assert_eq!(m.unloaded(), vec!["some-model".to_string()]);
        }
    }
}
