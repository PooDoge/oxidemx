//! Thin wrapper around `oxidemx_shared::AppConfig` with first-run
//! bootstrap. Keeps the single-source-of-truth contract: the daemon
//! and the overlay both deserialize the same JSON via the same Rust
//! structs.

use oxidemx_shared::config::seed_default_config_if_missing;
use oxidemx_shared::AppConfig;
use tracing::info;

pub fn load() -> Result<AppConfig, oxidemx_shared::config::ConfigError> {
    let path = match oxidemx_shared::config::default_config_path() {
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

/// Persist the committed chat window size into `overlay.chat_size`.
///
/// Edits the on-disk JSON as a `serde_json::Value` instead of
/// round-tripping through `AppConfig` — the overlay must never drop
/// keys it doesn't model (the daemon/settings own most of the file).
/// Best-effort: failures are logged, the in-memory size still wins
/// for this session.
pub fn save_chat_size(w: u32, h: u32) {
    let Some(path) = oxidemx_shared::config::default_config_path() else {
        return;
    };
    let mut root: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let overlay = root
        .as_object_mut()
        .expect("root forced to object above")
        .entry("overlay")
        .or_insert_with(|| serde_json::json!({}));
    if !overlay.is_object() {
        *overlay = serde_json::json!({});
    }
    overlay
        .as_object_mut()
        .expect("overlay forced to object above")
        .insert("chat_size".into(), serde_json::json!([w, h]));
    match serde_json::to_string_pretty(&root) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!(error = %e, "failed to persist chat_size");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to serialise config for chat_size"),
    }
}

/// Watch `~/.config/oxidemx/config.json` for changes and yield a
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
pub fn watch_stream() -> impl futures_util::stream::Stream<Item = oxidemx_shared::AppConfig> {
    use futures_util::StreamExt;
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};
    use std::time::{Duration, Instant};

    let (tx, rx) = async_channel::unbounded::<oxidemx_shared::AppConfig>();
    let path = match oxidemx_shared::config::default_config_path() {
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
        let mut watcher: RecommendedWatcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    use notify::EventKind;
                    if matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) {
                        let _ = raw_tx.send(());
                    }
                }
            }) {
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
            if tx.is_closed() {
                return;
            }
            // Block until at least one event arrives.
            let recv = raw_rx.recv_timeout(if pending {
                debounce
            } else {
                Duration::from_millis(250)
            });
            match recv {
                Ok(()) => {
                    pending = true;
                    last_emit = Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if pending && last_emit.elapsed() >= debounce {
                        // Settled — load + emit.
                        match oxidemx_shared::AppConfig::load_from(&path_for_loader) {
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

/// Watch the widgets install dir (`~/.config/oxidemx/widgets/`) and
/// invoke `on_change` after each debounced burst of filesystem
/// events — installs, uninstalls, and manifest edits all land here.
/// The widget-host bridge passes a closure that try_sends
/// `HostCtl::RescanWidgets` at the worker.
///
/// Must be called from a tokio runtime context (uses
/// `spawn_blocking`, like `watch_stream`). Watches recursively:
/// bundles unpack as `<id>/widget.json` + assets, and the events we
/// care about are one level down.
pub fn spawn_widgets_dir_watcher(on_change: impl Fn() + Send + 'static) {
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};
    use std::time::{Duration, Instant};

    let Some(dir) = oxidemx_widget_host::WidgetRegistry::widgets_dir() else {
        tracing::warn!("widgets dir unresolvable (no HOME?); rescan watcher disabled");
        return;
    };

    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<()>();
    let watch_dir = dir.clone();
    tokio::task::spawn_blocking(move || {
        let mut watcher: RecommendedWatcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    use notify::EventKind;
                    if matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) {
                        let _ = raw_tx.send(());
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("widgets watcher init failed: {e}");
                    return;
                }
            };
        if let Err(e) = std::fs::create_dir_all(&watch_dir) {
            tracing::warn!("widgets watcher: couldn't ensure {}: {e}", watch_dir.display());
            return;
        }
        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::Recursive) {
            tracing::warn!("widgets watcher: failed to watch {}: {e}", watch_dir.display());
            return;
        }
        std::thread::park();
        drop(watcher); // explicit
    });

    // Debounce: an install unpacks several files back-to-back; one
    // rescan per burst is plenty (the registry scan re-reads every
    // manifest).
    let debounce = Duration::from_millis(400);
    tokio::task::spawn_blocking(move || {
        let mut last_event = Instant::now();
        let mut pending = false;
        loop {
            let recv = raw_rx.recv_timeout(if pending {
                debounce
            } else {
                Duration::from_millis(250)
            });
            match recv {
                Ok(()) => {
                    pending = true;
                    last_event = Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if pending && last_event.elapsed() >= debounce {
                        pending = false;
                        tracing::info!("widgets dir changed — requesting rescan");
                        on_change();
                    }
                }
                Err(_) => return,
            }
        }
    });
}
