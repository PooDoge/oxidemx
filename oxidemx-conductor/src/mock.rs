//! A deterministic in-process `LLMProvider` for headless verification
//! and the CLI's `--mock` mode.
//!
//! It proves the supervisor's DAG scheduling — joins, retries,
//! timeouts, cancellation, artifact flow — without a network call or
//! an API key, so the P3 exit criterion ("research-digest runs
//! headless end-to-end with events on stdout") is reproducible in CI.
//!
//! Per the `building-llm-agents-in-rust` skill, only `chat_with_tools`
//! is implemented; the other `LLMProvider` super-traits are stubbed.
//! The responder is a plain closure over the message slice, so a test
//! can script per-step outputs precisely (match on the system persona
//! or the latest user content) or use the default echoing responder.

use std::sync::Arc;

use autoagents::async_trait;
use autoagents::llm::chat::{
    ChatMessage, ChatProvider, ChatResponse, ChatRole, StructuredOutputFormat, Tool,
};
use autoagents::llm::completion::{CompletionProvider, CompletionRequest, CompletionResponse};
use autoagents::llm::embedding::EmbeddingProvider;
use autoagents::llm::error::LLMError;
use autoagents::llm::models::ModelsProvider;
use autoagents::llm::{LLMProvider, ToolCall};

type Responder = Arc<dyn Fn(&[ChatMessage]) -> String + Send + Sync>;

/// A scriptable mock chat provider.
#[derive(Clone)]
pub struct MockProvider {
    responder: Responder,
}

impl MockProvider {
    /// Build from an arbitrary responder closure.
    pub fn new(responder: impl Fn(&[ChatMessage]) -> String + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            responder: Arc::new(responder),
        })
    }

    /// Default responder: echoes the latest user message verbatim, so
    /// a step's output faithfully carries its full input (which makes
    /// join / context-flow assertions trivial — the final answer
    /// contains traces of every ancestor whose output reached it).
    pub fn echoing() -> Arc<Self> {
        Self::new(|msgs| {
            let last_user = msgs
                .iter()
                .rev()
                .find(|m| m.role == ChatRole::User)
                .map(|m| m.content.as_str())
                .unwrap_or("");
            format!("MOCK-OUTPUT << {last_user}")
        })
    }

    /// Script replies by substring match against the concatenated
    /// message text (first match wins; `default` otherwise). Useful
    /// for giving each step a distinct, assertable output.
    pub fn scripted(rules: Vec<(&'static str, &'static str)>, default: &'static str) -> Arc<Self> {
        Self::new(move |msgs| {
            let joined: String = msgs.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n");
            for (needle, reply) in &rules {
                if joined.contains(needle) {
                    return reply.to_string();
                }
            }
            default.to_string()
        })
    }
}

#[derive(Debug)]
struct MockResponse(String);

impl std::fmt::Display for MockResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl ChatResponse for MockResponse {
    fn text(&self) -> Option<String> {
        Some(self.0.clone())
    }
    fn tool_calls(&self) -> Option<Vec<ToolCall>> {
        None
    }
}

#[async_trait]
impl ChatProvider for MockProvider {
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: Option<&[Tool]>,
        _json_schema: Option<StructuredOutputFormat>,
    ) -> Result<Box<dyn ChatResponse>, LLMError> {
        Ok(Box::new(MockResponse((self.responder)(messages))))
    }
}

#[async_trait]
impl CompletionProvider for MockProvider {
    async fn complete(
        &self,
        _req: &CompletionRequest,
        _json_schema: Option<StructuredOutputFormat>,
    ) -> Result<CompletionResponse, LLMError> {
        Err(LLMError::ProviderError("mock: no completion".into()))
    }
}

#[async_trait]
impl EmbeddingProvider for MockProvider {
    async fn embed(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>, LLMError> {
        // Cheap deterministic pseudo-embedding (length-based), enough
        // for any code path that only needs *some* vector back.
        Ok(input
            .iter()
            .map(|s| vec![s.len() as f32, s.chars().filter(|c| c.is_alphabetic()).count() as f32])
            .collect())
    }
}

impl ModelsProvider for MockProvider {}
impl LLMProvider for MockProvider {}

#[cfg(test)]
mod tests {
    use super::*;
    use autoagents::llm::chat::MessageType;

    fn user(text: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            message_type: MessageType::Text,
            content: text.into(),
        }
    }

    #[tokio::test]
    async fn echoing_carries_the_input() {
        let p = MockProvider::echoing();
        let resp = p.chat_with_tools(&[user("fetch the changelog")], None, None).await.unwrap();
        assert!(resp.text().unwrap().contains("fetch the changelog"));
    }

    #[tokio::test]
    async fn scripted_matches_first_rule() {
        let p = MockProvider::scripted(vec![("digest", "DIGESTED"), ("answer", "ANSWERED")], "DEFAULT");
        let r = p.chat_with_tools(&[user("please digest this")], None, None).await.unwrap();
        assert_eq!(r.text().unwrap(), "DIGESTED");
        let r = p.chat_with_tools(&[user("unrelated")], None, None).await.unwrap();
        assert_eq!(r.text().unwrap(), "DEFAULT");
    }
}
