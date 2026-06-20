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
/// 4. If the path exists on disk and is a symlink, its resolved target must
///    also remain under `cwd`. A tracked symlink escaping `cwd` is rejected.
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
    if !tracked {
        return Some(false);
    }

    // ── 4. Symlink escape guard ───────────────────────────────────────────────
    // If the path exists AND is a symlink, resolve the link target and verify
    // it also stays under cwd. A tracked symlink pointing outside cwd must not
    // be auto-approvable.
    if let Ok(meta) = std::fs::symlink_metadata(&abs) {
        if meta.file_type().is_symlink() {
            // Read the raw link target (may be relative or absolute).
            let link_target = std::fs::read_link(&abs).ok()?;
            // Resolve relative targets relative to the symlink's parent directory.
            let resolved_target = if link_target.is_absolute() {
                link_target
            } else {
                let parent = abs.parent()?;
                parent.join(&link_target)
            };
            // Lexically normalise the resolved target (no fs access, fail-safe).
            let normalised_target = normalise_path(&resolved_target)?;
            // The target must still be under cwd.
            if normalised_target.strip_prefix(cwd).is_err() {
                return Some(false);
            }
        }
    }
    // If the path doesn't exist yet, there's no symlink to follow — proceed.

    Some(true)
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

    // Re-use the commit helper from lib.rs tests.
    use crate::tests::commit_file;

    // ── original tests (must stay passing) ───────────────────────────────────

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

    // ── Fix 3: symlink escape guard ───────────────────────────────────────────

    /// A git-tracked symlink whose target escapes cwd must return false.
    ///
    /// If symlink creation is not supported on the test filesystem (e.g. some
    /// CI environments), the symlink_metadata call inside classify_path will
    /// not see a symlink and will treat the path as a normal file that happens
    /// not to exist — returning false at the git-tracked check anyway (the
    /// target file is not committed). We mark the test with a comment rather
    /// than skipping it so it remains visible.
    #[test]
    fn symlink_escaping_cwd_denied() {
        let d = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(d.path()).unwrap();

        // Create a symlink inside the repo that points outside cwd.
        let link_path = d.path().join("evil_link");
        // If symlink creation fails (unsupported FS), skip gracefully.
        match std::os::unix::fs::symlink("/etc/passwd", &link_path) {
            Err(_) => {
                // Cannot create symlink on this filesystem — test is vacuously
                // satisfied because classify_path will return false anyway
                // (the path is not git-tracked).
                return;
            }
            Ok(_) => {}
        }

        // Stage and commit the symlink so it passes the git-tracked check.
        commit_file(&repo, "evil_link");

        // The symlink IS tracked but its target escapes cwd → must return false.
        assert!(
            !classify_path(d.path(), "evil_link"),
            "symlink pointing outside cwd should not be auto-approvable"
        );
    }

    /// A git-tracked symlink whose target stays inside cwd should return true.
    #[test]
    fn symlink_within_cwd_allowed() {
        let d = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(d.path()).unwrap();

        // Create the real target file.
        std::fs::write(d.path().join("real.rs"), "fn main() {}").unwrap();
        commit_file(&repo, "real.rs");

        // Create a symlink to it (relative target, stays within cwd).
        let link_path = d.path().join("alias_link");
        match std::os::unix::fs::symlink("real.rs", &link_path) {
            Err(_) => return, // symlinks not supported — skip
            Ok(_) => {}
        }
        commit_file(&repo, "alias_link");

        assert!(
            classify_path(d.path(), "alias_link"),
            "symlink within cwd to a tracked file should be auto-approvable"
        );
    }
}
