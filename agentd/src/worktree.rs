//! Git worktree seam: create/remove per-conversation worktrees under `.oxide/worktrees/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::Worktree;

// ---------------------------------------------------------------------------
// Trait seam
// ---------------------------------------------------------------------------

pub trait Git: Send + Sync {
    fn worktree_add(
        &self,
        repo: &Path,
        dst: &Path,
        branch: &str,
        base_ref: &str,
    ) -> Result<(), String>;

    fn worktree_remove(&self, repo: &Path, dst: &Path) -> Result<(), String>;

    fn is_clean(&self, worktree: &Path) -> Result<bool, String>;
}

// ---------------------------------------------------------------------------
// Real implementation (shells git)
// ---------------------------------------------------------------------------

pub struct RealGit;

fn resolve_base(base_ref: &str) -> &str {
    match base_ref {
        "head" => "HEAD",
        "fresh" => "origin/HEAD",
        other => other,
    }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd
        .output()
        .map_err(|e| format!("spawn error: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "git exited {}: {}",
            out.status,
            stderr.trim()
        ))
    }
}

impl Git for RealGit {
    fn worktree_add(
        &self,
        repo: &Path,
        dst: &Path,
        branch: &str,
        base_ref: &str,
    ) -> Result<(), String> {
        let base = resolve_base(base_ref);
        run(Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("worktree")
            .arg("add")
            .arg("-b")
            .arg(branch)
            .arg(dst)
            .arg(base))
    }

    fn worktree_remove(&self, repo: &Path, dst: &Path) -> Result<(), String> {
        run(Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("worktree")
            .arg("remove")
            .arg(dst))
    }

    fn is_clean(&self, worktree: &Path) -> Result<bool, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(worktree)
            .arg("status")
            .arg("--porcelain")
            .output()
            .map_err(|e| format!("spawn error: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "git status exited {}: {}",
                out.status,
                stderr.trim()
            ));
        }
        Ok(out.stdout.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Copy literal-path entries from `repo/.oxideinclude` into `dst`.
///
/// Blank, comment (`#`), glob (`*`/`?`/`[`), absolute, and parent-dir (`..`)
/// entries are silently skipped.  Only paths that exist in `repo` are copied.
fn copy_oxideinclude(repo: &Path, dst: &Path) -> Result<(), String> {
    let include_path = repo.join(".oxideinclude");
    if !include_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&include_path)
        .map_err(|e| format!("read .oxideinclude: {e}"))?;
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip blank, comment, and glob lines (anything with * ? [ ])
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.contains('*')
            || trimmed.contains('?')
            || trimmed.contains('[')
        {
            continue;
        }
        // Guard: reject absolute paths and any path that escapes the repo
        // via parent-dir components ("../secret", "/etc/passwd", etc.).
        let p = std::path::Path::new(trimmed);
        if p.is_absolute()
            || p.components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            continue; // .oxideinclude entries must stay inside the repo
        }
        let src = repo.join(trimmed);
        if src.exists() {
            let dest = dst.join(trimmed);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            std::fs::copy(&src, &dest)
                .map_err(|e| format!("copy {trimmed}: {e}"))?;
        }
    }
    Ok(())
}

/// Create a per-conversation worktree at `repo/.oxide/worktrees/<name>`.
///
/// Copies any literal-path entries from `repo/.oxideinclude` into the new
/// worktree at the same relative path.  Comment/blank/glob lines are skipped.
pub fn create_conversation_worktree(
    git: &dyn Git,
    repo: &Path,
    name: &str,
    base_ref: &str,
) -> Result<Worktree, String> {
    let dst: PathBuf = repo.join(".oxide").join("worktrees").join(name);
    let branch = format!("worktree-{name}");

    git.worktree_add(repo, &dst, &branch, base_ref)?;
    copy_oxideinclude(repo, &dst)?;

    Ok(Worktree {
        path: dst,
        branch,
        base_ref: base_ref.into(),
    })
}

/// Remove the worktree only if it has no uncommitted changes.
///
/// Returns `Ok(true)` if removed, `Ok(false)` if dirty (not removed).
pub fn remove_if_unchanged(
    git: &dyn Git,
    wt: &Worktree,
    repo: &Path,
) -> Result<bool, String> {
    if git.is_clean(&wt.path)? {
        git.worktree_remove(repo, &wt.path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Test mock (pub(crate) so interface tests can inject it)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) struct MockGit {
    pub clean: bool,
    pub calls: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl Git for MockGit {
    fn worktree_add(
        &self,
        _r: &Path,
        dst: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("add {} {} {}", dst.display(), branch, base));
        Ok(())
    }

    fn worktree_remove(&self, _r: &Path, dst: &Path) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("remove {}", dst.display()));
        Ok(())
    }

    fn is_clean(&self, _w: &Path) -> Result<bool, String> {
        Ok(self.clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_uses_oxide_worktrees_path_and_branch() {
        let g = MockGit {
            clean: true,
            calls: Default::default(),
        };
        let wt = create_conversation_worktree(
            &g,
            std::path::Path::new("/repo"),
            "feat-x",
            "head",
        )
        .unwrap();
        assert!(wt.path.ends_with(".oxide/worktrees/feat-x"));
        assert_eq!(wt.branch, "worktree-feat-x");
        assert_eq!(wt.base_ref, "head");
    }

    #[test]
    fn remove_if_unchanged_only_removes_clean() {
        let dirty = MockGit {
            clean: false,
            calls: Default::default(),
        };
        let wt = Worktree {
            path: "/repo/.oxide/worktrees/x".into(),
            branch: "worktree-x".into(),
            base_ref: "head".into(),
        };
        assert!(
            !remove_if_unchanged(&dirty, &wt, std::path::Path::new("/repo")).unwrap()
        );
        assert!(dirty.calls.lock().unwrap().is_empty());
    }

    /// `.oxideinclude` path-escape guard: safe relative entries are copied;
    /// absolute and parent-dir entries are rejected and never escape the repo.
    #[test]
    fn oxideinclude_rejects_absolute_and_parent_dir_paths() {
        use std::fs;

        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let dst = repo.join(".oxide").join("worktrees").join("t");
        fs::create_dir_all(&dst).unwrap();

        // Safe file that should be copied.
        fs::write(repo.join("keep.txt"), b"safe").unwrap();

        // Sentinel above the repo root — must NOT be touched.
        let sentinel = tmp.path().join("escape.txt");
        fs::write(&sentinel, b"secret").unwrap();

        // .oxideinclude: one safe line, one absolute, one parent-dir.
        let include = "keep.txt\n/etc/hostname\n../escape.txt\n";
        fs::write(repo.join(".oxideinclude"), include.as_bytes()).unwrap();

        copy_oxideinclude(&repo, &dst).unwrap();

        // Safe entry copied.
        assert!(
            dst.join("keep.txt").exists(),
            "keep.txt should have been copied into the worktree"
        );

        // Absolute and parent-dir entries must NOT have been acted on.
        assert!(
            !dst.join("etc").join("hostname").exists(),
            "/etc/hostname must not appear in the worktree"
        );
        assert!(
            !dst.join("escape.txt").exists(),
            "../escape.txt must not appear in the worktree"
        );

        // The sentinel file one level above the repo must be untouched.
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            b"secret",
            "sentinel file above repo must not be modified"
        );
    }
}
