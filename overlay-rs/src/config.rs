//! Thin wrapper around `juhradial_shared::AppConfig` plus an inotify
//! watcher that emits `ConfigChanged` events when the user saves from
//! the editor (or hand-edits `~/.config/juhradial/config.json`).
//!
//! Keeps the single-source-of-truth contract: the daemon and the overlay
//! both deserialize the same JSON via the same Rust structs.

use juhradial_shared::AppConfig;

pub fn load() -> Result<AppConfig, juhradial_shared::config::ConfigError> {
    if let Some(path) = juhradial_shared::config::default_config_path() {
        AppConfig::load_from(&path)
    } else {
        Ok(AppConfig::default())
    }
}

// TODO: pub fn watch(tx: async_channel::Sender<()>) -> notify::Result<RecommendedWatcher>
//        — debounce inotify events on config.json + profiles/*.json and
//          push a reload signal so the overlay refreshes its slice cache.
