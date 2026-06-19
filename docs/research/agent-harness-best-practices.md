# Autonomous Coding Harness: Best Practices & Design Synthesis

*Researched 2026-06-19. Sources: Augment Code, Praetorian, fast.io, Zylos Research, DigitalApplied, Zeroshot, Medium.*

---

## 1. Proven Agent-Driven-Development Patterns

### 1.1 Spec-Driven Development (SDD): The Durable Loop

The dominant 2025-2026 pattern. **The spec is the sovereign artifact;** prompts are disposable. Concrete loop:

```
evidence → spec → plan → implement → verify (vs spec) → persist
```

Key properties:
- **Spec before code** — agent intent is captured as a structured document (acceptance criteria, constraints, file conventions) *before* any code is generated. This prevents "confident wrong code" (drift).
- **Plan before implement** — spec is decomposed into a typed step DAG (with dependencies, owners, success criteria) *before* any tool call touches the filesystem.
- **Verify against spec, not just syntax** — review gates check whether output satisfies acceptance criteria, not merely whether it compiles. The compiler is necessary but not sufficient.
- **Session persistence** — spec + plan + partial outputs survive restarts; agents resume mid-DAG without cold-starting.
- **Evidence linkage** — requirements trace back to the source (user report, issue, constraint), preventing later "why did we build this?" drift.

**Failure modes of SDD:**
- Vague acceptance criteria make the verify step subjective → infinite refinement loops.
- Spec not updated when implementation discovers new constraints → spec/impl diverge.
- Plan granularity mismatch: too coarse = large ambiguous tasks; too fine = orchestration tax.

**This project maps to:** spec = task card in conductor DAG; plan = FlowDoc validated by conductor; verify = test/compiler gates + evaluator agent per step.

---

### 1.2 The To-Do / Step Ledger Pattern

Used by Claude Code's TodoWrite, SDD practitioners, and the Praetorian platform. **A durable, on-disk ledger is the single source of truth for progress.**

Concrete structure (YAML/JSON manifest):
```
{
  "task_id": "...",
  "spec_ref": "path/to/spec.md",
  "status": "in_progress",            // pending | in_progress | blocked | done | failed
  "current_step": "step_3_write_tests",
  "steps_completed": ["step_1_plan", "step_2_scaffold"],
  "steps_remaining": ["step_3_write_tests", "step_4_verify"],
  "artifacts": ["src/foo.rs", "tests/foo_test.rs"],
  "decision_journal_ref": "journal/task_xyz.md"
}
```

**Atomic write protocol:** write to `task.tmp` → `fsync` → rename to `task.json`. Prevents corrupt state on crash.

**Resume loop:** on startup, agent checks for existing ledger, hydrates variables, jumps to `current_step`. No replay of completed steps.

**Event-sourcing variant:** append events to a log rather than overwriting state. Enables replay, audit, and rollback. Prefer this when the DAG has joins (multiple upstream steps must complete before downstream starts).

**What makes it work:**
- Progress is visible without running the agent.
- Human can inspect/edit the ledger to fix bad state.
- Compaction (summarize completed steps into a single "prior work" block) keeps context windows trim.

---

### 1.3 Plan-and-Execute (with ReAct inner loop)

**Plan phase:** orchestrator decomposes task into a flat or DAG step list (structured JSON). Each step has: `id`, `description`, `depends_on[]`, `tool_calls_expected[]`, `success_criteria`, `assigned_worker`.

**Execute phase:** workers run steps. Each step uses a ReAct inner loop:
- `Reason` — think about current state vs step goal.
- `Act` — call a tool (read/write/shell/search).
- `Observe` — parse tool output, update local state.
- Loop until step `success_criteria` passes or `max_iterations` hit.

**Guardrails:**
- Hard cap: max `N` iterations per step (10 is typical; 3-5 for coding substeps).
- Loop detection: if consecutive reasoning traces share >90% string similarity → escalate, don't retry.
- Tool-call budget: cap total tool invocations per task (e.g., 50) as a billing + runaway guard.
- Circuit breaker: if step fails 3× with different approaches, escalate to orchestrator.

**Failure modes:**
- Context rot: long ReAct chains accumulate irrelevant tool output → model ignores late-stage instructions. Fix: compact tool outputs aggressively (strip whitespace, truncate to relevant sections, summarize large files).
- "Lost in the middle": model attends to first/last tool outputs and ignores middle. Fix: keep tool outputs short; put the critical datum first.
- Unverified claims: model reports "tests pass" without actually running them. Fix: treat all agent self-reports as unverified until the ground-truth tool (cargo test, cargo check) confirms.

---

### 1.4 Orchestrator-Worker

**Pattern:** A thin orchestrator (≤150 lines, minimal context) maintains the DAG and dispatches work; stateless workers (≤150 lines each) execute one step at a time.

**Why thin is critical:** Praetorian's research shows monolithic 1,200-line agent bodies cause "attention dilution" — the model stops following late-stage instructions. Their 150-line thin agent reduces per-spawn token use from ~24,000 to ~2,700.

**Parallelism:** steps with no dependency edges run as concurrent tokio tasks (JoinSet). When three tests fail across separate files, spawn three concurrent investigator agents.

**Worker roles (Praetorian 5-role pattern, adapt as needed):**
1. Spec Lead — produces typed FlowDoc from task brief.
2. Developer — implements one step, writes tests.
3. Reviewer — checks impl against spec (independent, no shared context with developer).
4. Test Lead — designs test strategy.
5. Verifier — runs tests, confirms ground truth.

**Failure modes:**
- Agent sprawl: uncontrolled worker proliferation exhausts budget. Fix: explicit worker inventory; orchestrator owns all spawns; max concurrent worker cap.
- Missing accountability on worker failure. Fix: orchestrator treats any worker exit without `success_criteria` confirmation as failure, not completion.

---

### 1.5 Evaluator-Optimizer

**Pattern:** generator produces output; separate evaluator (different context, possibly different model) scores it against objective criteria. Loop until pass or iteration limit.

**What makes it work:** role separation prevents single-agent bias ("it looks right to me"). Criteria must be objective and externally verifiable (test passage rate, lint score, API surface matches spec).

**Cap at 2-3 cycles.** Beyond 3 cycles with no improvement = the criteria are wrong or the task needs human input.

**For a coding harness:** generator = developer worker; evaluator = separate cargo check + test runner + optional critic LLM. Critic LLM only adds value when test results are ambiguous (e.g., integration test flakiness diagnosis).

---

### 1.6 Reflection / Self-Critique

Only worth the cost when verification criteria are **objective and externally computable.** Compiler errors and test failures are ideal: the agent gets exact error text, can reason about it, and produce a targeted fix. "Is this code good?" is not.

**Rule:** never run a reflection cycle without a ground-truth signal (cargo check / cargo test output). Self-report without ground truth is "vibe-checking" and will loop indefinitely.

**Cost:** ~2× LLM calls per cycle. Cap at 1-2 cycles.

---

### 1.7 The Brainstorm → Spec → Plan → Subagent-Driven-Development Loop (Superpowers Model)

This project already uses this pattern (superpowers skills). It works because:

1. **Brainstorm** — surface implicit requirements, explore design space, identify unknowns. Output: a structured brief.
2. **Spec** — convert brief into typed acceptance criteria. Output: a spec doc that is version-controlled.
3. **Plan** — decompose spec into independent tasks, each with: description, inputs, outputs, success criteria. Write to a **ledger file** (e.g., `docs/plans/task-foo.md` or `MANIFEST.yaml`).
4. **Fresh subagent per task** — each task spawns a new agent with a minimal context window (spec + plan step only, not full history). Prevents context rot.
5. **Per-task review gate** — after each task completes, a reviewer agent (or automated tests) validates output against acceptance criteria before marking done in ledger.
6. **Final review** — all tasks complete → final integration review, then merge.

**Why fresh subagent per task is critical:** accumulated conversation history causes models to anchor on early decisions and rationalize away later contradictions. Starting fresh with a clean spec eliminates this.

**This project maps to:** conductor FlowDoc = ledger; each FlowNode = one subagent invocation; JoinSet parallelism for independent nodes; per-node evaluator gate before edge traversal.

---

## 2. Autonomous Operation: Running Unattended Safely

### 2.1 State Persistence & Resumability

**Minimal viable ledger** (stored on disk, atomic writes):
```rust
struct TaskLedger {
    task_id: Ulid,
    spec_ref: PathBuf,           // path to spec doc
    status: TaskStatus,          // Pending | Running | Blocked | Done | Failed
    current_step_id: StepId,
    steps: Vec<StepState>,       // each with status + artifacts + journal_ref
    tool_budget_remaining: u32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
```

Write discipline: always `write → fsync → rename`. Load on startup; jump to `current_step_id`.

**Event-sourcing for joins:** append `StepCompleted { step_id, output_digest, timestamp }` events to an append-only log. Orchestrator rebuilds DAG state by replaying events. This gives free audit trail, replay, and rollback.

**Compaction:** when a step completes, summarize its output into 2-5 sentences + artifact refs, discard raw tool transcripts. Store summary in the ledger; full transcript goes to the decision journal. This keeps the orchestrator's context window O(steps_remaining), not O(all_tool_outputs).

### 2.2 Approval Policies

Three tiers based on destructiveness/irreversibility:

| Tier | Tool class | Policy |
|------|-----------|--------|
| **Auto** | read/search/compile/test | Always allow; no gate |
| **Confirm** | write file, shell commands (non-destructive) | Allow in autonomous mode; log + notify |
| **Gate** | git push, system changes, external API calls, delete | Require explicit approval even in autonomous mode |

In practice: implement as a `ToolPolicy` enum on each tool descriptor. The orchestrator checks policy before dispatching. In "autonomous mode," Confirm tools run but are logged to the decision journal for post-hoc review.

### 2.3 Budget / Turn Caps

**Per-task caps (hard limits, enforced by conductor):**
- `max_steps`: total steps in DAG (default: 20).
- `max_tool_calls_per_step`: ReAct iterations (default: 10).
- `max_total_tool_calls`: total for the task (default: 100).
- `max_context_tokens`: trigger compaction at 75%, hard block at 85%.
- `max_wall_clock`: overall timeout (default: 30 min for a coding task).

Exceeding any cap → step/task enters `Failed` state with reason. Orchestrator can decide: retry with smaller scope, escalate, or halt.

### 2.4 Self-Verification: Ground Truth, Not Self-Report

**The compiler is ground truth.** `cargo check` and `cargo test` are infallible self-verification; agent self-report is not. Every step that produces Rust code must end with a ground-truth tool call.

**Completion promises:** a step is done only when the ledger receives a machine-readable confirmation token (e.g., `{ "verified": true, "test_run_id": "..." }`), not when the agent says "done."

**Scratchpad pattern (Praetorian):** each agent maintains a persistent scratchpad recording: last approach tried, why it failed, next approach planned. Prevents "Groundhog Day" loops where an agent retries the same failing approach.

### 2.5 When to Stop / Escalate

**Stop and mark Failed when:**
- Any hard cap is exceeded.
- The same error appears 3× without a different root cause.
- Ground-truth verification fails after 2 reflection cycles.
- A Gate-tier tool is needed but no human approval is available.

**Escalate (notify + pause) when:**
- Spec ambiguity detected: multiple valid interpretations with different outcomes.
- A dependency is missing (crate not in Cargo.toml, env var unset).
- A security-sensitive change is detected.

**Escalation Advisor (Praetorian pattern):** when stuck >3 times on a step, dispatch an out-of-band critic (local model is fine) that reads the scratchpad and injects a one-sentence hint. This breaks cognitive deadlock without burning a large-model call.

---

## 3. Local-LLM Flow Optimization

### 3.1 What Small Models Are Good At

| Task | Good fit? | Notes |
|------|-----------|-------|
| Routing / intent classification | Excellent | Single-class output; high accuracy at small model size |
| Extracting structured fields from short text | Excellent | With constrained JSON grammar |
| Summarizing a completed step for the ledger | Good | Keep prompt + output short |
| Generating a step DAG from a spec | Good | With schema validation post-output |
| Critique / redaction of a cloud response | Reasonable | Simple rubrics only |
| Multi-step reasoning / planning | Poor | Unreliable; use cloud model |
| Ambiguous instruction interpretation | Poor | Use cloud model |
| Novel API design | Poor | Use cloud model |

**Rule: verify-don't-trust.** Small model outputs that drive orchestrator state (step DAG, routing decisions) must pass a schema validator before use. Never trust raw text from a small model to be structurally correct.

### 3.2 The Local-Model Planning Pattern

```
cloud model        → produces spec (rich, ambiguous input → structured output)
local model        → decomposes spec into typed step DAG (JSON, schema-validated)
conductor          → validates DAG (no cycles, all deps resolved)
cloud model workers → execute steps (one per FlowNode)
local model        → summarizes completed step for ledger (compaction)
local model        → routes next step to cheapest capable worker
```

**Concrete pattern — local model as DAG decomposer:**
1. Prompt: spec + JSON schema for `FlowDoc` + few-shot examples.
2. Local model outputs candidate JSON (temperature 0 or low).
3. `serde_json::from_str` + jsonschema validator gate.
4. On parse/validation failure: retry once with the error appended; escalate to cloud on second failure.
5. Conductor validates DAG semantics (topological sort, dependency existence).

**Constrained generation:** if mistral.rs supports grammar-constrained generation (GGUF models via llama.cpp grammar), use it. Constrained decoding makes local models ~3× more reliable for structured output by preventing hallucinated keys.

### 3.3 Routing: Send to Cheapest Capable Model

Use a local classifier (or a simple rule set) to route:
- **Local (mistral.rs):** summarization, schema generation, routing decisions, cheap critique.
- **Cloud (Gemini/Claude):** spec writing, complex reasoning, ambiguous instructions, novel code.

Rule-based routing (< 1ms) is sufficient for most cases. ML routing (embeddings cosine sim) only worth building if you have ≥1,000 labelled examples of local-vs-cloud quality diff.

**Cost reality check (2026 pricing):** a frontier call costs ~$5-15/M tokens; a local inference call costs ~$0 marginal (hardware already running). Route aggressively to local for any task where correctness is verifiable (structured output + schema gate + compiler).

### 3.4 Compaction via Local Model

The local model is ideal for turning a 2,000-token tool transcript into a 100-token summary:
```
Prompt: "Summarize what changed. Output: { changed_files: [], key_finding: '...', errors_resolved: [] }"
```
Run at step boundaries. This keeps orchestrator context O(1) per completed step rather than O(tool_outputs).

---

## 4. Rust + AI Best Practices (2025-2026)

### 4.1 Async Patterns

**Use `JoinSet` for sub-agent lifecycle management.** Sub-agent tasks must not outlive the scope that spawned them.

```rust
let mut join_set: JoinSet<StepResult> = JoinSet::new();
for step in ready_steps {
    let token = cancellation_token.child_token();
    join_set.spawn(execute_step(step, token));
}
while let Some(result) = join_set.join_next().await {
    // handle result, update ledger
}
// JoinSet drop cancels all remaining tasks automatically
```

**`CancellationToken` (from `tokio-util`)** for cooperative cancellation. Pass a child token to every sub-agent; cancel parent to abort all children. Never rely on future-drop for cleanup — explicitly signal with the token, then await shutdown.

**Channel patterns:**
- `mpsc` for task result reporting (workers → orchestrator).
- `watch` for shared state (current task status, budget remaining).
- `oneshot` for single-use responses (sub-agent result).
- Avoid `Mutex` across `.await` points; use `tokio::sync::Mutex` if unavoidable.

**`select!` for racing futures:**
```rust
tokio::select! {
    result = worker.run() => handle_result(result),
    _ = cancellation_token.cancelled() => handle_cancel(),
    _ = tokio::time::sleep(timeout) => handle_timeout(),
}
```

### 4.2 Trait / Seam Design

Converging standard (Rig, AutoAgents, ADK-Rust):

```rust
#[async_trait]
pub trait CompletionProvider: Send + Sync {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError>;
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;  // JSON Schema
    async fn execute(&self, params: serde_json::Value) -> Result<ToolOutput, ToolError>;
}

// derive macro generates JSON schema from struct
#[derive(Tool, Deserialize)]
struct ReadFileTool { path: PathBuf }
```

**Derive macros for JSON schema generation** (via `schemars` crate) eliminate hand-written schema boilerplate and keep schema in sync with the Rust type. This is the single largest source of runtime bugs in Python agent frameworks: schema drift.

**Provider seam pattern** (already in oxidemx-agent): one `AiProvider` trait, multiple impls (Gemini, Claude, OpenAI, MistralRs). The conductor and workers program to the trait; swapping providers requires zero changes to orchestration logic.

### 4.3 Error Handling

**The canonical rule (2025 consensus):**
- `thiserror` for library crates: typed, matchable, implements `std::error::Error`.
- `anyhow` for binary/application crates: ergonomic `?` propagation, context via `.context()`.

For agent frameworks that are libraries-with-a-binary (like ours):
```rust
// In crate lib.rs:
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
    #[error("tool execution failed: {tool} — {source}")]
    Tool { tool: String, #[source] source: Box<dyn std::error::Error + Send + Sync> },
    #[error("step budget exceeded: {limit} tool calls")]
    BudgetExceeded { limit: u32 },
    #[error("ledger write failed: {0}")]
    Ledger(#[from] std::io::Error),
}

// In main.rs / agentd:
fn main() -> anyhow::Result<()> { ... }
```

**Never panic in agent workers.** Worker panics propagate as `JoinError::is_panic()` in the `JoinSet`. The orchestrator should catch these, log the panic payload, and treat the step as Failed — not crash the daemon.

### 4.4 Structured Output (serde + schemars + jsonschema)

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct FlowDoc {
    pub task_id: String,
    pub steps: Vec<FlowStep>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct FlowStep {
    pub id: String,
    pub description: String,
    pub depends_on: Vec<String>,
    pub assigned_model_tier: ModelTier,  // Local | Cloud
    pub success_criteria: Vec<String>,
    pub tool_budget: u32,
}

// Schema generation for LLM prompt injection:
let schema = schemars::schema_for!(FlowDoc);
let schema_json = serde_json::to_string_pretty(&schema).unwrap();
// Inject into prompt: "Output ONLY valid JSON matching this schema: {schema_json}"

// Validation of LLM output:
let compiled = jsonschema::validator_for(&schema_json_value)?;
compiled.validate(&llm_output_value)?;
```

### 4.5 Naming Conventions (2025-2026 Convergence)

| Concept | Naming pattern | Example |
|---------|---------------|---------|
| Agent struct | `{Role}Agent` | `DeveloperAgent`, `ReviewerAgent` |
| Provider trait impl | `{Provider}Client` | `GeminiClient`, `MistralRsClient` |
| Tool struct | `{Verb}{Noun}Tool` | `ReadFileTool`, `RunShellTool` |
| Flow step | `FlowStep` / `StepNode` | — |
| Task ledger | `TaskLedger` / `Manifest` | — |
| Orchestrator | `Conductor` / `Orchestrator` | — |
| Result type | `StepResult`, `AgentResult` | — |
| Entry point | `.run()` / `.execute()` returning `Future` | — |

### 4.6 Testing Non-Deterministic LLM Systems

**Inject at the trait boundary.** Mock `CompletionProvider` that returns fixed responses:
```rust
struct MockProvider { responses: Vec<CompletionResponse> }
#[async_trait]
impl CompletionProvider for MockProvider {
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        Ok(self.responses[self.call_count.fetch_add(1, Ordering::SeqCst)].clone())
    }
}
```

**Test categories:**
1. **Unit:** mock provider, mock tools; test orchestration logic (DAG traversal, ledger writes, budget enforcement).
2. **Golden tests:** fixed provider response → assert ledger state + artifacts. Detect regressions in parsing/routing.
3. **Integration:** real local model (mistral.rs) + mock cloud; test structured output schema compliance.
4. **Eval harness:** N real tasks with known-good outputs; score success rate. Run periodically, not in CI. This is the only way to track aggregate quality over time.

**`tokio::time::pause()` for time-sensitive tests** (timeout logic, TTL enforcement). Avoids real sleeps in test suite.

**No first-class deterministic replay exists yet** in Rust agent frameworks (as of 2026). This is an open problem and a differentiation opportunity.

---

## 5. Recommendations: Concrete Harness Design

### 5.1 The Core Loop (Spec-Driven, Ledger-Anchored)

```
User request
  → [Cloud LLM] write spec + acceptance criteria → spec.md
  → [Local LLM] decompose spec into FlowDoc (DAG JSON) → schema-validate
  → Conductor validates DAG (topo sort, dep check)
  → Ledger initialized (MANIFEST.yaml, atomic write)
  → For each ready FlowNode (parallel where no deps):
      → Spawn thin worker (DeveloperAgent, fresh context: spec + step only)
      → Worker runs ReAct inner loop (max 10 tool calls)
      → Worker calls cargo check / cargo test (ground truth)
      → Worker writes step summary (local LLM compaction) to ledger
      → ReviewerAgent validates step output vs acceptance criteria
      → Ledger: mark step Done + artifact refs
  → All steps done → final integration review (ReviewerAgent, full diff)
  → Ledger: mark task Done
```

### 5.2 Prioritized Build List

**P0 — Ledger Engine (build first — everything else depends on it)**
- `TaskLedger` struct: atomic write, resume-on-startup, event-append log.
- `StepState` with status enum, artifact refs, tool budget counter.
- Conductor integration: FlowNode completion writes to ledger; DAG traversal reads ledger for ready nodes.
- *Maps to:* extend `oxidemx-conductor`'s `FlowDoc` → add persistent `MANIFEST.yaml` layer on top.

**P1 — Thin Worker Pattern + JoinSet Parallelism**
- Enforce ≤150-line worker bodies.
- Orchestrator spawns workers into a `JoinSet`; no orphaned tasks.
- Per-worker `CancellationToken`; budget enforced at tool-call dispatch.
- *Maps to:* agentd D-Bus dispatch → conductor supervisor → `JoinSet` worker pool.

**P2 — Local-Model DAG Decomposer**
- Prompt template: spec + `FlowDoc` JSON schema + 2-3 few-shot examples.
- mistral.rs at temperature 0, grammar-constrained if supported.
- Schema validation gate; retry-once on failure; escalate to cloud on second failure.
- *Maps to:* mistral.rs `AiProvider::MistralRs` + `schemars` schema injection.

**P3 — Tool Policy + Approval Gate**
- `ToolPolicy` enum on each `Tool` impl: `Auto | Confirm | Gate`.
- Orchestrator checks policy before dispatch; Gate tools pause and notify via D-Bus.
- Log all Confirm-tier calls to decision journal.
- *Maps to:* existing tool registry in `oxidemx-agent`; add `policy()` to `Tool` trait.

**P4 — Ground-Truth Verification Step**
- Every developer worker step ends with a mandatory `CargoCheckTool` + `CargoTestTool` call.
- Step result is `Failed` until ground-truth passes (not agent self-report).
- `CompletionPromise` token written to ledger only after tool output confirms pass.
- *Maps to:* `RunShellTool` with whitelisted commands; add `required_verification: Option<ToolId>` to `FlowStep`.

**P5 — Scratchpad + Escalation Advisor**
- Each worker maintains a persistent scratchpad (`journal/{task_id}/{step_id}.md`).
- If step fails 3×: dispatch local-model Escalation Advisor (reads scratchpad, produces one-sentence hint).
- Hint injected into next worker's context.
- *Maps to:* decision journal already exists; add advisor dispatch to conductor retry logic.

**P6 — Compaction at Step Boundaries**
- After each step completes, run local model to summarize tool transcript → 100-token summary.
- Store summary in ledger; discard raw transcript (or archive to journal).
- *Maps to:* postprocess hook in conductor after `StepCompleted` event.

### 5.3 What to Build First

**The ledger engine (P0).** Every other primitive — parallelism, local-model planning, approval gates, ground-truth verification — requires reliable, resumable task state. Without the ledger, you have a one-shot agent that loses all progress on restart, cannot be inspected, and cannot be safely extended with approval gates. The ledger is the structural foundation; build it before anything else.

---

## 6. Key Guardrail Reminders

- **Never treat agent self-report as ground truth for code.** Run the compiler/tests. Always.
- **Fresh subagent per task step.** Accumulated context causes anchoring and rationalization. Pay the spawn cost.
- **Thin workers.** ≤150 lines of prompt context per worker. Attention dilution is real: 24k tokens → 2.7k tokens is a 9× improvement in instruction-following quality.
- **Hard iteration caps, not soft.** Budget enforcement in conductor/JoinSet, not in the prompt. Prompts can be argued away; code cannot.
- **Verify local-model structured output with a schema gate, every time.** Local models will occasionally emit malformed JSON. The gate costs 0.1ms; a corrupt DAG costs hours.
- **The spec is the contract.** If the spec is wrong, the implementation will be confidently wrong. Invest in spec quality, not prompt cleverness.

---

## Sources

- [Spec-Driven Development with AI Coding Agents (Zeroshot)](https://zeroshot.ghost.io/spec-driven-development-with-ai-coding-agents/)
- [Agentic Design Patterns 2026 (Augment Code)](https://www.augmentcode.com/guides/agentic-design-patterns)
- [Deterministic AI Orchestration (Praetorian)](https://www.praetorian.com/blog/deterministic-ai-orchestration-a-platform-architecture-for-autonomous-development/)
- [AI Agent Workflow State Persistence (fast.io)](https://fast.io/resources/ai-agent-workflow-state-persistence/)
- [Rust-Native AI Agent Frameworks 2026 (Zylos Research)](https://zylos.ai/research/2026-04-01-rust-native-ai-agent-frameworks-ecosystem-2026/)
- [LLM Model Routing 2026 (DigitalApplied)](https://www.digitalapplied.com/blog/llm-model-routing-2026-cost-quality-optimization-engineering-guide)
- [Spec-Driven Development Definitive Guide (Medium/Predict)](https://medium.com/predict/spec-driven-development-with-ai-coding-agents-the-definitive-guide-453fba1baf39)
- [Agentic Design Patterns (SitePoint)](https://www.sitepoint.com/the-definitive-guide-to-agentic-design-patterns-in-2026/)
- [Rust AI Agent Frameworks Infrastructure (Zylos Research)](https://zylos.ai/en/research/2026-03-31-rust-ai-agent-frameworks-infrastructure/)
- [Spec-Driven Development Map 30+ Frameworks (Medium)](https://medium.com/@visrow/spec-driven-development-is-eating-software-engineering-a-map-of-30-agentic-coding-frameworks-6ac0b5e2b484)
- [Error Handling with thiserror and anyhow (OneUptime)](https://oneuptime.com/blog/post/2026-01-25-error-types-thiserror-anyhow-rust/view)
