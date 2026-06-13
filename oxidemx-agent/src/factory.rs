//! The single provider-construction seam.
//!
//! Every consumer (overlay, CLI, future agentd) builds its LLM
//! provider here, so adding/swapping a backend touches one place.
//! All but `ClaudeCode` ride AutoAgents' built-in backends (standard
//! chat APIs, history shipped via the executor's memory).

use std::sync::Arc;

use autoagents::llm::backends::{anthropic::Anthropic, google::Google, ollama::Ollama, openai::OpenAI};
use autoagents::llm::builder::LLMBuilder;
use autoagents::llm::error::LLMError;
use autoagents::llm::LLMProvider;
use oxidemx_shared::config::AiProvider;

use crate::claude_code::ClaudeCodeProvider;

/// Build the configured LLM provider. `api_key` is ignored for the
/// keyless providers (Ollama, Claude Code).
pub fn provider_from_config(
    provider: AiProvider,
    model: &str,
    api_key: &str,
) -> Result<Arc<dyn LLMProvider>, LLMError> {
    Ok(match provider {
        AiProvider::Gemini => LLMBuilder::<Google>::new()
            .api_key(api_key)
            .model(model)
            .build()? as Arc<dyn LLMProvider>,
        AiProvider::OpenAi => LLMBuilder::<OpenAI>::new()
            .api_key(api_key)
            .model(model)
            .build()? as Arc<dyn LLMProvider>,
        AiProvider::Anthropic => LLMBuilder::<Anthropic>::new()
            .api_key(api_key)
            .model(model)
            .build()? as Arc<dyn LLMProvider>,
        AiProvider::Ollama => {
            let mut b = LLMBuilder::<Ollama>::new().model(model);
            // Ollama needs no key; the builder still accepts an empty one.
            if !api_key.is_empty() {
                b = b.api_key(api_key);
            }
            b.build()? as Arc<dyn LLMProvider>
        }
        AiProvider::ClaudeCode => {
            let m = (!model.is_empty()).then(|| model.to_string());
            ClaudeCodeProvider::new(m) as Arc<dyn LLMProvider>
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_providers_construct() {
        for p in [AiProvider::Gemini, AiProvider::OpenAi, AiProvider::Anthropic, AiProvider::Ollama] {
            assert!(
                provider_from_config(p, p.default_model(), "k").is_ok(),
                "{p:?} failed to construct"
            );
        }
    }

    #[test]
    fn claude_code_constructs_without_key() {
        assert!(provider_from_config(AiProvider::ClaudeCode, "", "").is_ok());
    }
}
