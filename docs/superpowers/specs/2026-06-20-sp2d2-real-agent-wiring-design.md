# SP2d-2 — Real agent wiring (gated Worker + policy Planner) — design

Date: 2026-06-20
Status: design (brainstormed + approved in-chat; pending spec review → writing-plans)
Second slice of SP2d. Wires the harness seams (built SP2a–SP2d-1) to a REAL agent:
the gated tool executor → `route_turn`, a real `Worker`, and a policy-driven
`PlannerModel`. Builds on `docs/superpowers/specs/2026-06-19-sp2d1-safety-primitives-design.md`
(GatedToolExecutor + RealCommandRunner) and the SP2 spec §4.2/§4.7/§5.1.
Read `docs/AI-ARCHITECTURE-STATUS.md` first.

## 1. Goal & scope

Connect the fully-tested harness engine to a live model so it can actually run a
coding step: gate every tool inline, run a step as an agent turn, and plan with the
right model. Headless-testable via the existing mock-provider/`MockEngine`/mock-Approver
seams; the live model path is exercised behind the real impls.

**In scope:** (1) wire `GatedToolExecutor` into `CoreTurnRunner`; (2) the `ApproverPrompt`
adapter (Attended approvals via SP1b `Approver`); (3) the real `CoreWorker`
(harness `Worker` seam, thin/text contract); (4) the gate↔executor reconciliation
(strip the executor's redundant post-hoc classify; propagate `NeedsApproval`); (5) the
`PolicyPlannerModel` (cloud-first planning + `LocalPreferred`/`Auto` toggles).

**Out of scope → SP2d-3:** the autonomous run-mode that drives the executor over a real
task graph + the `RunTask`/`TaskStatus`/`ResumeTask`/`ReviewApprovals` D-Bus surface +
the SDD-flow template. Structured edge schemas (the Step carries `input/output_schema`
already; tightening them is later). OS sandbox (defense-in-depth, later).

## 2. Decisions (locked in brainstorm)

- **Worker contract = thin/text first** (the agent turn's reply is the step output;
  structured edges minimal for now); the **Planner sets `verify_cmd`** per step.
- **Inline gate is authoritative; strip the executor's redundant classify.** SP2d-1's
  `GatedToolExecutor` gates every tool BEFORE execution (preventive). The SP2c executor's
  post-hoc `ApprovalClassifier` pass is now redundant and is REMOVED; approval reaches the
  executor only as a `NeedsApproval` outcome the Worker surfaces. (`oxidemx-harness` drops
  its `oxidemx-approval` dependency.)
- **Planning is cloud-first by default** (quality-critical); `LocalPreferred` / `Auto`
  (token-budget-aware) are opt-in policies. The small local model serves the *cheap*
  roles (routing/compaction), not the critical plan.

## 3. Tool-gate + approval wiring

- `CoreTurnRunner::run_turn` (agentd) builds
  `GatedToolExecutor::new(Arc::new(AgentToolExecutor::new(paths.clone(), host.clone())),
  ApprovalClassifier::from_config(project_cfg), prompt, mode, paths.cwd.clone())` and
  passes it as `exec` to `route_turn`. Every tool the agent calls is gated inline.
- `mode: GateMode` — `Attended` for interactive chat (the default chat path), `Autonomous`
  for an unattended task run (SP2d-3 sets it). Per-project allow/deny rules come from
  `<cwd>/.oxidemx/config.toml` via `ApprovalClassifier::from_config` (§5.1).
- **`ApproverPrompt`** (agentd) implements SP2d-1's `ApprovalPrompt`: `confirm(tool, reason)`
  → builds an approval card, calls the SP1b `Approver::request(...)` (emits
  `ApprovalRequested` + awaits the oneshot the overlay's RespondApproval resolves), maps
  the `Verdict` to `bool`. No lock held across the await (the `Approver` already handles
  this). In Autonomous mode `prompt` is `None` → Ask-tier returns `NEEDS_APPROVAL`.

## 4. The real `CoreWorker` (thin/text)

- `CoreWorker` (agentd) implements `oxidemx_harness::Worker`:
  `run_step(brief) -> Result<StepOutput, HarnessError>`.
- It runs ONE agentic turn via `oxidemx_agent_core::runtime::route_turn`:
  prompt = the step brief (`brief.title` + `brief.goal` + the `brief.inputs` from
  `needs`), `exec` = the `GatedToolExecutor`, a `StreamBridge` to capture `tool`-kind
  events (→ `StepOutput.tool_calls`). The turn's reply text → `StepOutput.text` and
  `output = {"text": reply, "artifacts": [...]}`.
- `verify_cmd` is read from the Step the Planner emitted (the Planner sets it for code
  steps, e.g. `("cargo", ["check"])`); the Worker forwards it in `StepOutput.verify_cmd`.
  (`PlanStep` gains an optional `verify` field — SP2b's `PlanOutput` extended; if absent,
  the step is treated as non-code and the executor completes it with the trivial promise.)
- **`NeedsApproval` propagation:** the `GatedToolExecutor` records each block (AutoDeny /
  Ask-in-Autonomous) into a shared `GateLog` (`Arc<Mutex<Vec<GateBlock{tool,reason}>>>`)
  the Worker holds. After the turn, if `GateLog` is non-empty, `run_step` returns
  `Err(HarnessError::NeedsApproval { tool, reason })`. The executor maps it to
  `block_step("needs-approval: …")` (non-blocking; the run continues other steps).

## 5. Gate ↔ executor reconciliation (`oxidemx-harness` change)

- Remove the executor's post-hoc `classifier.classify(...)` step and the
  `ApprovalClassifier`/`oxidemx-approval` dependency from `oxidemx-harness`.
- Add `HarnessError::NeedsApproval { tool: String, reason: String }`.
- Executor's run loop: on a worker `Err(HarnessError::NeedsApproval{..})` →
  `block_step(needs-approval)` + continue. The caps (`max_total_tool_calls`, per-step
  budget), edge schema-validation, the verifier-to-Done gate, and the re-pick guard ALL
  stay unchanged. The 3 SP2c approval tests are reworked to drive the new path: a mock
  `Worker` that returns `Err(NeedsApproval)` → the step ends `Blocked`, the run continues.

## 6. The `PolicyPlannerModel` (cloud-first; toggles)

- `PolicyPlannerModel` (agentd) implements `oxidemx_planner::PlannerModel`:
  `complete(req) -> Result<String, PlannerError>`. Holds a `PlannerPolicy` +
  handles to the cloud provider factory and the `LocalModelService`.
- `pub enum PlannerPolicy { Cloud, LocalPreferred, Auto }` (config-default `Cloud`; a
  task run may override).
- `complete` model selection:
  - **`Cloud`** → always the cloud model (quality-first); `req.escalate` retries (same or
    a stronger cloud model — config `planner_escalate_model`).
  - **`LocalPreferred`** → `escalate=false` → the local model via the SP2b
    `SchemaConstraint::JsonSchema(req.json_schema)` (generate-time gate); `escalate=true`
    → cloud. (This is the SP2b local→cloud retry/escalate path, opt-in.)
  - **`Auto`** → `Cloud` unless the run's remaining token budget is below a config
    threshold, then behave like `LocalPreferred`. Budget is read from the ledger/run
    context (§2.5).
- The cheap local roles (routing/compaction inside the Worker) are unaffected — they
  already use the local model where wired; this policy governs ONLY the plan call.

## 7. Architecture / data flow

```
Planner (SP2b) ──complete(PlanRequest)──▶ PolicyPlannerModel
                                            ├ Cloud (default)  → cloud provider
                                            └ LocalPreferred/Auto → local (SchemaConstraint) → cloud on escalate
Executor (SP2c) ──run_step(brief)──▶ CoreWorker ──route_turn(exec=GatedToolExecutor)──▶ reply + tool events
                                       │  GatedToolExecutor: classify→inner | Deny/Ask→GateLog + Err
                                       └ GateLog non-empty → Err(NeedsApproval) → executor block_step
```

## 8. Error handling

- Gate `NeedsApproval`/denied → `HarnessError::NeedsApproval` → `Blocked` step (recoverable;
  the user/`ReviewApprovals` resolves it in SP2d-3). Worker LLM error → `fail_step`.
  Planner all-models-fail → `PlannerError::Unresolved` (SP2b). No lock across await in any
  path (Worker, ApproverPrompt, PlannerModel all only await provider/Approver calls).

## 9. Testing (headless)

- **CoreWorker:** over the existing mock-provider seam (or a `MockTurnRunner`-style inner)
  + a mock `GatedToolExecutor`/`GateLog`: a step runs → `StepOutput{text,output,tool_calls,
  verify_cmd}`; a gated-Deny tool during the turn → `Err(NeedsApproval)`. (The real
  `route_turn` path needs a live provider — covered by the live-bus/`--ignored` test.)
- **Executor reconciliation:** the reworked SP2c tests — mock `Worker` returns
  `NeedsApproval` → step `Blocked`, run continues; caps/edge tests unchanged + still green.
- **PolicyPlannerModel:** mock cloud + mock `LocalModelService`; assert `Cloud`→cloud call,
  `LocalPreferred`+escalate=false→local-with-constraint, escalate=true→cloud, `Auto`+
  low-budget→local. No live models.
- **ApproverPrompt:** a `RecordingApprover` (or drive the real `Approver` + respond) →
  approve→true, deny→false.

## 10. Out of scope / sequencing

SP2d-3 (autonomous run-mode + D-Bus `RunTask`/`TaskStatus`/`ResumeTask`/`ReviewApprovals`
+ SDD-flow template) turns these pieces into a user-drivable autonomous loop. Then the OS
sandbox (defense-in-depth), then SP2e (AutoAgents adoptions). The SP2c followups (real
per-call `ok` into `record_tool_call`; code-step verify gate via `Step.kind`) land in
SP2d-2/3 as the real worker reports per-call results.
