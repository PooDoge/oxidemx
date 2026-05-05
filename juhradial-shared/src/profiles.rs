//! Per-application menu profiles.
//!
//! Users can map a focused-window resource class (e.g. "firefox",
//! "code", "konsole") to a profile name. When the daemon's
//! `window_tracker` reports a class change, the overlay calls
//! `ProfileResolver::menu_for(class)` to get the slice list it should
//! render. Profile menus live in
//! `~/.config/juhradial/profiles/<name>.json` as standalone files —
//! easier to share, easier to delete, and easier to detect via inotify
//! than nested objects in `config.json`.
//!
//! The mapping `class → profile name` is stored on the main
//! `AppConfig` as `app_profiles: BTreeMap<String, String>`. Empty
//! mapping = no per-app behaviour, just always render the main menu.
//!
//! This module is the loader / resolver only — the daemon's
//! `window_tracker` is responsible for *detecting* the focused class
//! and emitting it; the overlay calls `menu_for()` on each focus
//! change. Splitting them keeps the loader pure-Rust and unit-testable
//! against synthetic file layouts (no D-Bus, no compositor).

use crate::config::{AppConfig, ConfigError, RadialMenuConfig};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Cached mapping from focused-window class to that class's
/// `RadialMenuConfig`. Created once at startup, refreshed via
/// `reload()` when inotify reports a change to `config.json` or any
/// file under the profiles dir.
pub struct ProfileResolver {
    /// The main config — slices used when no per-app override
    /// matches.
    main: AppConfig,
    /// Profile name → fully-loaded menu. Built at construction time
    /// by reading every file under the profiles dir; entries
    /// referenced from `main.app_profiles` but missing on disk are
    /// silently dropped (and `menu_for` falls back to the main menu).
    by_profile: HashMap<String, RadialMenuConfig>,
}

impl ProfileResolver {
    /// Load using the standard config paths
    /// (`~/.config/juhradial/config.json` +
    /// `~/.config/juhradial/profiles/`).
    pub fn load_default() -> Result<Self, ConfigError> {
        let main_path = crate::config::default_config_path()
            .ok_or_else(|| ConfigError::Io(std::io::Error::other("no $HOME for config")))?;
        let profiles_dir = crate::config::profiles_dir()
            .ok_or_else(|| ConfigError::Io(std::io::Error::other("no $HOME for profiles")))?;
        Self::load_from(&main_path, &profiles_dir)
    }

    /// Load with explicit paths — used by the unit tests against a
    /// tempdir layout, and by anyone embedding the overlay in tooling
    /// that doesn't follow the XDG defaults.
    pub fn load_from(main_path: &Path, profiles_dir: &Path) -> Result<Self, ConfigError> {
        let main = AppConfig::load_from(main_path)?;
        let mut by_profile = HashMap::new();
        if profiles_dir.is_dir() {
            for entry in std::fs::read_dir(profiles_dir).map_err(ConfigError::Io)? {
                let entry = entry.map_err(ConfigError::Io)?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let name = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                // Profile files use the same schema as the main
                // config so users can copy / fork their main menu
                // into a profile and tweak.
                let cfg = match AppConfig::load_from(&path) {
                    Ok(c) => c,
                    Err(_) => continue, // malformed profile — skip
                };
                by_profile.insert(name, cfg.radial_menu);
            }
        }
        Ok(ProfileResolver { main, by_profile })
    }

    /// The main menu (no per-app override).
    pub fn main_menu(&self) -> &RadialMenuConfig {
        &self.main.radial_menu
    }

    /// Look up the menu for a focused window class. Match order:
    ///   1. Exact match on `app_profiles` key.
    ///   2. Case-insensitive match on `app_profiles` key (so users
    ///      don't have to remember whether KWin reported "firefox"
    ///      or "Firefox").
    ///   3. Fall back to the main menu.
    /// Missing profile files (binding present but file deleted)
    /// also fall back to the main menu.
    pub fn menu_for(&self, focused_class: Option<&str>) -> &RadialMenuConfig {
        let class = match focused_class {
            Some(s) if !s.is_empty() => s,
            _ => return self.main_menu(),
        };

        // Direct lookup first.
        let profile_name = self
            .main
            .app_profiles
            .get(class)
            .or_else(|| {
                let lc = class.to_lowercase();
                self.main
                    .app_profiles
                    .iter()
                    .find(|(k, _)| k.to_lowercase() == lc)
                    .map(|(_, v)| v)
            });

        match profile_name {
            Some(name) => self
                .by_profile
                .get(name)
                .unwrap_or(&self.main.radial_menu),
            None => &self.main.radial_menu,
        }
    }

    /// List the available profile names — for the editor to populate
    /// a "use which profile for this app?" dropdown.
    pub fn profile_names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.by_profile.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    /// All declared `class → profile-name` bindings — for the editor
    /// to show which apps currently have overrides.
    pub fn bindings(&self) -> impl Iterator<Item = (&str, &str)> {
        self.main
            .app_profiles
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Save a new binding to the in-memory `app_profiles` map. The
    /// caller is responsible for persisting the main `AppConfig`
    /// back to disk (the editor does this via the same JSON path it
    /// loaded from).
    pub fn set_binding(&mut self, class: String, profile_name: String) {
        self.main.app_profiles.insert(class, profile_name);
    }

    /// Remove a binding. Returns the profile name that was bound,
    /// if any.
    pub fn remove_binding(&mut self, class: &str) -> Option<String> {
        self.main.app_profiles.remove(class)
    }

    /// Borrow the underlying main `AppConfig` so the caller can save
    /// it back to disk after editing bindings.
    pub fn main_config(&self) -> &AppConfig {
        &self.main
    }

    /// Mutable borrow for the editor.
    pub fn main_config_mut(&mut self) -> &mut AppConfig {
        &mut self.main
    }

    /// Replace the main config with a freshly-loaded one and
    /// re-scan the profiles dir. Called by the inotify watcher.
    pub fn reload(&mut self, main_path: &Path, profiles_dir: &Path) -> Result<(), ConfigError> {
        let new = Self::load_from(main_path, profiles_dir)?;
        *self = new;
        Ok(())
    }

    /// Persist the main config (including `app_profiles` edits) back
    /// to disk. Profile files themselves are managed separately —
    /// `save_profile` does that.
    pub fn save_main(&self, main_path: &Path) -> Result<(), ConfigError> {
        let json = serde_json::to_string_pretty(&self.main).map_err(ConfigError::Parse)?;
        if let Some(parent) = main_path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        std::fs::write(main_path, json).map_err(ConfigError::Io)?;
        Ok(())
    }

    /// Write a single profile file to disk and update the in-memory
    /// cache. Used by the editor when saving the slice list for a
    /// per-app override.
    pub fn save_profile(
        &mut self,
        profiles_dir: &Path,
        name: &str,
        mut menu: RadialMenuConfig,
    ) -> Result<(), ConfigError> {
        // Make sure the menu is in the multi-page shape before
        // either caching or writing — keeps the on-disk file and
        // the cache identical to what the loader would produce, so
        // callers don't have to remember to normalize.
        menu.normalize_pages();
        // Profile files use the full AppConfig schema for forward
        // compatibility — today they only carry radial_menu, but
        // future fields (hotkeys, conditional-slice predicates)
        // should live next to the slices they relate to.
        let mut cfg = AppConfig::default();
        cfg.radial_menu = menu.clone();
        let path = profile_path(profiles_dir, name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        let json = serde_json::to_string_pretty(&cfg).map_err(ConfigError::Parse)?;
        std::fs::write(&path, json).map_err(ConfigError::Io)?;
        self.by_profile.insert(name.to_string(), menu);
        Ok(())
    }
}

fn profile_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, body).unwrap();
        p
    }

    fn main_with_bindings(bindings: &[(&str, &str)]) -> String {
        let map: serde_json::Map<String, serde_json::Value> = bindings
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
            .collect();
        let v = serde_json::json!({
            "theme": "dracula",
            "radial_menu": { "slices": [
                { "label": "Default Slice", "type": "exec", "command": "true",
                  "color": "green", "icon": "" }
            ]},
            "app_profiles": map,
        });
        serde_json::to_string_pretty(&v).unwrap()
    }

    fn profile_body(label: &str) -> String {
        let v = serde_json::json!({
            "radial_menu": { "slices": [
                { "label": label, "type": "exec", "command": "true",
                  "color": "blue", "icon": "" }
            ]}
        });
        serde_json::to_string_pretty(&v).unwrap()
    }

    #[test]
    fn no_bindings_always_returns_main_menu() {
        let tmp = TempDir::new().unwrap();
        let main = write(tmp.path(), "config.json", &main_with_bindings(&[]));
        let resolver = ProfileResolver::load_from(&main, &tmp.path().join("profiles")).unwrap();
        assert_eq!(
            resolver.menu_for(Some("firefox")).pages[0].slices[0].label,
            "Default Slice"
        );
        assert_eq!(
            resolver.menu_for(None).pages[0].slices[0].label,
            "Default Slice"
        );
    }

    #[test]
    fn matching_binding_returns_profile_menu() {
        let tmp = TempDir::new().unwrap();
        let profiles_dir = tmp.path().join("profiles");
        let main = write(
            tmp.path(),
            "config.json",
            &main_with_bindings(&[("firefox", "browsing")]),
        );
        write(&profiles_dir, "browsing.json", &profile_body("Browsing Slice"));
        let resolver = ProfileResolver::load_from(&main, &profiles_dir).unwrap();
        assert_eq!(
            resolver.menu_for(Some("firefox")).pages[0].slices[0].label,
            "Browsing Slice"
        );
        // Non-matching class falls back to main.
        assert_eq!(
            resolver.menu_for(Some("konsole")).pages[0].slices[0].label,
            "Default Slice"
        );
    }

    #[test]
    fn binding_present_but_profile_file_missing_falls_back() {
        let tmp = TempDir::new().unwrap();
        let main = write(
            tmp.path(),
            "config.json",
            &main_with_bindings(&[("firefox", "ghost")]),
        );
        let resolver = ProfileResolver::load_from(&main, &tmp.path().join("profiles")).unwrap();
        // 'ghost' isn't on disk — fall back to main.
        assert_eq!(
            resolver.menu_for(Some("firefox")).pages[0].slices[0].label,
            "Default Slice"
        );
    }

    #[test]
    fn case_insensitive_match() {
        let tmp = TempDir::new().unwrap();
        let profiles_dir = tmp.path().join("profiles");
        let main = write(
            tmp.path(),
            "config.json",
            &main_with_bindings(&[("Firefox", "browsing")]),
        );
        write(&profiles_dir, "browsing.json", &profile_body("Browsing Slice"));
        let resolver = ProfileResolver::load_from(&main, &profiles_dir).unwrap();
        // Window class "firefox" (lowercase) still resolves the
        // "Firefox" (capitalised) binding.
        assert_eq!(
            resolver.menu_for(Some("firefox")).pages[0].slices[0].label,
            "Browsing Slice"
        );
    }

    #[test]
    fn save_profile_writes_disk_and_updates_cache() {
        let tmp = TempDir::new().unwrap();
        let profiles_dir = tmp.path().join("profiles");
        let main = write(tmp.path(), "config.json", &main_with_bindings(&[]));
        let mut resolver = ProfileResolver::load_from(&main, &profiles_dir).unwrap();

        let menu: RadialMenuConfig = serde_json::from_str(
            r#"{"slices":[{"label":"Saved","type":"exec","command":"true","color":"red","icon":""}]}"#,
        )
        .unwrap();
        resolver.save_profile(&profiles_dir, "test", menu).unwrap();

        // File on disk.
        let on_disk = profiles_dir.join("test.json");
        assert!(on_disk.exists());

        // Cache updated.
        resolver.set_binding("foo".into(), "test".into());
        assert_eq!(
            resolver.menu_for(Some("foo")).pages[0].slices[0].label,
            "Saved"
        );

        // Re-load from disk verifies persistence.
        let reloaded = ProfileResolver::load_from(&main, &profiles_dir).unwrap();
        assert!(reloaded.profile_names().contains(&"test"));
    }

    #[test]
    fn malformed_profile_is_skipped_silently() {
        let tmp = TempDir::new().unwrap();
        let profiles_dir = tmp.path().join("profiles");
        let main = write(
            tmp.path(),
            "config.json",
            &main_with_bindings(&[("firefox", "broken")]),
        );
        write(&profiles_dir, "broken.json", "{ this is not JSON ");
        let resolver = ProfileResolver::load_from(&main, &profiles_dir).unwrap();
        // Malformed profile -> no entry -> falls back.
        assert_eq!(
            resolver.menu_for(Some("firefox")).pages[0].slices[0].label,
            "Default Slice"
        );
    }

    #[test]
    fn empty_class_falls_back() {
        let tmp = TempDir::new().unwrap();
        let main = write(tmp.path(), "config.json", &main_with_bindings(&[]));
        let resolver = ProfileResolver::load_from(&main, &tmp.path().join("profiles")).unwrap();
        assert_eq!(
            resolver.menu_for(Some("")).pages[0].slices[0].label,
            "Default Slice"
        );
    }
}
