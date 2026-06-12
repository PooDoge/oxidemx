//! Worker integration tests (plan Task 5): reconcile, permission-gated
//! HTTP with caching/dedup, timer clamping, and settings resolution.
//!
//! All tests install the PDK fixture widget into a throwaway widgets dir
//! and drive the worker purely over its `HostCtl`/`HostEvent` channels.

mod common;

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use oxidemx_shared::config::AppConfig;
use oxidemx_widget_host::http::HttpFetcher;
use oxidemx_widget_host::stats::StatsSource;
use oxidemx_widget_host::worker::{self, HostCtl, HostEvent, InstanceId, SliceEvent};
use oxidemx_widget_proto::{Prim, Scene, SystemStatsSnapshot, WedgeGeom};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join(format!("oxidemx-widget-worker-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Install the built pdk fixture into `root/pdk-widget/` with the given
/// manifest fields.
fn install_pdk(root: &Path, wasm: &Path, refresh_ms: u64, permissions: &[&str]) {
    let dir = root.join("pdk-widget");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(wasm, dir.join("widget.wasm")).unwrap();
    std::fs::write(dir.join("icon.svg"), "<svg/>").unwrap();
    let manifest = serde_json::json!({
        "id": "pdk-widget",
        "name": "PDK Fixture",
        "version": "0.1.0",
        "author": "tests",
        "api_version": 1,
        "entry": "widget.wasm",
        "icon": "icon.svg",
        "permissions": permissions,
        "slice": { "refresh_ms": refresh_ms },
    });
    std::fs::write(dir.join("widget.json"), manifest.to_string()).unwrap();
}

/// AppConfig with one page ("Apps") of the given slices and the given
/// `widgets` store (two-bag JSON shape).
fn cfg(slices: serde_json::Value, widgets: serde_json::Value) -> Arc<AppConfig> {
    let cfg: AppConfig = serde_json::from_value(serde_json::json!({
        "radial_menu": { "pages": [ { "name": "Apps", "slices": slices } ] },
        "widgets": widgets,
    }))
    .expect("test config must parse");
    Arc::new(cfg)
}

fn custom_slice(instance_key: &str) -> serde_json::Value {
    serde_json::json!({
        "label": "W",
        "type": "widget",
        "color": "teal",
        "widget": {
            "source": { "custom": "pdk-widget" },
            "instance_key": instance_key,
        },
    })
}

fn geom() -> WedgeGeom {
    WedgeGeom {
        width: 200.0,
        height: 160.0,
        inner_radius: 60.0,
        outer_radius: 160.0,
        angle_start: 0.0,
        angle_end: 0.785,
        hovered: 0.0,
    }
}

fn iid(key: &str) -> InstanceId {
    InstanceId { instance_key: key.into(), widget_id: "pdk-widget".into() }
}

/// First Text prim's content (tile() puts the big value first).
fn scene_value(scene: &Scene) -> Option<&str> {
    scene.prims.iter().find_map(|p| match p {
        Prim::Text { content, .. } => Some(content.as_str()),
        _ => None,
    })
}

fn scene_texts(scene: &Scene) -> Vec<&str> {
    scene.prims.iter()
        .filter_map(|p| match p {
            Prim::Text { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect()
}

/// Wait (wall clock) for the next Scene event, skipping other HostEvents.
async fn next_scene(
    rx: &async_channel::Receiver<HostEvent>,
    timeout: Duration,
) -> Option<(InstanceId, Scene, u64)> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(HostEvent::Scene { instance, scene, revision })) => {
                return Some((instance, scene, revision));
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => return None,
        }
    }
}

/// Wait for a Scene whose big value matches `pred`, for `instance`.
async fn scene_matching(
    rx: &async_channel::Receiver<HostEvent>,
    instance: &InstanceId,
    timeout: Duration,
    pred: impl Fn(&Scene) -> bool,
) -> Option<Scene> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let (id, scene, _) = next_scene(rx, remaining).await?;
        if &id == instance && pred(&scene) {
            return Some(scene);
        }
    }
}

// ---------------------------------------------------------------------------
// fetcher doubles
// ---------------------------------------------------------------------------

/// Panics if the worker ever reaches the network.
struct NoFetch;

impl HttpFetcher for NoFetch {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = (u16, Vec<u8>)> + Send + 'a>> {
        panic!("test must not hit the network (url: {url})");
    }
}

/// Counts upstream hits; responds 200 "hello" after a small real delay so
/// concurrent requests are observably in-flight.
struct CountingFetcher {
    hits: Arc<AtomicUsize>,
}

impl HttpFetcher for CountingFetcher {
    fn fetch<'a>(
        &'a self,
        _url: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = (u16, Vec<u8>)> + Send + 'a>> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            (200, b"hello".to_vec())
        })
    }
}

// ---------------------------------------------------------------------------
// stats doubles
// ---------------------------------------------------------------------------

/// Deterministic stats source: cpu_pct counts up from `next` on every
/// sample, so each push is distinguishable in the rendered scene.
struct FakeStats {
    next: f32,
}

impl StatsSource for FakeStats {
    fn sample(&mut self) -> SystemStatsSnapshot {
        let mut snap = SystemStatsSnapshot::default();
        snap.cpu_pct = Some(self.next);
        self.next += 1.0;
        snap
    }
}

fn spawn_worker(
    root: PathBuf,
    fetcher: Box<dyn HttpFetcher>,
) -> (
    async_channel::Sender<HostCtl>,
    async_channel::Receiver<HostEvent>,
    tokio::task::JoinHandle<()>,
) {
    spawn_worker_with_stats(root, fetcher, Box::new(FakeStats { next: 0.0 }))
}

fn spawn_worker_with_stats(
    root: PathBuf,
    fetcher: Box<dyn HttpFetcher>,
    stats: Box<dyn StatsSource>,
) -> (
    async_channel::Sender<HostCtl>,
    async_channel::Receiver<HostEvent>,
    tokio::task::JoinHandle<()>,
) {
    let (ctl_tx, ctl_rx) = async_channel::unbounded();
    let (ev_tx, ev_rx) = async_channel::unbounded();
    let handle = worker::spawn_with(ctl_rx, ev_tx, root, fetcher, stats);
    (ctl_tx, ev_rx, handle)
}

/// Paused-clock helper: yield-pump the runtime and collect the big values
/// of every Scene currently in the channel (never awaits → never lets the
/// paused clock auto-advance).
async fn drain_scene_values(events: &async_channel::Receiver<HostEvent>) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..10_000 {
        tokio::task::yield_now().await;
        match events.try_recv() {
            Ok(HostEvent::Scene { scene, .. }) => {
                if let Some(v) = scene_value(&scene) {
                    out.push(v.to_string());
                }
            }
            Ok(_) => continue,
            Err(async_channel::TryRecvError::Empty) => continue,
            Err(async_channel::TryRecvError::Closed) => panic!("worker died"),
        }
    }
    out
}

/// Paused-clock helper: yield-pump until a Scene with big value `want`
/// arrives (panics after the pump budget runs out).
async fn wait_scene_value(events: &async_channel::Receiver<HostEvent>, want: &str) {
    for _ in 0..100_000 {
        tokio::task::yield_now().await;
        match events.try_recv() {
            Ok(HostEvent::Scene { scene, .. }) => {
                if scene_value(&scene) == Some(want) {
                    return;
                }
            }
            Ok(_) => continue,
            Err(async_channel::TryRecvError::Empty) => continue,
            Err(async_channel::TryRecvError::Closed) => panic!("worker died"),
        }
    }
    panic!("no scene with value {want:?} arrived");
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

/// ConfigChanged with one Custom slice spawns an instance (one Scene
/// arrives); removing the slice drops it (a follow-up Click produces
/// no Scene).
#[tokio::test(flavor = "multi_thread")]
async fn reconcile_spawns_and_drops_instances() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("reconcile");
    install_pdk(&root, &wasm, 900_000, &[]);
    let (ctl, events, _h) = spawn_worker(root, Box::new(NoFetch));

    ctl.send(HostCtl::ConfigChanged(cfg(
        serde_json::json!([custom_slice("apps.slot0")]),
        serde_json::json!({}),
    )))
    .await
    .unwrap();

    let (id, scene, revision) = next_scene(&events, Duration::from_secs(20))
        .await
        .expect("placing a custom widget must produce a Scene");
    assert_eq!(id, iid("apps.slot0"));
    assert!(revision >= 1);
    assert_eq!(scene_value(&scene), Some("0"), "initial render shows the count");

    // Drop the slice; the instance must go away.
    ctl.send(HostCtl::ConfigChanged(cfg(
        serde_json::json!([]),
        serde_json::json!({}),
    )))
    .await
    .unwrap();

    // Probe: a Click for the gone instance must not produce a Scene.
    ctl.send(HostCtl::Slice { instance: iid("apps.slot0"), ev: SliceEvent::Click, geom: geom() })
        .await
        .unwrap();
    assert!(
        next_scene(&events, Duration::from_millis(400)).await.is_none(),
        "dropped instance must not render"
    );
}

/// HttpGet to a host missing from the manifest's net: allowlist gets an
/// immediate status-0 response with the reason as the body — observable
/// because the fixture re-renders it into its scene.
#[tokio::test(flavor = "multi_thread")]
async fn http_permission_denied() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("denied");
    // Allowlist a DIFFERENT host than the one the widget asks for.
    install_pdk(&root, &wasm, 900_000, &["net:api.example.com"]);
    let (ctl, events, _h) = spawn_worker(root, Box::new(NoFetch));

    ctl.send(HostCtl::ConfigChanged(cfg(
        serde_json::json!([custom_slice("apps.slot0")]),
        serde_json::json!({
            "global": { "pdk-widget": { "fetch_url": "https://evil.example.org/x" } }
        }),
    )))
    .await
    .unwrap();

    let scene = scene_matching(&events, &iid("apps.slot0"), Duration::from_secs(20), |s| {
        scene_value(s) == Some("h0")
    })
    .await
    .expect("denied fetch must surface as a status-0 HttpResponse scene");
    let texts = scene_texts(&scene).join(" ");
    assert!(
        texts.contains("evil.example.org") || texts.to_lowercase().contains("denied"),
        "denial reason should reach the widget body: {texts:?}"
    );
}

/// Two instances of the same widget fetching the same URL produce exactly
/// one upstream hit (in-flight dedup + cache), and both get the response.
#[tokio::test(flavor = "multi_thread")]
async fn http_cache_dedupes() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("dedupe");
    install_pdk(&root, &wasm, 900_000, &["net:api.example.com"]);
    let hits = Arc::new(AtomicUsize::new(0));
    let (ctl, events, _h) =
        spawn_worker(root, Box::new(CountingFetcher { hits: hits.clone() }));

    ctl.send(HostCtl::ConfigChanged(cfg(
        serde_json::json!([custom_slice("apps.slot0"), custom_slice("apps.slot1")]),
        serde_json::json!({
            "global": { "pdk-widget": { "fetch_url": "https://api.example.com/data" } }
        }),
    )))
    .await
    .unwrap();

    // One pass over the event stream: both instances must show the
    // shared response (order is not deterministic).
    let mut pending: Vec<InstanceId> = vec![iid("apps.slot0"), iid("apps.slot1")];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !pending.is_empty() {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or_else(|| panic!("instances never got the response: {pending:?}"));
        let (id, scene, _) = next_scene(&events, remaining)
            .await
            .unwrap_or_else(|| panic!("instances never got the response: {pending:?}"));
        if scene_value(&scene) == Some("h200") {
            assert!(scene_texts(&scene).iter().any(|t| t.contains("hello")));
            pending.retain(|p| p != &id);
        }
    }
    assert_eq!(hits.load(Ordering::SeqCst), 1, "one upstream hit for two instances");
}

/// A widget asking for a 1s timer with a 5s manifest refresh floor must
/// not fire within 2s — and must fire once the clamped period elapses.
#[tokio::test(start_paused = true)]
async fn timer_clamps_to_refresh_floor() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("clamp");
    install_pdk(&root, &wasm, 5_000, &[]); // floor: 5s
    let (ctl, events, _h) = spawn_worker(root, Box::new(NoFetch));

    ctl.send(HostCtl::ConfigChanged(cfg(
        serde_json::json!([custom_slice("apps.slot0")]),
        serde_json::json!({
            "global": { "pdk-widget": { "timer_secs": 1 } }
        }),
    )))
    .await
    .unwrap();

    // Pump (paused clock: yield, never sleep) until the initial Scene lands.
    let mut initial = None;
    for _ in 0..100_000 {
        tokio::task::yield_now().await;
        match events.try_recv() {
            Ok(HostEvent::Scene { scene, .. }) => {
                initial = Some(scene);
                break;
            }
            Ok(_) => continue,
            Err(async_channel::TryRecvError::Empty) => continue,
            Err(async_channel::TryRecvError::Closed) => panic!("worker died"),
        }
    }
    initial.expect("initial scene");

    // The widget asked for 1s; the manifest floor is 5s. Nothing within 2s.
    tokio::time::advance(Duration::from_secs(2)).await;
    let mut got_scene = false;
    for _ in 0..1_000 {
        tokio::task::yield_now().await;
        if let Ok(HostEvent::Scene { .. }) = events.try_recv() {
            got_scene = true;
            break;
        }
    }
    assert!(!got_scene, "1s timer must be clamped to the 5s floor");

    // ...but the clamped timer does fire after 5s total.
    tokio::time::advance(Duration::from_secs(4)).await;
    let mut fired = false;
    for _ in 0..100_000 {
        tokio::task::yield_now().await;
        if let Ok(HostEvent::Scene { .. }) = events.try_recv() {
            fired = true;
            break;
        }
    }
    assert!(fired, "clamped timer must fire at the 5s floor");
}

/// Settings resolution layers defaults <- global <- instance bag; the
/// instance override is what reaches the widget.
///
/// Also asserts the v1 reload-on-settings-change behaviour: when settings
/// differ on reconcile the instance is dropped and re-initialised, so the
/// widget's init-time commands (Log + SetTimer) are observable again and
/// the scene value reflects the new `echo` setting.
#[tokio::test(flavor = "multi_thread")]
async fn settings_resolution_reaches_widget() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("settings");
    install_pdk(&root, &wasm, 900_000, &[]);
    let (ctl, events, _h) = spawn_worker(root, Box::new(NoFetch));

    // --- initial placement ---
    ctl.send(HostCtl::ConfigChanged(cfg(
        serde_json::json!([custom_slice("apps.slot0")]),
        serde_json::json!({
            "global": { "pdk-widget": { "echo": "global-val" } },
            "instances": { "apps.slot0": { "pdk-widget": { "echo": "inst-val" } } }
        }),
    )))
    .await
    .unwrap();

    let (id, scene, _) = next_scene(&events, Duration::from_secs(20))
        .await
        .expect("scene after placement");
    assert_eq!(id, iid("apps.slot0"));
    assert_eq!(
        scene_value(&scene),
        Some("inst-val"),
        "instance bag must override the global bag (scope: instance)"
    );

    // --- settings change → instance reload (v1 semantics) ---
    // Send a new config with a changed echo value.  The worker must drop
    // and re-init the instance; the new scene value must reflect the new
    // setting (proving that init ran again with the new bag).
    ctl.send(HostCtl::ConfigChanged(cfg(
        serde_json::json!([custom_slice("apps.slot0")]),
        serde_json::json!({
            "instances": { "apps.slot0": { "pdk-widget": { "echo": "new-val" } } }
        }),
    )))
    .await
    .unwrap();

    let scene = scene_matching(&events, &iid("apps.slot0"), Duration::from_secs(20), |s| {
        scene_value(s) == Some("new-val")
    })
    .await
    .expect("settings change must reload the instance and produce a scene with the new echo");
    assert_eq!(
        scene_value(&scene),
        Some("new-val"),
        "reloaded instance must render the new setting value"
    );
}

// ---------------------------------------------------------------------------
// system-stats push feed (plan 4 Task 2)
// ---------------------------------------------------------------------------

/// Config that puts the fixture into its stats mode (renders cpu_pct as
/// the big value).
fn stats_cfg() -> Arc<AppConfig> {
    cfg(
        serde_json::json!([custom_slice("apps.slot0")]),
        serde_json::json!({ "global": { "pdk-widget": { "mode": "stats" } } }),
    )
}

/// With the `system-stats` permission and the menu open, the instance
/// gets one push immediately on MenuOpened and another on each 1s tick.
#[tokio::test(start_paused = true)]
async fn stats_pushed_while_menu_open() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("stats-open");
    install_pdk(&root, &wasm, 900_000, &["system-stats"]);
    let (ctl, events, _h) = spawn_worker_with_stats(
        root,
        Box::new(NoFetch),
        Box::new(FakeStats { next: 42.0 }),
    );

    ctl.send(HostCtl::ConfigChanged(stats_cfg())).await.unwrap();
    // Initial render: no stats arrived yet → the fixture's stub value.
    wait_scene_value(&events, "--").await;

    // MenuOpened → an immediate push (first sample: 42).
    ctl.send(HostCtl::MenuOpened { page: "Apps".into() }).await.unwrap();
    wait_scene_value(&events, "42").await;

    // Each 1s tick while open pushes a fresh sample.
    tokio::time::advance(Duration::from_secs(1)).await;
    wait_scene_value(&events, "43").await;
    tokio::time::advance(Duration::from_secs(1)).await;
    wait_scene_value(&events, "44").await;
}

/// No pushes while the menu is closed: the 1s feed must stay idle.
#[tokio::test(start_paused = true)]
async fn no_stats_while_closed() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("stats-closed");
    install_pdk(&root, &wasm, 900_000, &["system-stats"]);
    let (ctl, events, _h) = spawn_worker_with_stats(
        root,
        Box::new(NoFetch),
        Box::new(FakeStats { next: 42.0 }),
    );

    ctl.send(HostCtl::ConfigChanged(stats_cfg())).await.unwrap();
    wait_scene_value(&events, "--").await;

    // Menu never opens; several seconds pass; nothing may render.
    for _ in 0..5 {
        tokio::time::advance(Duration::from_secs(1)).await;
        let values = drain_scene_values(&events).await;
        assert!(values.is_empty(), "no stats push while the menu is closed: {values:?}");
    }

    // ...and after an open/close cycle the feed stops again.
    ctl.send(HostCtl::MenuOpened { page: "Apps".into() }).await.unwrap();
    wait_scene_value(&events, "42").await;
    ctl.send(HostCtl::MenuClosed).await.unwrap();
    let _ = drain_scene_values(&events).await; // flush the MenuClosed pump
    for _ in 0..5 {
        tokio::time::advance(Duration::from_secs(1)).await;
        let values = drain_scene_values(&events).await;
        assert!(values.is_empty(), "no stats push after MenuClosed: {values:?}");
    }
}

/// Without the `system-stats` permission the instance never receives the
/// event — neither on MenuOpened nor from the tick.
#[tokio::test(start_paused = true)]
async fn no_stats_without_permission() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let root = tmp("stats-noperm");
    install_pdk(&root, &wasm, 900_000, &[]); // no system-stats
    let (ctl, events, _h) = spawn_worker_with_stats(
        root,
        Box::new(NoFetch),
        Box::new(FakeStats { next: 42.0 }),
    );

    ctl.send(HostCtl::ConfigChanged(stats_cfg())).await.unwrap();
    wait_scene_value(&events, "--").await;

    ctl.send(HostCtl::MenuOpened { page: "Apps".into() }).await.unwrap();
    for _ in 0..3 {
        tokio::time::advance(Duration::from_secs(1)).await;
        let values = drain_scene_values(&events).await;
        assert!(
            values.is_empty(),
            "instance without system-stats must never see the event: {values:?}"
        );
    }
}
