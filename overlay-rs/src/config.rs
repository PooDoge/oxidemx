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

/// Watch `~/.config/juhradial/config.json` for changes and yield a
/// fresh `AppConfig` each time it's written. Dropped events are
/// debounced — saves from text editors typically produce a flurry
/// (write + chmod + rename), and we only want one reload per
/// write-burst.
///
/// Returns a `Stream<AppConfig>` that the iced application
/// subscribes to via `Subscription::run` (same pattern as
/// `dbus::stream`). Events arrive on the iced main loop so the
/// reload happens single-threaded with the rest of state mutation.
///
/// The watcher background task is spawned on tokio (already a
/// dep for `iced::time::every`); the channel between watcher and
/// stream is async-channel for executor independence.
pub fn watch_stream() -> impl futures_util::stream::Stream<Item = juhradial_shared::AppConfig> {
    use futures_util::StreamExt;
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};
    use std::time::{Duration, Instant};

    let (tx, rx) = async_channel::unbounded::<juhradial_shared::AppConfig>();
    let path = match juhradial_shared::config::default_config_path() {
        Some(p) => p,
        None => {
            // No HOME / no XDG_CONFIG_HOME — emit nothing forever.
            return rx.boxed();
        }
    };

    // Use a sync mpsc + tokio task to bridge `notify`'s callback
    // (which runs on its own thread) into our async world.
    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<()>();
    let watcher_path = path.clone();
    tokio::task::spawn_blocking(move || {
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    use notify::EventKind;
                    if matches!(
                        event.kind,
                        EventKind::Modify(_)
                            | EventKind::Create(_)
                            | EventKind::Remove(_)
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

        // Watch the *parent directory* — atomic-rename writes
        // (most editors) replace the inode rather than modifying
        // it in place, so a watch on the file itself loses the
        // hook after first write.
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

        // The watcher object lives until this function returns —
        // park forever.
        std::thread::park();
        drop(watcher); // explicit
    });

    // Debounce + load → forward AppConfig values into the async
    // channel. 150 ms covers an editor's write+rename burst.
    let debounce = Duration::from_millis(150);
    let path_for_loader = path.clone();
    tokio::task::spawn_blocking(move || {
        let mut last_emit = Instant::now() - Duration::from_secs(60);
        let mut pending = false;
        loop {
            // Block until at least one event arrives.
            let recv = raw_rx.recv_timeout(if pending { debounce } else { Duration::from_secs(60) });
            match recv {
                Ok(()) => {
                    pending = true;
                    last_emit = Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if pending && last_emit.elapsed() >= debounce {
                        // Settled — load + emit.
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
