# SP2d-2 — Real agent wiring (gated Worker + policy Planner) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Wire the harness seams to a live agent — the inline tool-gate into the turn path, a real `CoreWorker`, the harness approval reconciliation, and a cloud-first `PolicyPlannerModel`.

**Architecture:** `oxidemx-harness` drops its post-hoc approval classify (the SP2d-1 inline gate is authoritative) and instead blocks on a `HarnessError::NeedsApproval` the Worker surfaces. In agentd, `GatedToolExecutor` gains a `GateLog`, an `ApproverPrompt` adapter wraps SP1b's `Approver`, `CoreWorker` runs a step via `route_turn` with the gated executor, and `PolicyPlannerModel` plans cloud-first (with local/budget opt-ins). Testable logic is mock-tested; the `route_turn`/live-provider glue is compile-wired + live-tested (same discipline as SP1c).

**Tech Stack:** Rust, tokio, async-trait; `oxidemx-harness`, `oxidemx-planner`, `oxidemx-approval`, `oxidemx-agent-core` (route_turn), `oxidemx-agent-local`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-20-sp2d2-real-agent-wiring-design.md`. Decisions: thin/text Worker; inline gate authoritative (strip executor classify); **cloud-first planning** + `LocalPreferred`/`Auto` toggles.
- `#![forbid(unsafe_code)]`; no `unwrap`/`expect` outside tests; poison-safe locks; **no lock/guard held across `.await`**; thiserror; `clippy -D warnings`; per-crate `cargo test` green each task; `cargo tree -p agentd | grep -i mistralrs` empty on default; `oxidemx-harness`/`oxidemx-planner` keep no reqwest/rustls via jsonschema.
- `oxidemx-harness` MUST drop its `oxidemx-approval` dependency (T1).
- The `route_turn` path is built from config (no injectable provider) → its real execution is compile-wired + the `#[ignore]` live test; the unit-testable logic is extracted to pure/mock-tested helpers.
- Worktree `../oxidemx-phase1`, branch `phase1-local-llm-gateway`. Commit per task.

## File structure

```
oxidemx-harness/src/executor.rs   # T1: drop classify; HarnessError::NeedsApproval -> block_step
oxidemx-harness/src/worker.rs     # T1: HarnessError gains NeedsApproval
oxidemx-harness/Cargo.toml        # T1: remove oxidemx-approval dep
agentd/src/tools/gated.rs         # T2: GateLog + GateBlock; gate records blocks
agentd/src/agent/approver_prompt.rs # T2: ApproverPrompt (impl ApprovalPrompt over SP1b Approver)
agentd/src/harness/worker.rs      # T3: CoreWorker (impl oxidemx_harness::Worker) + pure helpers
agentd/src/harness/planner.rs     # T4: PolicyPlannerModel (impl oxidemx_planner::PlannerModel)
oxidemx-planner/src/planner.rs    # T3: PlanStep gains optional `verify`
agentd/src/interface.rs           # T3: CoreTurnRunner wraps exec in GatedToolExecutor (chat=Attended)
```

---

### Task 1: Harness reconciliation — strip classify, add `NeedsApproval`

**Files:** Modify `oxidemx-harness/src/{executor.rs,worker.rs,Cargo.toml}`.

**Interfaces:**
- `HarnessError` gains `NeedsApproval { tool: String, reason: String }` (`#[non_exhaustive]` already).
- The executor's run loop: REMOVE the `ApprovalClassifier::classify(...)` step (and the `classifier` field + `oxidemx-approval` dep). On a worker result `Err(HarnessError::NeedsApproval{tool,reason})` → `block_step(manifest, step_id, &format!("needs-approval: {tool}: {reason}"), now)` + continue (non-blocking). Caps (total + per-step), edge schema-validation, the verifier-to-Done gate, the re-pick guard, and concurrency are UNCHANGED. `Executor::new` loses the `classifier` param.

- [ ] **Step 1: Rework the 3 approval tests** to the new path (replace the classifier-driven tests). Example replacing `ask_tier_tool_blocks_step_but_run_continues`:
```rust
#[tokio::test]
async fn needs_approval_blocks_step_run_continues() {
    let env = ExecEnv::new();  // ledger + 2 independent steps a,b
    // MockWorker: step "a" -> Err(HarnessError::NeedsApproval{tool:"execute_command".into(), reason:"git commit".into()});
    //             step "b" -> Ok(StepOutput::done())
    let report = env.run().await;
    assert_eq!(env.status("a"), StepStatus::Blocked);
    assert_eq!(env.status("b"), StepStatus::Done);   // run continued
    assert!(report.blocked >= 1);
}
```
(Keep the `total_tool_cap_blocks` + `edge_schema_violation_blocks_consumer` tests — they don't use the classifier; only the approval-via-classify tests are reworked. The `MockWorker` gains a way to return a scripted `Err(NeedsApproval)` per step id.)
- [ ] **Step 2: Run → FAIL** (`cargo test -p oxidemx-harness`).
- [ ] **Step 3: Implement** — delete the classifier field/param/dep + the classify branch; add the `NeedsApproval` variant + the `block_step` mapping in the result-apply loop.
- [ ] **Step 4: Run → PASS**; confirm `cargo tree -p oxidemx-harness | grep -i approval` empty + no reqwest/rustls.
- [ ] **Step 5: Commit** — `refactor(harness): inline gate authoritative — drop classify, block on Worker NeedsApproval`

---

### Task 2: `GateLog` + `ApproverPrompt` adapter

**Files:** Modify `agentd/src/tools/gated.rs`; Create `agentd/src/agent/approver_prompt.rs`; Modify `agentd/src/agent/mod.rs`.

**Interfaces:**
- `gated.rs`: `pub struct GateBlock { pub tool: String, pub reason: String }`; `pub type GateLog = Arc<Mutex<Vec<GateBlock>>>`. `GatedToolExecutor` gains an `Option<GateLog>` field (constructor arg). On a block (`AutoDeny`, or `Ask` returning `NEEDS_APPROVAL`), if `Some(log)` → push a `GateBlock` (poison-safe lock, no await held). Chat path passes `None`; the Worker passes `Some`.
- `approver_prompt.rs`: `pub struct ApproverPrompt { approver: Arc<Approver>, project: String, thread: String }` impl `oxidemx_agent_core::…`? No — impl `crate::tools::gated::ApprovalPrompt`: `async fn confirm(&self, tool, reason) -> bool` → builds a card json, calls `self.approver.request(&self.project, &self.thread, card).await` (the SP1b `Approver`), maps `Verdict::{Allow|Always}` → true, else false. No lock across the await.

- [ ] **Step 1: Failing tests**
```rust
#[tokio::test]
async fn gate_records_block_in_log_on_deny() {
    let log: GateLog = Arc::new(Mutex::new(Vec::new()));
    let rec = Arc::new(RecordingExecutor::default());
    let g = GatedToolExecutor::new(rec.clone(), ApprovalClassifier::default(), None,
        GateMode::Autonomous, tempfile::tempdir().unwrap().path().into(), Some(log.clone()));
    let _ = g.execute("execute_command", serde_json::json!({"command":"git push --force"}), &None).await;
    assert!(rec.calls().is_empty());                         // gate held
    assert_eq!(log.lock().unwrap_or_else(|e| e.into_inner()).len(), 1);  // recorded
}
#[tokio::test]
async fn approver_prompt_maps_verdict_to_bool() {
    // a #[cfg(test)] Approver-like that returns Verdict::Allow -> confirm() == true; Deny -> false
    // (drive the real Approver: spawn confirm(), respond Allow via approver.respond, assert true)
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the `GateLog`/`GateBlock` field + recording in `gated.rs` (add the `Option<GateLog>` param to `new`; update SP2d-1's call sites/tests to pass `None`), and `ApproverPrompt`. Confirm the SP2d-1 gated tests still pass (add `None` arg).
- [ ] **Step 4: Run → PASS** (`cargo test -p agentd`).
- [ ] **Step 5: Commit** — `feat(agentd): GateLog (block audit) + ApproverPrompt adapter (Attended approvals via Approver)`

---

### Task 3: `CoreWorker` + gate-into-chat + `PlanStep.verify`

**Files:** Create `agentd/src/harness/worker.rs`, `agentd/src/harness/mod.rs`; Modify `agentd/src/interface.rs` (CoreTurnRunner gate wiring), `agentd/src/lib.rs`, `oxidemx-planner/src/planner.rs` (PlanStep.verify).

**Interfaces:**
- `oxidemx-planner` `PlanStep` gains `#[serde(default)] pub verify: Option<(String, Vec<String>)>` (the verify command the planner emits per code step); `Planner::plan` maps it onto the ledger `Step`… actually the `Step` doesn't carry verify; the Worker reads it from the brief. Simpler: `WorkerBrief` is built by the executor from the `Step`; but `Step` has no verify field. **Decision:** thread the verify via `StepOutput.verify_cmd` set by the Worker from the *plan* — so the `CoreWorker` needs the plan's per-step verify. Carry it on the ledger `Step` instead: add `#[serde(default)] pub verify: Option<(String,Vec<String>)>` to `oxidemx-ledger::Step` (one field), the Planner sets it via `StepGraph`, the executor copies it into the `WorkerBrief`, and the Worker forwards it as `StepOutput.verify_cmd`. (Update `WorkerBrief` to carry `verify: Option<(String,Vec<String>)>`.)
- `agentd/src/harness/worker.rs`: `pub struct CoreWorker { emitter: Arc<dyn EventEmitter>, host: Arc<dyn HostCapability>, classifier: ApprovalClassifier, paths: ProjectPaths }` impl `oxidemx_harness::Worker`. `run_step(brief)`:
  - build a `GateLog` + a `GatedToolExecutor` (Autonomous, prompt=None, `Some(gatelog)`) over a fresh `AgentToolExecutor(paths, host)`;
  - run `route_turn(AgentMode::Agentic, "", &prompt_from(brief), Some(stream_sink), &history_from(brief), None, &session_id, &exec)` — capturing tool events via the StreamBridge;
  - **after the turn:** if the `GateLog` is non-empty → `Err(HarnessError::NeedsApproval{ tool, reason })` (first block); on `route_turn` Err → `Err(HarnessError::Worker(e))`; else `Ok(StepOutput{ text: reply, output: json!({"text": reply}), tool_calls: <captured>, verify_cmd: brief.verify.clone() })`.
- **Pure helpers (unit-tested without route_turn):** `fn prompt_from(brief: &WorkerBrief) -> String`; `fn gatelog_to_outcome(log: &[GateBlock]) -> Option<HarnessError>` (returns `Some(NeedsApproval)` if non-empty). The `route_turn` call is the compile-wired glue.
- `interface.rs`: `CoreTurnRunner::run_turn` wraps its `AgentToolExecutor` in `GatedToolExecutor::new(..., ApprovalClassifier::from_config_or_default(paths), Some(ApproverPrompt), GateMode::Attended, paths.cwd, None /* chat needs no GateLog */)`. (Chat now gates tools + surfaces approvals via the card.)

- [ ] **Step 1: Failing tests** (pure helpers + the planner field)
```rust
#[test]
fn gatelog_to_outcome_flags_needs_approval() {
    assert!(super::gatelog_to_outcome(&[]).is_none());
    let o = super::gatelog_to_outcome(&[GateBlock{tool:"x".into(),reason:"r".into()}]);
    assert!(matches!(o, Some(HarnessError::NeedsApproval{..})));
}
#[test]
fn plan_step_verify_round_trips() {  // in oxidemx-planner
    let j = r#"{"steps":[{"id":"a","title":"code","needs":[],"verify":["cargo",["check"]]}]}"#;
    let p: PlanOutput = serde_json::from_str(j).unwrap();
    assert_eq!(p.steps[0].verify, Some(("cargo".into(), vec!["check".into()])));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `CoreWorker` (with the pure helpers), the `verify` fields (`Step`/`WorkerBrief`/`PlanStep`), the executor copying `Step.verify`→`WorkerBrief.verify`, and the CoreTurnRunner gate wiring. Build both `cargo build -p agentd` + `--features mistral`. Extend the `#[ignore]` live test to drive a gated tool-using turn if cheap.
- [ ] **Step 4: Run → PASS** (`cargo test -p agentd -p oxidemx-planner -p oxidemx-ledger`); mistralrs absent on default.
- [ ] **Step 5: Commit** — `feat(agentd): CoreWorker (gated route_turn) + chat tool-gating + PlanStep.verify`

---

### Task 4: `PolicyPlannerModel` (cloud-first; toggles)

**Files:** Create `agentd/src/harness/planner.rs`; Modify `agentd/src/harness/mod.rs`.

**Interfaces:**
- `pub enum PlannerPolicy { Cloud, LocalPreferred, Auto }` (config-default `Cloud`).
- `pub struct PolicyPlannerModel { policy: PlannerPolicy, cloud: Arc<dyn CloudComplete>, local: Arc<dyn LocalModelService>, budget_floor: u64, remaining_budget: u64 }` impl `oxidemx_planner::PlannerModel`. `CloudComplete` is a small seam (`async fn complete(&self, system, goal, json_schema, stronger: bool) -> Result<String, String>`) so the cloud path is mock-testable; the real impl calls the cloud provider factory.
- `complete(req)` selection:
  - `Cloud` → `cloud.complete(system, goal, schema, req.escalate)` (escalate → `stronger=true`).
  - `LocalPreferred` → if `!req.escalate` → `local.chat_with_model(..., SchemaConstraint::JsonSchema(req.json_schema))`-style call (the SP2b constrained path) ; else → `cloud.complete(..., stronger=false)`.
  - `Auto` → if `remaining_budget >= budget_floor` behave as `Cloud`, else as `LocalPreferred`.
  - Map errors to `PlannerError::Model`.

- [ ] **Step 1: Failing tests** (mock `CloudComplete` + mock `LocalModelService`, recording which was called)
```rust
#[tokio::test]
async fn cloud_policy_uses_cloud() {
    let (cloud, local) = (RecordingCloud::ok("{}"), RecordingLocal::ok("{}"));
    let m = PolicyPlannerModel::new(PlannerPolicy::Cloud, cloud.clone(), local.clone(), 0, 0);
    m.complete(req(false)).await.unwrap();
    assert_eq!(cloud.calls(), 1); assert_eq!(local.calls(), 0);
}
#[tokio::test]
async fn local_preferred_uses_local_then_cloud_on_escalate() {
    let (cloud, local) = (RecordingCloud::ok("{}"), RecordingLocal::ok("{}"));
    let m = PolicyPlannerModel::new(PlannerPolicy::LocalPreferred, cloud.clone(), local.clone(), 0, 0);
    m.complete(req(false)).await.unwrap(); assert_eq!(local.calls(), 1);
    m.complete(req(true)).await.unwrap();  assert_eq!(cloud.calls(), 1);
}
#[tokio::test]
async fn auto_low_budget_uses_local() {
    let (cloud, local) = (RecordingCloud::ok("{}"), RecordingLocal::ok("{}"));
    let m = PolicyPlannerModel::new(PlannerPolicy::Auto, cloud.clone(), local.clone(), 1000, 10 /*remaining < floor*/);
    m.complete(req(false)).await.unwrap();
    assert_eq!(local.calls(), 1); assert_eq!(cloud.calls(), 0);
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `PolicyPlannerModel` + the `CloudComplete` seam (+ a real impl over the cloud factory, compile-wired) + the mock recorders. No lock across await.
- [ ] **Step 4: Run → PASS** (`cargo test -p agentd`).
- [ ] **Step 5: Commit** — `feat(agentd): PolicyPlannerModel — cloud-first planning + LocalPreferred/Auto toggles`

---

## Self-Review
- **Spec coverage:** §3 gate+ApproverPrompt wiring → T2 (adapter + GateLog) + T3 (CoreTurnRunner wiring); §4 CoreWorker thin/text + NeedsApproval → T3; §5 strip classify + NeedsApproval → T1; §6 PolicyPlannerModel cloud-first + toggles → T4; §2.5 budget-aware Auto → T4. The autonomous run-mode + D-Bus (SP2d-3) is out of scope.
- **Placeholders:** none; test + impl concrete. The `route_turn`/cloud-factory glue is explicitly compile-wired + live-tested (the pure helpers + the mock-seam'd PolicyPlannerModel/CloudComplete carry the unit tests) — the same honest discipline as SP1c.
- **Type consistency:** `HarnessError::NeedsApproval` (T1) is returned by `CoreWorker` (T3); `GateLog`/`GateBlock` (T2) consumed by `CoreWorker` (T3); `ApproverPrompt` (T2) used by CoreTurnRunner (T3); `Step.verify`/`WorkerBrief.verify`/`PlanStep.verify` consistent across ledger/harness/planner (T3); `PlannerPolicy`/`CloudComplete` (T4). The `verify` field is added to `oxidemx-ledger::Step` (one new `#[serde(default)]` field — backward compatible).
