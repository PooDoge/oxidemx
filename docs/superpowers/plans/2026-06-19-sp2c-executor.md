# SP2c — Orchestrator-worker executor + ApprovalClassifier + edges — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Execute a validated `StepGraph` autonomously — dispatch ready steps to thin fresh-context workers, gate every tool call through the risk-tiered `ApprovalClassifier`, validate data edges by schema, verify each code step against `cargo` (ground truth), enforce hard caps, and drive it all through the resumable ledger — fully headless-testable.

**Architecture:** A **ledger-native executor** (`oxidemx-harness`): a tokio `JoinSet` loop over `TaskLedger::ready_steps`, dispatching to a `Worker` trait seam (mock in tests; the real AutoAgents agent + cloud/local models are wired in SP2d). Tool calls pass through `oxidemx-approval` (new crate); code steps end in a `Verifier` (cargo); data edges validate by JSON-schema; budgets/caps live in the ledger. The conductor stays for authored FlowDoc flows — the harness executor is the ledger-task scheduler (they share the conductor's `CancellationToken`/`ApprovalPolicy` vocabulary, not its run loop).

**Tech Stack:** Rust, tokio (`JoinSet`, `CancellationToken`), serde/serde_json, `git2` + `shell-words` (approval), `jsonschema` (edge validation, default-features=false), thiserror, async-trait. Consumes `oxidemx-ledger` (SP2a/b) + `oxidemx-planner` (SP2b).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-19-sp2-autonomous-coding-harness-design.md` §3, §4.4, §4.7, §5, §5.1. Research: `docs/research/approval-guards.md`, `agent-harness-best-practices.md`.
- New crates `oxidemx-approval`, `oxidemx-harness` (workspace members). Both UI-free, no agentd/overlay deps. The Worker + model are trait seams (real impls = SP2d) so SP2c is 100% headless-mock-testable.
- `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`; no `unwrap`/`expect` outside tests; poison-safe locks; **no lock/guard across `.await`**; `thiserror` (`#[non_exhaustive]`); `clippy -D warnings`; per-crate `cargo test` green each task; no clock calls in pure logic (caller-supplied `ts`).
- **Ground-truth rule (§2.3):** a code step → `Done` ONLY via a `Verifier`-produced `CompletionPromise` (the ledger already enforces non-empty token; the verifier supplies it on `cargo` pass).
- **Hard caps in CODE (§2.5):** `max_tool_calls_per_step`, `max_total_tool_calls`, wall-clock — enforced by the executor at dispatch, not asked of a model.
- **Approval non-blocking (§5.1):** an `Ask`-tier tool in autonomous mode → the step goes `Blocked{needs-approval}`; the executor continues other ready steps (never hangs).
- Worktree `../oxidemx-phase1`, branch `phase1-local-llm-gateway`. Commit per task.

## File structure

```
oxidemx-ledger/src/model.rs        # T1: Step gains budget + input/output_schema; record_tool_call; TaskCreated
oxidemx-ledger/src/store.rs        # T1: record_tool_call transition + TaskCreated on create
oxidemx-approval/Cargo.toml        # T2 new crate
oxidemx-approval/src/lib.rs        # Tier, ApprovalClassifier, ToolAction
oxidemx-approval/src/reversible.rs # git2 reversibility classifier
oxidemx-approval/src/shell.rs      # shell-words argv classifier + denylist
oxidemx-harness/Cargo.toml         # T3-T5 new crate
oxidemx-harness/src/lib.rs
oxidemx-harness/src/verify.rs      # T3: CommandRunner seam + Verifier
oxidemx-harness/src/worker.rs      # T4: Worker seam + WorkerBrief/StepOutput
oxidemx-harness/src/executor.rs    # T4/T5: the run loop
oxidemx-harness/src/edge.rs        # T5: edge schema validation
```

---

### Task 1: Ledger executor primitives (tool-calls, budget, schemas, TaskCreated)

**Files:** Modify `oxidemx-ledger/src/{model.rs,store.rs}`.

**Interfaces:**
- `Step` gains (public fields): `pub budget: StepBudget` and `pub input_schema: Option<serde_json::Value>`, `pub output_schema: Option<serde_json::Value>` (all `#[serde(default)]`). `pub struct StepBudget { pub max_tool_calls: Option<u32> }` (`#[serde(default)]`, Default = None=unlimited). `Step` keeps `tool_calls: u32` (existing).
- `TaskLedger` gains `record_tool_call(&self, &mut TaskManifest, step_id, tool_name, ok: bool, now) -> Result<(), LedgerError>`: increments the step's `tool_calls`, appends a `LedgerEvent::ToolCall{step,name,ok,ts}`, saves. Errors `BadTransition` if the step isn't `Running`.
- `Step::budget_exceeded(&self) -> bool` = `budget.max_tool_calls.is_some_and(|m| self.tool_calls >= m)`.
- `TaskLedger::create` now also appends a `LedgerEvent::TaskCreated{task_id,goal,ts}` (it currently writes the manifest but emits no event) — keep the existing `create(&mut, now)` signature.

- [ ] **Step 1: Failing tests**
```rust
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
    s.budget = StepBudget { max_tool_calls: Some(2) };
    s.tool_calls = 2;
    assert!(s.budget_exceeded());
    s.tool_calls = 1;
    assert!(!s.budget_exceeded());
}
```
- [ ] **Step 2: Run → FAIL** (`cargo test -p oxidemx-ledger`).
- [ ] **Step 3: Implement** the fields + `StepBudget` + `record_tool_call` (mirror the existing transition methods: guard Running, append event, `updated_ts=now`, save) + `TaskCreated` in `create` + `budget_exceeded`. (`tool_calls` is a public field so the test can set it.)
- [ ] **Step 4: Run → PASS** (all prior 12 ledger tests still green).
- [ ] **Step 5: Commit** — `feat(ledger): record_tool_call + StepBudget + step schemas + TaskCreated event`

---

### Task 2: `oxidemx-approval` — the risk-tiered ApprovalClassifier

**Files:** Create `oxidemx-approval/{Cargo.toml,src/lib.rs,src/reversible.rs,src/shell.rs}`; Modify root `Cargo.toml`.

**Interfaces:** (from `docs/research/approval-guards.md` + spec §5.1)
- Deps: `git2`, `shell-words`, `serde`/`serde_json`, `thiserror`.
- `pub enum Tier { AutoAllow, AutoAllowIfReversible, Ask, AutoDeny }`.
- `pub struct ApprovalClassifier { /* config: per-binary rules, denylist, allowlist */ }` with `pub fn from_config(cfg: ClassifierConfig) -> Self` + `Default`. `pub fn classify(&self, tool: &str, args: &serde_json::Value, cwd: &Path) -> Decision` where `pub struct Decision { pub tier: Tier, pub reason: String }`.
- Tool classification: reads (`read_file`/`list_dir`/`search_file`/`google_search`) → `AutoAllow`; file mutations (`write_file`/edit/delete) → `reversible::classify_path(cwd, path)` → `AutoAllowIfReversible` if reversible else `Ask`; `execute_command` → `shell::classify_command(cmd)`; host/network/unknown → `Ask`.
- `reversible.rs`: `pub fn classify_path(cwd: &Path, path: &str) -> bool` — true iff the resolved path is UNDER `cwd` (canonicalized, no `..` escape) AND git-tracked in HEAD (`git2::Repository::discover(cwd)` → `head()?.peel_to_tree()?.get_path(rel).is_ok()`) AND not under `.git/`, `.ssh/`, or a global config. Pure in-process (no subprocess). Returns false on any error (fail-safe).
- `shell.rs`: `pub fn classify_command(cmd: &str) -> Decision` — (1) if `cmd` contains a shell metacharacter (`; & | ` `` ` `` $( ${ > < newline`) or a leading `FOO=bar`, return `Ask` ("compound/redirected command — review"); (2) `shell-words::split(cmd)` → argv; (3) hard `AutoDeny` if `argv[0]` ∈ {`sudo`,`su`,`pkexec`,`rm` with a non-cwd/`-rf` arg,`curl`,`wget`,`nc`,`ncat`} or `argv[0..2]` ∈ {`git push --force`/`--force-with-lease`,`git clean`,`git reset --hard` (configurable)}; (4) `AutoAllow` for read-only `argv[0]`+`argv[1]` (`git diff/status/log/show`, `cargo check/test/build/clippy/fmt`, `ls`,`cat`,`rg`,`grep`,`pwd`,`echo`); (5) else `Ask`.

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn reads_auto_allow_and_force_push_auto_deny() {
    let c = ApprovalClassifier::default();
    let d = tempfile::tempdir().unwrap();
    assert_eq!(c.classify("read_file", &serde_json::json!({"file_path":"x"}), d.path()).tier, Tier::AutoAllow);
    assert_eq!(c.classify("execute_command", &serde_json::json!({"command":"git push --force"}), d.path()).tier, Tier::AutoDeny);
    assert_eq!(c.classify("execute_command", &serde_json::json!({"command":"cargo test"}), d.path()).tier, Tier::AutoAllow);
}
#[test]
fn metachars_downgrade_to_ask() {
    let c = ApprovalClassifier::default();
    let d = tempfile::tempdir().unwrap();
    // 'cargo test' alone is AutoAllow, but chained it must become Ask (not auto-run the chained part)
    assert_eq!(c.classify("execute_command", &serde_json::json!({"command":"cargo test && rm -rf /"}), d.path()).tier, Tier::Ask);
}
#[test]
fn reversible_tracked_file_in_repo() {
    // init a git repo, commit a file, assert classify_path true for it + false for an untracked/escaping path
    let d = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(d.path()).unwrap();
    std::fs::write(d.path().join("tracked.rs"), "x").unwrap();
    // stage+commit tracked.rs (use git2 index/commit; helper in the test)
    commit_file(&repo, "tracked.rs");
    assert!(reversible::classify_path(d.path(), "tracked.rs"));
    assert!(!reversible::classify_path(d.path(), "untracked.rs"));
    assert!(!reversible::classify_path(d.path(), "../escape.rs"));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the three files. Add a `#[cfg(test)] commit_file` helper using git2 (write tree + commit to HEAD). Be conservative: anything not provably safe → `Ask` (never silently auto-allow). Add to workspace members.
- [ ] **Step 4: Run → PASS** (`cargo test -p oxidemx-approval`).
- [ ] **Step 5: Commit** — `feat(approval): risk-tiered ApprovalClassifier (git2 reversibility + shell argv safety)`

---

### Task 3: `oxidemx-harness` — the Verifier (cargo as ground truth)

**Files:** Create `oxidemx-harness/{Cargo.toml,src/lib.rs,src/verify.rs}`; Modify root `Cargo.toml`.

**Interfaces:**
- Deps: `oxidemx-ledger` (path), `serde`/`serde_json`, `thiserror`, `async-trait`, `tokio`.
- `verify.rs`: `#[async_trait] pub trait CommandRunner: Send+Sync { async fn run(&self, program: &str, args: &[String], cwd: &Path) -> CommandResult; }` where `pub struct CommandResult { pub ok: bool, pub stdout: String, pub stderr: String }`. (The real impl = `tokio::process::Command`, SP2d.) `#[cfg(test)] MockRunner` returns scripted results.
- `pub struct Verifier<R: CommandRunner> { runner: R }` with `pub async fn verify(&self, step_id: &str, program: &str, args: &[String], cwd: &Path, now: u64) -> Result<CompletionPromise, VerifyFailure>` — runs the command; on `ok` returns a `CompletionPromise { step_id, verifier: program, token: <short hash of ok+stdout>, ts: now }`; on failure returns `VerifyFailure { output: stderr+stdout (truncated) }` (the critique input for the reflection loop). `pub struct VerifyFailure { pub output: String }`.

- [ ] **Step 1: Failing tests**
```rust
#[tokio::test]
async fn verify_pass_yields_promise() {
    let v = Verifier::new(MockRunner::ok("Finished test"));
    let p = v.verify("a","cargo",&["test".into()], std::path::Path::new("."), 7).await.unwrap();
    assert_eq!(p.step_id, "a");
    assert!(!p.token.is_empty());
}
#[tokio::test]
async fn verify_fail_yields_critique() {
    let v = Verifier::new(MockRunner::fail("error[E0308]: mismatched types"));
    let f = v.verify("a","cargo",&["check".into()], std::path::Path::new("."), 7).await.unwrap_err();
    assert!(f.output.contains("E0308"));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `verify.rs` + `MockRunner`. `lib.rs` crate attrs + `pub mod verify;`. Add to workspace members.
- [ ] **Step 4: Run → PASS** (`cargo test -p oxidemx-harness`).
- [ ] **Step 5: Commit** — `feat(harness): Verifier + CommandRunner seam (cargo-as-ground-truth -> CompletionPromise)`

---

### Task 4: `oxidemx-harness` — the executor core loop (Worker seam)

**Files:** Create `oxidemx-harness/src/{worker.rs,executor.rs}`; Modify `lib.rs`.

**Interfaces:**
- `worker.rs`: `pub struct WorkerBrief { pub step_id: String, pub title: String, pub goal: String, pub inputs: serde_json::Value /* the validated payloads from `needs` */ }`; `pub struct StepOutput { pub text: String, pub output: serde_json::Value, pub tool_calls: Vec<ToolInvocation>, pub verify_cmd: Option<(String, Vec<String>)> }`; `pub struct ToolInvocation { pub name: String, pub args: serde_json::Value }`; `#[async_trait] pub trait Worker: Send+Sync { async fn run_step(&self, brief: WorkerBrief) -> Result<StepOutput, HarnessError>; }`. `#[cfg(test)] MockWorker` scripted per step id.
- `executor.rs`: `pub struct Executor<W,R> { worker: W, verifier: Verifier<R>, classifier: ApprovalClassifier, caps: Caps, cwd: PathBuf }`; `pub struct Caps { pub max_total_tool_calls: Option<u32> }`. `pub async fn run(&self, ledger: &TaskLedger, manifest: &mut TaskManifest, now: u64) -> RunReport`. Loop (single-worker sequential for THIS task; parallelism added in T5): while `ready_steps()` non-empty AND not all-terminal — pick a ready step, `start_step`, build a `WorkerBrief` (inputs = the `output`s of its `needs` steps), `worker.run_step`, record each tool call via `record_tool_call`, run the verifier if `verify_cmd` is Some → `complete_step` with the promise (else complete with a trivial promise for non-code steps), on worker/verify failure `fail_step`. `RunReport { completed, failed, blocked }`.

- [ ] **Step 1: Failing test (mock worker, multi-step)**
```rust
#[tokio::test]
async fn executor_runs_a_two_step_task_to_done() {
    let d = tempfile::tempdir().unwrap();
    let led = TaskLedger::new(d.path());
    let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "build x".into());
    m.steps = vec![ Step::new("a","plan"),
                    { let mut b = Step::new("b","code"); b.needs = vec!["a".into()]; b } ];
    led.create(&mut m, 0).unwrap();
    let worker = MockWorker::scripted(/* a -> output{}, b -> output + verify_cmd cargo check */);
    let exec = Executor::new(worker, Verifier::new(MockRunner::ok("ok")),
                             ApprovalClassifier::default(), Caps{max_total_tool_calls:None}, d.path().into());
    let report = exec.run(&led, &mut m, 10).await;
    assert_eq!(report.completed, 2);
    assert!(m.steps.iter().all(|s| s.status() == StepStatus::Done));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `worker.rs` + the sequential `executor.rs` loop (parallelism in T5). No lock across await.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** — `feat(harness): executor core loop over the ledger (Worker seam + verify-to-Done)`

---

### Task 5: Approval gating + caps + edge validation + parallelism

**Files:** Create `oxidemx-harness/src/edge.rs`; Modify `oxidemx-harness/src/executor.rs`, `Cargo.toml` (add `oxidemx-approval`, `jsonschema` default-features=false, `tokio` `JoinSet`).

**Interfaces:**
- `edge.rs`: `pub fn validate_edge(payload: &Value, schema: &Value) -> Result<(), String>` (jsonschema `validator_for`+`iter_errors`, like the planner). The executor, before passing a `needs` step's `output` as a consumer's input, validates it against the consumer's `input_schema` (if set) → on failure `block_step(…, "schema-violation: …")`.
- Executor gains: (a) **approval gating** — before recording each `ToolInvocation`, `classifier.classify(tool,args,cwd)`: `AutoDeny` → skip + fail/critique; `Ask` → `block_step(step,"needs-approval: …")` and CONTINUE to other ready steps (non-blocking); `AutoAllow`/`AutoAllowIfReversible` → proceed. (b) **caps** — track a process `total_tool_calls`; if a step would exceed `Caps.max_total_tool_calls` or its own `budget_exceeded()`, `block_step(…,"budget-exhausted")`. (c) **parallelism** — dispatch all currently-ready steps concurrently via `tokio::task::JoinSet` (clone the manifest's ready step briefs out, run workers concurrently, then apply results to the ledger sequentially to avoid races — collect results, then transition). Keep "no lock across await": don't hold a manifest borrow across the JoinSet await; snapshot briefs, await, then mutate.

- [ ] **Step 1: Failing tests**
```rust
#[tokio::test]
async fn ask_tier_tool_blocks_step_but_run_continues() {
    // a 2-independent-step task; step a's worker emits an `execute_command: "git commit -m x"` (Ask),
    // step b is a clean no-tool step. Assert: a -> Blocked(needs-approval), b -> Done, run doesn't hang.
}
#[tokio::test]
async fn edge_schema_violation_blocks_consumer() {
    // step a outputs {"n":"not-an-int"}, step b needs a + input_schema requires {n:integer}.
    // Assert b -> Blocked(schema-violation); a -> Done.
}
#[tokio::test]
async fn total_tool_cap_blocks() {
    // Caps.max_total_tool_calls = 1; a worker that wants 2 tool calls -> step Blocked(budget-exhausted).
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** edge validation + approval gating + caps + JoinSet parallelism. Add deps. `jsonschema` MUST be `default-features=false`.
- [ ] **Step 4: Run → PASS** (`cargo test -p oxidemx-harness`); confirm `cargo tree -p oxidemx-harness | grep -iE "reqwest|rustls"` shows none via jsonschema.
- [ ] **Step 5: Commit** — `feat(harness): approval gating + hard caps + edge schema-validation + parallel dispatch`

---

## Self-Review
- **Spec coverage:** §4.7 boundary 3 (edge) → T5; §5.1 ApprovalClassifier (tiers + reversibility + shell-safety, non-blocking Ask) → T2 + T5; §2.3 verifier/ground-truth → T3 + T4; §2.5 hard caps → T1 (budget) + T5 (total cap); §4.4 orchestrator-worker + parallel → T4 + T5; ledger primitives (record_tool_call/budget/TaskCreated, from SP2a/b followups) → T1. The SDD-flow template + autonomous-run D-Bus surface + the REAL Worker/model impls are SP2d (out of this plan, behind the `Worker`/`CommandRunner` seams).
- **Placeholders:** none; the MockWorker/MockRunner scripts are described per test. git2 commit helper spelled out as a test helper. The reflection/replan loop on verify-failure is minimal here (fail_step + critique captured) — the full generate→critique→refine iteration is a Worker-internal concern (SP2d) since it needs a real model; the executor records the failure + critique for it.
- **Type consistency:** `StepBudget`/`record_tool_call` (T1) used by T5 caps; `Tier`/`ApprovalClassifier`/`Decision` (T2) used by T5; `Verifier`/`CompletionPromise` (T3) used by T4; `Worker`/`WorkerBrief`/`StepOutput`/`Executor` (T4) extended by T5; `validate_edge` (T5) reuses the planner's jsonschema pattern. Seams (`Worker`,`CommandRunner`) keep real models/agentd out of SP2c.
