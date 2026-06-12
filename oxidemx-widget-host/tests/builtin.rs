//! Bundled built-in widgets load end-to-end (plan 4 Task 3): build the
//! cpu guest from `widgets/builtin/cpu`, install it into a throwaway
//! widgets dir with its real manifest (`system-stats` permission), and
//! drive the worker: MenuOpened must trigger a stats push whose render
//! contains the native "%"-formatted CPU value.

// `fixture_wasm` is unused in this binary — only `build_wasm_at` is.
#[allow(dead_code)]
mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use oxidemx_shared::config::AppConfig;
use oxidemx_widget_host::stats::StatsSource;
use oxidemx_widget_host::worker::{self, HostCtl, HostEvent};
use oxidemx_widget_proto::{Prim, Scene, SystemStatsSnapshot};

/// Deterministic stats double: always reports cpu 23%, 8 cores, 52°C.
struct FixedStats;

impl StatsSource for FixedStats {
    fn sample(&mut self) -> SystemStatsSnapshot {
        let mut snap = SystemStatsSnapshot::default();
        snap.cpu_pct = Some(23.0);
        snap.cpu_cores = Some(8);
        snap.cpu_temp_c = Some(52.0);
        snap
    }
}

/// Panics if the worker ever reaches the network.
struct NoFetch;

impl oxidemx_widget_host::http::HttpFetcher for NoFetch {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = (u16, Vec<u8>)> + Send + 'a>> {
        panic!("builtin widgets must not hit the network (url: {url})");
    }
}

fn scene_texts(scene: &Scene) -> Vec<&str> {
    scene
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn builtin_widgets_load() {
    // Build the real cpu guest (widgets/builtin/cpu).
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest dir has parent")
        .to_path_buf();
    let cpu_dir = workspace_root.join("widgets").join("builtin").join("cpu");
    let Some(wasm) = common::build_wasm_at("builtin-cpu", cpu_dir.clone(), "cpu") else { return };

    // Install it with its REAL manifest (carries the system-stats
    // permission) into a throwaway widgets dir.
    let root = std::env::temp_dir()
        .join(format!("oxidemx-widget-builtin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("cpu");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&wasm, dir.join("widget.wasm")).unwrap();
    std::fs::copy(cpu_dir.join("icon.svg"), dir.join("icon.svg")).unwrap();
    std::fs::copy(cpu_dir.join("widget.json"), dir.join("widget.json")).unwrap();

    let (ctl, ctl_rx) = async_channel::unbounded();
    let (ev_tx, events) = async_channel::unbounded();
    let _handle = worker::spawn_with(
        ctl_rx,
        ev_tx,
        root.clone(),
        Box::new(NoFetch),
        Box::new(FixedStats),
    );

    // Place the widget.
    let cfg: AppConfig = serde_json::from_value(serde_json::json!({
        "radial_menu": { "pages": [ { "name": "Apps", "slices": [ {
            "label": "CPU",
            "type": "widget",
            "color": "teal",
            "widget": { "source": { "custom": "cpu" }, "instance_key": "apps.slot0" },
        } ] } ] },
        "widgets": {},
    }))
    .unwrap();
    ctl.send(HostCtl::ConfigChanged(Arc::new(cfg))).await.unwrap();

    // MenuOpened pushes stats immediately (spec §16) → a Scene whose
    // big value carries the native "%" formatting must arrive.
    ctl.send(HostCtl::MenuOpened { page: "Apps".into() }).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .expect("timed out waiting for the stats-driven scene");
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(HostEvent::Scene { scene, .. })) => {
                let texts = scene_texts(&scene);
                // Pre-stats renders show the "—" placeholder; wait for
                // the push-fed one.
                if texts.first().is_some_and(|t| t.contains('%')) {
                    assert_eq!(
                        texts,
                        vec!["23%", "8 cores · 52°C", "CPU"],
                        "scene must reproduce the native CPU typography"
                    );
                    break;
                }
            }
            Ok(Ok(HostEvent::InstanceFailed { instance, error })) => {
                panic!("instance {}/{} failed: {error}", instance.widget_id, instance.instance_key);
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) => panic!("worker event channel closed"),
            Err(_) => panic!("timed out waiting for the stats-driven scene"),
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}
