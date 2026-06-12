//! Integration test proving that a PDK-built widget (`fixtures/pdk-widget`)
//! loads and round-trips correctly in the wasmi host.

mod common;

use oxidemx_widget_host::instance::{CallOutcome, WidgetInstance};
use oxidemx_widget_proto::{Event, HostCmd, WedgeGeom};
use oxidemx_widget_proto::settings::Settings;

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
