//! Config loading and inotify-based live-reload for `~/.config/juhradial/config.json`.
//!
//! Pattern mirrors `overlay-rs/src/config.rs`: a background tokio task
//! watches the config directory for changes, debounces editor write bursts,
//! and forwards fresh `AppConfig` values through an `async_channel` that
//! iced's `Subscription::run` consumes.

use juhradial_shared::config::seed_default_config_if_missing;
use juhradial_shared::AppConfig;
use tracing::info;

/// Load the current config from `~/.config/juhradial/config.json`.
/// Seeds the file with defaults on first run (non-fatal if it fails).
pub fn load() -> Result<AppConfig, juhradial_shared::config::ConfigError> {
    let path = match juhradial_shared::config::default_config_path() {
        Some(p) => p,
        None => return Ok(AppConfig::default()),
    };

    match seed_default_config_if_missing(&path) {
        Ok(true) => info!(?path, "seeded default config (first run)"),
        Ok(false) => {}
        Err(e) => tracing::warn!("could not seed default config: {e}"),
    }

    AppConfig::load_from(&path)
}

/// Watch `~/.config/juhradial/config.json` for changes and yield a
/// fresh `AppConfig` each time it's written. Debounced to coalesce
/// the write+rename burst that most text editors produce.
///
/// Returns a `Stream<AppConfig>` for `iced::Subscription::run`.
pub fn watch_stream() -> impl futures_util::stream::Stream<Item = juhradial_shared::AppConfig> {
    use futures_util::StreamExt;
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};
    use std::time::{Duration, Instant};

    let (tx, rx) = async_channel::unbounded::<juhradial_shared::AppConfig>();
    let path = match juhradial_shared::config::default_config_path() {
        Some(p) => p,
        None => return rx.boxed(),
    };

    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<()>();
    let watcher_path = path.clone();
    tokio::task::spawn_blocking(move || {
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    use notify::EventKind;
                    if matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) {
                        let _ = raw_tx.send(());
                    }
                }
            },
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("config watcher init failed: {e}");
                return;
            }
        };

        // Watch the parent directory — atomic-rename writes (most editors)
        // replace the inode rather than modifying it in place, so a watch
        // on the file itself loses the hook after the first write.
        let dir = match watcher_path.parent() {
            Some(d) => d.to_path_buf(),
            None => return,
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("config watcher: couldn't ensure {}: {e}", dir.display());
            return;
        }
        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            tracing::warn!("config watcher: failed to watch {}: {e}", dir.display());
            return;
        }

        std::thread::park();
        drop(watcher);
    });

    let debounce = Duration::from_millis(150);
    let path_for_loader = path.clone();
    tokio::task::spawn_blocking(move || {
        let mut last_emit = Instant::now() - Duration::from_secs(60);
        let mut pending = false;
        loop {
            let recv =
                raw_rx.recv_timeout(if pending { debounce } else { Duration::from_secs(60) });
            match recv {
                Ok(()) => {
                    pending = true;
                    last_emit = Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if pending && last_emit.elapsed() >= debounce {
                        match juhradial_shared::AppConfig::load_from(&path_for_loader) {
                            Ok(cfg) => {
                                if tx.send_blocking(cfg).is_err() {
                                    return;
                                }
                            }
                            Err(e) => tracing::warn!(
                                "config reload failed for {}: {e}",
                                path_for_loader.display()
                            ),
                        }
                        pending = false;
                    }
                }
                Err(_) => return,
            }
        }
    });

    rx.boxed()
}
