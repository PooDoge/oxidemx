//! Parallel executor with inline gate, hard caps, and edge validation.
//!
//! [`Executor`] drives a [`TaskManifest`] to completion.  In each iteration it
//! snapshots ALL currently-ready steps, dispatches them concurrently (via a
//! collected Vec of futures driven round-robin), and then applies the results
//! to the ledger **sequentially** to avoid manifest races.
//!
//! # Approval gating
//!
//! Approval decisions are made by the `GatedToolExecutor` (SP2d-1) BEFORE the
//! worker receives a tool call.  When a tool requires human approval the worker
//! returns `Err(HarnessError::NeedsApproval { tool, reason })`.  The executor
//! maps this to `block_step(manifest, step_id, "needs-approval: {tool}: {reason}", now)`
//! and **continues the run loop to other ready steps** (non-blocking).
//!
//! # Caps
//!
//! Two independent budget checks are performed during result application:
//! 1. Process-level cap: if `total_tool_calls >= caps.max_total_tool_calls`
//!    the step is blocked with `"budget-exhausted"`.
//! 2. Per-step budget: if the step's `budget_exceeded()` was true at snapshot
//!    time it is blocked before dispatch.
//!
//! # Edge validation
//!
//! After building the `inputs` map for a consumer step (from the outputs of
//! its `needs` steps) the executor validates those inputs against the
//! consumer's `input_schema` (if present).  On failure the consumer step is
//! `block_step`-ped with reason `"schema-violation: ..."`.
//!
//! # Borrow-across-await discipline
//!
//! The executor NEVER holds a borrow into `manifest` across an `.await` point.
//! All data needed for worker dispatch is snapshotted into owned values before
//! the concurrent phase.  Manifest mutations only happen after all workers have
//! returned, in a sequential apply loop.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use futures::future::join_all;
use oxidemx_ledger::{CompletionPromise, TaskLedger, TaskManifest};
use serde_json::Value;

use crate::edge::validate_edge;
use crate::verify::{CommandRunner, Verifier};
use crate::worker::{StepOutput, Worker, WorkerBrief};
use crate::HarnessError;

// ── Public types ──────────────────────────────────────────────────────────────

/// Resource caps for the whole executor run.
#[derive(Clone, Debug, Default)]
pub struct Caps {
    /// Maximum total tool calls across all steps for this run.  `None` = unlimited.
    pub max_total_tool_calls: Option<u32>,
}

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

// ── Internal snapshot types (owned, no manifest borrow) ──────────────────────

/// Owned snapshot of a ready step, built before worker dispatch.
struct StepBrief {
    id: String,
    title: String,
    goal: String,
    inputs: Value,
    /// The consumer's `input_schema`, if any (for edge validation).
    input_schema: Option<Value>,
    /// Whether the step's own per-step budget was already exceeded.
    budget_exceeded: bool,
    /// The step's per-step tool-call budget limit, if any.
    #[allow(dead_code)]
    budget_max_tool_calls: Option<u32>,
}

/// Result of running a single step's worker.
struct WorkerResult {
    step_id: String,
    outcome: Result<StepOutput, HarnessError>,
}

// ── Executor ──────────────────────────────────────────────────────────────────

/// Drives a validated task manifest to completion.
///
/// Dispatches all currently-ready steps concurrently (collect futures, drive
/// sequentially while holding no manifest borrow), then applies results to the
/// manifest sequentially to avoid ledger races.
pub struct Executor<W: Worker, R: CommandRunner> {
    worker: W,
    verifier: Verifier<R>,
    caps: Caps,
    /// Working directory passed to the verifier.
    cwd: PathBuf,
}

impl<W: Worker, R: CommandRunner> Executor<W, R> {
    /// Create a new executor.
    pub fn new(
        worker: W,
        verifier: Verifier<R>,
        caps: Caps,
        cwd: PathBuf,
    ) -> Self {
        Self {
            worker,
            verifier,
            caps,
            cwd,
        }
    }

    /// Run the task described by `manifest` until no more steps are ready.
    ///
    /// Terminates when either:
    /// - All steps are terminal (`Done` / `Failed` / `Blocked` / `Skipped`), or
    /// - No ready steps remain (all Pending steps are blocked behind failed
    ///   or blocked deps and will never become ready).
    pub async fn run(
        &self,
        ledger: &TaskLedger,
        manifest: &mut TaskManifest,
        now: u64,
    ) -> RunReport {
        // Accumulated outputs from completed steps for downstream `inputs`.
        let mut step_outputs: HashMap<String, Value> = HashMap::new();
        let mut report = RunReport::default();
        // Re-pick guard: IDs already dispatched (or pre-flight blocked) this
        // run so a transition error can't re-surface the same step.
        let mut dispatched: HashSet<String> = HashSet::new();
        // Process-level tool-call counter.
        let mut total_tool_calls: u32 = 0;

        loop {
            // ── Step 1: Snapshot all ready, undispatched step briefs ─────────
            // No borrows of `manifest` survive past this block.
            let briefs: Vec<StepBrief> = manifest
                .ready_steps()
                .into_iter()
                .filter(|s| !dispatched.contains(&s.id))
                .map(|s| {
                    let inputs_map: serde_json::Map<String, Value> = s
                        .needs
                        .iter()
                        .filter_map(|dep_id| {
                            step_outputs
                                .get(dep_id)
                                .map(|v| (dep_id.clone(), v.clone()))
                        })
                        .collect();
                    StepBrief {
                        id: s.id.clone(),
                        title: s.title.clone(),
                        goal: manifest.goal.clone(),
                        inputs: Value::Object(inputs_map),
                        input_schema: s.input_schema.clone(),
                        budget_exceeded: s.budget_exceeded(),
                        budget_max_tool_calls: s.budget.max_tool_calls,
                    }
                })
                .collect();

            if briefs.is_empty() {
                break;
            }

            // Mark all as dispatched immediately so the re-pick guard fires
            // correctly even if a pre-flight check fails.
            for b in &briefs {
                dispatched.insert(b.id.clone());
            }

            // ── Step 2: Pre-flight — edge validation, budget, start_step ─────
            // Builds the list of steps to actually send to the worker.
            let mut worker_briefs: Vec<(String, WorkerBrief)> = Vec::new();

            for brief in briefs {
                // Edge validation: validate assembled inputs against consumer schema.
                if let Some(ref schema) = brief.input_schema {
                    if let Err(e) = validate_edge(&brief.inputs, schema) {
                        let reason = format!("schema-violation: {e}");
                        let _ = ledger.block_step(manifest, &brief.id, &reason, now);
                        report.blocked += 1;
                        continue;
                    }
                }

                // Per-step budget pre-check.
                if brief.budget_exceeded {
                    let _ = ledger.block_step(manifest, &brief.id, "budget-exhausted", now);
                    report.blocked += 1;
                    continue;
                }

                // Transition Pending -> Running.
                if let Err(e) = ledger.start_step(manifest, &brief.id, now) {
                    let _ = ledger.fail_step(manifest, &brief.id, &e.to_string(), now);
                    report.failed += 1;
                    continue;
                }

                worker_briefs.push((
                    brief.id.clone(),
                    WorkerBrief {
                        step_id: brief.id,
                        title: brief.title,
                        goal: brief.goal,
                        inputs: brief.inputs,
                    },
                ));
            }

            if worker_briefs.is_empty() {
                // All steps blocked/failed at pre-flight; loop to re-check
                // (the next iteration will find no new ready steps and break).
                continue;
            }

            // ── Step 3: Concurrent worker dispatch ───────────────────────────
            // All worker futures for the ready batch are polled concurrently via
            // `futures::future::join_all`.  This overlaps I/O waits (LLM calls,
            // network) so N independent steps take ~max(latencies) rather than
            // ~sum(latencies).  It is concurrent on one task -- not multi-core
            // parallelism -- which is exactly what I/O-bound LLM calls need.
            //
            // No manifest borrow is held here: all data was snapshotted into
            // owned `WorkerBrief` values before this point (step 1 + step 2).
            // `&self.worker` is borrowed immutably for the duration of the
            // single `join_all(...).await` call; no `tokio::spawn`, no `Arc`,
            // no `'static` bound required.
            //
            // Ledger writes happen exclusively in step 4, after `join_all`
            // returns, keeping the manifest race-free.

            let worker = &self.worker;
            let futs = worker_briefs.into_iter().map(|(step_id, wb)| async move {
                WorkerResult { step_id, outcome: worker.run_step(wb).await }
            });
            let results: Vec<WorkerResult> = join_all(futs).await;

            // ── Step 4: Sequential result application ────────────────────────
            // All manifest mutations happen here, after all workers returned.
            for WorkerResult { step_id, outcome } in results {
                match outcome {
                    Err(HarnessError::NeedsApproval { ref tool, ref reason }) => {
                        // Inline gate surfaced: block this step non-blocking.
                        let block_reason = format!("needs-approval: {tool}: {reason}");
                        let _ = ledger.block_step(manifest, &step_id, &block_reason, now);
                        report.blocked += 1;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        let _ = ledger.fail_step(manifest, &step_id, &msg, now);
                        report.failed += 1;
                    }
                    Ok(output) => {
                        // Caps, evaluated per tool invocation.
                        let mut step_blocked = false;
                        let mut block_reason = String::new();

                        // Retrieve the step's budget limit from the manifest.
                        let step_budget_max = manifest
                            .step(&step_id)
                            .and_then(|s| s.budget.max_tool_calls);

                        for (step_call_count, inv) in (0_u32..).zip(output.tool_calls.iter()) {
                            // Process-level total cap check.
                            if let Some(max) = self.caps.max_total_tool_calls {
                                if total_tool_calls >= max {
                                    step_blocked = true;
                                    block_reason = "budget-exhausted".into();
                                    break;
                                }
                            }

                            // Per-step budget check before recording the call.
                            if let Some(max) = step_budget_max {
                                if step_call_count >= max {
                                    step_blocked = true;
                                    block_reason = "budget-exhausted".into();
                                    break;
                                }
                            }

                            let _ = ledger.record_tool_call(
                                manifest,
                                &step_id,
                                &inv.name,
                                true,
                                now,
                            );
                            total_tool_calls += 1;
                        }

                        if step_blocked {
                            let _ =
                                ledger.block_step(manifest, &step_id, &block_reason, now);
                            report.blocked += 1;
                            continue;
                        }

                        // Store output for downstream steps.
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
                                        let _ = ledger.complete_step(
                                            manifest, &step_id, promise, now,
                                        );
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
                                let promise = CompletionPromise {
                                    step_id: step_id.clone(),
                                    verifier: "none".to_string(),
                                    token: "ok".to_string(),
                                    ts: now,
                                };
                                let _ =
                                    ledger.complete_step(manifest, &step_id, promise, now);
                                report.completed += 1;
                            }
                        }
                    }
                }
            }
        }

        // Count remaining Pending steps as blocked (their dependency failed).
        report.blocked += manifest
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
    use crate::HarnessError;
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

    // ── T4 tests (must stay green) ────────────────────────────────────────────

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
                        // read_file is AutoAllow regardless of path/git status.
                        name: "read_file".into(),
                        args: serde_json::json!({"file_path": "main.rs"}),
                    }],
                    verify_cmd: Some(("cargo".into(), vec!["check".into()])),
                },
            ),
        ]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::ok("ok")),
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

        let worker = MockWorker::from_iter([(
            "a",
            StepOutput {
                text: "err".into(),
                output: serde_json::json!({}),
                tool_calls: vec![],
                verify_cmd: Some(("cargo".into(), vec!["check".into()])),
            },
        )]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::fail("E0308")),
            Caps { max_total_tool_calls: None },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        assert_eq!(report.completed, 0);
        assert_eq!(report.failed, 1);
        assert_eq!(m.step("b").unwrap().status(), StepStatus::Pending);
        assert_eq!(report.blocked, 1);
    }

    // ── T5 tests ──────────────────────────────────────────────────────────────

    /// A `NeedsApproval` error from the worker blocks the step but the run
    /// continues to other independent steps (non-blocking).
    #[tokio::test]
    async fn needs_approval_blocks_step_run_continues() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());

        // Two fully independent steps (no `needs`).
        let mut m = TaskManifest::new(TaskId::from_raw("t-approval".into()), "test".into());
        m.steps = vec![Step::new("a", "risky"), Step::new("b", "clean")];
        led.create(&mut m, 0).unwrap();

        // "a" -> Err(NeedsApproval) -- inline gate blocked execute_command.
        // "b" -> Ok(done) -- completes normally.
        let worker = MockWorker::from_iter([(
            "b",
            StepOutput {
                text: "clean step".into(),
                output: serde_json::json!({}),
                tool_calls: vec![],
                verify_cmd: None,
            },
        )])
        .with_error(
            "a",
            HarnessError::NeedsApproval {
                tool: "execute_command".into(),
                reason: "git commit".into(),
            },
        );

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::ok("ok")),
            Caps { max_total_tool_calls: None },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        // "b" must reach Done -- the run did not hang on "a"'s NeedsApproval block.
        assert_eq!(m.step("b").unwrap().status(), StepStatus::Done, "b must be Done");
        // "a" must be Blocked due to needs-approval.
        assert_eq!(
            m.step("a").unwrap().status(),
            StepStatus::Blocked,
            "a must be Blocked(needs-approval)"
        );
        assert_eq!(report.completed, 1, "only b completed");
        assert!(report.blocked >= 1, "at least one step blocked");
    }

    /// A consumer step whose assembled inputs fail its `input_schema` must be
    /// blocked with "schema-violation", while the producing step is Done.
    #[tokio::test]
    async fn edge_schema_violation_blocks_consumer() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());

        // Step "a" outputs {"n": "not-an-int"}.
        // Step "b" needs "a" and has input_schema requiring that the "a" key
        // contains an object with an integer "n" field.
        let mut m = TaskManifest::new(TaskId::from_raw("t-edge".into()), "edge test".into());
        let step_a = Step::new("a", "producer");
        let mut step_b = Step::new("b", "consumer");
        step_b.needs = vec!["a".into()];
        // inputs = {"a": <output of step a>} so schema must validate that shape.
        step_b.input_schema = Some(serde_json::json!({
            "type": "object",
            "properties": {
                "a": {
                    "type": "object",
                    "properties": { "n": { "type": "integer" } },
                    "required": ["n"]
                }
            },
            "required": ["a"]
        }));
        m.steps = vec![step_a, step_b];
        led.create(&mut m, 0).unwrap();

        // "a" outputs {"n": "not-an-int"} -- not an integer -> schema-violation.
        let worker = MockWorker::from_iter([(
            "a",
            StepOutput {
                text: "done".into(),
                output: serde_json::json!({"n": "not-an-int"}),
                tool_calls: vec![],
                verify_cmd: None,
            },
        )]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::ok("ok")),
            Caps { max_total_tool_calls: None },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        assert_eq!(m.step("a").unwrap().status(), StepStatus::Done, "a must be Done");
        assert_eq!(
            m.step("b").unwrap().status(),
            StepStatus::Blocked,
            "b must be Blocked(schema-violation)"
        );
        assert_eq!(report.completed, 1, "only a completed");
    }

    /// When `max_total_tool_calls = 1` and the worker emits 2 tool calls,
    /// the second call hits the cap and the step is blocked.
    #[tokio::test]
    async fn total_tool_cap_blocks() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());

        let mut m = TaskManifest::new(TaskId::from_raw("t-cap".into()), "cap test".into());
        m.steps = vec![Step::new("a", "greedy")];
        led.create(&mut m, 0).unwrap();

        // Worker emits 2 `read_file` calls.
        // Cap is 1 -> after recording the first call total reaches 1 (= max),
        // the second call triggers total_tool_calls >= max -> block.
        let worker = MockWorker::from_iter([(
            "a",
            StepOutput {
                text: "greedy".into(),
                output: serde_json::json!({}),
                tool_calls: vec![
                    ToolInvocation {
                        name: "read_file".into(),
                        args: serde_json::json!({"file_path": "a.rs"}),
                    },
                    ToolInvocation {
                        name: "read_file".into(),
                        args: serde_json::json!({"file_path": "b.rs"}),
                    },
                ],
                verify_cmd: None,
            },
        )]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::ok("ok")),
            Caps { max_total_tool_calls: Some(1) },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        assert_eq!(
            m.step("a").unwrap().status(),
            StepStatus::Blocked,
            "step must be Blocked(budget-exhausted)"
        );
        let _ = report;
    }

    /// Worker returning `Err` -> step transitions to `Failed`, run continues to
    /// independent steps.
    #[tokio::test]
    async fn worker_error_fails_step_run_continues() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());

        // Two independent steps.  "a" has no mock script -> worker returns Err.
        let mut m = TaskManifest::new(TaskId::from_raw("t-werr".into()), "worker err".into());
        m.steps = vec![Step::new("a", "fails"), Step::new("b", "ok")];
        led.create(&mut m, 0).unwrap();

        let worker = MockWorker::from_iter([(
            "b",
            StepOutput {
                text: "clean".into(),
                output: serde_json::json!({}),
                tool_calls: vec![],
                verify_cmd: None,
            },
        )]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::ok("ok")),
            Caps { max_total_tool_calls: None },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        assert_eq!(m.step("a").unwrap().status(), StepStatus::Failed, "a must be Failed");
        assert_eq!(m.step("b").unwrap().status(), StepStatus::Done, "b must be Done");
        assert_eq!(report.failed, 1);
        assert_eq!(report.completed, 1);
    }

    /// Per-step tool-call budget is enforced independently of the global cap.
    /// A step with `max_tool_calls = 1` emitting 2 tool calls -> blocked.
    /// Global `max_total_tool_calls = None` (unlimited) ensures it's the per-step cap firing.
    #[tokio::test]
    async fn per_step_tool_cap_blocks() {
        let d = tempfile::tempdir().unwrap();
        let led = TaskLedger::new(d.path());

        let mut m = TaskManifest::new(TaskId::from_raw("t-step-cap".into()), "per-step cap test".into());
        let mut step_a = Step::new("a", "exceeds-budget");
        step_a.budget.max_tool_calls = Some(1);
        m.steps = vec![step_a];
        led.create(&mut m, 0).unwrap();

        // Worker emits 2 `read_file` calls.
        // Per-step budget is 1 -> after recording the first call step_call_count reaches 1 (= max),
        // the second call triggers step_call_count >= max -> block.
        let worker = MockWorker::from_iter([(
            "a",
            StepOutput {
                text: "exceeds budget".into(),
                output: serde_json::json!({}),
                tool_calls: vec![
                    ToolInvocation {
                        name: "read_file".into(),
                        args: serde_json::json!({"file_path": "a.rs"}),
                    },
                    ToolInvocation {
                        name: "read_file".into(),
                        args: serde_json::json!({"file_path": "b.rs"}),
                    },
                ],
                verify_cmd: None,
            },
        )]);

        let exec = Executor::new(
            worker,
            Verifier::new(MockRunner::ok("ok")),
            Caps { max_total_tool_calls: None },
            d.path().into(),
        );

        let report = exec.run(&led, &mut m, 10).await;

        assert_eq!(
            m.step("a").unwrap().status(),
            StepStatus::Blocked,
            "step must be Blocked(budget-exhausted)"
        );
        assert_eq!(report.blocked, 1);
        assert_eq!(report.completed, 0);
    }
}
