//! Sequential executor core loop.
//!
//! [`Executor`] drives a [`TaskManifest`] to completion by repeatedly asking
//! the manifest for its ready steps, dispatching each to the [`Worker`] seam,
//! running the verifier if the worker requested it, and recording every
//! transition in the [`TaskLedger`].
//!
//! # Borrow-across-await discipline
//!
//! The executor never holds a `&Step` (or any borrow into `manifest`) across
//! an `.await` point.  Before every async call it snapshots the data it needs
//! into owned `String` / `Value` / `Vec` values, then mutates the manifest
//! through the ledger APIs after the call returns.

use std::collections::HashMap;
use std::path::PathBuf;

use oxidemx_ledger::{CompletionPromise, TaskLedger, TaskManifest};
use serde_json::Value;

use crate::verify::{CommandRunner, Verifier};
use crate::worker::{Worker, WorkerBrief};

// ── Approval / capability stubs (full gating in Task 5) ──────────────────────

/// Approval classifier stub (Task 5 will fill this in).
///
/// For now every tool invocation is auto-approved.
#[derive(Clone, Debug, Default)]
pub struct ApprovalClassifier;

/// Resource caps for the whole executor run.
#[derive(Clone, Debug, Default)]
pub struct Caps {
    /// Maximum total tool calls across all steps.  `None` = unlimited.
    pub max_total_tool_calls: Option<u32>,
}

// ── Public types ──────────────────────────────────────────────────────────────

/// Report returned after [`Executor::run`] finishes.
#[derive(Clone, Debug, Default)]
pub struct RunReport {
    /// Number of steps that reached `Done`.
    pub completed: usize,
    /// Number of steps that reached `Failed`.
    pub failed: usize,
    /// Number of steps that were Pending but could not run because a dependency
    /// failed (never became `ready`; the loop terminates naturally).
    pub blocked: usize,
}

/// Drives a validated task manifest to completion.
///
/// Sequential for Task 4; parallelism is added in Task 5.
pub struct Executor<W: Worker, R: CommandRunner> {
    worker: W,
    verifier: Verifier<R>,
    /// Approval classifier (Task 5 gating; currently auto-approves all).
    _classifier: ApprovalClassifier,
    /// Resource caps (Task 5 enforcement; currently unchecked).
    _caps: Caps,
    /// Working directory passed to the verifier.
    cwd: PathBuf,
}

impl<W: Worker, R: CommandRunner> Executor<W, R> {
    /// Create a new executor.
    pub fn new(
        worker: W,
        verifier: Verifier<R>,
        classifier: ApprovalClassifier,
        caps: Caps,
        cwd: PathBuf,
    ) -> Self {
        Self {
            worker,
            verifier,
            _classifier: classifier,
            _caps: caps,
            cwd,
        }
    }

    /// Run the task described by `manifest` until no more steps are ready.
    ///
    /// The loop terminates when either:
    /// - All steps are terminal (`Done` / `Failed` / `Skipped`), or
    /// - No ready steps remain — which happens once all Pending steps are
    ///   blocked behind a step that failed (their `needs` will never all be
    ///   `Done`, so they never become ready).
    ///
    /// # Errors
    ///
    /// Ledger and worker errors are demoted to step failures where possible.
    /// The outer `Result` is `Ok` unless a ledger write itself is corrupt.
    pub async fn run(
        &self,
        ledger: &TaskLedger,
        manifest: &mut TaskManifest,
        now: u64,
    ) -> RunReport {
        // Stores each completed step's structured output so downstream steps
        // can receive it as `inputs`.
        let mut step_outputs: HashMap<String, Value> = HashMap::new();

        let mut report = RunReport::default();

        loop {
            // Snapshot the IDs + metadata of every ready step — no borrows
            // held past this block.
            let ready: Vec<(String, String, Vec<String>)> = manifest
                .ready_steps()
                .into_iter()
                .map(|s| (s.id.clone(), s.title.clone(), s.needs.clone()))
                .collect();

            if ready.is_empty() {
                break;
            }

            // Sequential: take the first ready step each iteration.
            let (step_id, title, needs) = ready.into_iter().next().expect("non-empty checked above");

            // Build the `inputs` map from already-completed `needs` outputs.
            let inputs_map: serde_json::Map<String, Value> = needs
                .iter()
                .filter_map(|dep_id| {
                    step_outputs
                        .get(dep_id)
                        .map(|v| (dep_id.clone(), v.clone()))
                })
                .collect();
            let inputs = Value::Object(inputs_map);

            // Snapshot goal (no borrow held across await).
            let goal = manifest.goal.clone();

            // Transition: Pending → Running.
            if let Err(e) = ledger.start_step(manifest, &step_id, now) {
                // Ledger inconsistency — skip and count as failed.
                report.failed += 1;
                let _ = ledger.fail_step(manifest, &step_id, &e.to_string(), now);
                continue;
            }

            let brief = WorkerBrief {
                step_id: step_id.clone(),
                title,
                goal,
                inputs,
            };

            // Dispatch to the worker — no manifest borrows held here.
            match self.worker.run_step(brief).await {
                Err(e) => {
                    let msg = e.to_string();
                    let _ = ledger.fail_step(manifest, &step_id, &msg, now);
                    report.failed += 1;
                }
                Ok(output) => {
                    // Record every tool invocation in the ledger.
                    for inv in &output.tool_calls {
                        // Approval gating is Task 5 — auto-approve for now.
                        let _ = ledger.record_tool_call(manifest, &step_id, &inv.name, true, now);
                    }

                    // Store the output for downstream steps.
                    step_outputs.insert(step_id.clone(), output.output.clone());

                    // Verify or complete trivially.
                    match output.verify_cmd {
                        Some((ref program, ref args)) => {
                            match self
                                .verifier
                                .verify(&step_id, program, args, &self.cwd, now)
                                .await
                            {
                                Ok(promise) => {
                                    let _ = ledger.complete_step(manifest, &step_id, promise, now);
                                    report.completed += 1;
                                }
                                Err(failure) => {
                                    let _ = ledger.fail_step(
                                        manifest,
                                        &step_id,
                                        &failure.output,
                                        now,
                                    );
                                    report.failed += 1;
                                }
                            }
                        }
                        None => {
                            // Non-code step: complete with a trivial promise.
                            let promise = CompletionPromise {
                                step_id: step_id.clone(),
                                verifier: "none".to_string(),
                                token: "ok".to_string(),
                                ts: now,
                            };
                            let _ = ledger.complete_step(manifest, &step_id, promise, now);
                            report.completed += 1;
                        }
                    }
                }
            }
        }

        // Count remaining Pending steps as blocked (their dependency failed).
        report.blocked = manifest
            .steps
            .iter()
            .filter(|s| s.status() == oxidemx_ledger::StepStatus::Pending)
            .count();

        report
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::MockRunner;
    use crate::worker::{MockWorker, StepOutput, ToolInvocation};
    use oxidemx_ledger::{Step, StepStatus, TaskId, TaskManifest};

    fn make_manifest() -> TaskManifest {
        let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "build x".into());
        m.steps = vec![
            Step::new("a", "plan"),
            {
                let mut b = Step::new("b", "code");
                b.needs = vec!["a".into()];
                b
            },
        ];
        m
    }

    #[tokio::test]
    async fn executor_runs_a_two_step_task_to_done() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        let mut m = make_manifest();
        led.create(&mut m, 0).unwrap();

        let worker = MockWorker::from_iter([
            (
                "a",
                StepOutput {
                    text: "plan done".into(),
                    output: serde_json::json!({}),
                    tool_calls: vec![],
                    verify_cmd: None,
                },
            ),
            (
                "b",
                StepOutput {
                    text: "code done".into(),
                    output: serde_json::json!({ "file": "main.rs" }),
                    tool_calls: vec![ToolInvocation {
                        name: "write_file".into(),
                        args: serde_json::json!({}),
                    }],
                    verify_cmd: Some(("cargo".into(), vec!["check".into()])),
                },
            ),
        ]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::ok("ok")),
            ApprovalClassifier::default(),
            Caps { max_total_tool_calls: None },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        assert_eq!(report.completed, 2);
        assert!(m.steps.iter().all(|s| s.status() == StepStatus::Done));
    }

    #[tokio::test]
    async fn failed_step_blocks_dependents() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());
        let mut m = make_manifest();
        led.create(&mut m, 0).unwrap();

        // "a" fails → "b" never runs (stays Pending / blocked)
        let worker = MockWorker::from_iter([(
            "a",
            StepOutput {
                text: "err".into(),
                output: serde_json::json!({}),
                tool_calls: vec![],
                // verify_cmd that will fail
                verify_cmd: Some(("cargo".into(), vec!["check".into()])),
            },
        )]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::fail("E0308")),
            ApprovalClassifier::default(),
            Caps { max_total_tool_calls: None },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        assert_eq!(report.completed, 0);
        assert_eq!(report.failed, 1);
        // "b" was never started; it remains Pending (blocked by dep failure)
        assert_eq!(m.step("b").unwrap().status(), StepStatus::Pending);
        assert_eq!(report.blocked, 1);
    }
}
