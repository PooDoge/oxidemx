//! agentd — persistent, project-aware agent host (org.oxidemx.Agent).
#![forbid(unsafe_code)]

pub mod error;
pub mod projects;

#[cfg(test)]
mod tests {
    use crate::projects::{ProjectKey, ProjectPaths};

    #[test]
    fn project_key_is_stable_and_distinct() {
        let d = tempfile::tempdir().unwrap();
        let a = ProjectKey::from_cwd(d.path());
        let b = ProjectKey::from_cwd(d.path());
        assert_eq!(a, b); // stable
        let d2 = tempfile::tempdir().unwrap();
        assert_ne!(a, ProjectKey::from_cwd(d2.path())); // distinct cwds differ
        assert!(a.as_str().contains('-')); // slug-hash shape
    }

    #[test]
    fn merge_prefers_project_local() {
        let proj = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proj.path().join(".oxidemx/skills")).unwrap();
        let p = ProjectPaths::resolve(proj.path());
        let roots = p.merged_skill_roots();
        // project-local skills dir is present and is LAST (wins on name collision)
        assert_eq!(
            roots.last().map(|r| r.ends_with(".oxidemx/skills")),
            Some(true)
        );
        assert!(p.store.components().any(|c| c.as_os_str() == "projects"));
    }
}
