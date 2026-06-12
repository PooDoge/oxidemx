//! Bridge between the iced app and the widget-host worker (Plan 2
//! Task 6). The worker (`oxidemx_widget_host::worker`) owns every
//! wasmi instance on its own tokio task; the overlay talks to it
//! exclusively over channels:
//!
//!   * UI → worker: [`HostCtl`] via the global `CTL_TX` sender —
//!     always `try_send` (the channel is unbounded; "never block"
//!     is the contract on the input paths).
//!   * worker → UI: [`HostEvent`] via the [`stream`] subscription,
//!     surfaced as `Message::WidgetHost` (same pattern as
//!     `ai_stream_stream`).
//!
//! The frame path never calls wasm: `update()` files each
//! `HostEvent::Scene` into `RadialState::widget_scenes` and the
//! painter replays the decoded prims (`render::slices::widgets::
//! draw_custom_widget`).

use std::sync::Mutex;

use oxidemx_widget_host::{HostCtl, HostEvent};
use oxidemx_widget_proto::WedgeGeom;

/// Global control sender, registered once when [`stream`] starts.
/// A `Mutex<Option<…>>` mirrors `ai_client::QUESTION_TX` — set from
/// the subscription runtime, read from the update/dispatch paths.
static CTL_TX: Mutex<Option<async_channel::Sender<HostCtl>>> = Mutex::new(None);

/// Fire-and-forget a control message at the worker. Never blocks:
/// the channel is unbounded so `try_send` only fails when the
/// worker is gone (startup race or crashed task) — both are logged
/// and dropped rather than propagated; widget input must never
/// stall the UI thread.
pub fn send(ctl: HostCtl) {
    let guard = CTL_TX.lock().unwrap();
    match guard.as_ref() {
        Some(tx) => {
            if let Err(e) = tx.try_send(ctl) {
                tracing::warn!("widget host control channel: {e}");
            }
        }
        None => tracing::debug!("widget host not started yet; control message dropped"),
    }
}

/// Wedge geometry for slot `index` of an `slot_count`-slice ring at
/// rest scale, mirroring the painter's layout math (`draw_slice`):
/// half-sweep shift centres slot 0 on 12 o'clock, ring insets of
/// 6 px on both radii (painter.rs RING_INNER/OUTER_INSET). Sent
/// with every `HostCtl::Slice` so guest renders use what the
/// painter actually lays out. `hovered` is the slot's hover-tween
/// progress in 0..=1.
pub fn wedge_geom_for_slot(index: usize, slot_count: usize, hovered: f32) -> WedgeGeom {
    let n = slot_count.clamp(2, 8) as f32;
    let sweep = std::f32::consts::TAU / n;
    let start = (index as f32) * sweep - sweep / 2.0 - std::f32::consts::FRAC_PI_2;
    let inner = crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0;
    let outer = crate::geometry::MENU_RADIUS as f32 - 6.0;
    WedgeGeom {
        // Chord width at the outer rim + radial extent — advisory
        // numbers for the guest's layout, same convention as the
        // worker's DEFAULT_GEOM.
        width: 2.0 * outer * (sweep / 2.0).sin(),
        height: outer - inner,
        inner_radius: inner,
        outer_radius: outer,
        angle_start: start,
        angle_end: start + sweep,
        hovered: hovered.clamp(0.0, 1.0),
    }
}

/// Subscription stream: spawns the worker once (the `Subscription::
/// run(fn)` identity keeps it alive for the app's lifetime), seeds
/// it with the on-disk config, wires the widgets-dir watcher to
/// `RescanWidgets`, and yields every `HostEvent` as a
/// `Message::WidgetHost`. Runs inside the iced/tokio runtime — the
/// worker's `tokio::spawn` calls are valid here, same as the AI
/// client streams.
pub fn stream() -> impl futures_util::stream::Stream<Item = crate::app::Message> {
    use futures_util::StreamExt;

    let (ctl_tx, ctl_rx) = async_channel::unbounded();
    let (ev_tx, ev_rx) = async_channel::unbounded();
    *CTL_TX.lock().unwrap() = Some(ctl_tx.clone());
    oxidemx_widget_host::spawn(ctl_rx, ev_tx);

    // Seed the worker with the current config so placed Custom
    // widgets come up without waiting for the first config edit
    // (the inotify watcher only fires on changes).
    match crate::config::load() {
        Ok(cfg) => {
            let _ = ctl_tx.try_send(HostCtl::ConfigChanged(std::sync::Arc::new(cfg)));
        }
        Err(e) => tracing::warn!("widget host: initial config load failed: {e}"),
    }

    // Widgets install dir → RescanWidgets (debounced in config.rs).
    let rescan_tx = ctl_tx.clone();
    crate::config::spawn_widgets_dir_watcher(move || {
        let _ = rescan_tx.try_send(HostCtl::RescanWidgets);
    });

    ev_rx.map(crate::app::Message::WidgetHost)
}

/// `--widget-smoke` (debug builds only): boot the worker + the real
/// config/widgets dirs WITHOUT iced, wait for the first validated
/// scene, print its revision, and exit. Exercises registry scan →
/// reconcile → wasm load/init/render → postcard decode end-to-end;
/// `scripts/widget-smoke.sh` drives it under a temp
/// XDG_CONFIG_HOME with the weather example installed.
#[cfg(debug_assertions)]
pub fn run_smoke() -> i32 {
    use std::time::Duration;

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("widget-smoke: tokio runtime failed: {e}");
            return 1;
        }
    };
    rt.block_on(async {
        let (ctl_tx, ctl_rx) = async_channel::unbounded();
        let (ev_tx, ev_rx) = async_channel::unbounded();
        oxidemx_widget_host::spawn(ctl_rx, ev_tx);

        let cfg = match crate::config::load() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("widget-smoke: config load failed: {e}");
                return 1;
            }
        };
        if ctl_tx
            .send(HostCtl::ConfigChanged(std::sync::Arc::new(cfg)))
            .await
            .is_err()
        {
            eprintln!("widget-smoke: worker dropped the control channel");
            return 1;
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            match tokio::time::timeout_at(deadline, ev_rx.recv()).await {
                Ok(Ok(HostEvent::RegistryChanged(list))) => {
                    println!("widget-smoke: registry has {} widget(s)", list.len());
                    for w in &list {
                        println!("widget-smoke:   {} v{} [{}]", w.id, w.version, w.state);
                    }
                }
                Ok(Ok(HostEvent::Scene {
                    instance,
                    scene,
                    revision,
                })) => {
                    println!(
                        "widget-smoke: scene instance={}/{} revision={} prims={}",
                        instance.widget_id,
                        instance.instance_key,
                        revision,
                        scene.prim_count()
                    );
                    return 0;
                }
                Ok(Ok(HostEvent::InstanceFailed { instance, error })) => {
                    eprintln!(
                        "widget-smoke: instance {}/{} failed: {error}",
                        instance.widget_id, instance.instance_key
                    );
                    return 1;
                }
                Ok(Err(_)) => {
                    eprintln!("widget-smoke: worker event channel closed");
                    return 1;
                }
                Err(_) => {
                    eprintln!("widget-smoke: timed out waiting for the first scene");
                    return 1;
                }
            }
        }
    })
}
