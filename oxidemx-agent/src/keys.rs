//! Per-provider API key resolution.
//!
//! Looks up the environment variable first, then a per-provider key
//! file under `~/.config/oxidemx/` (0600, written by the settings AI
//! tab). Ollama and Claude Code need no key.

use std::path::PathBuf;

use oxidemx_shared::config::AiProvider;

/// Path to a provider's key file, e.g. `~/.config/oxidemx/openai.key`.
pub fn key_path(provider: AiProvider) -> Option<PathBuf> {
    let stem = provider.key_file_stem()?;
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(format!(".config/oxidemx/{stem}.key")))
}

/// The configured key for `provider`: env var, else the key file.
/// `None` for keyless providers or when nothing is configured.
pub fn provider_key(provider: AiProvider) -> Option<String> {
    if let Some(env) = provider.key_env() {
        if let Ok(k) = std::env::var(env) {
            let k = k.trim().to_string();
            if !k.is_empty() {
                return Some(k);
            }
        }
    }
    let path = key_path(provider)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}
