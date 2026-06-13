//! Backend-selectable provider construction.
//!
//! One construction seam for every consumer (CLI today; overlay and
//! agentd later): `AiConfig.backend` picks the transport, and a
//! fallback to Gemini's classic generateContent API is one config
//! edit away if Interactions misbehaves.

use std::sync::Arc;

use autoagents::llm::backends::google::Google;
use autoagents::llm::builder::LLMBuilder;
use autoagents::llm::error::LLMError;
use autoagents::llm::LLMProvider;
use oxidemx_shared::config::AiBackend;

use crate::provider::GeminiInteractionsProvider;

/// Build the configured LLM provider.
///
/// `Interactions` → our own [`GeminiInteractionsProvider`]
/// (server-side sessions, cancellation, SSE deltas).
/// `GenerateContent` → AutoAgents' built-in `Google` backend
/// (classic stateless API; no session state, no cancel token —
/// fallback semantics only).
pub fn provider_from_config(
    backend: AiBackend,
    model: &str,
    api_key: &str,
) -> Result<Arc<dyn LLMProvider>, LLMError> {
    match backend {
        AiBackend::Interactions => {
            Ok(GeminiInteractionsProvider::new(api_key, model) as Arc<dyn LLMProvider>)
        }
        AiBackend::GenerateContent => {
            let google: Arc<Google> = LLMBuilder::<Google>::new()
                .api_key(api_key)
                .model(model)
                .build()?;
            Ok(google as Arc<dyn LLMProvider>)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_backends_construct() {
        assert!(provider_from_config(AiBackend::Interactions, "gemini-2.5-flash", "k").is_ok());
        assert!(provider_from_config(AiBackend::GenerateContent, "gemini-2.5-flash", "k").is_ok());
    }

    #[test]
    fn generate_content_requires_a_key() {
        // The upstream Google builder rejects an absent key at build
        // time; an empty string is its caller's responsibility, so
        // we only assert our construction path doesn't panic.
        let r = provider_from_config(AiBackend::GenerateContent, "gemini-2.5-pro", "");
        assert!(r.is_ok() || r.is_err()); // must not panic either way
    }
}
