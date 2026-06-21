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

    // Copy .oxideinclude literal paths if the file exists.
    let include_path = repo.join(".oxideinclude");
    if include_path.exists() {
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
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockGit {
        clean: bool,
        calls: std::sync::Mutex<Vec<String>>,
    }

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
        assert_eq!(
            remove_if_unchanged(&dirty, &wt, std::path::Path::new("/repo")).unwrap(),
            false
        );
        assert!(dirty.calls.lock().unwrap().is_empty());
    }
}
