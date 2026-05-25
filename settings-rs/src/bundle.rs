//! Self-contained config export / import bundle.
//!
//! A "bundle" is a single JSON document containing the user's
//! whole JuhRadial setup — radial config + recorded macros + saved
//! themes — so it can be exported once and imported on a new
//! machine without fishing for sidecar files.
//!
//! The bundle also stays forward-compatible: every collection
//! field has `#[serde(default)]` so older bundles missing a
//! section still parse, and a future field with the same default
//! treatment will deserialise without panic.
//!
//! Backwards-compatibility with the old "just AppConfig" export
//! format is provided by a custom deserialiser that falls back to
//! "this is a raw AppConfig, no macros / themes" when the
//! `__bundle_version` discriminator is missing.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level bundle. `__bundle_version` lets us evolve the schema
/// in place — bumps tell the importer to apply migrations or
/// reject. Today only "1" is recognised.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigBundle {
    #[serde(default = "default_bundle_version")]
    pub __bundle_version: u32,
    pub config: juhradial_shared::AppConfig,
    #[serde(default)]
    pub macros: Vec<BundledMacro>,
    #[serde(default)]
    pub themes: Vec<BundledTheme>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundledMacro {
    /// Filename stem without the `.json` extension. Bundle stores
    /// the entire macro JSON verbatim under `body`, so the
    /// daemon's macro loader sees byte-identical input.
    pub id: String,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundledTheme {
    pub slug: String,
    pub theme: juhradial_shared::theme::Theme,
}

fn default_bundle_version() -> u32 {
    1
}

impl ConfigBundle {
    /// Snapshot the current on-disk state into a bundle.
    pub fn capture(config: juhradial_shared::AppConfig) -> Self {
        ConfigBundle {
            __bundle_version: default_bundle_version(),
            config,
            macros: collect_macros(),
            themes: collect_themes(),
        }
    }

    /// Apply every section of a bundle: write macros + themes to
    /// their respective directories, then return the AppConfig so
    /// the caller can install it into State. Errors are collected
    /// + returned; partial success is the norm here (one bad
    /// macro shouldn't block restoring everything else).
    pub fn install(self) -> (juhradial_shared::AppConfig, Vec<String>) {
        let mut errors = Vec::new();
        for m in self.macros {
            if let Err(e) = write_macro(&m) {
                errors.push(format!("macro {}: {e}", m.id));
            }
        }
        for t in self.themes {
            if let Err(e) =
                juhradial_shared::theme::save_user_theme(&t.slug, &t.theme)
            {
                errors.push(format!("theme {}: {e}", t.slug));
            }
        }
        (self.config, errors)
    }
}

fn collect_macros() -> Vec<BundledMacro> {
    let dir = match crate::tabs::macros::macros_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<BundledMacro> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                return None;
            }
            let id = p.file_stem().and_then(|s| s.to_str())?.to_string();
            let bytes = std::fs::read(&p).ok()?;
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).ok()?;
            Some(BundledMacro { id, body })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn collect_themes() -> Vec<BundledTheme> {
    let slugs = juhradial_shared::theme::list_user_theme_slugs();
    slugs
        .into_iter()
        .filter_map(|slug| {
            juhradial_shared::theme::Theme::load(
                &juhradial_shared::theme::ThemeName::from(slug.as_str()),
            )
            .map(|theme| BundledTheme { slug, theme })
        })
        .collect()
}

fn write_macro(m: &BundledMacro) -> Result<(), String> {
    let dir = crate::tabs::macros::macros_dir()
        .ok_or_else(|| "no config dir".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let path: PathBuf = dir.join(format!("{}.json", m.id));
    let json = serde_json::to_string_pretty(&m.body)
        .map_err(|e| format!("serialise: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Try to parse `bytes` as a bundle first; fall back to a bare
/// `AppConfig` for backwards compatibility with the older export
/// format. Returns `(config, errors)` after applying any
/// macro/theme files in the bundle.
pub fn parse_and_install(bytes: &[u8]) -> Result<(juhradial_shared::AppConfig, Vec<String>), String> {
    if let Ok(bundle) = serde_json::from_slice::<ConfigBundle>(bytes) {
        if bundle.__bundle_version == 1 {
            return Ok(bundle.install());
        }
        return Err(format!(
            "unsupported bundle version: {}",
            bundle.__bundle_version
        ));
    }
    // Older format — bare AppConfig.
    let cfg: juhradial_shared::AppConfig = serde_json::from_slice(bytes)
        .map_err(|e| format!("parse: {e}"))?;
    Ok((cfg, Vec::new()))
}
