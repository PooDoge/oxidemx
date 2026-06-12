//! Builds fixture guest crates (oxidemx-widget-host/fixtures/*) for
//! wasm32-wasip1, once per test process, and hands back the wasm path.
//! Missing wasm target → eprintln a skip message and return None; tests
//! return early instead of failing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

pub const WASM_TARGET: &str = "wasm32-wasip1";

fn target_installed() -> bool {
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    *INSTALLED.get_or_init(|| {
        Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.trim() == WASM_TARGET)
            })
            .unwrap_or(false)
    })
}

/// Build `fixtures/<name>` (release, wasm32-wasip1) and return the wasm
/// artifact path. Cached per name for the lifetime of the test process.
pub fn fixture_wasm(name: &str) -> Option<PathBuf> {
    if !target_installed() {
        eprintln!(
            "SKIP: {WASM_TARGET} target not installed \
             (run `rustup target add {WASM_TARGET}`); skipping {name} test"
        );
        return None;
    }
    static BUILT: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    if let Some(p) = built.get(name) {
        return Some(p.clone());
    }

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name);
    // Explicit --target-dir: keep the artifact inside the fixture dir even
    // if CARGO_TARGET_DIR is set (also dodges the workspace target lock).
    let target_dir = dir.join("target");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", WASM_TARGET])
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("failed to spawn cargo for the fixture build");
    assert!(status.success(), "fixture {name} failed to build");

    let wasm = target_dir
        .join(WASM_TARGET)
        .join("release")
        .join(format!("{}.wasm", name.replace('-', "_")));
    assert!(wasm.is_file(), "expected fixture artifact at {}", wasm.display());
    built.insert(name.to_string(), wasm.clone());
    Some(wasm)
}

/// Build a crate at an arbitrary path (outside `fixtures/`) and return the
/// wasm artifact. `key` is a unique cache key; `crate_dir` is the directory
/// containing `Cargo.toml`; `bin_stem` is the wasm file name without extension.
pub fn build_wasm_at(key: &str, crate_dir: PathBuf, bin_stem: &str) -> Option<PathBuf> {
    if !target_installed() {
        eprintln!(
            "SKIP: {WASM_TARGET} target not installed; skipping {key} test"
        );
        return None;
    }
    static BUILT_EXT: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    let mut built = BUILT_EXT.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    if let Some(p) = built.get(key) {
        return Some(p.clone());
    }

    let target_dir = crate_dir.join("target");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", WASM_TARGET])
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("failed to spawn cargo for external wasm build");
    assert!(status.success(), "{key} wasm build failed");

    let wasm = target_dir
        .join(WASM_TARGET)
        .join("release")
        .join(format!("{}.wasm", bin_stem.replace('-', "_")));
    assert!(wasm.is_file(), "expected artifact at {}", wasm.display());
    built.insert(key.to_string(), wasm.clone());
    Some(wasm)
}
