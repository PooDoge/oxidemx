//! Thin wrapper around `juhradial_shared::AppConfig` with first-run
//! bootstrap. Keeps the single-source-of-truth contract: the daemon
//! and the overlay both deserialize the same JSON via the same Rust
//! structs.

use juhradial_shared::config::seed_default_config_if_missing;
use juhradial_shared::AppConfig;
use tracing::info;

pub fn load() -> Result<AppConfig, juhradial_shared::config::ConfigError> {
    let path = match juhradial_shared::config::default_config_path() {
        Some(p) => p,
        None => return Ok(AppConfig::default()),
    };

    // Seed the bundled 8-slice starter config on first run so the
    // overlay always opens with something visible. Non-fatal: if
    // seeding fails (e.g. read-only ~), fall through to load_from
    // which will return the empty default.
    match seed_default_config_if_missing(&path) {
        Ok(true) => info!(?path, "seeded default config (first run)"),
        Ok(false) => {} // existed already
        Err(e) => tracing::warn!("could not seed default config: {e}"),
    }

    AppConfig::load_from(&path)
}

// TODO: pub fn watch(tx: async_channel::Sender<()>) -> notify::Result<RecommendedWatcher>
//        — debounce inotify events on config.json + profiles/*.json and
//          push a reload signal so the overlay refreshes its slice cache.
