//! Integration tests for the oxidemx-widget CLI library.
//!
//! These call the lib functions directly (not the binary) and use `tempfile`
//! to isolate all filesystem state.  The XDG_CONFIG_HOME environment variable
//! is overridden per-test to point at a temp dir so `widgets_dir()` returns a
//! sandboxed path.

use std::path::Path;

use oxidemx_widget_cli::{install, pack, verify, CliError};
use oxidemx_widget_host::{SignatureState, WidgetRegistry};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Create a minimal widget fixture directory under `base`.
///
/// Writes `widget.json`, `icon.svg`, and a dummy `widget.wasm` (a few bytes —
/// not a real wasm module; the tests exercise CLI / registry logic only, not
/// the wasm runtime).
fn make_weather_fixture(base: &Path) {
    let widget_json = r#"{
  "id": "weather",
  "name": "Weather",
  "version": "1.0.0",
  "author": "test",
  "api_version": 1,
  "entry": "widget.wasm",
  "icon": "icon.svg"
}"#;
    std::fs::write(base.join("widget.json"), widget_json).unwrap();
    std::fs::write(base.join("icon.svg"), "<svg/>").unwrap();
    // Dummy wasm — just some bytes; tests don't actually load it into wasmi.
    std::fs::write(base.join("widget.wasm"), b"\x00asm\x01\x00\x00\x00").unwrap();
}

/// Set `OXIDEMX_DEV_KEY_PATH` to `<xdg>/dev-signing.key` so all key I/O stays
/// inside the test's temp tree.
fn set_dev_key_path(xdg: &Path) {
    let key_path = xdg.join("oxidemx").join("dev-signing.key");
    std::env::set_var("OXIDEMX_DEV_KEY_PATH", &key_path);
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Pack → verify round-trip using the dev key.
///
/// The produced bundle must verify as `SignatureState::Unknown` (dev key, not
/// the pinned registry key) and report a non-empty fingerprint.
#[test]
fn pack_verify_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let widget_dir = tmp.path().join("weather");
    std::fs::create_dir_all(&widget_dir).unwrap();
    make_weather_fixture(&widget_dir);

    let xdg = tmp.path().join("xdg");
    std::fs::create_dir_all(&xdg).unwrap();
    set_dev_key_path(&xdg);

    let out = tmp.path().join("weather.oxw");
    let pack_result = pack(&widget_dir, Some(&out)).expect("pack should succeed");

    assert!(out.exists(), "bundle file should be created");
    assert!(!pack_result.fingerprint.is_empty(), "fingerprint should be non-empty");

    let ver = verify(&out).expect("verify should succeed");
    match ver.state {
        SignatureState::Unknown(fp) => {
            assert_eq!(fp, pack_result.fingerprint, "fingerprints should match");
            assert_eq!(fp.len(), 16, "fingerprint is hex of 8 bytes = 16 chars");
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}

/// Tamper one byte in the zip content after packing — verify must fail.
#[test]
fn tampered_bundle_fails_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let widget_dir = tmp.path().join("weather");
    std::fs::create_dir_all(&widget_dir).unwrap();
    make_weather_fixture(&widget_dir);

    let xdg = tmp.path().join("xdg");
    std::fs::create_dir_all(&xdg).unwrap();
    set_dev_key_path(&xdg);

    let out = tmp.path().join("weather.oxw");
    pack(&widget_dir, Some(&out)).expect("pack should succeed");

    // Flip one byte near the start of the bundle (after the PK header).
    let mut bytes = std::fs::read(&out).unwrap();
    let flip_pos = bytes.len() / 2;
    bytes[flip_pos] ^= 0xFF;
    std::fs::write(&out, bytes).unwrap();

    let result = verify(&out);
    // Either the zip itself is malformed, or signature verification fails.
    assert!(
        result.is_err(),
        "verify of a tampered bundle should fail, got {:?}",
        result.map(|r| format!("{:?}", r.state))
    );
}

/// Install places files into a temp XDG widgets dir, and a subsequent
/// `WidgetRegistry::scan` reflects `SignatureState::Unknown`.
#[test]
fn install_then_scan_shows_unknown_signature() {
    let tmp = tempfile::tempdir().unwrap();
    let widget_dir = tmp.path().join("weather");
    std::fs::create_dir_all(&widget_dir).unwrap();
    make_weather_fixture(&widget_dir);

    let xdg = tmp.path().join("xdg");
    std::fs::create_dir_all(&xdg).unwrap();
    set_dev_key_path(&xdg);
    // Point XDG_CONFIG_HOME at the test dir so widgets_dir() returns a safe path.
    std::env::set_var("XDG_CONFIG_HOME", &xdg);

    let out = tmp.path().join("weather.oxw");
    pack(&widget_dir, Some(&out)).expect("pack should succeed");

    let install_root = xdg.join("oxidemx").join("widgets");
    std::fs::create_dir_all(&install_root).unwrap();

    let result = install(&out, false, Some(&install_root)).expect("install should succeed");
    assert_eq!(result.id, "weather");
    assert!(result.install_dir.exists(), "install dir should exist");
    assert!(result.install_dir.join("widget.json").exists());
    assert!(result.install_dir.join("widget.wasm").exists());
    assert!(result.install_dir.join("icon.svg").exists());

    // The install should have written a SIGNATURE file (from the zip).
    assert!(
        result.install_dir.join("SIGNATURE").exists(),
        "SIGNATURE file should be extracted from the bundle"
    );

    // Registry scan should find the widget as Ready with Unknown signature.
    let reg = WidgetRegistry::scan(&install_root);
    let w = reg.get("weather").expect("weather should be in registry");

    assert_eq!(
        w.state,
        oxidemx_widget_host::WidgetState::Ready,
        "widget should be Ready after install"
    );
    match &w.signature_state {
        SignatureState::Unknown(fp) => {
            assert!(!fp.is_empty(), "fingerprint should be non-empty");
            assert_eq!(fp.len(), 16, "fingerprint is hex of 8 bytes");
        }
        other => panic!(
            "expected Unknown(fingerprint) since dev key ≠ pinned registry key, got {other:?}"
        ),
    }
}

/// Id collision without --force is refused; with --force the widget is replaced.
#[test]
fn id_collision_refused_and_forced() {
    let tmp = tempfile::tempdir().unwrap();
    let widget_dir = tmp.path().join("weather");
    std::fs::create_dir_all(&widget_dir).unwrap();
    make_weather_fixture(&widget_dir);

    let xdg = tmp.path().join("xdg");
    std::fs::create_dir_all(&xdg).unwrap();
    set_dev_key_path(&xdg);
    std::env::set_var("XDG_CONFIG_HOME", &xdg);

    let out = tmp.path().join("weather.oxw");
    pack(&widget_dir, Some(&out)).expect("pack should succeed");

    let install_root = xdg.join("oxidemx").join("widgets");
    std::fs::create_dir_all(&install_root).unwrap();

    // First install — should succeed.
    install(&out, false, Some(&install_root)).expect("first install should succeed");

    // Second install without force — should fail with IdCollision.
    let err = install(&out, false, Some(&install_root))
        .expect_err("second install without --force should fail");
    match err {
        CliError::IdCollision(id) => assert_eq!(id, "weather"),
        other => panic!("expected IdCollision, got {other:?}"),
    }

    // Second install with force — should succeed.
    let result = install(&out, true, Some(&install_root))
        .expect("install with --force should succeed");
    assert_eq!(result.id, "weather");
}
