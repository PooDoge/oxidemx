# AutoAgents Design-Pattern Catalog
# For: oxidemx coding-harness design
# Source: liquidos-ai/AutoAgents @ main (pinned crate: 0.3.7 = latest published)
# Researched: 2026-06-19

---

## Quick Reference

**5 patterns in the repo** (single crate, not per-subdir):
`chaining` | `routing` | `parallel` | `reflection` | `planning`

**0.3.7 is the latest published crate** — main is 37 commits ahead with unreleased features.
The examples on `main` compile against the same published API; no pattern here requires post-0.3.7 API.

**Two execution models:**
- `DirectAgent` — synchronous `.run(task)`, no actor system. Used for routing, testing.
- `ActorAgent` — async, Ractor-backed, pub/sub via `Topic<Task>`. Used for all multi-agent patterns.

---

## 1. Design-Pattern Catalog

### 1.1 Chaining (Prompt Chaining)

**What it does:** Sequential pipeline. Each agent's `on_run_complete` hook publishes its output
as the next agent's input task. Classic ETL-style: extract → transform → format.

**AutoAgents API used:**
- `#[agent(name, description)]` derive macro on each stage struct
- `AgentHooks::on_run_complete` to hand off
- `ctx.publish(Topic::<Task>::new("next_agent"), Task::new(result))` for handoff
- `AgentBuilder::<_, ActorAgent>::new(...)` + `.subscribe(topic)` for wiring
- `SingleThreadedRuntime` + `Environment`

**Code skeleton:**
```rust
#[agent(name = "stage_1", description = "Extract X from input")]
pub struct Stage1 {}

#[async_trait]
impl AgentHooks for Stage1 {
    async fn on_run_complete(&self, _task: &Task, result: &Self::Output, ctx: &Context) {
        let _ = ctx.publish(Topic::<Task>::new("stage_2"), Task::new(result)).await;
    }
}

#[agent(name = "stage_2", description = "Transform X into Y")]
#[derive(AgentHooks)]   // no-op default hooks
pub struct Stage2 {}

// Wiring:
let runtime = SingleThreadedRuntime::new(None);
AgentBuilder::<_, ActorAgent>::new(BasicAgent::new(Stage1 {}))
    .llm(llm.clone()).runtime(runtime.clone())
    .subscribe(Topic::<Task>::new("stage_1"))
    .memory(Box::new(SlidingWindowMemory::new(10)))
    .build().await?;
// ... same for Stage2 ...
runtime.publish(&Topic::<Task>::new("stage_1"), Task::new("input text")).await?;
```

**Coding harness fit:** Spec → Plan → Code → Test pipeline. Each stage is an agent.
A spec-parser agent extracts requirements, publishes to a planner agent, which publishes
to a coder agent. Natural fit for linear code-generation workflows.

---

### 1.2 Routing

**What it does:** An LLM classifier agent decides which handler (agent or function) should
process the request. The router uses `DirectAgent` (synchronous, no actor overhead) and
returns a single routing token; Rust code matches on it.

**AutoAgents API used:**
- `AgentBuilder::<_, DirectAgent>::new(...)` — no `.runtime()` or `.subscribe()` needed
- `handle.agent.run(Task::new(input))` → returns `String` routing token
- Plain Rust `match` dispatches to typed handlers

**Code skeleton:**
```rust
#[agent(
    name = "router",
    description = "Classify the request. Output ONLY one word: 'fast' | 'deep' | 'unknown'"
)]
#[derive(AgentHooks)]
pub struct RouterAgent {}

let handle = AgentBuilder::<_, DirectAgent>::new(BasicAgent::new(RouterAgent {}))
    .llm(llm.clone())
    .memory(Box::new(SlidingWindowMemory::new(5)))
    .build().await?;

let decision = handle.agent.run(Task::new(user_request.clone())).await?;
match decision.trim() {
    "fast"  => fast_handler(user_request),
    "deep"  => deep_handler(user_request),
    _       => unknown_handler(user_request),
}
```

**Coding harness fit:** Model selection (fast/cheap model for trivial edits, full model for
architecture decisions). Task routing (linter fix vs. refactor vs. new feature). Language/
framework detection to dispatch to specialised code agents.

---

### 1.3 Parallelization

**What it does:** Multiple agents run concurrently on the same input; results are collected
via the `Environment`'s event stream and aggregated by a synthesis agent once all arrive.
Uses `SubmissionId` to correlate results back to the original task.

**AutoAgents API used:**
- Multiple `AgentBuilder::<_, ActorAgent>` subscribed to different topics
- `environment.take_event_receiver(None)` → `BoxEventStream<Event>`
- `Event::TaskComplete { result, sub_id, actor_name, .. }` to collect partial results
- `runtime.publish(&synthesis_topic, Task::new(aggregated))` once all results are in
- `task.submission_id` for correlation

**Code skeleton:**
```rust
// Three parallel workers, one synthesis agent
let sub_id = task.submission_id;

// Spawn event collector in background
let runtime_clone = runtime.clone();
tokio::spawn(async move {
    let mut results: HashMap<String, String> = HashMap::new();
    let expected = ["worker_a", "worker_b", "worker_c"];
    while let Some(event) = event_stream.next().await {
        if let Event::TaskComplete { result, sub_id: sid, actor_name, .. } = event {
            if sid == sub_id { results.insert(actor_name, result); }
            if expected.iter().all(|k| results.contains_key(*k)) {
                runtime_clone.publish(&synthesis_topic, Task::new(
                    serde_json::to_string(&results).unwrap()
                )).await.ok();
            }
        }
    }
});

// Fan out to all workers simultaneously
runtime.publish(&topic_a, task.clone()).await?;
runtime.publish(&topic_b, task.clone()).await?;
runtime.publish(&topic_c, task.clone()).await?;
```

**Coding harness fit:** Parallel file editing (one agent per file, synthesis agent merges
diffs). Parallel analysis (security agent + style agent + test-coverage agent on the same
PR, results merged into a unified review). Bulk code search with multiple query strategies.

---

### 1.4 Reflection (Evaluator-Optimizer)

**What it does:** Iterative generate → critique → refine loop between two agents. Generator
produces output, Critic evaluates it. If not perfect (sentinel string `CODE_IS_PERFECT`),
Critic publishes a refinement task back to Generator. Bounded by `max_iterations`.

**AutoAgents API used:**
- Two `ActorAgent` instances on separate topics (`code_generator`, `code_critic`)
- `on_run_complete` on the Generator publishes to the Critic topic
- `on_run_complete` on the Critic: if sentinel found → done; else publish refinement back
- `Arc<AtomicUsize>` for shared iteration counter across hook calls
- `on_run_start` hook for progress printing (optional)
- `SlidingWindowMemory::new(20)` — shared across both agents for context

**Code skeleton:**
```rust
#[agent(name = "generator", description = "Generate code; refine if given critique")]
pub struct Generator {
    iteration: Arc<AtomicUsize>,
}

#[async_trait]
impl AgentHooks for Generator {
    async fn on_run_complete(&self, _task: &Task, result: &Self::Output, ctx: &Context) {
        self.iteration.fetch_add(1, Ordering::SeqCst);
        let critique_task = format!("Review this code:\n{result}\n\
            If perfect, reply CODE_IS_PERFECT. Else give specific fixes.");
        ctx.publish(Topic::<Task>::new("critic"), Task::new(critique_task)).await.ok();
    }
}

#[agent(name = "critic", description = "Review code; output CODE_IS_PERFECT or specific fixes")]
pub struct Critic {
    max_iterations: usize,
    current: Arc<AtomicUsize>,
}

#[async_trait]
impl AgentHooks for Critic {
    async fn on_run_complete(&self, task: &Task, result: &Self::Output, ctx: &Context) {
        let n = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        if result.contains("CODE_IS_PERFECT") || n >= self.max_iterations {
            println!("Done after {n} iterations");
        } else {
            let refine = format!("Previous:\n{code}\nFixes needed:\n{result}\nRefine it.",
                code = extract_code(task));
            ctx.publish(Topic::<Task>::new("generator"), Task::new(refine)).await.ok();
        }
    }
}
```

**Coding harness fit:** Code review loop (generate patch → review → refine until approved).
Test generation loop (generate tests → run → fix failures → re-run). Self-correcting
code agent where the LLM acts as both author and critic. Direct analogue to the
evaluator-optimizer pattern from Anthropic's agent design guide.

---

### 1.5 Planning (Orchestrator-Worker)

**What it does:** A strategic planner agent decomposes a complex task into a structured
multi-step plan (numbered list with success criteria). Then a plan-executor agent runs
each step sequentially, publishing the next step to itself on `SUCCESS`, or looping back
to the planner on `PARTIAL`/`BLOCKED` for adaptive replanning.

**AutoAgents API used:**
- `StrategicPlanner` as `ActorAgent` on topic `strategic_planner`
- `PlanExecutor` as `ActorAgent` on topic `plan_executor`
- `on_run_complete` on Planner: parses plan text, publishes step 1 to executor topic
- `on_run_complete` on Executor: matches STATUS field, self-publishes next step or
  publishes back to planner for revision
- `Arc<AtomicUsize>` for step counting
- `on_run_start` hook returns `HookOutcome::Continue` or `HookOutcome::Abort`
- Helper fns: `extract_steps_from_plan`, `extract_status_from_result`,
  `extract_full_plan_from_task`, `extract_original_task`
- 90-second `tokio::time::sleep` timeout as the runaway guard

**Code skeleton:**
```rust
#[agent(
    name = "orchestrator",
    description = "Break task into STEPS: 1. ... 2. ... SUCCESS_CRITERIA: ..."
)]
pub struct Orchestrator { steps_created: Arc<AtomicUsize> }

#[async_trait]
impl AgentHooks for Orchestrator {
    async fn on_run_complete(&self, _task: &Task, result: &Self::Output, ctx: &Context) {
        let steps = extract_steps(result);  // parse numbered list
        if !steps.is_empty() {
            let first = format!("EXECUTE STEP 1:\n{}\nFULL PLAN:\n{}", steps[0], result);
            ctx.publish(Topic::<Task>::new("worker"), Task::new(first)).await.ok();
        }
    }
}

#[agent(name = "worker", description = "Execute step. Report STATUS: SUCCESS|PARTIAL|BLOCKED")]
pub struct Worker { step: Arc<AtomicUsize> }

#[async_trait]
impl AgentHooks for Worker {
    async fn on_run_complete(&self, task: &Task, result: &Self::Output, ctx: &Context) {
        let n = self.step.fetch_add(1, Ordering::SeqCst) + 1;
        match extract_status(result).as_str() {
            "SUCCESS" => {
                if let Some(next) = next_step(task, n) {
                    ctx.publish(Topic::<Task>::new("worker"), Task::new(next)).await.ok();
                }
            }
            _ => {
                let replan = format!("REVISE PLAN:\n{}\nBLOCKED:\n{}", task.prompt, result);
                ctx.publish(Topic::<Task>::new("orchestrator"), Task::new(replan)).await.ok();
            }
        }
    }
}
```

**Coding harness fit:** Most directly maps to a coding orchestrator. High-level task
(implement feature X) → Planner breaks into subtasks (write types, implement logic, write
tests, update docs) → Worker executes each, with replanning on compiler errors or test
failures. The BLOCKED→replan loop is the self-healing mechanism for build failures.

---

## 2. Pipeline Example: LLM Optimization Middleware

**What it is:** `PipelineBuilder` is an LLM _middleware_ stack, not agent orchestration.
It wraps any `Arc<dyn LLMProvider>` and returns a new `Arc<dyn LLMProvider>` — a
transparent drop-in. All agent code stays unchanged; the pipeline intercepts LLM calls.

**Location:** `examples/pipeline/src/main.rs`

**Cargo feature required:** `autoagents = { features = ["openai", "logging", "optim"] }`

**Layers available in 0.3.7:**
- `CacheLayer` — in-process LRU cache with optional TTL and max-size cap
  - `ChatCacheKeyMode::UserPromptOnly` (ignores system prompt changes) or full-history key
  - Cache hit served in microseconds vs. hundreds of milliseconds network round-trip
  - `cache_completions`, `cache_embeddings`, `cache_streaming` flags
- `RetryLayer` — exponential backoff with configurable `max_attempts` and `initial_backoff`

**Composition (outermost layer first):**
```rust
// Cache intercepts before retry — hits never reach retry or network
let llm: Arc<dyn LLMProvider> = PipelineBuilder::new(base_provider)
    .add_layer(CacheLayer::new(CacheConfig {
        chat_key_mode: ChatCacheKeyMode::UserPromptOnly,
        ttl: Some(Duration::from_secs(3600)),
        max_size: Some(1000),
        cache_completions: true,
        cache_embeddings: true,
        cache_streaming: true,
    }))
    .add_layer(RetryLayer::new(RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(200),
        ..RetryConfig::default()
    }))
    .build();
```

**Four scenarios demonstrated:**
1. Cache hit/miss — same query twice; first is network, second is microsecond cache hit
2. Independent entries — distinct queries each get their own cache slot
3. TTL expiry — short TTL pipeline, entry re-fetched after 300ms sleep
4. Agent integration — `ReActAgent` uses cached `Arc<dyn LLMProvider>` transparently;
   second run with fresh `SlidingWindowMemory` produces identical message sequence → all hits

**Coding harness relevance:** Drop `PipelineBuilder` in front of any LLM provider in
`ai_client.rs`. The `UserPromptOnly` key mode means repeated code-review tasks on the
same file content are cached even if system prompt varies. Retry handles transient API
errors without bespoke retry logic.

**Structured output in agents:**
```rust
#[derive(Debug, Serialize, Deserialize, AgentOutput)]
pub struct CalcOutput {
    #[output(description = "The numeric result")]   result: i64,
    #[output(description = "Brief explanation")]    explanation: String,
}
```
`AgentOutputT` generates the JSON schema injected into the LLM prompt. `ReActAgentOutput`
is the intermediate output type for tool-using agents; implement `From<ReActAgentOutput>`
to parse it into your structured type with a fallback.

---

## 3. Multi-Agent Coordination: Real API

AutoAgents has no dedicated "fleet" or "handoff" abstraction beyond what the patterns use.
Coordination is entirely pub/sub via typed topics. The coordination primitives are:

### Topics and publish/subscribe
```rust
let topic = Topic::<Task>::new("agent_name");          // typed topic
runtime.publish(&topic, Task::new("payload")).await?;  // from outside agents
ctx.publish(topic, Task::new("payload")).await?;       // from inside agent hook
```
`Topic<T>` is generic over message type. Only `Task` is used in all examples.

### Event stream (fan-in aggregation)
```rust
let receiver: BoxEventStream<Event> = environment.take_event_receiver(None).await?;
// Then in a tokio::spawn:
while let Some(event) = receiver.next().await {
    match event {
        Event::TaskComplete { result, sub_id, actor_name, .. } => { /* collect */ }
        _ => {}
    }
}
```
`SubmissionId` (UUID) on each `Task` correlates results from parallel agents.

### Shared memory across agents
```rust
let mem = Box::new(SlidingWindowMemory::new(30));
// Pass Arc-cloned memory to each AgentBuilder:
AgentBuilder::new(agent1).memory(mem.clone()).build().await?;
AgentBuilder::new(agent2).memory(mem.clone()).build().await?;
```
Note: `Box` is used in the builder API; the memory itself is `Arc`-wrapped internally.

### Shared LLM provider
All agents share `Arc<dyn LLMProvider>` (cloned cheaply). With `PipelineBuilder`, all
agents automatically share the same cache, so one agent's LLM call warms the cache for
another agent processing the same content.

### No built-in handoff / agent-spawning API
There is no `spawn_agent()`, `delegate_to()`, or `fleet` API. Multi-agent coordination
is achieved entirely by publishing to known topic names. This is by design — it keeps
the runtime topology static and type-safe.

### Other notable examples in the repo
| Example | What it shows |
|---|---|
| `coding_agent` | Full code-generation agent with filesystem tools |
| `mcp` | MCP tool server integration (tools via external MCP server) |
| `guardrails` | Input/output content filtering via `autoagents-guardrails` |
| `rag_qdrant_agent` | RAG with Qdrant vector store |
| `vector_store_in_memory` | In-process vector store for semantic search |
| `llamacpp_agent` | Local inference via llama.cpp (no API key needed) |
| `mistral_rs` | Local inference via mistral.rs |
| `wasm_runner` | WASM-sandboxed tool execution |
| `telemetry` | OpenTelemetry tracing across agents |
| `safe_local_agent` | Multi-turn agent with guardrails + local LLM |

---

## 4. 0.3.7 vs Main: API Delta Table

0.3.7 is the latest published crate (released 2026-03-25). Main is 37 commits ahead.
**All design-pattern and pipeline examples compile against 0.3.7.** Nothing in the
examples requires post-0.3.7 API.

| Feature / API | In 0.3.7? | Status on main | Risk if used |
|---|---|---|---|
| `PipelineBuilder` + `CacheLayer` + `RetryLayer` | YES (added 0.3.6) | Stable | Safe |
| `#[agent]`, `#[tool]`, `AgentHooks`, `AgentOutput` macros | YES | Stable | Safe |
| `DirectAgent` / `ActorAgent` / `AgentBuilder` | YES | Stable | Safe |
| `SlidingWindowMemory`, `MemoryProvider` trait | YES | Stable | Safe |
| `SingleThreadedRuntime`, `Environment`, `Topic` | YES | Stable | Safe |
| `ReActAgent` prebuilt executor | YES | Stable | Safe |
| `BasicAgent` prebuilt executor | YES | Stable | Safe |
| `CodeActAgent` (JS/TS via quickjs) | NO | Added post-0.3.7 (#214, 2026-04-09) | Do NOT use |
| OpenAI Responses API (`responses_api` feature) | NO | Added post-0.3.7 (#213, 2026-04-08) | Do NOT use |
| `ChatProvider::model()` default impl | NO | Added post-0.3.7 (#237, 2026-06-04) | Do NOT use in impl |
| Anthropic structured output (`output_config.format`) | NO | Added post-0.3.7 (#236, 2026-06-03) | Do NOT use |
| `SamplingOverrides` per-call overrides (llamacpp) | NO | Added post-0.3.7 (#227) | Do NOT use |
| llamacpp KV-cache prefix reuse | NO | Added post-0.3.7 (#222) | Do NOT use |
| `ToolInput` macro panic fix | In 0.3.7 (partial) | Fixed post-0.3.7 (#204) | Known bug in 0.3.7 — workaround: avoid complex ToolInput generics |
| `TurnEngine` abstraction | YES (added 0.3.3) | Stable | Safe |
| `autoagents-guardrails` crate | YES (added 0.3.6) | Stable | Safe |
| Python bindings (maturin) | YES (added 0.3.6) | Not relevant for Rust | N/A |
| WASM / WASI support | YES (0.3.x) | Bug fixes post-0.3.7 | Avoid WASM-specific code paths |
| Qdrant named vectors | YES (added 0.3.4) | Stable | Safe |
| Telemetry / OpenTelemetry | YES (added 0.3.3) | Stable | Safe |
| `autoagents-speech` (TTS/STT) | YES (added 0.3.5) | Stable | Safe |

**Key 0.3.7 internals confirmed safe:** The `autoagents_core::agent::prebuilt::executor`
module path (used by `BasicAgent`, `ReActAgent`) is stable. The `autoagents::llm::optim`
module path (used by `CacheLayer`, `RetryLayer`) is stable. The `autoagents::llm::pipeline`
module path (used by `PipelineBuilder`) is stable.

**Vendor-patch risk:** Our project vendors AutoAgents with a local patch for Gemini
streaming (`\r\n\r\n` SSE trap). Post-0.3.7 commits touch SSE buffer logic (#197 in
0.3.7 itself). Any future version bump must re-validate our vendor patch still applies.

---

## 5. Recommendations: Top 4 Patterns for the Coding Harness

### Recommendation 1: Planning (Orchestrator-Worker) — Implement First

**Why:** Directly solves the core problem: decompose "implement feature X" into typed
subtasks (spec → types → impl → tests → docs), execute sequentially, replan on compiler
error or test failure.

**AutoAgents primitives:** `StrategicPlanner` + `PlanExecutor` as `ActorAgent` pair on
`strategic_planner` / `plan_executor` topics. Structured STATUS field (SUCCESS/PARTIAL/BLOCKED)
drives the loop. `SlidingWindowMemory::new(30)` shared across both for full plan context.

**Coding-harness adaptation:** Replace STATUS text parsing with a proper `#[derive(AgentOutput)]`
struct: `{ status: PlanStatus, next_step: Option<String>, output: String }`. Add tool calls
for `read_file`, `write_file`, `run_cargo_check` in the Worker agent.

---

### Recommendation 2: Reflection — Implement Second

**Why:** Self-correcting code generation. The generate → review → refine loop with
`CODE_IS_PERFECT` sentinel and `max_iterations` cap is directly usable for: patch
generation, test writing, and doc-comment improvement.

**AutoAgents primitives:** `CodeGenerator` + `CodeCritic` pair as `ActorAgent`. Shared
`Arc<AtomicUsize>` for iteration count. `SlidingWindowMemory::new(20)` shared.
`on_run_start` for progress hooks.

**Coding-harness adaptation:** Replace the Python factorial example with: Generator
receives `(file_path, diff_request)`, produces a patch; Critic runs `cargo check` output
as part of its critique prompt. Sentinel becomes `PATCH_IS_CORRECT`.

---

### Recommendation 3: Parallelization — Implement Third

**Why:** Parallel code analysis is the most direct productivity win — run security audit,
style check, and test-coverage analysis concurrently on the same PR diff, then synthesize.

**AutoAgents primitives:** 3 `ActorAgent` workers on separate topics, fan-out via three
`runtime.publish()` calls, `environment.take_event_receiver()` + `BoxEventStream<Event>` +
`Event::TaskComplete` with `SubmissionId` correlation, synthesis `ActorAgent`.

**Coding-harness adaptation:** Workers are `SecurityAuditAgent`, `StyleAgent`,
`TestCoverageAgent`. Synthesis agent merges into `CodeReviewReport` struct. The shared
`PipelineBuilder` cache means repeated analysis of the same file content hits the cache
on the second worker.

---

### Recommendation 4: PipelineBuilder (Cache + Retry) — Add Immediately

**Why:** Zero-code-change LLM optimization. Drop into `ai_client.rs` in front of any
`LLMProvider`. `ChatCacheKeyMode::UserPromptOnly` is ideal for code agents where the
system prompt may vary but the user task (file content + diff request) is stable.

**AutoAgents primitives:** `PipelineBuilder::new(base).add_layer(CacheLayer::new(...)).add_layer(RetryLayer::new(...)).build()`. Result is `Arc<dyn LLMProvider>` — pass to all agents unchanged.

**Coding-harness adaptation:** Set TTL to 1 hour. Use `UserPromptOnly` key mode. This
immediately handles: retrying transient API errors, caching repeated code-review calls
on the same content (e.g., during the reflection loop), and warming the cache across
parallel workers.

---

### Routing: Defer (but keep for model selection)

The routing pattern with `DirectAgent` is low complexity and fits model selection (fast
model for syntax fixes, full model for architecture) or task classification (is this a
linter fix, a refactor, or a new feature?). Implement after the above four when you have
enough task types to route between.

---

## Appendix: Core API Quick Reference (0.3.7 verified)

```rust
// Imports
use autoagents::prelude::*;
use autoagents::core::actor::Topic;
use autoagents::core::agent::{ActorAgent, DirectAgent, AgentBuilder, AgentHooks, Context, HookOutcome};
use autoagents::core::agent::memory::SlidingWindowMemory;
use autoagents::core::agent::prebuilt::executor::{BasicAgent, ReActAgent};
use autoagents::core::agent::task::Task;
use autoagents::core::environment::Environment;
use autoagents::core::runtime::{SingleThreadedRuntime, TypedRuntime};
use autoagents::core::utils::BoxEventStream;
use autoagents::llm::{LLMProvider, backends::openai::OpenAI, builder::LLMBuilder};
use autoagents::llm::optim::{CacheConfig, CacheLayer, ChatCacheKeyMode, RetryConfig, RetryLayer};
use autoagents::llm::pipeline::PipelineBuilder;
use autoagents::protocol::{Event, SubmissionId};
use autoagents_derive::{agent, tool, AgentHooks, AgentOutput, ToolInput};

// Build an ActorAgent
AgentBuilder::<_, ActorAgent>::new(BasicAgent::new(MyStruct {}))
    .llm(llm.clone())
    .runtime(runtime.clone())
    .subscribe(Topic::<Task>::new("my_agent"))
    .memory(Box::new(SlidingWindowMemory::new(10)))
    .build()
    .await?;

// Build a DirectAgent (no runtime/subscribe)
let handle = AgentBuilder::<_, DirectAgent>::new(BasicAgent::new(MyStruct {}))
    .llm(llm.clone())
    .build().await?;
let output: String = handle.agent.run(Task::new("input")).await?;

// HookOutcome in on_run_start
async fn on_run_start(&self, _task: &Task, _ctx: &Context) -> HookOutcome {
    HookOutcome::Continue   // or HookOutcome::Abort
}

// Structured output
#[derive(Debug, Serialize, Deserialize, AgentOutput)]
pub struct MyOutput {
    #[output(description = "...")]  field: String,
}

// Tool definition
#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct MyToolArgs {
    #[input(description = "...")]  arg: String,
}

#[tool(name = "MyTool", description = "...", input = MyToolArgs)]
struct MyTool;

#[async_trait]
impl ToolRuntime for MyTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let a: MyToolArgs = serde_json::from_value(args)?;
        Ok(json!(a.arg))
    }
}
```
