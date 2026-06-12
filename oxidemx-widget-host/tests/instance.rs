//! Integration tests for the wasmi instance lifecycle: load/init/event/
//! render against the raw-ABI fixture widget, plus the strike rules
//! (trap, fuel exhaustion, oversized scene) and the api_version gate.

mod common;

use oxidemx_widget_host::instance::{CallOutcome, WidgetInstance, MAX_STRIKES};
use oxidemx_widget_proto::settings::Settings;
use oxidemx_widget_proto::{Event, HostCmd, SettingValue, WedgeGeom};

fn settings(mode: &str) -> Settings {
    vec![("mode".to_string(), SettingValue::Str(mode.to_string()))]
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

#[test]
fn lifecycle_ok() {
    let Some(wasm) = common::fixture_wasm("strike-widget") else { return };
    let mut inst = WidgetInstance::load(&wasm, &settings("ok")).expect("load ok fixture");

    // init must have issued exactly one Log cmd through the omx_cmd import.
    let cmds = inst.drain_cmds();
    assert_eq!(cmds.len(), 1, "init should issue exactly one cmd, got {cmds:?}");
    assert!(matches!(cmds[0], HostCmd::Log { .. }), "expected Log, got {:?}", cmds[0]);
    assert!(inst.drain_cmds().is_empty(), "drain must consume the queue");

    match inst.on_event(&Event::Click) {
        CallOutcome::NeedsRender(true) => {}
        other => panic!("expected NeedsRender(true), got {other:?}"),
    }
    match inst.render(&geom()) {
        CallOutcome::Scene(scene) => assert_eq!(scene.prims.len(), 2),
        other => panic!("expected Scene, got {other:?}"),
    }
    assert!(!inst.is_disabled());
    assert!(inst.last_error().is_none());
}

/// Shared 3-strike shape for event-call failures: two non-disabling
/// strikes, Disabled exactly on the third, Skipped ever after.
fn assert_event_strikes_disable(mut inst: WidgetInstance) {
    for n in 1..MAX_STRIKES {
        match inst.on_event(&Event::Click) {
            CallOutcome::NeedsRender(false) => {}
            other => panic!("strike {n} should be NeedsRender(false), got {other:?}"),
        }
        assert!(!inst.is_disabled(), "must not disable before strike {MAX_STRIKES}");
        assert!(inst.last_error().is_some(), "strike must record an error");
    }
    match inst.on_event(&Event::Click) {
        CallOutcome::Disabled => {}
        other => panic!("strike {MAX_STRIKES} should report Disabled, got {other:?}"),
    }
    assert!(inst.is_disabled());
    match inst.on_event(&Event::Click) {
        CallOutcome::Skipped => {}
        other => panic!("calls after disable should be Skipped, got {other:?}"),
    }
}

#[test]
fn fuel_exhaustion_strikes_and_disables() {
    let Some(wasm) = common::fixture_wasm("strike-widget") else { return };
    let inst = WidgetInstance::load(&wasm, &settings("spin")).expect("load spin fixture");
    assert_event_strikes_disable(inst);
}

#[test]
fn trap_strikes() {
    let Some(wasm) = common::fixture_wasm("strike-widget") else { return };
    let inst = WidgetInstance::load(&wasm, &settings("trap")).expect("load trap fixture");
    assert_event_strikes_disable(inst);
}

#[test]
fn oversized_scene_strikes() {
    let Some(wasm) = common::fixture_wasm("strike-widget") else { return };
    let mut inst =
        WidgetInstance::load(&wasm, &settings("huge_scene")).expect("load huge_scene fixture");
    for n in 1..MAX_STRIKES {
        match inst.render(&geom()) {
            CallOutcome::NeedsRender(false) => {}
            other => panic!("render strike {n} should be NeedsRender(false), got {other:?}"),
        }
        assert!(!inst.is_disabled());
    }
    match inst.render(&geom()) {
        CallOutcome::Disabled => {}
        other => panic!("render strike {MAX_STRIKES} should report Disabled, got {other:?}"),
    }
    assert!(inst.is_disabled());
    assert!(matches!(inst.render(&geom()), CallOutcome::Skipped));
}

#[test]
fn api_version_mismatch_fails_load() {
    let Some(wasm) = common::fixture_wasm("wrong-version-widget") else { return };
    let err = match WidgetInstance::load(&wasm, &settings("ok")) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("loading an api_version 99 widget must fail"),
    };
    assert!(err.contains("99"), "error should mention the bad version: {err}");
}

#[test]
fn success_does_not_reset_strikes() {
    // Strike twice via oversized scenes... not possible to mix modes in one
    // instance, so prove it differently: a successful event between two
    // failed renders must not buy back a strike. huge_scene mode events are
    // OK (return 1) while renders always fail.
    let Some(wasm) = common::fixture_wasm("strike-widget") else { return };
    let mut inst =
        WidgetInstance::load(&wasm, &settings("huge_scene")).expect("load huge_scene fixture");
    assert!(matches!(inst.render(&geom()), CallOutcome::NeedsRender(false))); // strike 1
    assert!(matches!(inst.on_event(&Event::Click), CallOutcome::NeedsRender(true))); // success
    assert!(matches!(inst.render(&geom()), CallOutcome::NeedsRender(false))); // strike 2
    assert!(matches!(inst.render(&geom()), CallOutcome::Disabled)); // strike 3
    assert!(inst.is_disabled());
}
