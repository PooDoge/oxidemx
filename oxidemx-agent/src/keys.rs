//! Per-provider API key resolution.
//!
//! Looks up the environment variable first, then a per-provider key
//! file under `~/.config/oxidemx/` (0600, written by the settings AI
//! tab). Ollama and Claude Code need no key.

use std::path::PathBuf;

use oxidemx_shared::config::AiProvider;

/// `(env var, key-file stem)` for the key-bearing providers.
fn key_spec(provider: AiProvider) -> Option<(&'static str, &'static str)> {
    match provider {
        AiProvider::Gemini => Some(("GEMINI_API_KEY", "gemini")),
        AiProvider::OpenAi => Some(("OPENAI_API_KEY", "openai")),
        AiProvider::Anthropic => Some(("ANTHROPIC_API_KEY", "anthropic")),
        AiProvider::Ollama | AiProvider::ClaudeCode => None,
    }
}

/// Path to a provider's key file, e.g. `~/.config/oxidemx/openai.key`.
pub fn key_path(provider: AiProvider) -> Option<PathBuf> {
    let (_, stem) = key_spec(provider)?;
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(format!(".config/oxidemx/{stem}.key")))
}

/// The configured key for `provider`: env var, else the key file.
/// `None` for keyless providers or when nothing is configured.
pub fn provider_key(provider: AiProvider) -> Option<String> {
    let (env, _) = key_spec(provider)?;
    if let Ok(k) = std::env::var(env) {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Some(k);
        }
    }
    let path = key_path(provider)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}
