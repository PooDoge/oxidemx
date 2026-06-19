# SP2 — Autonomous coding harness — design

Date: 2026-06-19
Status: design (research-grounded proposal; authored autonomously while the user is away — **for review on return**; foundational, design-stable pieces may begin building per the user's "continue building out everything we can" directive).
Part of the agent re-architecture (`docs/superpowers/specs/2026-06-18-agent-framework-rearchitecture-design.md`). Builds on SP1a–SP1c (agentd backend complete + headless-tested).
Grounded in `docs/research/{autoagents-capabilities,autoagents-patterns,agent-harness-best-practices}.md`. Read `docs/AI-ARCHITECTURE-STATUS.md` first.

## 1. Goal

Make the agent run **autonomously** as a **coding-first** harness: take a goal, decompose it into a resumable to-do/step plan, execute steps (editing files, running shell + `cargo`), self-verify against the compiler/tests, run sub-agents in parallel where useful, and persist all of it so it survives restarts/compaction — using the small **local model** to drive cheap flow logic (planning, routing, compaction) via **schema-gated structured output**, offloading the cloud.

## 2. Research-driven principles (the non-negotiables)

From `docs/research/agent-harness-best-practices.md`:
1. **The Ledger is the foundation** — an atomic-write, on-disk task manifest (step status, artifacts, budgets) is the prerequisite for resumability, inspection, approval gates, and parallel workers.
2. **Thin workers, fresh context per step** — bounded prompt context per spawned worker; fresh sub-agent per step beats accumulated history (context rot is the #1 long-run failure).
3. **Compiler/tests are ground truth** — a step is `Done` only when a verifier tool writes a machine-readable completion token, never on agent self-report. Every code step ends in `cargo check`/`cargo test`.
4. **Local model for schema-gated structured flow logic** — spec→typed step-DAG, routing, compaction; every local-model structured output passes a JSON-schema validator before entering the conductor; retry once, escalate to cloud on second failure. **Verify, don't trust.**
5. **Hard caps in code, not prompts** — `max_tool_calls_per_step`, `max_total_tool_calls`, context thresholds, `max_wall_clock`, enforced by the conductor/JoinSet at dispatch, not asked for in a prompt.

## 3. Architecture (layers; reuse what exists)

```
                ┌─────────────────────────── agentd ───────────────────────────┐
  goal/spec ──▶ │ TaskLedger (NEW, atomic on-disk manifest + event log)         │
                │   └ resumable: rebuilt on startup; the source of truth         │
                │ Planner (NEW): spec → typed StepGraph  [LOCAL model, schema]   │
                │ oxidemx-conductor (EXISTING DAG engine): runs the StepGraph    │
                │   ├ joins / retry / timeout / cancel (keep — research: keep)   │
                │   └ dispatches steps to workers (parallel via JoinSet)         │
                │ Workers = AutoAgents ReActAgent + native tools (fresh context) │
                │   ├ patterns: orchestrator-worker, reflection, parallel        │
                │   └ verifier tool: cargo check/test → CompletionPromise token  │
                │ AutoAgents adoptions: PipelineBuilder(retry+cache), guardrails, │
                │   #[derive(AgentOutput)] structured output, AgentHooks(caps),  │
                │   telemetry                                                     │
                │ Journal (EXISTING, SP1b): every step/tool/decision recorded    │
                └───────────────────────────────────────────────────────────────┘
```

**Decision (research-backed): keep `oxidemx-conductor`; do NOT replace it with AutoAgents' runtime.** AutoAgents' `SingleThreadedRuntime`/`Topic`/`Environment` is flat pub-sub with no DAG, joins, per-node retry/timeout, or cancel — exactly what the conductor provides. They compose: the conductor dispatches parallel steps *to* AutoAgents agents. The Ledger is a persistence layer **over** the conductor's `FlowDoc`/run model.

## 4. Components

### 4.1 TaskLedger (the foundation — build first)
- An atomic-write on-disk manifest per task under the project store (`projects/<key>/tasks/<task_id>/`): `manifest.json` (goal, steps[], each with `id/title/status/needs/artifacts/budget_spent/verifier_token`), `events.jsonl` (append-only step/tool/decision log — unifies with the SP1b journal), `artifacts/`.
- `Status ∈ {Pending, Running, Blocked, Done, Failed, Skipped}`. `Done` requires a `CompletionPromise` written by a verifier, not the agent.
- **Resumable:** on agentd startup, scan for in-flight tasks; rebuild state from the manifest + event log; offer to resume. Atomic writes (write-temp+rename) so a crash never corrupts it.
- Maps onto the conductor: a `FlowPlan` is derived from / kept in sync with the ledger's `StepGraph`; conductor `RunEvent`s append to `events.jsonl`.

### 4.2 Planner (local-model, schema-gated)
- `spec/goal → StepGraph` via the **local model** with `#[derive(AgentOutput)]` + strict `StructuredOutputFormat`; the typed result is validated against a JSON schema before becoming a `StepGraph`. Retry once on invalid; escalate to cloud on second failure.
- Also hosts the cheap local roles (research-backed): routing/classification (already in `oxidemx-agent-local` modes), step summarization/compaction for context control.

### 4.3 Step execution (orchestrator-worker, thin workers)
- The conductor runs the `StepGraph`; each ready step is dispatched to a **fresh** worker (AutoAgents `ReActAgent` + the native tool set from SP1c), with **bounded context** (the step's own brief + only the artifacts it `needs`), not the whole transcript.
- **Verifier tool** (`cargo check`/`cargo test`/custom): a step that emits code must call it; only its machine-readable pass token flips the step to `Done`. Replans on failure (the `planning` pattern's PARTIAL/BLOCKED replanning).

### 4.4 Patterns (from `docs/research/autoagents-patterns.md`, all 0.3.7-safe)
- **Planning / orchestrator-worker** — decompose → run steps → replan on compiler/test failure (the spine).
- **Reflection / evaluator-optimizer** — generate→critique→refine for patches, with `cargo` output as the critique input; `max_iterations` cap.
- **Parallel** — fan-out independent steps / multi-lens review (security/style/tests) concurrently, merge results (conductor JoinSet + AutoAgents `SubmissionId` aggregation).
- **Routing** — local classifier picks model/path per step.

### 4.5 Spec-driven-development as a first-class flow
- Encode the project's own proven loop (**brainstorm → spec → plan → implement-per-step → verify → review**) as a built-in `FlowDoc` template the harness can run autonomously on a goal — mirroring the superpowers method this project is built with (ledger + fresh-worker-per-task + per-step review gate + final review). This is the headline "spec-driven development harness" the user asked for.

### 4.6 AutoAgents adoptions (research-backed, incremental, zero/low-risk)
1. `PipelineBuilder` + retry/cache `LLMLayer` at the provider factory seam (rate-limit backoff + response cache) — drop-in.
2. Guardrails (`PromptInjectionGuard`, `RegexPiiRedactionGuard`) on tool-output/input (tool output is an injection surface).
3. `#[derive(AgentOutput)]` + strict structured output → typed `StepGraph`/`ReviewReport`/`DiagnosticSummary` (replaces ad-hoc parsing).
4. `AgentHooks::on_tool_call/on_tool_result` → enforce the hard caps + structured tool logging.
5. `autoagents-telemetry` OTLP over the existing `Event` stream → spans + turn/token metrics.

## 5. Autonomous-operation safety
- **Hard caps in code** (§2.5) at the conductor/JoinSet + via AgentHooks. **Approval policy** per tool (SP1b `Approver`): destructive/host tools gated unless `Autonomous` policy is explicitly set for unattended runs. **Budget** (token/turn/wall-clock) tracked in the ledger; stop + escalate on exhaustion. **Ground-truth gating** (§2.3). **Resumability** (§4.1) so unattended crashes recover.

## 6. Rust / AI conventions (from `docs/research/agent-harness-best-practices.md`)
- Async: tokio structured concurrency, `JoinSet` for fan-out, `CancellationToken` for cancel (already used by the conductor). No lock across `.await` (project rule).
- Errors: `thiserror` for library crates, typed `#[non_exhaustive]` enums. `#![forbid(unsafe_code)]`.
- Typed structured output: `serde` + JSON schema validation at the boundary; never trust raw model text for control flow.
- Naming: `*Ledger`/`*Graph`/`Step`/`Worker`/`Planner`/`Verifier`/`*Policy`; flows are `FlowDoc`/`FlowPlan` (existing). Agent/tool types follow AutoAgents (`*Agent`, `ToolT`).
- Testing: mock providers + `MockEngine` (existing) for control logic; golden/eval tests for planner output shape; the compiler/tests are the harness's own ground truth in integration tests.

## 7. Decomposition (sub-projects; each its own plan → SDD)
- **SP2a — TaskLedger** (the foundation): atomic manifest + event log + resume-on-startup + conductor integration. Headless-testable. **Build first** (research consensus; design-stable; low-risk).
- **SP2b — Local-model Planner**: schema-gated `spec→StepGraph` + routing/compaction roles. Headless-testable with mock + the local engine behind `mistral`.
- **SP2c — Orchestrator-worker execution + verifier + patterns** (planning/reflection/parallel/routing) over the ledger+conductor, thin workers, hard caps via AgentHooks.
- **SP2d — SDD flow template** (§4.5) + autonomous run mode (budget/approval/resume) exposed over the `org.oxidemx.Agent` D-Bus surface (RunTask/TaskStatus/ResumeTask).
- **SP2e — AutoAgents adoptions** (§4.6) — pipeline/guardrails/telemetry; can land incrementally alongside.

## 8. Open questions for the user (resolved provisionally; confirm on return)
1. **Autonomous default approval policy** — provisional: gated-by-default (destructive/host tools need approval); an explicit `--autonomous`/config opt-in flips to autonomous with hard caps. (Safer default.)
2. **Ledger vs FlowDoc relationship** — provisional: the ledger is the persistent source of truth; a `FlowPlan` is derived from it for execution (not a second store). Confirm we don't want the ledger to BE a FlowDoc extension instead.
3. **How much to build autonomously now** — provisional: build SP2a (TaskLedger) + SP2e adoptions (low-risk, design-stable) while away; hold SP2b–d (planner + execution semantics) for review since they encode more contested judgment.

## 9. Out of scope
SP1c T8b (overlay flip+delete — gated on the user's GUI walkthrough). SP-Learn (the self-improving loop consumes this harness's ledger/journal later). Federation/external workers (later SP). GUI for task/step visualization (after the backend harness works).
