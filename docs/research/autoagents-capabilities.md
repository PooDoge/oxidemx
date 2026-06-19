# AutoAgents 0.3.7 — Capability Inventory & Gap Analysis

Researched from the vendored source at
`vendor/AutoAgents/crates/` on 2026-06-19.

---

## 1. Capability Inventory

### 1.1 Core Agent Traits and Execution

| Symbol | File | One-line usage |
|--------|------|----------------|
| `AgentDeriveT` | `autoagents-core/src/agent/base.rs` | Core agent trait: exposes `description()`, `name()`, `tools()`, and associated `Output` type. Implemented manually or via the `#[agent]` proc-macro. |
| `BaseAgent<T, A>` | `autoagents-core/src/agent/base.rs` | Generic wrapper holding the inner implementation, `Arc<dyn LLMProvider>`, optional memory, an event-channel `Sender`, and a phantom agent-type marker. |
| `AgentExecutor` | `autoagents-core/src/agent/executor/mod.rs` | Strategy trait — implement `execute(&Task, Arc<Context>)` and optionally `execute_stream(...)` for custom execution loops. |
| `TurnResult<T>` | `autoagents-core/src/agent/executor/mod.rs` | Enum `Continue(Option<T>) | Complete(T)` returned from each turn of a multi-turn executor. |
| `ExecutorConfig` | `autoagents-core/src/agent/executor/mod.rs` | Holds `max_turns: usize` (default 10). |
| `AgentHooks` | `autoagents-core/src/agent/hooks.rs` | Lifecycle hook trait: `on_agent_create`, `on_run_start` (can `Abort`), `on_run_complete`, `on_turn_start/complete`, `on_tool_call` (can `Abort`), `on_tool_start/result/error`, `on_agent_shutdown`. Derivable via `#[derive(AgentHooks)]`. |
| `HookOutcome` | `autoagents-core/src/agent/hooks.rs` | `Continue | Abort` — hooks that can block execution return this. |
| `AgentOutputT` | `autoagents-core/src/agent/output.rs` | Trait for typed outputs: `output_schema() -> &'static str`, `structured_output_format() -> Value`. Derivable via `#[derive(AgentOutput)]`. |
| `Context` | `autoagents-core/src/agent/context.rs` | Runtime context passed to executors: holds LLM, memory, tools, config, event-tx, stream flag. |
| `AgentBuilder<T, A>` | `autoagents-core/src/agent/builder.rs` | Fluent builder: `.llm(...)`, `.memory(...)`, `.stream(bool)`, `.runtime(...)`, `.subscribe(Topic<Task>)`. `.build()` produces `DirectAgentHandle` or `ActorAgentHandle` depending on the marker type `A`. |
| `Task` | `autoagents-core/src/agent/task.rs` (and `autoagents-protocol`) | Unit of work: `prompt: String`, `submission_id: Uuid`. Created via `Task::new("...")`. |

### 1.2 Prebuilt Executors

| Symbol | File | One-line usage |
|--------|------|----------------|
| `DirectAgent` (marker) | `autoagents-core/src/agent/direct.rs` | Marker type: agent runs in the caller's async task, no actor system required. |
| `DirectAgentHandle<T>` | `autoagents-core/src/agent/direct.rs` | Contains `agent: BaseAgent<T, DirectAgent>` and `rx: BoxEventStream<Event>`. Call `agent.run(task)` or `agent.run_stream(task)`. |
| `ActorAgent` (marker) | `autoagents-core/src/agent/actor.rs` | Marker type: agent lives inside a `ractor` actor, receives `Task` messages. |
| `ActorAgentHandle<T>` | `autoagents-core/src/agent/actor.rs` | Contains `agent: Arc<BaseAgent<...>>` and `actor_ref: ActorRef<Task>`. Send tasks via `actor_ref.cast(task)` or subscribe to a `Topic<Task>`. |
| `ReActAgent<T>` | `autoagents-core/src/agent/prebuilt/executor/react.rs` | Wraps any `AgentDeriveT` in a multi-turn Reason+Act loop with tool execution, streaming deltas, and `TurnDelta::Text / ToolResults / Done` events. |
| `ReActAgentOutput` | `autoagents-core/src/agent/prebuilt/executor/react.rs` | Output struct: `response: String`, `tool_calls: Vec<ToolCallResult>`, `done: bool`. Has `.try_parse::<T>()` and `.parse_or_map(fallback)` helpers. |
| `BasicAgent<T>` | `autoagents-core/src/agent/prebuilt/executor/basic.rs` | Single-turn executor — one LLM call, no tool loop. |
| `BasicAgentOutput` | same | `response: String`. |

### 1.3 Actor / Runtime / Pub-Sub

| Symbol | File | One-line usage |
|--------|------|----------------|
| `Runtime` trait | `autoagents-core/src/runtime/mod.rs` | Abstract runtime: `subscribe_any`, `publish_any`, `tx()`, `run()`, `stop()`, `subscribe_events()`. |
| `TypedRuntime` trait | `autoagents-core/src/runtime/mod.rs` | Auto-blanket over `Runtime`; adds typed `subscribe<M>(&Topic<M>, ActorRef<M>)` and `publish<M>(&Topic<M>, M)`. |
| `SingleThreadedRuntime` | `autoagents-core/src/runtime/single_threaded.rs` | Concrete runtime backed by `tokio::sync::mpsc` + `broadcast` channels and a `HashMap<String, Subscription>`. Delivery happens in a background loop. |
| `Topic<M>` | `autoagents-core/src/actor/topic.rs` | Typed topic handle: `Topic::<Task>::new("jobs")`. Used with `TypedRuntime::subscribe/publish`. |
| `AnyActor` | `autoagents-core/src/actor/mod.rs` | Object-safe actor trait: `send_any(Arc<dyn Any>)`. `ActorRef<M>` and `ActorRef<SharedMessage<M>>` implement this so the runtime can store heterogeneous actor lists. |
| `CloneableMessage` / `ActorMessage` | `autoagents-core/src/actor/messaging.rs` | Marker traits for messages. `CloneableMessage` = cheaply broadcastable; `SharedMessage<M>` wraps non-clone `M` in an `Arc`. |
| `Transport` / `LocalTransport` | `autoagents-core/src/actor/transport.rs` | Delivery strategy seam. `LocalTransport` is the default in-process transport. |
| `Environment` | `autoagents-core/src/environment.rs` | Top-level container: registers multiple `Runtime`s, exposes a unified event stream, provides `run()` and `shutdown()`. |
| `EventFanout` | `autoagents-core/src/event_fanout.rs` | Broadcasts one `BoxEventStream<Event>` to N subscribers via `broadcast::channel`. Used by `DirectAgentHandle::subscribe_events()`. |
| `ractor` re-export | `autoagents-core/src/lib.rs` | The full `ractor` crate is re-exported as `autoagents_core::ractor::*` for native builds. |

### 1.4 Event Protocol

| Symbol | File | One-line usage |
|--------|------|----------------|
| `Event` enum | `autoagents-protocol/src/protocol.rs` | Serialisable protocol events: `NewTask`, `TaskStarted`, `TaskComplete`, `TaskError`, `PublishMessage`, `ToolCallRequested`, `ToolCallCompleted`, `ToolCallFailed`, `TurnStarted`, `TurnCompleted`, `StreamChunk`, `StreamToolCall`, `StreamComplete`. |
| `InternalEvent` | `autoagents-protocol/src/protocol.rs` | `ProtocolEvent(Event) | Shutdown` — internal to the runtime's event loop. |
| `StreamingTurnResult` | `autoagents-protocol/src/protocol.rs` | `Complete(String) | ToolCallsProcessed(Vec<ToolCallResult>)`. |
| `StreamChunk` | `autoagents-protocol/src/llm.rs` | `Text(String) | ToolUseComplete{index, tool_call} | ReasoningContent(String) | Done{stop_reason}`. |
| `ActorID / SubmissionId / RuntimeID / EventId` | `autoagents-protocol/src/protocol.rs` | All `type X = Uuid` aliases. |
| `ToolCallResult` | `autoagents-protocol/src/tool.rs` | `tool_name`, `success: bool`, `arguments: Value`, `result: Value`. |

### 1.5 LLM Pipeline and Providers

| Symbol | File | One-line usage |
|--------|------|----------------|
| `LLMProvider` | `autoagents-llm/src/lib.rs` | Supertrait aggregating `ChatProvider + CompletionProvider + EmbeddingProvider + ModelsProvider`. |
| `ChatProvider` | `autoagents-llm/src/chat/mod.rs` | `chat(messages, json_schema)` and `chat_with_tools(messages, tools, json_schema)`. |
| `StructuredOutputFormat` | `autoagents-llm/src/chat/mod.rs` | `{name, description, schema: Option<Value>, strict: Option<bool>}` — passed to the LLM for constrained JSON output. |
| `LLMBuilder` | `autoagents-llm/src/builder.rs` | Fluent builder across all backends; selects provider by enum or string. |
| `PipelineBuilder` | `autoagents-llm/src/pipeline/mod.rs` | Composes `LLMLayer` middleware over a base `Arc<dyn LLMProvider>`; first-added layer is outermost. |
| `LLMLayer` | `autoagents-llm/src/pipeline/mod.rs` | `fn build(self: Box<Self>, next: Arc<dyn LLMProvider>) -> Arc<dyn LLMProvider>` — implement to add caching, routing, compression, retry, etc. |
| Backends | `autoagents-llm/src/backends/` | `anthropic`, `azure_openai`, `deepseek`, `google`, `groq`, `minimax`, `ollama`, `openai`, `openrouter`, `phind`, `xai`. |
| `openai_compatible` provider | `autoagents-llm/src/providers/openai_compatible.rs` | Generic OpenAI-compatible provider; covers MistralRs and local inference servers. |
| `autoagents-mistral-rs` | `autoagents-mistral-rs/src/` | Native in-process `mistral.rs` backend: `MistralRsConfig`, `MistralRsProvider`. |
| `autoagents-llamacpp` | `autoagents-llamacpp/src/` | llama.cpp backend with HuggingFace model download helpers. |

### 1.6 Memory

| Symbol | File | One-line usage |
|--------|------|----------------|
| `MemoryProvider` | `autoagents-core/src/agent/memory/mod.rs` | Trait: `remember(&ChatMessage)`, `recall(query, limit)`, `clear()`, `memory_type()`, `size()`, `clone_box()`. Optional reactive extension: `get_event_receiver() -> Option<broadcast::Receiver<MessageEvent>>`. |
| `SlidingWindowMemory` | `autoagents-core/src/agent/memory/sliding_window.rs` | Keeps the N most recent messages. Constructed via `SlidingWindowMemory::new(n)`. |
| `MessageCondition` | `autoagents-core/src/agent/memory/mod.rs` | Rich predicate for reactive memory triggers: `Any`, `Eq`, `Contains`, `NotContains`, `RoleIs`, `RoleNot`, `LenGt`, `Custom(Arc<Fn>)`, `Empty`, `All(Vec<...>)`, `AnyOf(Vec<...>)`, `Regex(String)`. |
| `MemoryType` | `autoagents-core/src/agent/memory/mod.rs` | `SlidingWindow | Custom`. |

### 1.7 Vector Store / Embeddings

| Symbol | File | One-line usage |
|--------|------|----------------|
| `VectorStoreIndex` | `autoagents-core/src/vector_store/mod.rs` | Trait: `insert_documents`, `insert_documents_with_ids`, `top_n(VectorSearchRequest)`, `top_n_ids(...)`, `insert_documents_with_named_vectors`. |
| `InMemoryVectorStore` | `autoagents-core/src/vector_store/in_memory_store.rs` | Default local vector store; stores `PreparedDocument` lists. |
| `Embed` / `TextEmbedder` | `autoagents-core/src/embeddings/mod.rs` | `Embed` trait: `fn embed(&self, embedder: &mut TextEmbedder)`; `TextEmbedder` collects text segments for batch embedding. |
| `EmbeddingProvider` | `autoagents-llm/src/embedding/model_provider.rs` | `async fn embed(text: Vec<String>) -> Result<Vec<Vec<f32>>, LLMError>`. |
| `autoagents-qdrant` | `autoagents-qdrant/src/lib.rs` | Qdrant vector database integration — `QdrantVectorStore` implements `VectorStoreIndex`. |

### 1.8 Tool System

| Symbol | File | One-line usage |
|--------|------|----------------|
| `ToolT` | `autoagents-core/src/tool/mod.rs` | Combined tool trait requiring `name()`, `description()`, `args_schema() -> Value`, and `ToolRuntime`. |
| `ToolRuntime` | `autoagents-core/src/tool/runtime/mod.rs` | `async fn execute(args: Value) -> Result<Value, ToolCallError>`. |
| `ToolInputT` | `autoagents-core/src/tool/mod.rs` | Marker trait with `fn io_schema() -> &'static str`; derived via `#[derive(ToolInput)]`. |
| `SharedTool` | `autoagents-core/src/tool/mod.rs` | `Arc<dyn ToolT>` wrapper that implements `ToolT`, for sharing tools across multiple agents without cloning. |
| `shared_tools_to_boxes(...)` | `autoagents-core/src/tool/mod.rs` | Helper: converts `&[Arc<dyn ToolT>]` to `Vec<Box<dyn ToolT>>` for `AgentDeriveT::tools()`. |
| WASM tool runtime | `autoagents-core/src/tool/runtime/wasm.rs` | `WasmRuntime` for executing WASM-compiled tools via `wasmtime`; enabled by the `wasmtime` feature. |
| MCP adapter | `autoagents-toolkit/src/mcp/` | `McpServerConnection`, `McpToolsManager`, `McpToolAdapter`, `McpToolWrapper` — connects to an external MCP server and exposes its tools as `Box<dyn ToolT>`. |

### 1.9 Toolkit (autoagents-toolkit)

Pre-built `ToolT` implementations:

| Tool | File |
|------|------|
| Filesystem: `ReadFile`, `WriteFile`, `CopyFile`, `MoveFile`, `DeleteFile`, `CreateDir`, `ListDir`, `SearchFile` | `autoagents-toolkit/src/tools/filesystem/` |
| Document parsing: `DocumentParser` — CSV, DOCX, HTML, JSON, Markdown, PDF, plain text, PPTX, XLSX, XML | `autoagents-toolkit/src/tools/document_parsing/` |
| Web search: `BraveSearch` | `autoagents-toolkit/src/tools/search/brave.rs` |
| Wolfram Alpha: `LlmApiTool`, `ShortAnswerTool`, `RecognizerTool` | `autoagents-toolkit/src/tools/wolfram_alpha/` |
| MCP bridge | `autoagents-toolkit/src/mcp/` |

### 1.10 Derive Macros (autoagents-derive)

| Macro | Generates |
|-------|-----------|
| `#[agent(executor = "react")]` | `impl AgentDeriveT` + optional executor wiring |
| `#[derive(AgentOutput)]` with `#[output(strict)]` | `impl AgentOutputT` with `output_schema()` and `structured_output_format()` |
| `#[tool]` attribute | `impl ToolT + ToolRuntime` from an async fn, using the fn's doc-comment as description |
| `#[derive(ToolInput)]` | `impl ToolInputT` with `io_schema()` from struct field types |
| `#[derive(AgentHooks)]` | Empty blanket `impl AgentHooks` (all hooks are no-ops by default) |

### 1.11 Guardrails (autoagents-guardrails)

| Symbol | File | One-line usage |
|--------|------|----------------|
| `Guardrails` | `autoagents-guardrails/src/engine.rs` | Top-level handle; `.builder()` → `GuardrailsBuilder` → `.build()`. Can `.wrap(provider)` directly or emit a `.layer()` for `PipelineBuilder`. |
| `GuardrailsBuilder` | same | `.input_guard(G)`, `.output_guard(G)`, `.input_guard_with_policy(G, policy)`, `.output_guard_with_policy(G, policy)`, `.enforcement_policy(...)`, `.input_sanitizer(fn)`, `.output_sanitizer(fn)`. |
| `InputGuard` / `OutputGuard` | `autoagents-guardrails/src/guard.rs` | Traits: `async fn inspect(&mut GuardedInput/Output, &GuardContext) -> Result<GuardDecision, GuardError>`. |
| `GuardDecision` | same | `Pass | Modify{violation: Option<V>} | Reject(V)`. |
| `EnforcementPolicy` | `autoagents-guardrails/src/policy.rs` | `Block | Sanitize | Audit`. Per-guard overrides supported. |
| Built-in guards | `autoagents-guardrails/src/guards/` | `PromptInjectionGuard`, `RegexPiiRedactionGuard`, `ToxicityGuard`. |
| `GuardrailsLayer` | `autoagents-guardrails/src/layer.rs` | Implements `LLMLayer`, so `PipelineBuilder::add_layer(guardrails.layer())` installs it inline. |

### 1.12 Telemetry (autoagents-telemetry)

| Symbol | File | One-line usage |
|--------|------|----------------|
| `TelemetryConfig` | `autoagents-telemetry/src/config.rs` | Service name, OTLP config, batch config, redaction config, `metrics_enabled`, `install_tracing_subscriber`. |
| `TelemetryHandle` | `autoagents-telemetry/src/runner/handle.rs` | Token returned by `start_telemetry(event_stream, config, ...)`. Call `.shutdown()` for graceful flush. |
| `Tracer` | `autoagents-telemetry/src/tracer.rs` | OpenTelemetry tracer wrapper. |
| `EventMapper` | `autoagents-telemetry/src/runner/mapper.rs` | Translates `Event` enum variants into OTLP spans and metrics automatically. |
| `LangfuseTelemetry` | `autoagents-telemetry/src/providers/langfuse.rs` | Optional Langfuse exporter (feature-gated). |
| `EventFanout` (telemetry) | `autoagents-telemetry/src/fanout.rs` | Separate fanout in the telemetry crate; feeds the event stream to multiple telemetry subscribers. |

### 1.13 Speech (autoagents-speech)

STT via Parakeet and TTS via Pocket TTS, with Silero VAD and audio capture/playback. Not relevant to the coding harness.

---

## 2. Gap Analysis

### 2.1 What OxideMX Already Uses

| AutoAgents Feature | How OxideMX Uses It |
|--------------------|---------------------|
| `LLMProvider` / `ChatProvider` | `oxidemx-agent/src/factory.rs` — wraps Google / OpenAI / Anthropic / Ollama backends |
| `LLMBuilder` | Same factory file, provider construction |
| `ReActAgent<T>` | `oxidemx-conductor/src/step_agent.rs` — the per-step agent inside the conductor's node runner |
| `DirectAgent` / `DirectAgentHandle` | CLI tool (`oxidemx-agent/src/bin/cli.rs`) and possibly the overlay turn loop |
| `AgentDeriveT` / `AgentHooks` (derive) | `oxidemx-agent/src/bin/cli.rs` with `#[agent]` + `#[derive(AgentHooks)]` macros |
| `SlidingWindowMemory` | `oxidemx-conductor/src/step_agent.rs` and `oxidemx-agent/src/bin/cli.rs` |
| `Task` | Passed through the conductor as the unit of work |
| `ToolT` / `ToolRuntime` / `ToolInputT` | `oxidemx-agent/src/tools.rs` — custom coding tools |
| `#[tool]` + `#[derive(ToolInput)]` | Same |
| `autoagents_toolkit` filesystem / search / document / MCP tools | `oxidemx-agent/src/toolkit.rs` |
| `Event` protocol stream | `oxidemx-agent-core/src/runtime.rs` — consumed for streaming token forwarding |

### 2.2 What OxideMX Duplicates with Custom Code

| AutoAgents Feature | OxideMX Duplicate | Assessment |
|--------------------|-------------------|------------|
| **Actor-based multi-agent runtime** (`SingleThreadedRuntime`, `Environment`, `Topic<M>`, `TypedRuntime`) | `oxidemx-conductor` FlowDoc→DAG→supervisor with joins/retry/timeout/cancel | **Significant overlap but conductors serve different roles.** AutoAgents' runtime is a flat pub-sub + per-actor inbox with no DAG or join semantics. The conductor has explicit graph traversal, join barriers, per-node retry/timeout, and cancel tokens. They are complementary: AutoAgents handles per-agent turn orchestration; the conductor handles multi-agent flow control. |
| **Event fanout** (`EventFanout`) | Custom event forwarding in `oxidemx-agent-core/src/runtime.rs` | OxideMX reinvents `EventFanout` for forwarding the `Event` stream to the UI. Could replace with `DirectAgentHandle::subscribe_events()` or the AutoAgents `EventFanout` type directly. |
| **Provider factory / multi-provider seam** | `oxidemx-agent/src/factory.rs` | Thin wrapper over `LLMBuilder`. Low duplication risk; the factory adds OxideMX-specific config (env-var keys, MistralRs integration, Claude Code CLI). Keep custom factory. |
| **Turn loop** (`oxidemx-agent-core`) | AutoAgents `AgentExecutor` + `TurnEngine` | The OxideMX turn loop was built before the vendored version was available. It is now functionally equivalent to `BasicAgent<T>` (non-ReAct path) or an early draft. The ReAct path is now duplicated by `ReActAgent<T>`. Consider replacing with `ReActAgent` for new agent types. |
| **Session management** (`oxidemx-agent/src/session.rs`) | No AutoAgents equivalent | OxideMX-specific: manages D-Bus channels, iced UI state, persona context. Not duplicated. |

### 2.3 What OxideMX Does NOT Use (Gap / Adoption Opportunity)

| Feature | Gap Level | Notes |
|---------|-----------|-------|
| **`ActorAgent` + `SingleThreadedRuntime` + `Topic<Task>`** | Medium | Spawning multiple agents as actors with pub-sub Task delivery would let the conductor dispatch work without bespoke `JoinSet` management for some patterns. Worth evaluating for the parallel subagent dispatch path. |
| **`PipelineBuilder` + `LLMLayer`** | High value, unused | Currently the `LLMProvider` is used raw. Adding a `CacheLayer`, `RetryLayer`, or `GuardrailsLayer` is trivial with the pipeline API. |
| **`Guardrails` / `GuardrailsBuilder`** | Completely unused | Input/output guard on LLM calls. Relevant for the autonomous coding harness: prevent prompt injection from tool output, redact secrets in responses, audit policy. |
| **Structured output via `AgentOutputT` / `StructuredOutputFormat`** | Partially used | `StructuredOutputFormat` is wired but the derive-based typed output (`#[derive(AgentOutput)]`) is not used project-wide. |
| **`VectorStoreIndex` + `InMemoryVectorStore` + `Embed`** | Partially used | Embeddings are called in `oxidemx-agent-core` (for memory recall). The `VectorStoreIndex` abstraction and `InMemoryVectorStore` are not used — OxideMX has its own in-process vector memory. |
| **`autoagents-qdrant`** | Unused | Persistent vector store for long-lived coding contexts. |
| **WASM tool runtime** (`WasmRuntime`, `wasmtime` feature) | Unused | Sandboxed tool execution. Could isolate untrusted coding tools. |
| **Telemetry** (`autoagents-telemetry`) | Unused | Full OTLP + metrics + Langfuse for agent runs. High value for the autonomous coding harness: cost tracking, latency tracing, turn counts. |
| **`MessageCondition` reactive memory** | Unused | Fine-grained triggers on memory events. Niche for coding harness but useful for persona-triggered memory. |
| **`AgentHooks` all 8 hooks** | Mostly unused | Only `on_run_start` / `on_agent_create` used in practice. Hooks for `on_tool_call` (gating), `on_tool_result`, `on_turn_start` would be useful for the coding harness to enforce tool budgets and log intermediate state. |
| **MCP adapter** (`McpToolsManager`, `McpToolWrapper`) | Partially used in toolkit.rs | Already imported in `oxidemx-agent/src/toolkit.rs`. Ensure it is wired to the coding harness tool registry. |

---

## 3. Structured Output — Exact API

AutoAgents' structured output goes through three layers:

### Layer 1: `AgentOutputT` trait (type contract)

```rust
// autoagents-core/src/agent/output.rs
pub trait AgentOutputT: Serialize + DeserializeOwned + Send + Sync {
    fn output_schema() -> &'static str;            // JSON Schema string
    fn structured_output_format() -> serde_json::Value;  // StructuredOutputFormat shape
}
```

### Layer 2: `#[derive(AgentOutput)]` proc-macro

```rust
// autoagents-derive/src/lib.rs
#[proc_macro_derive(AgentOutput, attributes(output, strict))]
pub fn agent_output(input: TokenStream) -> TokenStream { ... }
```

The `output` attribute selects a field subset; `strict = true` enables strict JSON mode. The macro generates both `output_schema()` (JSON Schema string) and `structured_output_format()` (a `serde_json::Value` in OpenAI's `response_format` shape: `{name, description, schema, strict}`).

### Layer 3: `StructuredOutputFormat` at the LLM call site

```rust
// autoagents-llm/src/chat/mod.rs
pub struct StructuredOutputFormat {
    pub name: String,
    pub description: Option<String>,
    pub schema: Option<serde_json::Value>,
    pub strict: Option<bool>,
}

// Passed to:
async fn chat(&self, messages: &[ChatMessage],
              json_schema: Option<StructuredOutputFormat>) -> ...;
async fn chat_with_tools(&self, messages, tools, json_schema: Option<StructuredOutputFormat>) -> ...;
```

### Wire-up in `BaseAgent`

`BaseAgent::agent_config()` calls `inner.output_schema()`, deserializes it into
`StructuredOutputFormat`, and stores it in `AgentConfig::output_schema`. The executor retrieves
this from `Context::config().output_schema` and passes it to `chat_with_tools` on every turn.

### How to use end-to-end (concrete)

```rust
#[derive(Serialize, Deserialize, AgentOutput)]
#[output(strict)]
struct CodeReviewResult {
    verdict: String,
    issues: Vec<String>,
    score: u32,
}

impl AgentDeriveT for MyReviewer {
    type Output = CodeReviewResult;
    fn output_schema(&self) -> Option<Value> {
        Some(CodeReviewResult::structured_output_format())
    }
    // ...
}
```

The LLM is then told via `json_schema` to return JSON matching `CodeReviewResult`'s schema,
and `ReActAgentOutput::try_parse::<CodeReviewResult>()` deserializes the response.

---

## 4. Multi-Agent Primitives

AutoAgents 0.3.7 provides three multi-agent patterns:

### 4.1 Topic Pub-Sub (mature)

Agents subscribe to `Topic<Task>` at build time; the runtime delivers tasks to all subscribers.
Pattern: fan-out one Task to N specialist agents.

```rust
let topic = Topic::<Task>::new("code-tasks");
let reviewer = AgentBuilder::<_, ActorAgent>::new(ReviewerImpl)
    .llm(llm.clone())
    .runtime(runtime.clone())
    .subscribe(topic.clone())    // Topic<Task> subscription
    .build().await?;

runtime.publish(&topic, Task::new("review PR #42")).await?;
// Reviewer's actor handle() fires with the task
```

**Maturity:** Solid. `SingleThreadedRuntime` has been tested with pub-sub routing.

### 4.2 Direct Actor Messaging (mature)

```rust
let handle = AgentBuilder::<_, ActorAgent>::new(impl_)
    .llm(llm).runtime(rt).build().await?;
handle.actor_ref.cast(Task::new("do thing"))?;
```

Single-agent unicast. No handoff or chaining primitive built in.

### 4.3 Sequential Chaining (not built in — requires composition)

AutoAgents 0.3.7 has **no built-in handoff or sub-agent spawning primitive**. There is no:
- `handoff(to: ActorRef, Task)` method
- Fleet / swarm manager
- Structured parallel fan-out with join semantics
- DAG execution

Multi-step pipelines must be composed manually by the caller, or — as OxideMX has done — by a
dedicated conductor. AutoAgents' `Topic<Task>` pub-sub is the primary primitive for parallel
dispatch; the conductor's `JoinSet` fills the join/barrier gap.

**Verdict:** AutoAgents multi-agent is functional for fan-out-to-actors use cases but immature for
complex orchestration. OxideMX's conductor is the right approach for anything beyond simple
pub-sub.

---

## 5. Recommendations — Top 5 Features to Adopt

Ranked by impact on the autonomous coding harness:

### Rank 1: `PipelineBuilder` + `LLMLayer` with retry and caching

**Why:** Every LLM call in the coding harness is currently raw. Adding a `RetryLayer`
(exponential back-off on rate limits) and a `CacheLayer` (prompt-level semantic cache) would
reduce latency and cost with zero changes to agent logic. `LLMLayer` is a clean seam — implement
once, slot in everywhere via `PipelineBuilder`.

**How to adopt:**
```rust
let llm = PipelineBuilder::new(base_provider)
    .add_layer(RetryLayer::new(3, Duration::from_secs(2)))
    .add_layer(CacheLayer::new(cache_config))
    .build();
```

### Rank 2: `Guardrails` + `GuardrailsLayer`

**Why:** The autonomous coding harness processes tool output (file reads, shell output, web content)
that is an injection surface. `PromptInjectionGuard` and `RegexPiiRedactionGuard` run synchronously
before each LLM call. `EnforcementPolicy::Sanitize` redacts but does not block, keeping the harness
available. This is a safety property that cannot be retrofitted later without significant effort.

**How to adopt:**
```rust
let guardrails = Guardrails::builder()
    .input_guard(PromptInjectionGuard::default())
    .input_guard(RegexPiiRedactionGuard::default())
    .enforcement_policy(EnforcementPolicy::Sanitize)
    .build();

let llm = PipelineBuilder::new(base_provider)
    .add_layer(guardrails.layer())
    .build();
```

### Rank 3: `#[derive(AgentOutput)]` + `StructuredOutputFormat` for all coding outputs

**Why:** The coding harness currently receives free-form text and parses it ad-hoc. Switching to
`AgentOutputT`-derived types (e.g., `CodeEditProposal`, `TestResult`, `DiagnosticSummary`) gives
compile-time schema generation, LLM-constrained JSON output (strict mode), and `try_parse::<T>()`
deserialization — eliminating fragile regex/string parsing.

**How to adopt:** Derive `AgentOutput` on result structs; set `output_schema` in `AgentDeriveT`;
receive typed results from `ReActAgentOutput::try_parse::<T>()`.

### Rank 4: `AgentHooks::on_tool_call` gate + `on_tool_result` logging

**Why:** The coding harness needs tool budget enforcement (max shell execs per run, file-write
limits) and structured logging of every tool result for debugging. `on_tool_call` returning
`HookOutcome::Abort` prevents a tool from running; `on_tool_result` receives the `ToolCallResult`
after success. These are already wired in `ReActAgent`'s turn loop — they just need to be
implemented, not built.

**How to adopt:** Implement `AgentHooks` on the coding agent struct:
```rust
async fn on_tool_call(&self, call: &ToolCall, ctx: &Context) -> HookOutcome {
    if self.shell_budget.fetch_add(1, Ordering::SeqCst) > MAX_SHELLS {
        return HookOutcome::Abort;
    }
    HookOutcome::Continue
}
```

### Rank 5: `autoagents-telemetry` OTLP integration

**Why:** The coding harness will run multi-step, multi-agent sessions. Without telemetry, cost
(token counts), latency (turn duration), and error rates are invisible. `TelemetryHandle` consumes
the existing `Event` stream — no changes to agent logic needed. `EventMapper` auto-generates spans
for each `TaskStarted`/`TaskComplete`/`ToolCallRequested`/`TurnStarted` event, and metrics track
turn counts and durations.

**How to adopt:**
```rust
let event_stream = runtime.subscribe_events().await?;
let _telemetry = start_telemetry(
    event_stream,
    TelemetryConfig::new("oxidemx-coding-harness"),
    None,
    Duration::from_secs(5),
)?;
```

---

## 6. Additional Notes

### WASM Tool Runtime

`autoagents-core/src/tool/runtime/wasm.rs` provides a `WasmRuntime` behind the `wasmtime` feature
flag. This is an optional sandboxing primitive for untrusted tool code. The feature is not enabled
in OxideMX's `Cargo.toml` and is not a priority, but it is available as a future safety layer for
executing user-supplied coding tools.

### Event Stream Architecture

AutoAgents emits a single `mpsc::Sender<Event>` into `BaseAgent` at build time. All protocol
events (turns, tool calls, streaming chunks) pass through this channel. `SingleThreadedRuntime`
aggregates these into a `broadcast::Sender<Event>` for multi-subscriber access.
`oxidemx-agent-core/src/runtime.rs` already taps this stream to forward tokens to the iced UI.
The architecture is sound and consistent with adding telemetry as a second subscriber.

### ractor Dependency

`autoagents-core` re-exports `ractor` for native (non-WASM) targets. OxideMX already considered
`ractor` as the session manager (see comment in `oxidemx-agent/src/session.rs`). The
`ActorAgent` path uses `ractor::Actor` internally, so adopting actor-based agent dispatch would
bring `ractor` into active use without adding a new dependency.

### Conductor vs AutoAgents Runtime — No Conflict

The conductor (`oxidemx-conductor`) operates at the flow level (DAG nodes, joins, retries at the
step level). AutoAgents' `Runtime` operates at the message-delivery level (topics, actor inboxes).
These are orthogonal layers. The conductor can internally use `ActorAgent`-wrapped agents for
parallel steps while still maintaining its own join/retry semantics at the graph level.

---

*File paths in this document are relative to `vendor/AutoAgents/crates/` or the project root
`/run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1/`.*
