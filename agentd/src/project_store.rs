//! Persisted registry of Projects — backed by `<store_base>/projects.json`.
//!
//! Each public method re-reads and re-writes the file on every call (the file
//! is tiny; stateless design keeps the store safe for multi-process access).
use std::path::PathBuf;

use tracing::warn;

use crate::model::{Project, ProjectId, PERSONAL_PROJECT_ID};

pub struct ProjectStore {
    store_base: PathBuf,
}

impl ProjectStore {
    pub fn new(store_base: PathBuf) -> Self {
        Self { store_base }
    }

    // ------------------------------------------------------------------ load/save

    fn path(&self) -> PathBuf {
        self.store_base.join("projects.json")
    }

    fn load(&self) -> Vec<Project> {
        let p = self.path();
        match std::fs::read_to_string(&p) {
            Ok(txt) => serde_json::from_str(&txt).unwrap_or_else(|e| {
                warn!("ProjectStore: failed to parse {}: {e}", p.display());
                vec![]
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => {
                warn!("ProjectStore: failed to read {}: {e}", p.display());
                vec![]
            }
        }
    }

    pub fn save(&self, projects: &[Project]) {
        let p = self.path();
        let tmp = self.store_base.join("projects.json.tmp");
        let json = match serde_json::to_string_pretty(projects) {
            Ok(j) => j,
            Err(e) => { warn!("ProjectStore: serialize failed: {e}"); return; }
        };
        if let Err(e) = std::fs::create_dir_all(&self.store_base) {
            warn!("ProjectStore: mkdir failed: {e}");
            return;
        }
        if let Err(e) = std::fs::write(&tmp, &json) {
            warn!("ProjectStore: write tmp failed: {e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &p) {
            warn!("ProjectStore: rename failed: {e}");
        }
    }

    // ------------------------------------------------------------------ public API

    /// Inserts + saves the Personal project if absent; returns it.
    pub fn ensure_personal(&self) -> Project {
        let mut projects = self.load();
        if let Some(p) = projects.iter().find(|p| p.id.as_str() == PERSONAL_PROJECT_ID) {
            return p.clone();
        }
        let personal = Project {
            id: ProjectId::from(PERSONAL_PROJECT_ID),
            name: "Personal".to_string(),
            default_working_dir: PathBuf::new(),
            created_at: now_ms(),
        };
        projects.push(personal.clone());
        self.save(&projects);
        personal
    }

    /// Creates a new project, appends it, saves, and returns it.
    pub fn create(&self, name: &str, default_working_dir: PathBuf) -> Project {
        let mut projects = self.load();
        let base_id = format!("proj-{}", now_ms());
        let id = unique_id(&base_id, &projects);
        let project = Project {
            id: ProjectId::from(id),
            name: name.to_string(),
            default_working_dir,
            created_at: now_ms(),
        };
        projects.push(project.clone());
        self.save(&projects);
        project
    }

    /// Returns the project with the given id, or `None` if not found.
    pub fn get(&self, id: &ProjectId) -> Option<Project> {
        self.load().into_iter().find(|p| &p.id == id)
    }

    /// Returns all projects, reading from disk.
    pub fn list(&self) -> Vec<Project> {
        self.load()
    }
}

// ------------------------------------------------------------------ helpers

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Appends a counter suffix until the id is not already taken.
fn unique_id(base: &str, projects: &[Project]) -> String {
    if !projects.iter().any(|p| p.id.as_str() == base) {
        return base.to_string();
    }
    let mut counter = 1u32;
    loop {
        let candidate = format!("{base}-{counter}");
        if !projects.iter().any(|p| p.id.as_str() == candidate) {
            return candidate;
        }
        counter += 1;
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_personal_then_create_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(tmp.path().to_path_buf());
        let personal = store.ensure_personal();
        assert_eq!(personal.id.as_str(), "personal");
        let p = store.create("My Repo", std::path::PathBuf::from("/home/x/repo"));
        assert_ne!(p.id.as_str(), "personal");
        // reload from disk → both present
        let store2 = ProjectStore::new(tmp.path().to_path_buf());
        let ids: Vec<String> = store2.list().iter().map(|p| p.id.to_string()).collect();
        assert!(ids.contains(&"personal".to_string()));
        assert!(ids.iter().any(|i| i.starts_with("proj-")));
    }
}
