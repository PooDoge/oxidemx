# SP2 — Autonomous coding harness — design

Date: 2026-06-19
Status: design (research-grounded proposal; authored autonomously while the user is away — **for review on return**; foundational, design-stable pieces may begin building per the user's "continue building out everything we can" directive).
Part of the agent re-architecture (`docs/superpowers/specs/2026-06-18-agent-framework-rearchitecture-design.md`). Builds on SP1a–SP1c (agentd backend complete + headless-tested).
Grounded in `docs/research/{autoagents-capabilities,autoagents-patterns,agent-harness-best-practices,approval-guards}.md`. Read `docs/AI-ARCHITECTURE-STATUS.md` first.

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

### 4.7 Schema-gated DAG (cross-cutting — the harness's structural backbone)
The harness separates **flexible planning** (the LLM proposes a step DAG) from **rigid execution** (every step + every data edge is bound by a strict type contract; a malformed payload halts that branch rather than propagating). A workflow has **four gate boundaries**; we gate all four:
1. **Plan boundary** (spec → DAG) — the Planner's output is a typed `StepGraph`, JSON-schema-validated before it can run (§4.2).
2. **Graph structure** — `StepGraph::validate()` rejects cycles (Kahn/DFS topo-sort failure), `needs` referencing missing step IDs (orphans), and unreachable nodes, BEFORE any step runs.
3. **Edge boundary** (node A out → node B in) — each `Step` declares an `output_schema` and `input_schema`; a finished step's structured output is validated against its `output_schema`, and a consuming step's inbound payload against its `input_schema`. A mismatch → `Blocked{schema-violation}` (a clean halt recorded in the ledger), never silent propagation. This is the piece the conductor lacks today (steps exchange free-form `artifact`/`handoff_markdown`); SP2c adds it.
4. **Completion boundary** (step → Done) — the `TaskLedger` ground-truth gate: `Done` only via a verified `CompletionPromise` (§2.3, built in SP2a).

**Two enforcement layers (use both):**
- **Generate-time (token-level alignment):** constrain the model *while* it generates so output is structurally valid by construction. **We keep `oxidemx-agent-local` and add this directly** — mistralrs 0.8.1 (our existing direct dep) re-exports `Constraint::{Regex, Lark, JsonSchema(serde_json::Value), Llguidance}` + `RequestBuilder::set_constraint`, so it's a ~20-line add: `constraint: Option<Constraint>` on `EngineRequest`, applied in `MistralEngine::generate`, with `LocalChatProvider` passing the Planner's output schema as `Constraint::JsonSchema`. (Investigation `docs/research/mistralrs-integration-comparison.md`: `autoagents-mistral-rs` pins mistralrs 0.7.0 — a minor behind — and lacks our VRAM/Mode/ResponseGuard machinery, so adopting/forking it is a net downgrade.) Cloud providers use strict-structured-output / tool-calling. Minimizes validation failures (critical for the small local model). *SP2b.*
- **Validate-time (compiled validators):** the `jsonschema` crate as the runtime "halt if invalid" backstop at the plan + edge boundaries. Schemas are derived from Rust types via `schemars` where possible (single source of truth).

**Violation → recovery loop** (the reflection/escalation pattern, §4.4): a schema violation feeds the validator's error back to the model as the critique input — retry once locally → escalate to cloud once → `Blocked{needs-human}`. Never an unbounded retry loop.

## 5. Autonomous-operation safety
- **Hard caps in code** (§2.5) at the conductor/JoinSet + via AgentHooks. **Budget** (token/turn/wall-clock) tracked in the ledger; stop + escalate on exhaustion. **Ground-truth gating** (§2.3). **Schema gating** (§4.7). **Resumability** (§4.1) so unattended crashes recover. **Approval policy** — §5.1.

### 5.1 Approval policy — risk-tiered, reversibility-aware, non-blocking (from `docs/research/approval-guards.md`)
The user's choice (Q1=gated-but-autonomous-opt-in) is refined into a **rule-based classifier** so approvals don't constantly block background work — only genuinely risky, irreversible actions ask. Every tool call is classified into one of four **safety tiers**:

| Tier | Meaning | Example |
|---|---|---|
| `AutoAllow` | pure read / no state change — always runs | `read_file`, `git diff HEAD`, `cargo check` |
| `AutoAllowIfReversible` | mutation that VCS can recover — runs if the reversibility check passes | edit/delete a git-tracked file inside cwd |
| `Ask` → **non-blocking** | risky but legitimate — recorded as a `Blocked{needs-approval}` ledger step; **the agent continues other ready steps** and the human batch-reviews later | `git commit`, install a dep, write an untracked file |
| `AutoDeny` | irreversible / out-of-scope — refused immediately with an explanation logged | `sudo`, `git push --force`, `git clean`, `rm -rf` outside cwd, `curl`/`nc` |

**Reversibility classifier** (the headline rule the user asked for): a file edit/delete is `AutoAllowIfReversible` iff — path resolves UNDER `cwd` (no `..` escape) **AND** the file is git-tracked in HEAD (`git2::Repository::head()?.peel_to_tree()?.get_path(rel).is_ok()` — pure in-process, no subprocess) **AND** the path is not `.git/…`, `~/.ssh/…`, or a global config. Tracked-in-repo ⇒ `git restore`/`git revert` recovers it ⇒ safe to auto-run.

**Shell-command safety (avoids the prefix-allowlist trap):** never allowlist by raw-string prefix (`git` would permit `git push --force`; `;`/`&&`/`|`/backticks/`$()` chain a denied command behind an allowed one). Instead: (1) if the command contains shell metacharacters or a leading `FOO=bar`, **downgrade to `Ask`** (don't auto-deny — compound commands are often legitimate); (2) parse argv with `shell-words::split`; (3) classify on `argv[0]` + `argv[1]` (binary + subcommand), against per-binary rules; (4) a hard `AutoDeny` denylist on `argv[0]`/subcommand; (5) where the agent controls argv, execute **without a shell** (`Command::new("git").arg("diff")`) so metachar injection is structurally impossible.

**Config + autonomy:** the tier rules live in `project-config` (`<cwd>/.oxidemx/config.toml`) so a repo can widen/narrow them; a session-scoped "always allow this exact command" grant reduces repeat asks. In **attended** mode `Ask` surfaces the SP1b `ApprovalRequested` card live; in **autonomous** mode `Ask` becomes the non-blocking `Blocked` ledger entry. `AutoAllow`/`AutoAllowIfReversible`/`AutoDeny` behave identically in both modes. Implemented as an `ApprovalClassifier` (Rust; `git2` + `shell-words`) the conductor consults before dispatching a tool, replacing the current flat `allowlist`.

## 6. Rust / AI conventions (from `docs/research/agent-harness-best-practices.md`)
- Async: tokio structured concurrency, `JoinSet` for fan-out, `CancellationToken` for cancel (already used by the conductor). No lock across `.await` (project rule).
- Errors: `thiserror` for library crates, typed `#[non_exhaustive]` enums. `#![forbid(unsafe_code)]`.
- Typed structured output: `serde` + JSON schema validation at the boundary; never trust raw model text for control flow.
- Naming: `*Ledger`/`*Graph`/`Step`/`Worker`/`Planner`/`Verifier`/`*Policy`; flows are `FlowDoc`/`FlowPlan` (existing). Agent/tool types follow AutoAgents (`*Agent`, `ToolT`).
- Testing: mock providers + `MockEngine` (existing) for control logic; golden/eval tests for planner output shape; the compiler/tests are the harness's own ground truth in integration tests.

## 7. Decomposition (sub-projects; each its own plan → SDD)
- **SP2a — TaskLedger** (the foundation): atomic manifest + event log + resume-on-startup + conductor integration. Headless-testable. **Build first** (research consensus; design-stable; low-risk).
- **SP2b — Local-model Planner + schema-gating primitives**: schema-gated `spec→StepGraph` (typed `StepGraph` distinct from `Vec<Step>`); **`StepGraph::validate()`** (cycles + orphans + reachability, §4.7 boundary 2 — *moved here from a followup*); **grammar-constrained decoding** in `oxidemx-agent-local` (§4.7 generate-time layer — *moved here from a followup*) backed by `jsonschema` validate-time; routing/compaction roles. Headless-testable with mock + the local engine behind `mistral`.
- **SP2c — Orchestrator-worker execution + verifier + edges + approval**: run the StepGraph over ledger+conductor with thin workers; the **verifier tool** (`cargo`-as-ground-truth); the patterns (planning/reflection/parallel/routing); **per-step edge schema validation** (§4.7 boundary 3) → `Blocked{schema-violation}`; the **`ApprovalClassifier`** (§5.1 — tiers + reversibility + shell-safety, `git2`+`shell-words`) replacing the flat allowlist; hard caps via AgentHooks; `record_tool_call` + per-step budget primitives the ledger needs (*from SP2a followups*).
- **SP2d — SDD flow template** (§4.5) + autonomous run mode (budget/approval/resume) exposed over the `org.oxidemx.Agent` D-Bus surface (RunTask/TaskStatus/ResumeTask/ReviewApprovals).
- **SP2e — AutoAgents adoptions** (§4.6) — pipeline/guardrails/telemetry; can land incrementally alongside.

## 8. Decisions (resolved with the user 2026-06-19)
1. **Approval policy → risk-tiered + reversibility-aware + non-blocking (§5.1).** Gated-but-autonomous-opt-in, refined into the 4-tier `ApprovalClassifier` so reversible/safe actions auto-run and only genuinely risky ones `Ask` (non-blocking — they become `Blocked` ledger steps the agent works around). The git-tracked reversibility rule + the shell-argv-safety rule are the core. (Research: `docs/research/approval-guards.md`.)
2. **Ledger is the source of truth; the conductor is a stateless executor over a derived `FlowPlan`.** One store. Data edges between steps are schema-validated (§4.7 boundary 3).
3. **Autonomy aggressiveness → full task-graph run, bounded by coded caps + the verification gate + single-escalation.** No per-step check-ins; unresolved work lands as `Blocked` steps for batch review. Hard caps in code, never prompts.
4. **Schema-gated DAG (§4.7) is a first-class structural pattern** — all four gate boundaries, generate-time + validate-time enforcement, violation→self-correct→escalate recovery.

## 9. Out of scope
SP1c T8b (overlay flip+delete — gated on the user's GUI walkthrough). SP-Learn (the self-improving loop consumes this harness's ledger/journal later). Federation/external workers (later SP). GUI for task/step visualization (after the backend harness works).
