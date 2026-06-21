//! agentd — persistent, project-aware agent host (org.oxidemx.Agent).
#![forbid(unsafe_code)]

pub mod agent;
pub mod error;
pub mod harness;
pub mod host_proxy;
pub mod interface;
pub mod journal;
pub mod model;
pub mod models;
pub mod projects;
pub mod run_bridge;
pub mod run_launcher;
pub mod seams;
pub mod sessions;
pub mod stream_bridge;
pub mod tools;

#[cfg(test)]
mod tests {
    use crate::projects::{ProjectKey, ProjectPaths};
    use crate::sessions::{TranscriptStore, TranscriptTurn};

    #[test]
    fn transcript_round_trips_per_thread() {
        let d = tempfile::tempdir().unwrap();
        let s = TranscriptStore::new(d.path().join("transcripts"));
        s.append("t1", &TranscriptTurn{role:"user".into(), text:"hi".into(), ts:1}).unwrap();
        s.append("t1", &TranscriptTurn{role:"assistant".into(), text:"yo".into(), ts:2}).unwrap();
        s.append("t2", &TranscriptTurn{role:"user".into(), text:"other".into(), ts:3}).unwrap();
        assert_eq!(s.read("t1").unwrap().len(), 2);
        let mut threads = s.list_threads().unwrap(); threads.sort();
        assert_eq!(threads, vec!["t1".to_string(), "t2".to_string()]);
    }

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
