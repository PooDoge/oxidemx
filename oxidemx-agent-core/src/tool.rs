use async_trait::async_trait;
use serde_json::Value;

use crate::events::StreamSink;

/// The seam between the core agent loop and whoever runs the tools. The overlay
/// implements this by delegating to its `execute_local_tool` dispatcher; agentd
/// (SP1b) implements it natively. Returns the tool's raw output string (JSON or
/// plain text) — the caller wraps it for the provider.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        name: &str,
        args: Value,
        sink: &Option<StreamSink>,
    ) -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoExec;
    #[async_trait]
    impl ToolExecutor for EchoExec {
        async fn execute(&self, name: &str, args: Value, _sink: &Option<StreamSink>) -> Result<String, String> {
            Ok(format!("{name}:{args}"))
        }
    }

    #[tokio::test]
    async fn executor_trait_dispatches_by_name() {
        let e = EchoExec;
        let out = e.execute("ping", serde_json::json!({"x":1}), &None).await.unwrap();
        assert_eq!(out, "ping:{\"x\":1}");
    }
}
