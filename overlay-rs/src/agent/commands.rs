//! Backend for the `execute_command` agent tool.
//!
//! The matcher + runner now live in `oxidemx_agent::allowlist` (one
//! source of truth, shared with the CLI/agentd harness). This module
//! re-exports them and keeps the two config-coupled helpers the
//! overlay needs: loading the persisted allowlist and appending to it
//! (the approval card's "always allow"). Matching policy is unchanged
//! — whole-token prefix match, compound-command split, wrapper strip.

pub use oxidemx_agent::allowlist::{is_allowlisted, run};

/// Append `entry` to the persisted allowlist (the approval card's
/// "always allow" action). Loads the live config, appends if new,
/// writes back pretty-printed. Errors are returned as strings —
/// callers surface them in the tool result rather than panicking.
pub fn add_allowlist_entry(entry: &str) -> Result<(), String> {
    let entry = entry.trim();
    if entry.is_empty() || entry == "*" {
        return Err("refusing to add an empty/match-all allowlist entry".to_string());
    }
    let path = oxidemx_shared::config::default_config_path().ok_or("config path unavailable")?;
    let mut cfg = oxidemx_shared::AppConfig::load_from(&path).map_err(|e| e.to_string())?;
    if cfg.overlay.ai.command_allowlist.iter().any(|e| e == entry) {
        return Ok(());
    }
    cfg.overlay.ai.command_allowlist.push(entry.to_string());
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

/// The user's configured allowlist, freshly loaded from the main
/// config file so mid-session edits take effect on the next call.
/// Falls back to the built-in default list when the config is
/// missing or unreadable (same behaviour as the rest of the app).
pub fn allowlist() -> Vec<String> {
    oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai.command_allowlist)
        .unwrap_or_else(|| oxidemx_shared::config::AiConfig::default().command_allowlist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_rejects_empty_and_match_all() {
        assert!(add_allowlist_entry("").is_err());
        assert!(add_allowlist_entry("*").is_err());
    }

    #[test]
    fn matcher_is_the_shared_impl() {
        // Smoke check the re-export wires through; the full matcher
        // suite lives in oxidemx_agent::allowlist.
        let al = vec!["git status".to_string()];
        assert!(is_allowlisted("git status --short", &al));
        assert!(!is_allowlisted("git stash", &al));
        assert!(!is_allowlisted("git status && rm -rf /", &al));
    }
}
