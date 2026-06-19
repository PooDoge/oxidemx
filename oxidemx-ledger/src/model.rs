//! Task and step model for the ledger.

use serde::{Deserialize, Serialize};

/// Computes FNV-1a 64-bit hash over the given bytes (version-stable).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x00000100000001b3);
    }
    h
}

/// A stable, deterministic identifier for a task.
///
/// Format: `<slug>-<8 hex digits of hash>`.
/// The hash is computed over `format!("{slug}{seed}")` using FNV-1a
/// to ensure deterministic, version-stable IDs.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    /// Create a TaskId from a raw string (trivial constructor).
    pub fn from_raw(s: String) -> Self {
        Self(s)
    }

    /// Generate a TaskId deterministically from a slug and seed.
    ///
    /// The hash is computed using FNV-1a over `format!("{slug}{seed}")`,
    /// formatted as `"{slug}-{:08x}"`.
    pub fn generate(slug: &str, seed: u64) -> Self {
        let input = format!("{}{}", slug, seed);
        let hash = fnv1a64(input.as_bytes());
        Self(format!("{}-{:08x}", slug, hash))
    }

    /// Return the raw ID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Status of a step in a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum StepStatus {
    /// Step is waiting to run.
    #[default]
    Pending,
    /// Step is currently executing.
    Running,
    /// Step is blocked waiting for a dependency.
    Blocked,
    /// Step completed successfully.
    Done,
    /// Step failed.
    Failed,
    /// Step was skipped.
    Skipped,
}

/// A single step within a task workflow.
///
/// **Invariant:** `status` and `verifier_token` are ground-truth fields settable only
/// through the ledger's guarded transitions (e.g., `complete_step`). This enforces
/// that external code cannot bypass the verified-completion promise. Access these
/// fields via the public `status()` and `verifier_token()` getter methods.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    /// Unique identifier for this step.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Current status. Set only via ledger's guarded transitions.
    #[serde(default)]
    pub(crate) status: StepStatus,
    /// IDs of steps that must complete before this one can run.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Artifact URLs or paths produced by this step.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// Number of tool calls made during execution.
    #[serde(default)]
    pub tool_calls: u32,
    /// Optional verification token. Set only via ledger's guarded transitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) verifier_token: Option<String>,
}

impl Step {
    /// Create a new step with default values.
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            status: StepStatus::default(),
            needs: Vec::new(),
            artifacts: Vec::new(),
            tool_calls: 0,
            verifier_token: None,
        }
    }

    /// The step's current status (set only via the ledger's guarded transitions).
    pub fn status(&self) -> StepStatus {
        self.status
    }

    /// The recorded verifier completion token, if the step is Done.
    pub fn verifier_token(&self) -> Option<&str> {
        self.verifier_token.as_deref()
    }
}

/// A promise that a step will be verified and completed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionPromise {
    /// The step ID being verified.
    pub step_id: String,
    /// Verifier identifier.
    pub verifier: String,
    /// Unique token for this promise.
    pub token: String,
    /// Timestamp when promise was created.
    pub ts: u64,
}

/// A complete task manifest with its steps and metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskManifest {
    /// The task ID.
    pub task_id: TaskId,
    /// High-level goal for the task.
    pub goal: String,
    /// Steps in the task workflow.
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Timestamp when the task was created (caller-supplied).
    pub created_ts: u64,
    /// Timestamp of last update (caller-supplied).
    pub updated_ts: u64,
}

impl TaskManifest {
    /// Create a new task manifest with the given ID and goal.
    ///
    /// Timestamps are initialized to 0; the caller is responsible for
    /// stamping them via the persistence store to maintain determinism.
    pub fn new(task_id: TaskId, goal: String) -> Self {
        Self {
            task_id,
            goal,
            steps: Vec::new(),
            created_ts: 0,
            updated_ts: 0,
        }
    }

    /// Return all pending steps whose dependencies (in `needs`) are all done.
    pub fn ready_steps(&self) -> Vec<&Step> {
        self.steps
            .iter()
            .filter(|step| {
                step.status == StepStatus::Pending
                    && step.needs.iter().all(|dep_id| {
                        self.step(dep_id)
                            .map(|dep| dep.status == StepStatus::Done)
                            .unwrap_or(false)
                    })
            })
            .collect()
    }

    /// Look up a step by ID.
    pub fn step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.id == id)
    }

    /// Look up a step by ID mutably.
    ///
    /// `pub(crate)` intentionally: external crates must not hold `&mut Step` directly,
    /// as that would bypass the ledger's guarded transitions and break the ground-truth
    /// invariant on `status` and `verifier_token`.
    pub(crate) fn step_mut(&mut self, id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_steps_respects_needs() {
        let mut m = TaskManifest::new(TaskId::from_raw("t-abc".into()), "goal".into());
        m.steps = vec![
            Step::new("a", "first"),
            {
                let mut s = Step::new("b", "second");
                s.needs = vec!["a".into()];
                s
            },
        ];
        // only "a" is ready (b needs a)
        let ready: Vec<_> = m.ready_steps().iter().map(|s| s.id.clone()).collect();
        assert_eq!(ready, vec!["a".to_string()]);
        // mark a Done → b becomes ready
        m.steps[0].status = StepStatus::Done;
        let ready2: Vec<_> = m.ready_steps().iter().map(|s| s.id.clone()).collect();
        assert_eq!(ready2, vec!["b".to_string()]);
    }

    #[test]
    fn manifest_serde_round_trips() {
        let m = TaskManifest::new(TaskId::generate("build", 42), "goal".into());
        let json = serde_json::to_string(&m).unwrap();
        let back: TaskManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.goal, "goal");
        assert!(back.task_id.as_str().contains('-'));
    }
}
