//! Tiny per-user UI-only state — currently just the last-visited
//! settings tab. Lives in a single text file
//! `~/.config/oxidemx/ui-state` so it doesn't intermingle with
//! the user's actual JSON config and stays trivially debuggable
//! (one line per knob).
//!
//! Best-effort I/O — corruption / missing file falls back to
//! sensible defaults, which keeps the launch path simple (no
//! migrations, no schema, no surprise).

use std::path::PathBuf;

/// Where the UI-state file lives. Co-located with the JSON config
/// dir so a `oxidemx-config-export` style tool could pick it up
/// alongside the rest, but stored as a plain text file so anyone
/// can `cat` / `echo >` it without thinking.
fn ui_state_path() -> Option<PathBuf> {
    oxidemx_shared::config::default_config_path()
        .and_then(|p| p.parent().map(|dir| dir.join("ui-state")))
}

/// Read the saved last-tab token. Returns `None` on any failure
/// (file missing, unreadable, empty) — caller should fall back to
/// the default tab.
pub fn load_last_tab() -> Option<String> {
    let path = ui_state_path()?;
    let bytes = std::fs::read(&path).ok()?;
    let s = String::from_utf8(bytes).ok()?;
    let tag = s.lines().next()?.trim().to_string();
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

/// Persist the named tab. Best-effort — silently ignores write
/// failures so the user can't end up in a state where the UI
/// won't switch tabs because we couldn't write a non-essential
/// preference file.
pub async fn save_last_tab(tag: String) {
    let path = match ui_state_path() {
        Some(p) => p,
        None => return,
    };
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, tag);
    })
    .await;
}
