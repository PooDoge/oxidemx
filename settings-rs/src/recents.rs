//! Persisted "recently used icons" list.
//!
//! Keeps a small ordered list of icon names the user has picked
//! through the icon picker. Surfaces at the top of the grid so
//! frequently-reached icons are one click away. Persisted to a
//! tiny JSON file (`~/.config/juhradial/recent-icons.json`) so
//! the list survives across settings restarts.
//!
//! Why a separate file: the main config is hot-reloaded by the
//! overlay via inotify, so churning that file every time the user
//! picks an icon would trigger overlay re-renders for no reason.
//! A sidecar file isolates this purely-UI state from the config
//! the overlay actually watches.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How many entries to keep. Big enough to cover a workflow's
/// common icons; small enough to render in a single row beside
/// the picker search bar without crowding.
pub const MAX_RECENTS: usize = 12;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OnDisk {
    items: Vec<String>,
}

/// Resolve the path of the recents file. Returns `None` only when
/// the runtime can't locate `$XDG_CONFIG_HOME` or `$HOME` — both
/// vanishingly unlikely in a normal session.
pub fn path() -> Option<PathBuf> {
    juhradial_shared::config::default_config_path()
        .and_then(|p| p.parent().map(|p| p.join("recent-icons.json")))
}

/// Load the recents list from disk. Missing file or any parse
/// error returns an empty list — corruption isn't fatal here, the
/// list is purely a UX nicety.
pub fn load() -> Vec<String> {
    let path = match path() {
        Some(p) => p,
        None => return Vec::new(),
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str::<OnDisk>(&s)
            .map(|d| d.items)
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Push `name` to the front of the list (most-recent first),
/// dedupe, cap at [`MAX_RECENTS`], and persist atomically. Drops
/// the persist on IO error — the in-memory list still updates so
/// the user sees the change for the rest of the session.
pub async fn save_after_pick(name: String, current: Vec<String>) -> Vec<String> {
    let mut next: Vec<String> = vec![name.clone()];
    for existing in current.into_iter() {
        if existing != name {
            next.push(existing);
            if next.len() >= MAX_RECENTS {
                break;
            }
        }
    }
    persist(&next).await;
    next
}

async fn persist(items: &[String]) {
    let path = match path() {
        Some(p) => p,
        None => return,
    };
    let payload = OnDisk {
        items: items.to_vec(),
    };
    let json = match serde_json::to_string_pretty(&payload) {
        Ok(j) => j,
        Err(_) => return,
    };
    // Atomic temp-file + rename so a crash mid-write doesn't
    // corrupt the existing list. Same pattern as the main
    // config writer.
    let tmp = path.with_extension("json.tmp");
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    })
    .await;
}
