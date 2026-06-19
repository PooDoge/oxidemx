//! Atomic on-disk task ledger storage.
//!
//! Persists `TaskManifest` to JSON files with crash-safe atomic writes
//! using write-to-temp + rename semantics.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::LedgerError;
use crate::model::TaskId;
use crate::TaskManifest;

/// Atomic on-disk store for task manifests.
///
/// All writes are atomic: data is written to a temporary file, then
/// atomically renamed to its final location. A crash mid-write never
/// corrupts the persisted manifest.
#[derive(Debug)]
pub struct TaskLedger {
    /// Base directory for all task storage.
    base: PathBuf,
}

impl TaskLedger {
    /// Create a new `TaskLedger` rooted at the given base directory.
    ///
    /// The base directory is created if it does not exist.
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
        }
    }

    /// Return the task directory for a given task ID.
    ///
    /// Format: `<base>/tasks/<task_id>`.
    pub fn task_dir(&self, task_id: &TaskId) -> PathBuf {
        self.base.join("tasks").join(task_id.as_str())
    }

    /// Create a new task, atomically writing its manifest and creating the artifacts directory.
    ///
    /// The manifest is written to `<task_dir>/manifest.json`.
    /// The `<task_dir>/artifacts` directory is created (empty).
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if:
    /// - Directory creation fails.
    /// - JSON serialization fails.
    /// - File write/rename fails.
    pub fn create(&self, manifest: &TaskManifest) -> Result<(), LedgerError> {
        let task_dir = self.task_dir(&manifest.task_id);

        // Ensure task directory exists
        fs::create_dir_all(&task_dir)?;

        // Ensure artifacts directory exists
        fs::create_dir_all(task_dir.join("artifacts"))?;

        // Atomically write manifest
        self._write_manifest(&task_dir, manifest)?;

        Ok(())
    }

    /// Load a task manifest from disk by task ID.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if:
    /// - The manifest file does not exist.
    /// - JSON deserialization fails.
    /// - File read fails.
    pub fn load(&self, task_id: &TaskId) -> Result<TaskManifest, LedgerError> {
        let task_dir = self.task_dir(task_id);
        let manifest_path = task_dir.join("manifest.json");

        if !manifest_path.exists() {
            return Err(LedgerError::NotFound(format!(
                "manifest not found: {}",
                manifest_path.display()
            )));
        }

        let json = fs::read_to_string(&manifest_path)?;
        let manifest = serde_json::from_str(&json)?;

        Ok(manifest)
    }

    /// Save (overwrite) an existing task manifest atomically.
    ///
    /// The manifest is written to a temporary file, then atomically renamed
    /// to overwrite the existing manifest.json. No .tmp file is left behind
    /// on success.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if:
    /// - JSON serialization fails.
    /// - File write/rename fails.
    pub fn save(&self, manifest: &TaskManifest) -> Result<(), LedgerError> {
        let task_dir = self.task_dir(&manifest.task_id);
        self._write_manifest(&task_dir, manifest)?;
        Ok(())
    }

    /// Internal helper: atomically write manifest to task directory.
    ///
    /// Writes to `<task_dir>/manifest.json.tmp`, then renames to
    /// `<task_dir>/manifest.json`. The rename is atomic on the same filesystem,
    /// so a crash mid-write never leaves a corrupt manifest.json.
    fn _write_manifest(&self, task_dir: &Path, manifest: &TaskManifest) -> Result<(), LedgerError> {
        let manifest_path = task_dir.join("manifest.json");
        let tmp_path = task_dir.join("manifest.json.tmp");

        // Serialize to JSON
        let json = serde_json::to_string_pretty(manifest)?;

        // Write to temporary file
        fs::write(&tmp_path, json)?;

        // Atomically rename temp → final
        fs::rename(&tmp_path, &manifest_path)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Step, StepStatus};

    #[test]
    fn create_load_save_round_trips_atomically() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        let mut m = TaskManifest::new(TaskId::from_raw("t-1".into()), "g".into());
        m.steps.push(Step::new("a", "first"));
        led.create(&m).unwrap();
        assert!(d.path().join("tasks/t-1/manifest.json").exists());
        assert!(d.path().join("tasks/t-1/artifacts").is_dir());
        let mut loaded = led.load(&TaskId::from_raw("t-1".into())).unwrap();
        assert_eq!(loaded.steps.len(), 1);
        loaded.steps[0].status = StepStatus::Running;
        led.save(&loaded).unwrap();
        // no .tmp left behind
        assert!(!d.path().join("tasks/t-1/manifest.json.tmp").exists());
        assert_eq!(led.load(&TaskId::from_raw("t-1".into())).unwrap().steps[0].status, StepStatus::Running);
    }
}
