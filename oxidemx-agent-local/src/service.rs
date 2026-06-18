//! [`LocalModelService`] — the public async trait consumed by agentd.

use async_trait::async_trait;

use crate::error::LocalError;
use crate::types::{ChatRequest, ChatResponse, ModelStatusInfo};

/// The public interface to the local-LLM session manager.
///
/// Implementations hold a model registry and an [`crate::engine::InferenceEngine`]
/// and enforce: one-at-a-time load, capability scoping, guard evaluation, and
/// idle eviction.
#[async_trait]
pub trait LocalModelService: Send + Sync {
    /// Run a chat turn using the currently-active model (or the default).
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LocalError>;

    /// Run a chat turn against the named model, loading it if necessary.
    async fn chat_with_model(
        &self,
        alias: &str,
        req: ChatRequest,
    ) -> Result<ChatResponse, LocalError>;

    /// Ensure the named model is loaded and ready.
    ///
    /// Any previously-loaded model is unloaded first (one-at-a-time rule).
    async fn ensure_loaded(&self, alias: &str) -> Result<(), LocalError>;

    /// Unload the named model, releasing its memory.
    async fn unload(&self, alias: &str) -> Result<(), LocalError>;

    /// Set the named model as the default for bare `chat()` calls.
    async fn set_active(&self, alias: &str) -> Result<(), LocalError>;

    /// Return a snapshot of every registered model's state.
    ///
    /// This method is **non-blocking** and never waits on the load mutex.
    fn status(&self) -> Vec<ModelStatusInfo>;
}
