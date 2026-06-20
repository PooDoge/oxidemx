//! Atomic on-disk task ledger storage.
//!
//! Persists `TaskManifest` to JSON files with crash-safe atomic writes
//! using write-to-temp + rename semantics.
//!
//! ## Durability guarantee
//!
//! Events are appended to `events.jsonl` BEFORE the manifest is saved.
//! On a process crash between the two writes, `events.jsonl` may record
//! one transition that the manifest does not reflect.  The event log is
//! authoritative; `resume` reconciles any orphaned `Running` steps.
//!
//! Atomicity is provided by write-temp + rename (process-crash-safe).
//! Writes are NOT fsync'd, so power-loss durability is not guaranteed.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use crate::error::LedgerError;
use crate::event::LedgerEvent;
use crate::model::{CompletionPromise, StepStatus, TaskId};
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
    /// Stamps `manifest.created_ts` and `manifest.updated_ts` with `now` before writing.
    /// Appends a `TaskCreated` event to the event log.
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
    pub fn create(&self, manifest: &mut TaskManifest, now: u64) -> Result<(), LedgerError> {
        manifest.created_ts = now;
        manifest.updated_ts = now;

        let task_dir = self.task_dir(&manifest.task_id);

        // Ensure task directory exists
        fs::create_dir_all(&task_dir)?;

        // Ensure artifacts directory exists
        fs::create_dir_all(task_dir.join("artifacts"))?;

        // Atomically write manifest
        self._write_manifest(&task_dir, manifest)?;

        // Append TaskCreated event
        let event = LedgerEvent::TaskCreated {
            task_id: manifest.task_id.as_str().to_string(),
            goal: manifest.goal.clone(),
            ts: now,
        };
        self.append_event(&manifest.task_id, &event)?;

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

    /// Append a single event to the event log.
    ///
    /// Events are appended as JSON lines to `<task_dir>/events.jsonl`.
    /// One JSON object per line, with no commas between entries.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if file operations or JSON serialization fail.
    pub fn append_event(&self, task_id: &TaskId, event: &LedgerEvent) -> Result<(), LedgerError> {
        let task_dir = self.task_dir(task_id);
        fs::create_dir_all(&task_dir)?;

        let events_path = task_dir.join("events.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)?;

        let json = serde_json::to_string(event)?;
        use std::io::Write;
        writeln!(file, "{}", json)?;

        Ok(())
    }

    /// Read all events from the event log.
    ///
    /// Parses the `events.jsonl` file, one JSON object per line.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if:
    /// - The file cannot be read.
    /// - JSON parsing fails for any line.
    pub fn read_events(&self, task_id: &TaskId) -> Result<Vec<LedgerEvent>, LedgerError> {
        let task_dir = self.task_dir(task_id);
        let events_path = task_dir.join("events.jsonl");

        if !events_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&events_path)?;
        let mut events = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let event = serde_json::from_str::<LedgerEvent>(line)?;
            events.push(event);
        }

        Ok(events)
    }

    /// Start a step: transition from Pending → Running.
    ///
    /// Validates the current status, mutates the manifest, appends the event,
    /// and saves the manifest.
    ///
    /// # Errors
    ///
    /// Returns `BadTransition` if the step is not Pending.
    pub fn start_step(
        &self,
        manifest: &mut TaskManifest,
        step_id: &str,
        now: u64,
    ) -> Result<(), LedgerError> {
        let step = manifest
            .step_mut(step_id)
            .ok_or_else(|| LedgerError::NotFound(format!("step not found: {}", step_id)))?;

        if step.status != StepStatus::Pending {
            return Err(LedgerError::BadTransition {
                from: step.status,
                to: StepStatus::Running,
            });
        }

        step.status = StepStatus::Running;
        let event = LedgerEvent::StepStarted {
            step: step_id.to_string(),
            ts: now,
        };

        self.append_event(&manifest.task_id, &event)?;
        manifest.updated_ts = now;
        self.save(manifest)?;
        Ok(())
    }

    /// Complete a step: transition from Running → Done.
    ///
    /// Validates the current status is Running, records the verifier token,
    /// appends the event, and saves the manifest.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - `BadTransition` if the step is not Running.
    /// - `MissingPromise` if the promise token is empty.
    pub fn complete_step(
        &self,
        manifest: &mut TaskManifest,
        step_id: &str,
        promise: CompletionPromise,
        now: u64,
    ) -> Result<(), LedgerError> {
        if promise.token.is_empty() {
            return Err(LedgerError::MissingPromise(format!(
                "completion promise requires non-empty token for step {}",
                step_id
            )));
        }

        let step = manifest
            .step_mut(step_id)
            .ok_or_else(|| LedgerError::NotFound(format!("step not found: {}", step_id)))?;

        if step.status != StepStatus::Running {
            return Err(LedgerError::BadTransition {
                from: step.status,
                to: StepStatus::Done,
            });
        }

        step.status = StepStatus::Done;
        step.verifier_token = Some(promise.token.clone());

        let event = LedgerEvent::StepDone {
            step: step_id.to_string(),
            token: promise.token,
            ts: now,
        };

        self.append_event(&manifest.task_id, &event)?;
        manifest.updated_ts = now;
        self.save(manifest)?;
        Ok(())
    }

    /// Fail a step: transition from Pending or Running → Failed.
    ///
    /// Validates the current status, appends the event, and saves the manifest.
    ///
    /// # Errors
    ///
    /// Returns `BadTransition` if the step is already Done, Failed, Skipped, or Blocked.
    pub fn fail_step(
        &self,
        manifest: &mut TaskManifest,
        step_id: &str,
        error: &str,
        now: u64,
    ) -> Result<(), LedgerError> {
        let step = manifest
            .step_mut(step_id)
            .ok_or_else(|| LedgerError::NotFound(format!("step not found: {}", step_id)))?;

        match step.status {
            StepStatus::Pending | StepStatus::Running => {}
            _ => {
                return Err(LedgerError::BadTransition {
                    from: step.status,
                    to: StepStatus::Failed,
                });
            }
        }

        step.status = StepStatus::Failed;
        let event = LedgerEvent::StepFailed {
            step: step_id.to_string(),
            error: error.to_string(),
            ts: now,
        };

        self.append_event(&manifest.task_id, &event)?;
        manifest.updated_ts = now;
        self.save(manifest)?;
        Ok(())
    }

    /// Block a step: transition to Blocked.
    ///
    /// Appends the event and saves the manifest.
    ///
    /// # Errors
    ///
    /// Returns `BadTransition` if the step is already in any terminal state
    /// (Done, Failed, or Skipped). A step already terminal cannot be re-blocked.
    pub fn block_step(
        &self,
        manifest: &mut TaskManifest,
        step_id: &str,
        reason: &str,
        now: u64,
    ) -> Result<(), LedgerError> {
        let step = manifest
            .step_mut(step_id)
            .ok_or_else(|| LedgerError::NotFound(format!("step not found: {}", step_id)))?;

        match step.status {
            StepStatus::Done | StepStatus::Failed | StepStatus::Skipped => {
                return Err(LedgerError::BadTransition {
                    from: step.status,
                    to: StepStatus::Blocked,
                });
            }
            _ => {}
        }

        step.status = StepStatus::Blocked;
        let event = LedgerEvent::StepBlocked {
            step: step_id.to_string(),
            reason: reason.to_string(),
            ts: now,
        };

        self.append_event(&manifest.task_id, &event)?;
        manifest.updated_ts = now;
        self.save(manifest)?;
        Ok(())
    }

    /// Skip a step: transition to Skipped.
    ///
    /// Appends the event and saves the manifest.
    ///
    /// # Errors
    ///
    /// Returns `BadTransition` if the step is already in any terminal state
    /// (Done, Failed, or Skipped). A step already terminal cannot be re-skipped.
    pub fn skip_step(
        &self,
        manifest: &mut TaskManifest,
        step_id: &str,
        reason: &str,
        now: u64,
    ) -> Result<(), LedgerError> {
        let step = manifest
            .step_mut(step_id)
            .ok_or_else(|| LedgerError::NotFound(format!("step not found: {}", step_id)))?;

        match step.status {
            StepStatus::Done | StepStatus::Failed | StepStatus::Skipped => {
                return Err(LedgerError::BadTransition {
                    from: step.status,
                    to: StepStatus::Skipped,
                });
            }
            _ => {}
        }

        step.status = StepStatus::Skipped;
        let event = LedgerEvent::StepSkipped {
            step: step_id.to_string(),
            reason: reason.to_string(),
            ts: now,
        };

        self.append_event(&manifest.task_id, &event)?;
        manifest.updated_ts = now;
        self.save(manifest)?;
        Ok(())
    }

    /// Record a tool call during step execution.
    ///
    /// Increments the step's `tool_calls` counter, appends a `ToolCall` event,
    /// and saves the manifest.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - `NotFound` if the step does not exist.
    /// - `BadTransition` if the step is not in `Running` state.
    pub fn record_tool_call(
        &self,
        manifest: &mut TaskManifest,
        step_id: &str,
        tool_name: &str,
        ok: bool,
        now: u64,
    ) -> Result<(), LedgerError> {
        let step = manifest
            .step_mut(step_id)
            .ok_or_else(|| LedgerError::NotFound(format!("step not found: {}", step_id)))?;

        if step.status != StepStatus::Running {
            return Err(LedgerError::BadTransition {
                from: step.status,
                to: StepStatus::Running,
            });
        }

        step.tool_calls += 1;

        let event = LedgerEvent::ToolCall {
            step: step_id.to_string(),
            name: tool_name.to_string(),
            ok,
            ts: now,
        };

        self.append_event(&manifest.task_id, &event)?;
        manifest.updated_ts = now;
        self.save(manifest)?;
        Ok(())
    }

    /// List all task IDs by scanning the tasks directory for manifest.json files.
    ///
    /// Returns a vector of task IDs found in `<base>/tasks/*/manifest.json`.
    /// Returns an empty vector if the tasks directory does not exist.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if directory listing fails.
    pub fn list_tasks(&self) -> Result<Vec<TaskId>, LedgerError> {
        let tasks_dir = self.base.join("tasks");

        if !tasks_dir.exists() {
            return Ok(Vec::new());
        }

        let mut task_ids = Vec::new();

        for entry in fs::read_dir(&tasks_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let manifest_path = path.join("manifest.json");
                if manifest_path.exists() {
                    if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                        task_ids.push(TaskId::from_raw(dir_name.to_string()));
                    }
                }
            }
        }

        Ok(task_ids)
    }

    /// Find all in-flight tasks.
    ///
    /// Loads all task manifests and returns those with at least one step
    /// that is not in a terminal state (i.e., any step is Pending, Running, or Blocked).
    /// A task is considered finished (not in-flight) if all steps are Done, Failed, or Skipped.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if listing or loading tasks fails.
    pub fn in_flight(&self) -> Result<Vec<TaskManifest>, LedgerError> {
        let task_ids = self.list_tasks()?;
        let mut in_flight = Vec::new();

        for task_id in task_ids {
            let manifest = self.load(&task_id)?;

            // Check if any step is in a non-terminal state
            let has_non_terminal = manifest.steps.iter().any(|step| {
                matches!(
                    step.status,
                    StepStatus::Pending | StepStatus::Running | StepStatus::Blocked
                )
            });

            if has_non_terminal {
                in_flight.push(manifest);
            }
        }

        Ok(in_flight)
    }

    /// Resume a task after a crash or restart.
    ///
    /// Loads the task manifest and reconciles crash state:
    /// - Any step left in `Running` state (due to a crash) is reset to `Pending`.
    /// - A `Note` event is appended for each reset step, timestamped with `now`.
    /// - `manifest.updated_ts` is set to `now` if any step was reset.
    /// - The manifest is saved.
    ///
    /// Events are appended BEFORE the manifest is saved; on a process crash
    /// between the two, `events.jsonl` may record one transition the manifest
    /// doesn't reflect — the event log is authoritative, and `resume` reconciles
    /// orphaned `Running` steps.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` if loading, saving, or appending events fails.
    pub fn resume(&self, task_id: &TaskId, now: u64) -> Result<TaskManifest, LedgerError> {
        let mut manifest = self.load(task_id)?;
        let mut any_reset = false;

        for step in &mut manifest.steps {
            if step.status == StepStatus::Running {
                step.status = StepStatus::Pending;
                any_reset = true;

                // Append a Note event recording the reset
                let note_event = LedgerEvent::Note {
                    step: Some(step.id.clone()),
                    text: "reset orphaned Running step to Pending on resume".to_string(),
                    ts: now,
                };
                self.append_event(task_id, &note_event)?;
            }
        }

        if any_reset {
            manifest.updated_ts = now;
        }

        self.save(&manifest)?;
        Ok(manifest)
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
        led.create(&mut m, 100).unwrap();
        assert_eq!(m.created_ts, 100);
        assert_eq!(m.updated_ts, 100);
        assert!(d.path().join("tasks/t-1/manifest.json").exists());
        assert!(d.path().join("tasks/t-1/artifacts").is_dir());
        let mut loaded = led.load(&TaskId::from_raw("t-1".into())).unwrap();
        assert_eq!(loaded.steps.len(), 1);
        assert_eq!(loaded.created_ts, 100);
        loaded.steps[0].status = StepStatus::Running;
        led.save(&loaded).unwrap();
        // no .tmp left behind
        assert!(!d.path().join("tasks/t-1/manifest.json.tmp").exists());
        assert_eq!(led.load(&TaskId::from_raw("t-1".into())).unwrap().steps[0].status, StepStatus::Running);
    }

    #[test]
    fn complete_requires_promise_and_records_token() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "g".into());
        m.steps.push(Step::new("a", "first"));
        led.create(&mut m, 1).unwrap();
        led.start_step(&mut m, "a", 1).unwrap();
        // empty token rejected
        assert!(matches!(
            led.complete_step(
                &mut m,
                "a",
                CompletionPromise {
                    step_id: "a".into(),
                    verifier: "cargo".into(),
                    token: "".into(),
                    ts: 2
                },
                2
            ),
            Err(LedgerError::MissingPromise(_))
        ));
        // valid token → Done + token recorded + event logged
        led.complete_step(
            &mut m,
            "a",
            CompletionPromise {
                step_id: "a".into(),
                verifier: "cargo".into(),
                token: "PASS".into(),
                ts: 2,
            },
            2,
        )
        .unwrap();
        assert_eq!(m.step("a").unwrap().status, StepStatus::Done);
        assert_eq!(m.step("a").unwrap().verifier_token.as_deref(), Some("PASS"));
        assert!(led
            .read_events(&m.task_id)
            .unwrap()
            .iter()
            .any(|e| matches!(e, LedgerEvent::StepDone { .. })));
    }

    #[test]
    fn illegal_transition_rejected() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "g".into());
        m.steps.push(Step::new("a", "first"));
        led.create(&mut m, 1).unwrap();
        // completing a Pending (not Running) step is illegal
        assert!(matches!(
            led.complete_step(
                &mut m,
                "a",
                CompletionPromise {
                    step_id: "a".into(),
                    verifier: "v".into(),
                    token: "PASS".into(),
                    ts: 1
                },
                1
            ),
            Err(LedgerError::BadTransition { .. })
        ));
    }

    #[test]
    fn resume_resets_crashed_running_steps() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "g".into());
        m.steps.push(Step::new("a","first"));
        led.create(&mut m, 1).unwrap();
        led.start_step(&mut m, "a", 2).unwrap();           // now Running, then "crash"
        // a fresh ledger over the same dir (simulating restart)
        let led2 = TaskLedger::new(d.path());
        assert_eq!(led2.in_flight().unwrap().len(), 1);     // detected as in-flight
        let resumed = led2.resume(&TaskId::from_raw("t".into()), 3).unwrap();
        assert_eq!(resumed.step("a").unwrap().status, StepStatus::Pending);  // Running reset to Pending
        assert_eq!(resumed.updated_ts, 3);                  // updated_ts stamped on resume
        assert!(led2.read_events(&resumed.task_id).unwrap().iter().any(|e| matches!(e, LedgerEvent::Note{..})));
    }

    #[test]
    fn list_tasks_finds_created_tasks() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        led.create(&mut TaskManifest::new(TaskId::from_raw("t1".into()), "g".into()), 1).unwrap();
        led.create(&mut TaskManifest::new(TaskId::from_raw("t2".into()), "g".into()), 1).unwrap();
        let mut ids: Vec<_> = led.list_tasks().unwrap().iter().map(|t| t.as_str().to_string()).collect();
        ids.sort();
        assert_eq!(ids, vec!["t1".to_string(), "t2".to_string()]);
    }

    #[test]
    fn record_tool_call_bumps_count_and_logs() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "g".into());
        m.steps.push(Step::new("a","first"));
        led.create(&mut m, 0).unwrap();
        led.start_step(&mut m, "a", 1).unwrap();
        led.record_tool_call(&mut m, "a", "read_file", true, 2).unwrap();
        led.record_tool_call(&mut m, "a", "execute_command", false, 3).unwrap();
        assert_eq!(m.step("a").unwrap().tool_calls, 2);
        let evs = led.read_events(&m.task_id).unwrap();
        assert!(evs.iter().any(|e| matches!(e, LedgerEvent::TaskCreated{..})));
        assert_eq!(evs.iter().filter(|e| matches!(e, LedgerEvent::ToolCall{..})).count(), 2);
    }

    #[test]
    fn budget_exceeded_at_cap() {
        let mut s = Step::new("a","x");
        s.budget = crate::model::StepBudget { max_tool_calls: Some(2) };
        s.tool_calls = 2;
        assert!(s.budget_exceeded());
        s.tool_calls = 1;
        assert!(!s.budget_exceeded());
    }
}
