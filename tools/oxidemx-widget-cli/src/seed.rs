//! Startup seeding of bundled built-in widgets (spec §16).
//!
//! The overlay and the settings app call [`seed_builtin_widgets`] once at
//! process start (before their first registry scan). It walks the seed
//! search dirs for `*.omxw` bundles and installs each one into the user's
//! widgets dir when the bundled version is strictly newer than (or there
//! is no) installed copy.
//!
//! ## Trust model — why no consent prompt
//!
//! Sideloaded bundles normally need explicit consent (`--force`, spec §4).
//! Seed bundles are different: they ship on the same install media as the
//! binaries themselves (`install.sh` copies them to
//! `<prefix>/share/oxidemx/widgets/` next to `<prefix>/bin/oxidemx-*`).
//! Anyone who can plant a bundle there can replace the overlay binary
//! outright, so a consent prompt would gate nothing — running the overlay
//! already implies trusting its bundled widgets. Hence unsigned/dev-signed
//! seed bundles install with `force` after seeding's own version gate.
//!
//! ## What seeding never does
//!
//! - Never downgrades or re-extracts an equal version (the strict
//!   `version_gt` gate), so a locally patched copy of a bundled widget
//!   survives until the shipped version actually moves forward.
//! - Never touches widget ids that have no bundle in the seed dirs —
//!   user-installed widgets are invisible to it.

use std::path::{Path, PathBuf};

use crate::{install, CliError, InstallResult};

/// What [`seed_from_dirs`] did for one bundle.
#[derive(Debug)]
pub struct SeedOutcome {
    pub id: String,
    /// The bundled version under consideration.
    pub version: String,
    pub action: SeedAction,
}

#[derive(Debug)]
pub enum SeedAction {
    /// Not previously installed — extracted fresh.
    Installed,
    /// Installed over an older copy. Carries the replaced version.
    Upgraded { from: String },
    /// Installed copy is the same version or newer — left untouched.
    SkippedUpToDate { installed: String },
    /// The bundle could not be parsed or installed.
    Failed(String),
}

impl std::fmt::Display for SeedOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.action {
            SeedAction::Installed => write!(f, "{} v{}: installed", self.id, self.version),
            SeedAction::Upgraded { from } => {
                write!(f, "{} v{}: upgraded from v{from}", self.id, self.version)
            }
            SeedAction::SkippedUpToDate { installed } => {
                write!(f, "{} v{}: up to date (installed v{installed})", self.id, self.version)
            }
            SeedAction::Failed(e) => write!(f, "{} v{}: FAILED: {e}", self.id, self.version),
        }
    }
}

/// Seed-bundle search dirs, highest priority first (spec §16 / plan 4):
///
/// 1. `$OXIDEMX_BUILTIN_WIDGETS_DIR` (tests, smoke scripts)
/// 2. `<exe dir>/../share/oxidemx/widgets` (`/usr/local/bin` →
///    `/usr/local/share/oxidemx/widgets`, the install.sh layout)
/// 3. each `$XDG_DATA_DIRS/oxidemx/widgets`
/// 4. `~/.local/share/oxidemx/widgets`
///
/// Only dirs that exist are returned. For a widget id present in several
/// dirs, the earliest dir wins.
pub fn builtin_seed_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(p) = std::env::var_os("OXIDEMX_BUILTIN_WIDGETS_DIR") {
        dirs.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            dirs.push(bin_dir.join("../share/oxidemx/widgets"));
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_DIRS") {
        for base in std::env::split_paths(&xdg) {
            if base.as_os_str().is_empty() {
                continue;
            }
            dirs.push(base.join("oxidemx").join("widgets"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/oxidemx/widgets"));
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

/// Seed from the standard search dirs into the standard widgets dir.
/// Returns one outcome per considered bundle (empty when no seed dir
/// exists — the common case on dev checkouts without an install).
pub fn seed_builtin_widgets() -> Result<Vec<SeedOutcome>, CliError> {
    let root = oxidemx_widget_host::WidgetRegistry::widgets_dir()
        .ok_or_else(|| CliError::Io(std::io::Error::other("cannot determine widgets_dir")))?;
    Ok(seed_from_dirs(&builtin_seed_dirs(), &root))
}

/// Core seeding pass, explicit dirs + install root (unit-testable).
pub fn seed_from_dirs(seed_dirs: &[PathBuf], install_root: &Path) -> Vec<SeedOutcome> {
    let mut outcomes = Vec::new();
    let mut seen_ids: Vec<String> = Vec::new();

    for dir in seed_dirs {
        let mut bundles: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "omxw") && p.is_file())
                .collect(),
            Err(_) => continue,
        };
        bundles.sort(); // deterministic order within a dir

        for bundle in bundles {
            let manifest = match bundle_manifest(&bundle) {
                Ok(m) => m,
                Err(e) => {
                    outcomes.push(SeedOutcome {
                        id: bundle.display().to_string(),
                        version: String::new(),
                        action: SeedAction::Failed(e.to_string()),
                    });
                    continue;
                }
            };
            if seen_ids.contains(&manifest.id) {
                continue; // an earlier (higher-priority) dir already won
            }
            seen_ids.push(manifest.id.clone());
            outcomes.push(seed_one(&bundle, &manifest, install_root));
        }
    }
    outcomes
}

/// Install `bundle` iff its version is strictly newer than the installed
/// copy (or nothing is installed under its id).
fn seed_one(
    bundle: &Path,
    manifest: &oxidemx_widget_proto::WidgetManifest,
    install_root: &Path,
) -> SeedOutcome {
    let installed = installed_version(install_root, &manifest.id);
    let action = match &installed {
        Some(cur) if !version_gt(&manifest.version, cur) => {
            SeedAction::SkippedUpToDate { installed: cur.clone() }
        }
        _ => {
            // `force`: replaces the older copy AND waives the sideload
            // consent gate — see the trust-model note in the module docs.
            match install(bundle, true, Some(install_root)) {
                Ok(InstallResult { .. }) => match installed {
                    Some(from) => SeedAction::Upgraded { from },
                    None => SeedAction::Installed,
                },
                Err(e) => SeedAction::Failed(e.to_string()),
            }
        }
    };
    SeedOutcome { id: manifest.id.clone(), version: manifest.version.clone(), action }
}

/// Read `widget.json` out of a `.omxw` zip without extracting it.
fn bundle_manifest(bundle: &Path) -> Result<oxidemx_widget_proto::WidgetManifest, CliError> {
    use std::io::Read;
    let bytes = std::fs::read(bundle)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .map_err(|e| CliError::Manifest(format!("not a zip: {e}")))?;
    let mut f = archive
        .by_name("widget.json")
        .map_err(|_| CliError::Manifest("bundle missing widget.json".into()))?;
    let mut s = String::new();
    f.read_to_string(&mut s)
        .map_err(|e| CliError::Manifest(format!("cannot read widget.json: {e}")))?;
    let manifest: oxidemx_widget_proto::WidgetManifest = serde_json::from_str(&s)
        .map_err(|e| CliError::Manifest(format!("widget.json does not parse: {e}")))?;
    manifest
        .validate()
        .map_err(|e| CliError::Manifest(format!("bundle manifest invalid: {e}")))?;
    Ok(manifest)
}

/// Version of the currently installed copy of `id`, if any.
fn installed_version(install_root: &Path, id: &str) -> Option<String> {
    let raw = std::fs::read_to_string(install_root.join(id).join("widget.json")).ok()?;
    let manifest: oxidemx_widget_proto::WidgetManifest = serde_json::from_str(&raw).ok()?;
    Some(manifest.version)
}

/// Semver-ish strict comparison: split on `.`, compare numerically
/// component-wise (missing components = 0, non-numeric components = 0).
/// `true` iff `a > b`.
fn version_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.').map(|c| c.trim().parse::<u64>().unwrap_or(0)).collect()
    };
    let (va, vb) = (parse(a), parse(b));
    let len = va.len().max(vb.len());
    for i in 0..len {
        let (ca, cb) = (va.get(i).copied().unwrap_or(0), vb.get(i).copied().unwrap_or(0));
        if ca != cb {
            return ca > cb;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gt_compares_numerically() {
        assert!(version_gt("1.1.0", "1.0.9"));
        assert!(version_gt("1.10.0", "1.9.0"), "numeric, not lexicographic");
        assert!(version_gt("2", "1.9.9"));
        assert!(version_gt("1.0.1", "1.0"));
        assert!(!version_gt("1.0.0", "1.0.0"));
        assert!(!version_gt("1.0", "1.0.0"), "trailing zeros equal");
        assert!(!version_gt("1.0.0", "1.1.0"));
        // Garbage components count as 0 rather than panicking.
        assert!(version_gt("1.1", "1.x"));
        assert!(!version_gt("x", "0"));
    }
}
