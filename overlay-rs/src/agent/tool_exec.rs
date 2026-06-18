use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;
use oxidemx_agent_core::events::StreamSink;
use oxidemx_agent_core::tool::ToolExecutor;

/// Overlay-side tool execution: delegates to the existing dispatcher (which owns
/// approval chips, activity, cards, and the UI-coupled tools).
pub struct OverlayToolExecutor;

#[async_trait]
impl ToolExecutor for OverlayToolExecutor {
    async fn execute(&self, name: &str, args: Value, sink: &Option<StreamSink>) -> Result<String, String> {
        crate::ai_client::tools::execute_local_tool(name, args, sink)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Shared executor handle the overlay hands to core for every turn.
pub fn executor() -> Arc<dyn ToolExecutor> {
    use once_cell::sync::Lazy;
    static EXEC: Lazy<Arc<dyn ToolExecutor + Send + Sync>> =
        Lazy::new(|| Arc::new(OverlayToolExecutor));
    EXEC.clone()
}
