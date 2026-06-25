//! Project model — keys, paths, and global-over-project merge helpers.
//!
//! A "project" is a directory the user has pointed agentd at (typically their
//! working directory). Each project gets:
//!
//! - A stable [`ProjectKey`] (a slug derived from the directory name plus an
//!   8-hex-digit hash of the canonicalized path).
//! - A [`ProjectPaths`] bundle that records where every agentd artifact for
//!   the project lives — both in the central per-project store
//!   (`~/.local/share/oxidemx/projects/<key>/`) and in the project-local
//!   `.oxide/` directory (`.oxidemx/` accepted for back-compat).
//! - Merge helpers that layer project-local config/skills/MCP OVER global
//!   config, so the project-local version wins on name collision (the same
//!   pattern Claude Code uses with `.claude/`).

use std::path::{Path, PathBuf};

// ── ProjectKey ────────────────────────────────────────────────────────────────

/// A stable, filesystem-safe identifier for a project.
///
/// Shape: `<dir-name-sanitized>-<8 hex digits of path hash>`.
/// The hash is computed over the canonicalized path string using FNV-1a so that two
/// different paths pointing to the same directory collapse to the same key, and the
/// hash remains stable across Rust toolchain versions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectKey(String);

/// Compute FNV-1a 64-bit hash over the given bytes (version-stable).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x00000100000001b3);
    }
    h
}

impl ProjectKey {
    /// Derive a [`ProjectKey`] from a working-directory path.
    ///
    /// Canonicalization is attempted first so symlinks and `..` components
    /// are resolved; if canonicalization fails (e.g. the directory does not
    /// yet exist in tests) the path is used as-given.
    pub fn from_cwd(cwd: &Path) -> ProjectKey {
        let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let path_str = canonical.to_string_lossy();

        // Stable hash of the full canonical path using FNV-1a.
        let hash = fnv1a64(path_str.as_bytes());

        // Slug: the last path component, sanitized to alphanumeric + hyphen.
        let dir_name = canonical
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "root".to_string());
        let slug = sanitize_slug(&dir_name);

        ProjectKey(format!("{slug}-{hash:08x}"))
    }

    /// The raw key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Replace non-alphanumeric characters (except `-`) with `-`, collapse
/// consecutive `-`, and truncate to 40 chars so the final key is
/// filesystem-friendly everywhere.
fn sanitize_slug(s: &str) -> String {
    let raw: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    // Collapse runs of '-'.
    let mut out = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for c in raw.chars() {
        if c == '-' {
            if !prev_dash {
                out.push(c);
            }
            prev_dash = true;
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

// ── ProjectPaths ─────────────────────────────────────────────────────────────

/// All filesystem locations relevant to a single agentd project.
///
/// - `store` is the central per-project store under `$XDG_DATA_HOME` (or
///   `~/.local/share`) — agentd writes transcripts, run journals, and learned
///   facts here.
/// - `local` is the project-local `.oxide/` directory inside `cwd` — users
///   can drop project-scoped skills, MCP config, or a `config.toml` here.
///   If `.oxide/` does not exist but `.oxidemx/` does, `.oxidemx/` is used
///   for back-compat; new projects get `.oxide/`.
#[derive(Clone)]
pub struct ProjectPaths {
    pub key: ProjectKey,
    pub cwd: PathBuf,
    /// `~/.local/share/oxidemx/projects/<key>`
    pub store: PathBuf,
    /// `<cwd>/.oxide` (preferred) or `<cwd>/.oxidemx` (back-compat)
    pub local: PathBuf,
}

impl ProjectPaths {
    /// Resolve all paths for the project rooted at `cwd`.
    ///
    /// `local` is set to `<cwd>/.oxide` if that directory exists, else
    /// `<cwd>/.oxidemx` (back-compat for existing projects), else the
    /// canonical new name `<cwd>/.oxide` (for projects not yet created).
    pub fn resolve(cwd: &Path) -> ProjectPaths {
        let key = ProjectKey::from_cwd(cwd);
        let store = data_dir().join("oxidemx").join("projects").join(key.as_str());
        let preferred = cwd.join(".oxide");
        let compat = cwd.join(".oxidemx");
        let local = if preferred.exists() {
            preferred
        } else if compat.exists() {
            compat
        } else {
            preferred // default to new name when neither exists yet
        };
        ProjectPaths {
            key,
            cwd: cwd.to_path_buf(),
            store,
            local,
        }
    }

    // ── Store sub-paths ───────────────────────────────────────────────────

    /// Directory where conversation transcripts are persisted.
    /// `<store>/transcripts/`
    pub fn transcripts_dir(&self) -> PathBuf {
        self.store.join("transcripts")
    }

    /// Directory where per-run outputs / artefacts are stored.
    /// `<store>/runs/`
    pub fn runs_dir(&self) -> PathBuf {
        self.store.join("runs")
    }

    /// Directory where conversation attachment blobs are persisted.
    /// `<store>/attachments/` — the [`crate::attachments::AttachmentStore`] root.
    /// Mirrors [`Self::transcripts_dir`] so attachments live beside the
    /// transcripts that reference them.
    pub fn attachments_dir(&self) -> PathBuf {
        self.store.join("attachments")
    }

    /// Path to the rolling journal JSONL file.
    /// `<store>/journal.jsonl`
    pub fn journal_path(&self) -> PathBuf {
        self.store.join("journal.jsonl")
    }

    /// Directory where learned project facts (from the agent's memory) live.
    /// `<store>/learned/`
    pub fn learned_dir(&self) -> PathBuf {
        self.store.join("learned")
    }

    /// Path to the project-store metadata file (key, cwd, created-at, …).
    /// `<store>/meta.json`
    pub fn meta_path(&self) -> PathBuf {
        self.store.join("meta.json")
    }

    // ── Merge helpers ────────────────────────────────────────────────────

    /// Merged skill-scan roots: global roots first (from
    /// `oxidemx_agent_core::skills::global_skill_roots()`), then
    /// `<cwd>/.claude/skills`, then `<local>/skills` last — so a
    /// project-local skill with the same name as a global one wins.
    ///
    /// `local` is `.oxide/` (preferred) or `.oxidemx/` (back-compat).
    pub fn merged_skill_roots(&self) -> Vec<PathBuf> {
        let mut roots = oxidemx_agent_core::skills::global_skill_roots();
        // Project-local Claude-compatible root (mirrors Claude Code's layout).
        roots.push(self.cwd.join(".claude").join("skills"));
        // Project-local .oxide root (wins on collision).
        roots.push(self.local.join("skills"));
        roots
    }

    /// Merged MCP server configuration: global `~/.config/oxidemx/mcp.toml`
    /// base, project-local `<local>/mcp.toml` servers override by name.
    ///
    /// `local` is `.oxide/` (preferred) or `.oxidemx/` (back-compat).
    /// Both files are expected to be TOML tables with a top-level `servers`
    /// key mapping server names to their config objects. Missing files are
    /// silently skipped. Returns a JSON `Value` so callers don't need to
    /// depend on `toml` directly.
    pub fn merged_mcp(&self) -> serde_json::Value {
        let global = read_mcp_toml(&global_mcp_path());
        let local = read_mcp_toml(&self.local.join("mcp.toml"));
        merge_mcp(global, local)
    }

    /// Project-local `<local>/config.toml`, if it exists and parses cleanly.
    pub fn project_config(&self) -> Option<toml::Value> {
        let path = self.local.join("config.toml");
        let src = std::fs::read_to_string(path).ok()?;
        src.parse::<toml::Value>().ok()
    }
}

// ── Data-dir helper ───────────────────────────────────────────────────────────

/// `$XDG_DATA_HOME` if set and absolute, else `$HOME/.local/share`.
fn data_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            return p;
        }
    }
    // Fall back to $HOME/.local/share.
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local").join("share"))
        .unwrap_or_else(|| PathBuf::from(".local/share"))
}

// ── MCP merge helpers ─────────────────────────────────────────────────────────

fn global_mcp_path() -> PathBuf {
    // Mirror oxidemx-shared's config-dir logic to avoid adding a dep.
    let config_base = if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            p
        } else {
            default_home_config()
        }
    } else {
        default_home_config()
    };
    config_base.join("oxidemx").join("mcp.toml")
}

fn default_home_config() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".config"))
        .unwrap_or_else(|| PathBuf::from(".config"))
}

/// Read an MCP TOML file and return its `servers` table as a JSON object.
/// Returns an empty object on any error.
fn read_mcp_toml(path: &Path) -> serde_json::Value {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return serde_json::Value::Object(Default::default()),
    };
    let toml_val: toml::Value = match src.parse() {
        Ok(v) => v,
        Err(_) => return serde_json::Value::Object(Default::default()),
    };
    // Extract the `servers` table; fall back to the whole doc if absent.
    let servers = toml_val
        .get("servers")
        .cloned()
        .unwrap_or(toml::Value::Table(Default::default()));
    toml_value_to_json(servers)
}

/// Merge project MCP servers on top of global ones (project wins by name).
fn merge_mcp(mut global: serde_json::Value, local: serde_json::Value) -> serde_json::Value {
    match (&mut global, local) {
        (serde_json::Value::Object(g), serde_json::Value::Object(l)) => {
            for (k, v) in l {
                g.insert(k, v);
            }
            global
        }
        _ => global,
    }
}

/// Convert a `toml::Value` to a `serde_json::Value` (lossy for datetime, but
/// MCP configs don't use TOML datetimes).
fn toml_value_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(t) => {
            let map = t
                .into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_sanitize_replaces_dots_and_spaces() {
        let s = sanitize_slug("my.project name");
        assert!(!s.contains('.'), "dots should be replaced");
        assert!(!s.contains(' '), "spaces should be replaced");
        assert!(s.contains("my"), "prefix kept");
    }

    #[test]
    fn slug_sanitize_collapses_dashes() {
        let s = sanitize_slug("foo...bar");
        assert_eq!(s, "foo-bar");
    }

    #[test]
    fn slug_sanitize_fallback_on_empty() {
        // A slug that reduces to nothing should return the fallback.
        let s = sanitize_slug("...");
        assert_eq!(s, "project");
    }

    #[test]
    fn store_contains_projects_segment() {
        let d = tempfile::tempdir().unwrap();
        let p = ProjectPaths::resolve(d.path());
        assert!(p.store.components().any(|c| c.as_os_str() == "projects"));
    }

    #[test]
    fn store_contains_key_segment() {
        let d = tempfile::tempdir().unwrap();
        let p = ProjectPaths::resolve(d.path());
        let key_str = p.key.as_str().to_string();
        assert!(p.store.ends_with(&key_str));
    }

    #[test]
    fn sub_paths_hang_off_store() {
        let d = tempfile::tempdir().unwrap();
        let p = ProjectPaths::resolve(d.path());
        assert!(p.transcripts_dir().starts_with(&p.store));
        assert!(p.runs_dir().starts_with(&p.store));
        assert!(p.journal_path().starts_with(&p.store));
        assert!(p.learned_dir().starts_with(&p.store));
        assert!(p.meta_path().starts_with(&p.store));
    }

    #[test]
    fn project_config_none_when_absent() {
        let d = tempfile::tempdir().unwrap();
        let p = ProjectPaths::resolve(d.path());
        assert!(p.project_config().is_none());
    }

    #[test]
    fn project_config_parses_when_present() {
        let d = tempfile::tempdir().unwrap();
        // Use back-compat .oxidemx to verify that resolve() still picks it up.
        std::fs::create_dir_all(d.path().join(".oxidemx")).unwrap();
        std::fs::write(d.path().join(".oxidemx/config.toml"), "name = \"test\"\n").unwrap();
        let p = ProjectPaths::resolve(d.path());
        // resolve() should choose .oxidemx (back-compat, since .oxide absent).
        assert!(p.local.ends_with(".oxidemx"), "back-compat path selected");
        let cfg = p.project_config().expect("should parse");
        assert_eq!(
            cfg.get("name").and_then(|v| v.as_str()),
            Some("test")
        );
    }

    #[test]
    fn merged_mcp_project_overrides_global() {
        // Write a temp "global" via XDG_CONFIG_HOME trick — we test the merge
        // logic directly using the internal helpers instead.
        let global = serde_json::json!({"a": 1, "b": 2});
        let local = serde_json::json!({"b": 99, "c": 3});
        let merged = merge_mcp(global, local);
        assert_eq!(merged["a"], 1);
        assert_eq!(merged["b"], 99); // project wins
        assert_eq!(merged["c"], 3);
    }

    #[test]
    fn merge_prefers_project_local() {
        let d = tempfile::tempdir().unwrap();
        // Create .oxide/skills so that resolve() picks .oxide (preferred).
        std::fs::create_dir_all(d.path().join(".oxide/skills")).unwrap();
        let p = ProjectPaths::resolve(d.path());
        let roots = p.merged_skill_roots();
        // Verify the skills roots are in the expected order, with both Claude-compatible
        // and oxide-local roots present, and .oxide/skills last.
        assert!(roots.iter().any(|r| r.ends_with(".claude/skills")));
        assert!(roots.last().is_some_and(|r| r.ends_with(".oxide/skills")));
    }
}
