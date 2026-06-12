//! Seeding tests (plan 4 Task 4): bundled built-ins install fresh,
//! upgrade strictly-older copies, skip equal/newer ones, and never touch
//! widget ids that have no seed bundle.

use std::path::{Path, PathBuf};

use oxidemx_widget_cli::seed::{seed_from_dirs, SeedAction};
use oxidemx_widget_cli::pack;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Write a minimal widget source dir (manifest + dummy wasm + icon).
fn make_widget_src(base: &Path, id: &str, version: &str) {
    std::fs::create_dir_all(base).unwrap();
    let manifest = serde_json::json!({
        "id": id,
        "name": id,
        "version": version,
        "author": "test",
        "api_version": 1,
        "entry": "widget.wasm",
        "icon": "icon.svg",
        "permissions": ["system-stats"],
    });
    std::fs::write(base.join("widget.json"), manifest.to_string()).unwrap();
    std::fs::write(base.join("icon.svg"), "<svg/>").unwrap();
    std::fs::write(base.join("widget.wasm"), b"\x00asm\x01\x00\x00\x00").unwrap();
}

/// Pack `<id>` at `version` into `seed_dir/<id>-<version>.omxw`.
fn make_bundle(tmp: &Path, seed_dir: &Path, id: &str, version: &str) -> PathBuf {
    // Keep dev-key I/O inside the temp tree (same pattern as cli.rs).
    std::env::set_var("OXIDEMX_DEV_KEY_PATH", tmp.join("dev-signing.key"));
    let src = tmp.join(format!("src-{id}-{version}"));
    make_widget_src(&src, id, version);
    std::fs::create_dir_all(seed_dir).unwrap();
    let out = seed_dir.join(format!("{id}-{version}.omxw"));
    pack(&src, Some(&out)).expect("pack should succeed");
    out
}

fn installed_version(root: &Path, id: &str) -> Option<String> {
    let raw = std::fs::read_to_string(root.join(id).join("widget.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v["version"].as_str().map(str::to_string)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn seed_installs_new() {
    let tmp = tempfile::tempdir().unwrap();
    let seed_dir = tmp.path().join("seed");
    let root = tmp.path().join("widgets");
    make_bundle(tmp.path(), &seed_dir, "cpu", "1.0.0");

    let outcomes = seed_from_dirs(&[seed_dir], &root);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].id, "cpu");
    assert!(matches!(outcomes[0].action, SeedAction::Installed), "{:?}", outcomes[0]);
    assert_eq!(installed_version(&root, "cpu").as_deref(), Some("1.0.0"));
    assert!(root.join("cpu/widget.wasm").is_file());
}

#[test]
fn seed_upgrades_older() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("widgets");

    // Install 1.0.0 first…
    let old_dir = tmp.path().join("seed-old");
    make_bundle(tmp.path(), &old_dir, "cpu", "1.0.0");
    seed_from_dirs(&[old_dir], &root);
    // …then leave a user fingerprint in the installed dir to prove the
    // upgrade replaces the directory wholesale.
    std::fs::write(root.join("cpu/user-marker"), b"x").unwrap();

    let new_dir = tmp.path().join("seed-new");
    make_bundle(tmp.path(), &new_dir, "cpu", "1.1.0");
    let outcomes = seed_from_dirs(&[new_dir], &root);
    assert_eq!(outcomes.len(), 1);
    match &outcomes[0].action {
        SeedAction::Upgraded { from } => assert_eq!(from, "1.0.0"),
        other => panic!("expected Upgraded, got {other:?}"),
    }
    assert_eq!(installed_version(&root, "cpu").as_deref(), Some("1.1.0"));
    assert!(!root.join("cpu/user-marker").exists(), "upgrade replaces the dir");
}

#[test]
fn seed_skips_newer_or_equal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("widgets");

    // Installed copy at 1.2.0 with a local marker.
    let cur_dir = tmp.path().join("seed-cur");
    make_bundle(tmp.path(), &cur_dir, "cpu", "1.2.0");
    seed_from_dirs(&[cur_dir], &root);
    std::fs::write(root.join("cpu/user-marker"), b"x").unwrap();

    // Equal version → skipped, marker intact.
    let eq_dir = tmp.path().join("seed-eq");
    make_bundle(tmp.path(), &eq_dir, "cpu", "1.2.0");
    let outcomes = seed_from_dirs(&[eq_dir], &root);
    match &outcomes[0].action {
        SeedAction::SkippedUpToDate { installed } => assert_eq!(installed, "1.2.0"),
        other => panic!("expected SkippedUpToDate, got {other:?}"),
    }
    assert!(root.join("cpu/user-marker").exists());

    // Older bundle → skipped too (never downgrade).
    let older_dir = tmp.path().join("seed-older");
    make_bundle(tmp.path(), &older_dir, "cpu", "1.1.9");
    let outcomes = seed_from_dirs(&[older_dir], &root);
    assert!(
        matches!(outcomes[0].action, SeedAction::SkippedUpToDate { .. }),
        "{:?}",
        outcomes[0]
    );
    assert_eq!(installed_version(&root, "cpu").as_deref(), Some("1.2.0"));
    assert!(root.join("cpu/user-marker").exists());
}

#[test]
fn seed_never_touches_foreign_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("widgets");

    // A user-installed widget with no corresponding seed bundle.
    let foreign = root.join("my-widget");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("widget.json"), r#"{"id":"my-widget"}"#).unwrap();
    std::fs::write(foreign.join("precious"), b"data").unwrap();

    let seed_dir = tmp.path().join("seed");
    make_bundle(tmp.path(), &seed_dir, "cpu", "1.0.0");
    let outcomes = seed_from_dirs(&[seed_dir], &root);

    assert_eq!(outcomes.len(), 1, "only the seeded id is considered");
    assert_eq!(outcomes[0].id, "cpu");
    assert_eq!(
        std::fs::read(foreign.join("precious")).unwrap(),
        b"data",
        "foreign widget untouched"
    );
    assert!(foreign.join("widget.json").exists());
}

/// Earlier dirs win for duplicate ids; an empty/missing dir is skipped.
#[test]
fn seed_dir_priority_first_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("widgets");

    let hi = tmp.path().join("hi");
    let lo = tmp.path().join("lo");
    make_bundle(tmp.path(), &hi, "cpu", "1.0.0");
    make_bundle(tmp.path(), &lo, "cpu", "9.9.9");

    let missing = tmp.path().join("does-not-exist");
    let outcomes = seed_from_dirs(&[missing, hi, lo], &root);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        installed_version(&root, "cpu").as_deref(),
        Some("1.0.0"),
        "higher-priority dir wins even over a newer version downstream"
    );
}
