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

/// Default base URL for the `MistralRs` provider when no endpoint is
/// supplied. Mirrors `oxidemx_shared::config`'s `default_local_endpoint`.
/// MUST keep the trailing slash (see [`normalize_base_url`]).
const DEFAULT_LOCAL_ENDPOINT: &str = "http://localhost:1234/v1/";

/// Placeholder bearer token for keyless OpenAI-compatible local servers.
/// The AutoAgents OpenAI backend rejects an empty key with `AuthError`
/// (`backends/openai.rs`), so we send the conventional `EMPTY` sentinel;
/// mistral.rs ignores the `Authorization` header by default.
const LOCAL_PLACEHOLDER_KEY: &str = "EMPTY";

/// Build the configured LLM provider. `api_key` is ignored for the
/// keyless providers (Ollama, mistral.rs, Claude Code).
///
/// Thin wrapper over [`provider_from_config_with_endpoint`] with no
/// endpoint override — kept for the CLI/conductor call sites that don't
/// carry an `AiConfig`. `MistralRs` falls back to [`DEFAULT_LOCAL_ENDPOINT`].
pub fn provider_from_config(
    provider: AiProvider,
    model: &str,
    api_key: &str,
) -> Result<Arc<dyn LLMProvider>, LLMError> {
    provider_from_config_with_endpoint(provider, model, api_key, None)
}

/// Build the configured LLM provider, optionally overriding the local
/// OpenAI-compatible `base_url` (used only by `MistralRs`). The overlay
/// passes `ai.local_endpoint` here; other providers ignore it.
pub fn provider_from_config_with_endpoint(
    provider: AiProvider,
    model: &str,
    api_key: &str,
    endpoint: Option<&str>,
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
        AiProvider::MistralRs => {
            // mistral.rs speaks the OpenAI wire protocol, so we ride the
            // OpenAI backend pointed at the local server. The base_url
            // must end in `/` or `Url::join("chat/completions")` drops
            // the `/v1` segment — normalize defensively.
            let base = normalize_base_url(endpoint.unwrap_or(DEFAULT_LOCAL_ENDPOINT));
            let key = if api_key.is_empty() { LOCAL_PLACEHOLDER_KEY } else { api_key };
            LLMBuilder::<OpenAI>::new()
                .api_key(key)
                .base_url(base)
                .model(model)
                .build()? as Arc<dyn LLMProvider>
        }
        AiProvider::ClaudeCode => {
            let m = (!model.is_empty()).then(|| model.to_string());
            ClaudeCodeProvider::new(m) as Arc<dyn LLMProvider>
        }
    })
}

/// Ensure a base URL ends with `/` so `reqwest::Url::join` appends rather
/// than replaces the final path segment.
fn normalize_base_url(url: &str) -> String {
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    }
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

    #[test]
    fn mistral_rs_constructs_without_key() {
        // Keyless: the EMPTY placeholder must satisfy the OpenAI backend.
        assert!(provider_from_config(AiProvider::MistralRs, "default", "").is_ok());
        // Custom endpoint without a trailing slash still constructs.
        assert!(provider_from_config_with_endpoint(
            AiProvider::MistralRs,
            "default",
            "",
            Some("http://localhost:8080/v1"),
        )
        .is_ok());
    }

    #[test]
    fn base_url_gets_trailing_slash() {
        assert_eq!(normalize_base_url("http://x/v1"), "http://x/v1/");
        assert_eq!(normalize_base_url("http://x/v1/"), "http://x/v1/");
    }
}
