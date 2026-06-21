//! Walk-up config resolver — `.oxide` / `.oxidemx` (back-compat).
//!
//! Mirrors the EditorConfig / Claude Code `.claude` walk-up pattern:
//! from a `working_dir` we walk up toward the filesystem root collecting
//! every `.oxide` (or `.oxidemx`) directory.  A layer whose `settings.toml`
//! contains `root = true` stops the walk (sentinel).  If no sentinel was
//! reached we also layer the `user_global` directory at the bottom.
//!
//! Merging is farthest-first → closest-wins for scalars; known list keys
//! (see `LIST_KEYS`) are unioned (concat + dedup, closest order first).
//!
//! Public surface:
//! - [`resolve_oxide_config`] — main entry point
//! - [`ResolvedConfig`] — merged result
//! - [`merge_toml`] — pure merge helper (exposed for tests / downstream)

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Known list keys (dotted paths whose arrays are unioned across layers) ────

/// Dotted key paths that are merged by union (concat+dedup) rather than
/// scalar-replace.  All other arrays/scalars follow closest-wins.
const LIST_KEYS: &[&str] = &[
    "permissions.allow",
    "permissions.deny",
    "skills",
    "mcp",
    "flows",
    "agents",
];

// ── ResolvedConfig ────────────────────────────────────────────────────────────

/// The merged result of layering zero or more `.oxide` / `.oxidemx` dirs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedConfig {
    /// Deep-merged settings (scalar closest-wins; list keys unioned).
    pub settings: toml::Value,
    /// Per-layer `<oxide>/skills` directories, closest-first.
    pub skill_roots: Vec<PathBuf>,
    /// Per-layer `<oxide>/mcp.toml` paths, closest-first.
    pub mcp_files: Vec<PathBuf>,
    /// Per-layer `<oxide>/OXIDE.md` paths, closest-first.
    pub instruction_files: Vec<PathBuf>,
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Resolve the merged `.oxide` config for `working_dir`.
///
/// Walk from `working_dir` up to the filesystem root.  At each directory look
/// for `<dir>/.oxide/` (preferred) or `<dir>/.oxidemx/` (back-compat).  Parse
/// `<oxide>/settings.toml` (if present) and collect auxiliary paths.
///
/// Stop collecting after the first layer whose settings contains `root = true`
/// (EditorConfig sentinel).  If no sentinel is encountered also include
/// `user_global` (treated as an implicit bottom layer).
///
/// Layers are applied farthest-first; the closest dir's settings win on
/// scalar conflicts.  List keys (see [`LIST_KEYS`]) are unioned.
pub fn resolve_oxide_config(working_dir: &Path, user_global: &Path) -> ResolvedConfig {
    // Walk up, collecting layers farthest-last so we can reverse at the end.
    let mut layers: Vec<(toml::Value, PathBuf)> = Vec::new(); // (settings, oxide_dir)
    let mut hit_root = false;

    let mut current = working_dir.to_path_buf();
    loop {
        if let Some(oxide_dir) = find_oxide_dir(&current) {
            let settings = read_settings_toml(&oxide_dir);
            let is_root = settings
                .get("root")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            layers.push((settings, oxide_dir));
            if is_root {
                hit_root = true;
                break;
            }
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }

    // If no root sentinel was hit, also layer the user-global dir at the bottom.
    if !hit_root {
        let settings = read_settings_toml(user_global);
        layers.push((settings, user_global.to_path_buf()));
    }

    // Reverse so farthest is first (index 0) and closest is last.
    layers.reverse();

    // ── Collect closest-first auxiliary paths ─────────────────────────────
    // layers is now farthest→closest; we want closest-first for the Vecs.
    let mut skill_roots: Vec<PathBuf> = Vec::new();
    let mut mcp_files: Vec<PathBuf> = Vec::new();
    let mut instruction_files: Vec<PathBuf> = Vec::new();

    for (_, oxide_dir) in layers.iter().rev() {
        let skills = oxide_dir.join("skills");
        if skills.exists() {
            skill_roots.push(skills);
        }
        let mcp = oxide_dir.join("mcp.toml");
        if mcp.exists() {
            mcp_files.push(mcp);
        }
        let md = oxide_dir.join("OXIDE.md");
        if md.exists() {
            instruction_files.push(md);
        }
    }

    // ── Merge settings farthest→closest ──────────────────────────────────
    let mut merged = toml::Value::Table(toml::map::Map::new());
    for (settings, _) in &layers {
        merged = merge_toml(merged, settings.clone(), LIST_KEYS);
    }

    ResolvedConfig {
        settings: merged,
        skill_roots,
        mcp_files,
        instruction_files,
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Return `<dir>/.oxide` if it exists, else `<dir>/.oxidemx` if it exists,
/// else `None`.
fn find_oxide_dir(dir: &Path) -> Option<PathBuf> {
    let preferred = dir.join(".oxide");
    if preferred.is_dir() {
        return Some(preferred);
    }
    let compat = dir.join(".oxidemx");
    if compat.is_dir() {
        return Some(compat);
    }
    None
}

/// Parse `<oxide_dir>/settings.toml` and return a `toml::Value::Table`.
/// Returns an empty table on any error or if the file is absent.
fn read_settings_toml(oxide_dir: &Path) -> toml::Value {
    let path = oxide_dir.join("settings.toml");
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return toml::Value::Table(toml::map::Map::new()),
    };
    src.parse::<toml::Value>()
        .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()))
}

// ── merge_toml ────────────────────────────────────────────────────────────────

/// Deep-merge two TOML values.
///
/// - Scalars / non-list arrays: `over` (closer) replaces `base`.
/// - Arrays under a **known list key** (dotted path in `list_keys`): union
///   (`over` items first, then `base` items not already present, dedup
///   preserving order).  The caller applies layers farthest→closest, so
///   each successive `merge_toml` call places the closer layer's entries
///   before the accumulated farther entries — yielding a globally
///   closest-first union after all layers are merged.
/// - Tables: recurse.
///
/// `path` is the dotted key path accumulated during recursion (used to
/// check membership in `list_keys`).
pub fn merge_toml(base: toml::Value, over: toml::Value, list_keys: &[&str]) -> toml::Value {
    merge_toml_at(base, over, list_keys, "")
}

fn merge_toml_at(
    base: toml::Value,
    over: toml::Value,
    list_keys: &[&str],
    path: &str,
) -> toml::Value {
    match (base, over) {
        (toml::Value::Table(mut b), toml::Value::Table(o)) => {
            for (k, v_over) in o {
                let child_path = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                let merged_child = if let Some(v_base) = b.remove(&k) {
                    merge_toml_at(v_base, v_over, list_keys, &child_path)
                } else {
                    v_over
                };
                b.insert(k, merged_child);
            }
            toml::Value::Table(b)
        }
        (toml::Value::Array(base_arr), toml::Value::Array(over_arr)) => {
            if list_keys.contains(&path) {
                // Union: over (closer layer) items first, then base items not
                // already present.  Caller merges farthest→closest, so each
                // call places the closer layer before accumulated farther
                // entries — yielding a globally closest-first union.
                let mut result = over_arr.clone();
                for item in base_arr {
                    if !result.contains(&item) {
                        result.push(item);
                    }
                }
                toml::Value::Array(result)
            } else {
                // Non-list array: closer wins.
                toml::Value::Array(over_arr)
            }
        }
        // Scalar (or type mismatch): closer wins.
        (_, over) => over,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: write a string to a file, creating parent dirs.
    fn write(path: &Path, content: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    // Helper: extract permissions.allow as Vec<String>.
    fn perm_allow(settings: &toml::Value) -> Vec<String> {
        settings
            .get("permissions")
            .and_then(|p| p.get("allow"))
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    // ── Unit test: merge_toml scalar-vs-list behaviour ─────────────────────

    #[test]
    fn merge_toml_scalar_closest_wins_list_is_union() {
        // Two layers: far (base) and close (over).
        let base_src = r#"
model = "flash"
[permissions]
allow = ["a", "b"]
"#;
        let over_src = r#"
model = "pro"
[permissions]
allow = ["b", "c"]
"#;
        let base: toml::Value = base_src.parse().unwrap();
        let over: toml::Value = over_src.parse().unwrap();
        let merged = merge_toml(base, over, LIST_KEYS);

        // Scalar: closer (over) wins.
        assert_eq!(
            merged.get("model").and_then(|v| v.as_str()),
            Some("pro"),
            "closer scalar must win"
        );

        // List union: over (closer) items first, then base-only items (dedup).
        // Expected order: over had ["b","c"], base had ["a","b"].
        // Result: ["b","c"] from over, then "a" from base (b already present).
        let allow = perm_allow(&merged);
        assert!(allow.contains(&"a".to_string()), "base-only item present");
        assert!(allow.contains(&"b".to_string()), "shared item present once");
        assert!(allow.contains(&"c".to_string()), "over-only item present");
        // Dedup: "b" appears exactly once.
        assert_eq!(allow.iter().filter(|&x| x == "b").count(), 1, "no duplicates");
        // Positional: over items come before base-only items (closest-first).
        // "b" (from over) must appear before "a" (base-only).
        let pos_b = allow.iter().position(|x| x == "b").unwrap();
        let pos_a = allow.iter().position(|x| x == "a").unwrap();
        assert!(
            pos_b < pos_a,
            "closer layer's 'b' (pos {pos_b}) must precede farther layer's 'a' (pos {pos_a}); got {allow:?}"
        );
    }

    // ── Integration test: walk-up semantics ────────────────────────────────

    #[test]
    fn walk_up_scalars_closest_win_lists_union_root_stops() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // user-global
        let ug = root.join("global");
        std::fs::create_dir_all(&ug).unwrap();
        write(
            &ug.join("settings.toml"),
            "model = \"flash\"\n[permissions]\nallow=[\"a\"]\n",
        );

        // project root .oxide (root=true)
        let proj = root.join("proj");
        let proj_ox = proj.join(".oxide");
        std::fs::create_dir_all(&proj_ox).unwrap();
        write(
            &proj_ox.join("settings.toml"),
            "root = true\nmodel=\"pro\"\n[permissions]\nallow=[\"b\"]\n",
        );

        // nested dir .oxide
        let sub = proj.join("src");
        let sub_ox = sub.join(".oxide");
        std::fs::create_dir_all(&sub_ox).unwrap();
        write(
            &sub_ox.join("settings.toml"),
            "[permissions]\nallow=[\"c\"]\n",
        );

        let rc = resolve_oxide_config(&sub, &ug);

        // scalar: closest (proj has model=pro; sub doesn't set it) → "pro"
        // (user-global "flash" shadowed by proj because root=true stops walk)
        assert_eq!(
            rc.settings.get("model").and_then(|v| v.as_str()),
            Some("pro")
        );

        // list union of permissions.allow across proj (.oxide root) + sub —
        // but NOT user-global ("a"), because root=true at proj stops the walk
        // before user-global is layered.
        let allow = perm_allow(&rc.settings);
        assert!(
            allow.contains(&"b".to_string()) && allow.contains(&"c".to_string()),
            "proj and sub allow entries must be present"
        );
        assert!(
            !allow.contains(&"a".to_string()),
            "root=true must stop the user-global underlay"
        );
        // Positional (closest-first): sub's "c" (closer layer) must come before
        // proj's "b" (farther layer).
        let pos_c = allow.iter().position(|x| x == "c").unwrap();
        let pos_b = allow.iter().position(|x| x == "b").unwrap();
        assert_eq!(
            allow,
            vec!["c".to_string(), "b".to_string()],
            "closest-first order required: sub 'c' (pos {pos_c}) before proj 'b' (pos {pos_b})"
        );
    }
}
