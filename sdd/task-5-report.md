# Task 5 Report — Approval gating + hard caps + edge schema-validation + parallel dispatch

## What was built

### 1. `oxidemx-harness/src/edge.rs` — `validate_edge`

`pub fn validate_edge(payload: &Value, schema: &Value) -> Result<(), String>` mirrors the
planner's pattern exactly: `jsonschema::validator_for(schema)` (fail-closed on malformed
schema — returns `Err("invalid schema: …")`) then `validator.iter_errors(payload)` collected
and joined on Err.  Three unit tests: valid payload → Ok, invalid payload → Err, malformed
schema → Err.

### 2. Approval gating — choice for `AutoDeny` and `Ask`

Per-tool-invocation classification via `ApprovalClassifier::classify(&inv.name, &inv.args, cwd)`:

- **`AutoDeny`** → `block_step(…, "tool auto-denied: <tool> — <reason>")` and stop processing
  that step.  _Choice rationale_: a denied tool means the step cannot make progress correctly
  (it was going to call an unsafe tool); silently skipping the call and continuing would
  produce garbage output.  Blocking is conservative and safe.

- **`Ask`** → `block_step(…, "needs-approval: <tool> — <reason>")` and **stop processing that
  step** (break out of the tool loop + `continue` the outer results loop).  The run loop is
  **non-blocking**: other ready steps in the same batch continue to their `complete_step` /
  `fail_step` / verify path.  The test `ask_tier_tool_blocks_step_but_run_continues` verifies
  this: step "a" is Blocked(needs-approval) while step "b" is Done in the same run.

- **`AutoAllow` / `AutoAllowIfReversible`** → `ledger.record_tool_call(…)` + `total_tool_calls += 1`.

### 3. Hard caps

Two checks during the sequential result-apply loop (after workers return):

1. **Process-level total cap**: before recording each tool call, if `total_tool_calls >= caps.max_total_tool_calls` → `block_step(…, "budget-exhausted")` and continue.  The test `total_tool_cap_blocks` uses cap=1 with a worker that emits 2 `read_file` (AutoAllow) calls: first call records and increments total to 1 (= max), second call hits `total >= max` and blocks.

2. **Per-step budget pre-check**: `s.budget_exceeded()` is snapshotted into `StepBrief.budget_exceeded` before worker dispatch.  If true at snapshot time the step is `block_step`-ped during pre-flight, before `start_step`.

### 4. Edge validation

When building a consumer step's `inputs` map (from the `needs` steps' stored outputs), if the
consumer step has `input_schema = Some(schema)`, `validate_edge(&inputs, &schema)` is called.
On Err → `block_step(consumer, "schema-violation: <joined errors>", now)` and skip dispatch.
The inputs object has shape `{"<dep_id>": <dep_output_value>, …}` so the schema must describe
that wrapper shape.  The test `edge_schema_violation_blocks_consumer` validates this: step "a"
outputs `{"n": "not-an-int"}`, step "b"'s input_schema requires `{a: {n: integer}}` → b is
Blocked, a is Done.

### 5. Parallel dispatch + sequential apply — no borrow across await

The executor loop:

1. **Snapshot phase**: `manifest.ready_steps()` → `Vec<StepBrief>` (all owned data, no
   manifest borrows held).  Re-pick guard (`dispatched: HashSet<String>`) marks all snapshotted
   IDs immediately.

2. **Pre-flight phase**: edge validation, budget check, `ledger.start_step` — all sequential,
   no awaits.

3. **Concurrent worker dispatch**: all `(step_id, WorkerBrief)` pairs are dispatched
   **concurrently** via `futures::future::join_all`.  All worker futures are polled together,
   overlapping I/O waits so N independent steps take `~max(latencies)` rather than
   `~sum(latencies)`.  `&self.worker` is borrowed immutably for the single
   `join_all(...).await` call — no `tokio::spawn`, no `Arc`, no `'static` bound required.
   Results are collected into `Vec<WorkerResult>` before any ledger mutation.

   No manifest borrow is held across the await: all data was snapshotted into owned values
   in Steps 1+2 before this phase begins.

4. **Sequential apply phase**: iterate `results` → approval gating → caps → verify → ledger
   mutations.  No manifest races possible.

### 6. Re-pick guard

`dispatched: HashSet<String>` is populated immediately when `ready_steps()` is snapshotted
(before pre-flight).  Even if `start_step` or `fail_step` errors, the step ID remains in
`dispatched` and cannot be re-picked in a later iteration.

### 7. `worker_error_fails_step_run_continues` test

`MockWorker` has no entry for step "a" → returns `HarnessError::Worker("no mock for step: a")`.
Executor catches the `Err` → `ledger.fail_step(…)`.  Step "b" (independent, has a mock entry)
proceeds normally to Done.  Asserts: `a == Failed`, `b == Done`, `failed == 1`, `completed == 1`.

## Test results

```
running 12 tests
test verify::tests::verify_fail_yields_critique ... ok
test verify::tests::verify_pass_yields_promise ... ok
test executor::tests::failed_step_blocks_dependents ... ok
test executor::tests::executor_runs_a_two_step_task_to_done ... ok
test executor::tests::auto_deny_tool_blocks_step ... ok
test executor::tests::total_tool_cap_blocks ... ok
test executor::tests::worker_error_fails_step_run_continues ... ok
test executor::tests::ask_tier_tool_blocks_step_but_run_continues ... ok
test edge::tests::malformed_schema_returns_err ... ok
test edge::tests::valid_payload_returns_ok ... ok
test edge::tests::invalid_payload_returns_err ... ok
test executor::tests::edge_schema_violation_blocks_consumer ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`cargo clippy -p oxidemx-harness -- -D warnings` → clean, zero warnings.

`cargo tree -p oxidemx-harness | grep -iE "reqwest|rustls"` → no output (jsonschema
`default-features = false` confirmed reqwest/rustls-free).

## Files changed

- `oxidemx-harness/src/edge.rs` — created
- `oxidemx-harness/src/executor.rs` — rewritten with full T5 logic
- `oxidemx-harness/src/lib.rs` — added `pub mod edge`, re-exported approval types from oxidemx-approval
- `oxidemx-harness/Cargo.toml` — added `oxidemx-approval` (path), `jsonschema 0.46 default-features=false`, `tokio rt-multi-thread`, `futures 0.3`

## Note on T4 test update

The original T4 test `executor_runs_a_two_step_task_to_done` used `write_file` as the tool
invocation for step "b".  With the real `ApprovalClassifier` wired in, `write_file` pointing
at a non-git-tracked file in a temp dir is classified as `Ask` (not reversible), which would
block step "b".  Updated to `read_file` (AutoAllow regardless of path/git state) — the test
is exercising executor flow, not approval gating.

## Concerns / follow-up

- **True OS-thread parallelism**: `join_all` gives concurrent I/O overlap on one task.  For
  CPU-bound work or strict multi-core isolation, `Arc<dyn Worker + Send + Sync>` +
  `JoinSet::spawn` would be needed.  For I/O-bound LLM calls this is unnecessary.
  Straightforward SP2d follow-up if ever required.

## SP2c final review fix — Per-step tool-call budget enforcement

**Commit**: `42b33f5 fix(harness): enforce per-step tool-call budget per-invocation`

The per-step budget was only checked ONCE at dispatch time, allowing a single step to exceed
its `max_tool_calls` limit if returning multiple AutoAllow invocations. Fixed by:

1. Carrying `budget_max_tool_calls: Option<u32>` into `StepBrief` snapshot (logical completeness).
2. Tracking `step_call_count` across invocations within a single step's result application.
3. Checking `step_call_count >= max` BEFORE recording each AutoAllow/Reversible call (guards the cap).
4. If exceeded, blocking the step with "budget-exhausted" and stopping that step's tool processing.
5. Both per-step and global-total caps apply independently; new test `per_step_tool_cap_blocks` confirms.

Test suite: **13 tests passing** (added 1 new test). `cargo clippy -p oxidemx-harness -- -D warnings` clean.
