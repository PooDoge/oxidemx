//! Filesystem registry over the widgets install dir
//! (`~/.config/oxidemx/widgets/<id>/widget.json`). No wasm here — just
//! manifest discovery + validation. Bad manifests yield
//! [`WidgetState::Incompatible`] with a human-readable reason (the
//! settings UI shows it verbatim); a broken dir never aborts the scan.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oxidemx_widget_proto::{WidgetManifest, API_VERSION};

/// One `<id>/` dir under the widgets install dir.
pub struct InstalledWidget {
    pub manifest: WidgetManifest,
    /// `~/.config/oxidemx/widgets/<id>/`
    pub dir: PathBuf,
    pub state: WidgetState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WidgetState {
    Ready,
    /// Bad manifest / api_version range / missing files — reason is shown
    /// verbatim in the picker (spec §9).
    Incompatible { reason: String },
}

pub struct WidgetRegistry {
    widgets: BTreeMap<String, InstalledWidget>,
}

impl WidgetRegistry {
    /// Scan `dir` for `<id>/widget.json`. Validation failures yield
    /// `Incompatible`, never errors; the dir name must match `manifest.id`.
    pub fn scan(dir: &Path) -> Self {
        let mut widgets = BTreeMap::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return WidgetRegistry { widgets },
        };
        for entry in entries.flatten() {
            let wdir = entry.path();
            if !wdir.is_dir() {
                continue;
            }
            let dir_id = entry.file_name().to_string_lossy().into_owned();
            let manifest_path = wdir.join("widget.json");
            if !manifest_path.is_file() {
                continue; // not a widget dir at all
            }
            let raw = match std::fs::read_to_string(&manifest_path) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("widget {dir_id}: unreadable widget.json: {e}");
                    continue;
                }
            };
            let (manifest, state) = match serde_json::from_str::<WidgetManifest>(&raw) {
                Ok(m) => {
                    let state = validate_installed(&m, &dir_id, &wdir);
                    (m, state)
                }
                Err(e) => {
                    // Keep a placeholder manifest so the picker can still
                    // list the dir with its failure reason.
                    let placeholder = placeholder_manifest(&dir_id);
                    let state = WidgetState::Incompatible {
                        reason: format!("widget.json does not parse: {e}"),
                    };
                    (placeholder, state)
                }
            };
            if let WidgetState::Incompatible { reason } = &state {
                log::warn!("widget {dir_id}: incompatible: {reason}");
            }
            widgets.insert(dir_id, InstalledWidget { manifest, dir: wdir, state });
        }
        WidgetRegistry { widgets }
    }

    pub fn get(&self, id: &str) -> Option<&InstalledWidget> {
        self.widgets.get(id)
    }

    pub fn iter_ready(&self) -> impl Iterator<Item = &InstalledWidget> {
        self.widgets.values().filter(|w| w.state == WidgetState::Ready)
    }

    /// `~/.config/oxidemx/widgets`
    pub fn widgets_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("oxidemx").join("widgets"))
    }
}

/// Spec §4/§5 install-time checks: structural validate, api_version match,
/// entry/icon present, dir name == manifest id.
fn validate_installed(m: &WidgetManifest, dir_id: &str, dir: &Path) -> WidgetState {
    if let Err(reason) = m.validate() {
        return WidgetState::Incompatible { reason };
    }
    if m.api_version != API_VERSION {
        return WidgetState::Incompatible {
            reason: format!(
                "requires widget API version {} but this OxideMX speaks version {}",
                m.api_version, API_VERSION
            ),
        };
    }
    if m.id != dir_id {
        return WidgetState::Incompatible {
            reason: format!("installed under {dir_id:?} but manifest id is {:?}", m.id),
        };
    }
    if !dir.join(&m.entry).is_file() {
        return WidgetState::Incompatible {
            reason: format!("entry module {:?} is missing from the bundle", m.entry),
        };
    }
    if !dir.join(&m.icon).is_file() {
        return WidgetState::Incompatible {
            reason: format!("icon file {:?} is missing from the bundle", m.icon),
        };
    }
    WidgetState::Ready
}

/// Stand-in manifest for dirs whose widget.json doesn't parse, so the
/// registry can still surface them as Incompatible.
fn placeholder_manifest(dir_id: &str) -> WidgetManifest {
    serde_json::from_value(serde_json::json!({
        "id": dir_id,
        "name": dir_id,
        "version": "0.0.0",
        "author": "",
        "api_version": 0,
        "entry": "widget.wasm",
        "icon": "icon.svg",
    }))
    .expect("placeholder manifest shape is static")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh scan root under temp_dir, like oxidemx-shared/src/migrate.rs.
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join(format!("oxidemx-widget-registry-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn manifest_json(id: &str, api_version: u32) -> String {
        format!(
            r#"{{ "id": "{id}", "name": "Test", "version": "0.1.0",
                  "author": "x", "api_version": {api_version},
                  "entry": "widget.wasm", "icon": "icon.svg" }}"#
        )
    }

    /// Write a widget dir; `entry`/`icon` control which payload files exist.
    fn write_widget(root: &Path, dir_id: &str, manifest: &str, entry: bool, icon: bool) -> PathBuf {
        let d = root.join(dir_id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("widget.json"), manifest).unwrap();
        if entry {
            std::fs::write(d.join("widget.wasm"), b"").unwrap();
        }
        if icon {
            std::fs::write(d.join("icon.svg"), "<svg/>").unwrap();
        }
        d
    }

    #[test]
    fn scan_finds_valid_widget() {
        let root = tmp("valid");
        let dir = write_widget(&root, "clock", &manifest_json("clock", 1), true, true);
        let reg = WidgetRegistry::scan(&root);
        let w = reg.get("clock").expect("clock should be found");
        assert_eq!(w.state, WidgetState::Ready);
        assert_eq!(w.manifest.id, "clock");
        assert_eq!(w.manifest.api_version, 1);
        assert_eq!(w.dir, dir);
        assert_eq!(reg.iter_ready().count(), 1);
    }

    #[test]
    fn bad_api_version_is_incompatible() {
        let root = tmp("apiver");
        write_widget(&root, "clock", &manifest_json("clock", 99), true, true);
        let reg = WidgetRegistry::scan(&root);
        let w = reg.get("clock").unwrap();
        match &w.state {
            WidgetState::Incompatible { reason } => {
                assert!(reason.contains("99"), "reason should mention the version: {reason}");
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
        assert_eq!(reg.iter_ready().count(), 0);
    }

    #[test]
    fn missing_entry_file_is_incompatible() {
        let root = tmp("noentry");
        write_widget(&root, "clock", &manifest_json("clock", 1), false, true);
        let reg = WidgetRegistry::scan(&root);
        let w = reg.get("clock").unwrap();
        match &w.state {
            WidgetState::Incompatible { reason } => {
                assert!(reason.contains("widget.wasm"), "reason should name the entry: {reason}");
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn id_mismatch_is_incompatible() {
        let root = tmp("idmismatch");
        write_widget(&root, "foo", &manifest_json("bar", 1), true, true);
        let reg = WidgetRegistry::scan(&root);
        let w = reg.get("foo").expect("registered under the dir name");
        match &w.state {
            WidgetState::Incompatible { reason } => {
                assert!(
                    reason.contains("foo") && reason.contains("bar"),
                    "reason should name both ids: {reason}"
                );
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn unparseable_manifest_is_incompatible_not_fatal() {
        let root = tmp("garbage");
        write_widget(&root, "broken", "{ not json at all", true, true);
        write_widget(&root, "clock", &manifest_json("clock", 1), true, true);
        let reg = WidgetRegistry::scan(&root);
        let broken = reg.get("broken").expect("broken dir still listed");
        assert!(matches!(broken.state, WidgetState::Incompatible { .. }));
        // The garbage dir must not poison scanning the good one.
        let clock = reg.get("clock").unwrap();
        assert_eq!(clock.state, WidgetState::Ready);
        assert_eq!(reg.iter_ready().count(), 1);
    }
}
