//! [`LocalChatProvider`] — AutoAgents [`ChatProvider`] adapter that routes
//! `chat_with_tools` calls to the embedded [`LocalModelService`], with no HTTP.
//!
//! Role mapping:
//! - `ChatRole::System`    → `Role::System`
//! - `ChatRole::User`      → `Role::User`
//! - `ChatRole::Assistant` → `Role::Assistant`
//! - `ChatRole::Tool`      → `Role::Tool`
//!
//! Mode selection: `Mode::ToolUse` when `tools` is `Some(&[..])` with at least
//! one element; `Mode::Chat` otherwise.

use std::fmt;
use std::sync::Arc;

use autoagents::async_trait;
use autoagents::llm::chat::{
    ChatMessage, ChatProvider, ChatResponse as AutoChatResponse, ChatRole, StructuredOutputFormat,
    Tool,
};
use autoagents::llm::completion::{CompletionProvider, CompletionRequest, CompletionResponse};
use autoagents::llm::embedding::EmbeddingProvider;
use autoagents::llm::error::LLMError;
use autoagents::llm::models::ModelsProvider;
use autoagents::llm::{LLMProvider, ToolCall};

use crate::engine::SchemaConstraint;
use crate::error::LocalError;
use crate::mode::Mode;
use crate::service::LocalModelService;
use crate::types::{ChatRequest, Message, Role};

// ── Response adapter ──────────────────────────────────────────────────────────

/// Wraps the text returned by the local service as an AutoAgents [`ChatResponse`].
#[derive(Debug)]
struct LocalResponse(String);

impl fmt::Display for LocalResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AutoChatResponse for LocalResponse {
    fn text(&self) -> Option<String> {
        (!self.0.is_empty()).then(|| self.0.clone())
    }

    fn tool_calls(&self) -> Option<Vec<ToolCall>> {
        None
    }
}

// ── Role mapping ──────────────────────────────────────────────────────────────

fn map_role(r: &ChatRole) -> Role {
    match r {
        ChatRole::System => Role::System,
        ChatRole::User => Role::User,
        ChatRole::Assistant => Role::Assistant,
        ChatRole::Tool => Role::Tool,
    }
}

// ── Error mapping ─────────────────────────────────────────────────────────────

fn map_err(e: LocalError) -> LLMError {
    LLMError::ProviderError(e.to_string())
}

// ── Provider ──────────────────────────────────────────────────────────────────

/// AutoAgents [`ChatProvider`] backed by a [`LocalModelService`].
///
/// Construct with [`LocalChatProvider::new`] and pass to an AutoAgents agent
/// builder in place of any remote provider.
pub struct LocalChatProvider {
    service: Arc<dyn LocalModelService>,
    alias: String,
}

impl LocalChatProvider {
    /// Create a new provider that routes requests to `service` under `alias`.
    pub fn new(service: Arc<dyn LocalModelService>, alias: String) -> Self {
        Self { service, alias }
    }
}

#[async_trait]
impl ChatProvider for LocalChatProvider {
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Tool]>,
        json_schema: Option<StructuredOutputFormat>,
    ) -> Result<Box<dyn AutoChatResponse>, LLMError> {
        let mode = match tools {
            Some(t) if !t.is_empty() => Mode::ToolUse,
            _ => Mode::Chat,
        };

        let msgs: Vec<Message> = messages
            .iter()
            .map(|m| Message {
                role: map_role(&m.role),
                content: m.content.clone(),
            })
            .collect();

        // Thread the JSON-schema structured-output request into a
        // SchemaConstraint so the engine applies constrained decoding.
        // When both tools AND a schema are present, tools take precedence
        // and the schema is silently ignored (matching provider semantics).
        let constraint = if tools.map(|t| !t.is_empty()).unwrap_or(false) {
            None
        } else {
            json_schema.and_then(|s| s.schema).map(SchemaConstraint::JsonSchema)
        };

        let req = ChatRequest {
            messages: msgs,
            mode,
            tools: vec![],
            sampling_override: None,
            system_template: None,
            constraint,
        };

        let resp = self
            .service
            .chat_with_model(&self.alias, req)
            .await
            .map_err(map_err)?;

        Ok(Box::new(LocalResponse(resp.text)))
    }
}

#[async_trait]
impl CompletionProvider for LocalChatProvider {
    async fn complete(
        &self,
        _req: &CompletionRequest,
        _json_schema: Option<StructuredOutputFormat>,
    ) -> Result<CompletionResponse, LLMError> {
        Err(LLMError::ProviderError(
            "local model service does not support text completion".into(),
        ))
    }
}

#[async_trait]
impl EmbeddingProvider for LocalChatProvider {
    async fn embed(&self, _input: Vec<String>) -> Result<Vec<Vec<f32>>, LLMError> {
        Err(LLMError::ProviderError(
            "local model service does not support embeddings".into(),
        ))
    }
}

impl ModelsProvider for LocalChatProvider {}
impl LLMProvider for LocalChatProvider {}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Verdict;
    use crate::types::{ChatResponse, ModelStatusInfo, Usage};

    struct StubService {
        reply: String,
    }

    impl StubService {
        fn returning(text: &str) -> Self {
            Self { reply: text.into() }
        }
    }

    #[async_trait]
    impl LocalModelService for StubService {
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, LocalError> {
            Ok(ChatResponse {
                text: self.reply.clone(),
                usage: Usage::default(),
                verdict: Verdict::Ok,
            })
        }

        async fn chat_with_model(
            &self,
            _alias: &str,
            _req: ChatRequest,
        ) -> Result<ChatResponse, LocalError> {
            Ok(ChatResponse {
                text: self.reply.clone(),
                usage: Usage::default(),
                verdict: Verdict::Ok,
            })
        }

        async fn ensure_loaded(&self, _alias: &str) -> Result<(), LocalError> {
            Ok(())
        }

        async fn unload(&self, _alias: &str) -> Result<(), LocalError> {
            Ok(())
        }

        async fn set_active(&self, _alias: &str) -> Result<(), LocalError> {
            Ok(())
        }

        fn status(&self) -> Vec<ModelStatusInfo> {
            vec![]
        }
    }

    #[tokio::test]
    async fn provider_maps_messages_and_returns_text() {
        let svc = Arc::new(StubService::returning("pong"));
        let p = LocalChatProvider::new(svc, "a".into());
        let msgs = [autoagents::llm::chat::ChatMessage::user()
            .content("ping")
            .build()];
        let resp = p.chat_with_tools(&msgs, None, None).await.unwrap();
        assert_eq!(resp.text().unwrap_or_default().trim(), "pong");
    }
}
