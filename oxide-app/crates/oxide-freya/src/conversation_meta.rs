//! Frontend-owned conversation titles + icons, persisted per project to
//! `<project>/.oxide/conversation-meta.json`. No backend involvement.
use std::collections::HashMap;
use std::path::PathBuf;

use bytes::Bytes;
use freya_icons::lucide;
use serde::{Deserialize, Serialize};

pub const DEFAULT_ICON: &str = "message-square";
pub const CURATED_ICONS: [&str; 16] = [
    "message-square", "terminal",   "code",     "search",
    "folder",         "bug",        "sparkles",  "git-branch",
    "flask-conical",  "book",       "zap",       "pin",
    "bot",            "wrench",     "file-text", "globe",
];

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConvMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

pub struct ConversationMetaStore {
    path: PathBuf,
    map: HashMap<String, ConvMeta>,
}

impl ConversationMetaStore {
    pub fn load(project_dir: &str, project_id: &str) -> Self {
        let path = Self::meta_path(project_dir, project_id);
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, ConvMeta>>(&s).ok())
            .unwrap_or_default();
        Self { path, map }
    }

    pub fn get(&self, id: &str) -> Option<&ConvMeta> {
        self.map.get(id)
    }

    pub fn as_map(&self) -> &HashMap<String, ConvMeta> {
        &self.map
    }

    pub fn set_title(&mut self, id: &str, title: String) {
        self.map.entry(id.to_string()).or_default().title = Some(title);
        self.persist();
    }

    pub fn set_icon(&mut self, id: &str, icon: String) {
        self.map.entry(id.to_string()).or_default().icon = Some(icon);
        self.persist();
    }

    fn meta_path(project_dir: &str, project_id: &str) -> PathBuf {
        if !project_dir.trim().is_empty() {
            return PathBuf::from(project_dir)
                .join(".oxide")
                .join("conversation-meta.json");
        }
        let base = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join(".config")
            .join("oxidemx")
            .join("projects")
            .join(project_id)
            .join("conversation-meta.json")
    }

    fn persist(&self) {
        // prune entries where both fields are None
        let pruned: HashMap<_, _> = self
            .map
            .iter()
            .filter(|(_, m)| m.title.is_some() || m.icon.is_some())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if let Some(parent) = self.path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("conversation_meta: mkdir failed: {e}");
                return;
            }
        }

        let tmp = self.path.with_extension("json.tmp");
        match serde_json::to_string_pretty(&pruned) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&tmp, &json) {
                    eprintln!("conversation_meta: write failed: {e}");
                    return;
                }
                if let Err(e) = std::fs::rename(&tmp, &self.path) {
                    eprintln!("conversation_meta: rename failed: {e}");
                }
            }
            Err(e) => eprintln!("conversation_meta: serialize failed: {e}"),
        }
    }
}

/// Derive a display title from the first user message: trim, truncate to 60 chars.
/// Returns `None` if the message is blank.
pub fn derive_title(msg: &str) -> Option<String> {
    let t = msg.trim();
    if t.is_empty() {
        return None;
    }
    Some(t.chars().take(60).collect())
}

/// Resolve the effective display title, preferring the user-set override.
pub fn effective_title(meta_title: Option<&str>, agentd_title: &str) -> String {
    if let Some(t) = meta_title {
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if !agentd_title.is_empty() {
        return agentd_title.to_string();
    }
    "New conversation".to_string()
}

/// Validate `meta_icon` against the curated set; fall back to `DEFAULT_ICON`.
pub fn effective_icon(meta_icon: Option<&str>) -> &'static str {
    match meta_icon {
        Some(name) => CURATED_ICONS
            .iter()
            .copied()
            .find(|&c| c == name)
            .unwrap_or(DEFAULT_ICON),
        None => DEFAULT_ICON,
    }
}

/// Return the SVG bytes for a curated icon name, or the default icon for unknown names.
/// Never panics.
///
/// Note: returns `bytes::Bytes` because that is what `freya_icons::lucide::*` produces.
pub fn icon_svg(name: &str) -> Bytes {
    match effective_icon(Some(name)) {
        "message-square" => lucide::message_square(),
        "terminal"       => lucide::terminal(),
        "code"           => lucide::code(),
        "search"         => lucide::search(),
        "folder"         => lucide::folder(),
        "bug"            => lucide::bug(),
        "sparkles"       => lucide::sparkles(),
        "git-branch"     => lucide::git_branch(),
        "flask-conical"  => lucide::flask_conical(),
        "book"           => lucide::book(),
        "zap"            => lucide::zap(),
        "pin"            => lucide::pin(),
        "bot"            => lucide::bot(),
        "wrench"         => lucide::wrench(),
        "file-text"      => lucide::file_text(),
        "globe"          => lucide::globe(),
        _                => lucide::message_square(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn derive_title_trims_caps_and_skips_empty() {
        assert_eq!(super::derive_title("  hello world  "), Some("hello world".to_string()));
        assert_eq!(super::derive_title("   "), None);
        let long = "x".repeat(80);
        assert_eq!(super::derive_title(&long).unwrap().chars().count(), 60);
    }

    #[test]
    fn effective_title_fallback_order() {
        assert_eq!(super::effective_title(Some("Override"), "agentd"), "Override");
        assert_eq!(super::effective_title(None, "agentd"), "agentd");
        assert_eq!(super::effective_title(None, ""), "New conversation");
    }

    #[test]
    fn effective_icon_validates_against_curated_set() {
        assert_eq!(super::effective_icon(Some("terminal")), "terminal");
        assert_eq!(super::effective_icon(Some("not-a-real-icon")), super::DEFAULT_ICON);
        assert_eq!(super::effective_icon(None), super::DEFAULT_ICON);
    }

    #[test]
    fn store_roundtrip_in_tempdir() {
        let dir = std::env::temp_dir().join(format!("oxide-meta-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = dir.to_string_lossy().to_string();
        let mut s = super::ConversationMetaStore::load(&dirs, "proj");
        s.set_title("c1", "My chat".into());
        s.set_icon("c1", "terminal".into());
        let reloaded = super::ConversationMetaStore::load(&dirs, "proj");
        let m = reloaded.get("c1").unwrap();
        assert_eq!(m.title.as_deref(), Some("My chat"));
        assert_eq!(m.icon.as_deref(), Some("terminal"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_or_corrupt_is_empty_not_fatal() {
        let s = super::ConversationMetaStore::load("/nonexistent/path/xyz", "proj");
        assert!(s.get("any").is_none());
    }

    #[test]
    fn icon_svg_never_panics_for_curated_or_unknown() {
        for n in super::CURATED_ICONS {
            assert!(!super::icon_svg(n).is_empty());
        }
        assert!(!super::icon_svg("unknown").is_empty()); // falls back to default
    }
}
