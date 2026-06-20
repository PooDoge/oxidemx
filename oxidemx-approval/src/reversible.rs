//! Path-reversibility classifier: determines whether a file path is git-tracked
//! under the current working directory and therefore safe to auto-approve for
//! file-mutation tools (changes can be recovered via `git checkout`).
//!
//! Every error path returns `false` (fail-safe = not auto-approvable).

use std::path::{Component, Path, PathBuf};

/// Returns `true` iff ALL three conditions hold:
///
/// 1. The resolved path is **under `cwd`** — joining `cwd` + `path` and
///    normalising `.`/`..` components without requiring the path to exist;
///    any escape via `..` is rejected.
/// 2. The path is **git-tracked in HEAD** — `git2::Repository::discover(cwd)`
///    → `repo.head()?.peel_to_tree()?.get_path(rel).is_ok()`.
/// 3. The relative path does **not** start with `.git/`, `.ssh/`, or `.config/`.
///
/// Returns `false` on any error (fail-safe: when in doubt, don't auto-allow).
pub fn classify_path(cwd: &Path, path: &str) -> bool {
    classify_path_inner(cwd, path).unwrap_or(false)
}

fn classify_path_inner(cwd: &Path, path: &str) -> Option<bool> {
    // ── 1. Resolve the path relative to cwd without touching the filesystem ──
    let raw: PathBuf = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd.join(path)
    };

    // Normalise by processing components — reject any `..` that would escape cwd.
    let normalised = normalise_path(&raw)?;

    // Ensure the normalised path is still under cwd.
    let rel = normalised.strip_prefix(cwd).ok()?;

    // rel must be non-empty (reject cwd itself).
    if rel.as_os_str().is_empty() {
        return Some(false);
    }

    // ── 3. Reject protected prefixes ──
    let rel_str = rel.to_str()?;
    if rel_str.starts_with(".git/")
        || rel_str == ".git"
        || rel_str.starts_with(".ssh/")
        || rel_str == ".ssh"
        || rel_str.starts_with(".config/")
        || rel_str == ".config"
    {
        return Some(false);
    }

    // ── 2. Check git-tracked in HEAD ──
    let repo = git2::Repository::discover(cwd).ok()?;
    let head = repo.head().ok()?;
    let tree = head.peel_to_tree().ok()?;

    // Use the path relative to the repo workdir (which may differ from cwd).
    let workdir = repo.workdir()?;
    let abs = cwd.join(rel);
    let rel_to_workdir = abs.strip_prefix(workdir).ok()?;

    let tracked = tree.get_path(rel_to_workdir).is_ok();
    Some(tracked)
}

/// Normalise a path by resolving `.` and `..` components lexically (no fs access).
/// Returns `None` if the path would escape to a parent of the root (impossible in
/// practice, but we fail-safe).
fn normalise_path(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {} // skip `.`
            Component::ParentDir => {
                // Pop one level; if we're already at the root we fail-safe.
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    Some(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_git_prefix_denied() {
        let d = tempfile::tempdir().unwrap();
        // Even without a real repo, the prefix check fires first.
        assert!(!classify_path(d.path(), ".git/config"));
    }

    #[test]
    fn absolute_escape_denied() {
        let d = tempfile::tempdir().unwrap();
        assert!(!classify_path(d.path(), "/etc/passwd"));
    }
}
