//! Integration tests proving that PDK-built widgets load and round-trip
//! correctly in the wasmi host:
//!
//! - `pdk_widget_round_trips`: fixture/pdk-widget init+event+render.
//! - `weather_widget_loads`: examples/widgets/weather init issues HttpGet
//!   with the configured latitude in the URL.

mod common;

use std::path::PathBuf;
use oxidemx_widget_host::instance::{CallOutcome, WidgetInstance};
use oxidemx_widget_proto::{Event, HostCmd, WedgeGeom};
use oxidemx_widget_proto::settings::{Settings, SettingValue};

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

/// Load the PDK-built fixture, exercise init → event → render round trip,
/// and verify that commands issued via `Ctx` methods arrive at the host.
#[test]
fn pdk_widget_round_trips() {
    let Some(wasm) = common::fixture_wasm("pdk-widget") else { return };
    let settings: Settings = vec![];
    let mut inst = WidgetInstance::load(&wasm, &settings).expect("PDK widget should load");

    // init must have issued a Log + SetTimer (from PdkWidget::init).
    let cmds = inst.drain_cmds();
    assert!(
        cmds.iter().any(|c| matches!(c, HostCmd::Log { .. })),
        "init must log; cmds = {cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| matches!(c, HostCmd::SetTimer { .. })),
        "init must set a timer; cmds = {cmds:?}"
    );

    // Click should request a re-render.
    match inst.on_event(&Event::Click) {
        CallOutcome::NeedsRender(true) => {}
        other => panic!("Click should produce NeedsRender(true), got {other:?}"),
    }

    // Additional Log issued during on_event.
    let cmds = inst.drain_cmds();
    assert!(
        cmds.iter().any(|c| matches!(c, HostCmd::Log { .. })),
        "on_event(Click) must log; cmds = {cmds:?}"
    );

    // render should produce a valid Scene.
    match inst.render(&geom()) {
        CallOutcome::Scene(scene) => {
            // tile() with value, sublabel, label (no sparkline) = 3 prims.
            assert_eq!(
                scene.prims.len(), 3,
                "PDK tile scene should have 3 prims (value, sublabel, label)"
            );
        }
        other => panic!("render should return a Scene, got {other:?}"),
    }

    assert!(!inst.is_disabled());
    assert!(inst.last_error().is_none());
}

/// Load the weather example widget with a location setting and verify that
/// init drains an HttpGet command whose url contains the configured latitude.
#[test]
fn weather_widget_loads() {
    // The weather widget lives outside the fixtures dir; use build_wasm_at.
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest dir has parent")
        .to_path_buf();
    let weather_dir = workspace_root.join("examples").join("widgets").join("weather");
    let Some(wasm) = common::build_wasm_at("weather", weather_dir, "weather") else { return };

    // Settings with a location (Oslo: lat=59.91, lon=10.75).
    let lat = 59.91_f64;
    let settings: Settings = vec![
        ("location".into(), SettingValue::Location {
            name: "Oslo, Norway".into(),
            lat,
            lon: 10.75,
        }),
        ("units".into(), SettingValue::Str("c".into())),
        ("refresh".into(), SettingValue::Num(900.0)),
    ];

    let mut inst = WidgetInstance::load(&wasm, &settings)
        .expect("weather widget should load");

    // init must drain a SetTimer and an HttpGet.
    let cmds = inst.drain_cmds();
    let has_timer = cmds.iter().any(|c| matches!(c, HostCmd::SetTimer { .. }));
    let http_cmd = cmds.iter().find(|c| matches!(c, HostCmd::HttpGet { id, .. } if id == "wx"));

    assert!(has_timer, "init must set a refresh timer; cmds = {cmds:?}");
    assert!(http_cmd.is_some(), "init must issue HttpGet with id 'wx'; cmds = {cmds:?}");

    // The HttpGet URL must contain the latitude.
    if let Some(HostCmd::HttpGet { url, .. }) = http_cmd {
        let lat_str = format!("{lat}");
        assert!(
            url.contains(&lat_str) || url.contains("59.91"),
            "HttpGet URL should contain latitude {lat}; url = {url}"
        );
    }

    assert!(!inst.is_disabled());
}
