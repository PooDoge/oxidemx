# mistralrs integration comparison: autoagents-mistral-rs vs oxidemx-agent-local

Date: 2026-06-19
Context: SP2b design gate — grammar-constrained / structured-output decoding from the local model.

---

## 1. Version delta

| Crate                      | mistralrs pin | Notes                                             |
|----------------------------|---------------|---------------------------------------------------|
| `autoagents-mistral-rs`    | `0.7.0`       | semver range (not pinned exact)                   |
| `oxidemx-agent-local`      | `=0.8.1`      | exact pin (our deliberate choice)                 |

The gap is one full minor version. Key API changes between 0.7.x and 0.8.x that matter here:

- **Builder rename**: `VisionModelBuilder` → `MultimodalModelBuilder`. Their code still uses `VisionModelBuilder`; ours correctly uses `MultimodalModelBuilder`.
- **Multi-model**: `MultiModelBuilder` + `Model::send_chat_request_with_model(req, alias)` + `Model::unload_model` / `Model::reload_model` — these are 0.8.x additions. Their crate has none of this; it holds a single `Arc<mistralrs::Model>`.
- **`Constraint` enum**: present in both 0.7.x and 0.8.x (see §5 below for full variants). The call-site API (`RequestBuilder::set_constraint(Constraint::JsonSchema(…))`) is identical in both versions.
- **`WebSearchOptions`**: added in 0.8.x. Their crate does not expose it.
- **ISQ variants**: 0.8.x adds more ISQ types (`AFQ2`–`AFQ8`, `F8Q8`, `MXFP4`, `HQQ4`, `HQQ8`). Their crate exposes ISQ pass-through but only the subset available in 0.7.x.

---

## 2. Feature matrix

| Feature                           | autoagents-mistral-rs (0.7.0)         | oxidemx-agent-local (0.8.1)              |
|-----------------------------------|---------------------------------------|------------------------------------------|
| **Grammar / constrained decoding** | **Supported** — `Constraint::Regex`, `Lark`, `Llguidance`, `JsonSchema` via `RequestBuilder::set_constraint` | **Absent** — `MistralEngine::generate` builds `RequestBuilder` with messages + sampling only; no `set_constraint` call |
| **JSON-schema structured output** | **Supported** — `build_request_builder` calls `request.set_constraint(Constraint::JsonSchema(schema_value))` when `json_schema` is `Some` and tools are `None` | **Absent** — `chat_with_tools` in `LocalChatProvider` receives `_json_schema` (leading underscore; parameter intentionally ignored) |
| **Streaming (text)**              | Supported — `chat_stream` + `spawn_response_stream` via `model.stream_chat_request` | Absent — `MistralEngine::generate` uses `send_chat_request_with_model` (blocking response only) |
| **Streaming (structured / tools)**| Supported — `chat_stream_struct` and `chat_stream_with_tools` with full `ToolStreamState` assembler | Absent |
| **Tool-calling**                  | Supported — `convert_tools`, `set_tools + set_tool_choice(Auto)`, `ToolCallResponse` extraction | Absent — `LocalChatProvider` drops the `tools` param when building `ChatRequest` (passes `vec![]`); `Mode::ToolUse` sets the capability flag but no tool definitions reach the engine |
| **ISQ quantization**              | Supported — `builder.with_isq(isq)` for HF models | Supported — `parse_isq` maps 22 ISQ string variants; `builder.with_isq(isq_type)` |
| **Vision / multimodal**           | Supported — `VisionModelBuilder` (0.7.x name); `convert_vision_messages` for image content | Supported — `MultimodalModelBuilder` (correct 0.8.x name); `Capabilities::VISION` guard |
| **Sampling controls**             | Supported — temperature, top_p, top_k, max_tokens | Supported — same four params |
| **Load/unload lifecycle**         | Absent — single `Arc<Model>` created at construction; no unload, no reload, no multi-model | Supported — `MultiModelBuilder` + `ensure_loaded_inner` (serial one-at-a-time) + `engine.unload` + idle-eviction sweep + `keep_resident` flag |
| **VRAM management / idle eviction**| Absent                               | Supported — `LocalModelManager::run_idle_sweep_once` + `spawn_idle_task` |
| **Capability scoping (`Mode`)**   | Absent                               | Supported — `Mode::required()` precheck, `Capabilities` bitflags |
| **ResponseGuard failsafe**        | Absent                               | Supported — post-inference heuristic checks (refusal, grounding, schema, repetition, length, term-preservation) |
| **Web-search augmentation**       | Absent                               | Supported — `WebSearchOptions` injected per-alias |
| **Embeddings**                    | Stub — `EmbeddingProvider::embed` returns `Err(LLMError::NoToolSupport)` | Absent — stub `Err` in `LocalChatProvider` |
| **PagedAttention**                | Supported — `PagedAttentionMetaBuilder` optional | Absent |
| **Dedicated tokio runtime**       | Yes — `OnceLock<Runtime>` to survive Python `.so` isolation | No — relies on caller's runtime |

---

## 3. What theirs gives us that we're missing (SP2b-critical)

### Grammar-constrained decoding (the SP2b need)

`autoagents-mistral-rs` fully wires `Constraint` into `build_request_builder`:

```rust
// their provider.rs, line 814-816
if tools.is_none() && let Some(schema) = json_schema && let Some(json_schema) = schema.schema {
    request = request.set_constraint(Constraint::JsonSchema(json_schema));
}
```

The `StructuredOutputFormat.schema` (`serde_json::Value`) is passed directly to `Constraint::JsonSchema`. The `ChatProvider::chat_with_tools` trait method already carries `json_schema: Option<StructuredOutputFormat>`, so callers that pass a schema get grammar-constrained output with **zero agent-layer glue**.

The full `Constraint` enum (same in both 0.7.x and 0.8.1):

```rust
pub enum Constraint {
    Regex(String),          // LALR-anchored regex
    Lark(String),           // Lark grammar string
    JsonSchema(Value),      // JSON Schema object
    Llguidance(LlguidanceGrammar), // llguidance grammar
    None,
}
```

All four are available in mistralrs 0.8.1 and callable via `RequestBuilder::set_constraint` (line 749 of messages.rs).

### Streaming (chat + tools + structured)

Three distinct streaming paths:
- `chat_stream` — simple text delta stream
- `chat_stream_struct` — structured `StreamResponse` with usage
- `chat_stream_with_tools` — full `StreamChunk` stream with `ToolUseStart` / `ToolUseInputDelta` / `ToolUseComplete` events

Our `MistralEngine` uses synchronous `send_chat_request_with_model` only.

### Tool-calling wired to the model

Their `convert_tools` + `set_tools(mistral_tools).set_tool_choice(Auto)` correctly hands tool schemas to the model's sampler. We flag `Mode::ToolUse` as a capability but strip tool definitions before hitting the engine.

---

## 4. What we have that theirs lacks — what would be lost by replacing ours

| Our feature                    | Impact of adopting theirs as-is               |
|-------------------------------|-----------------------------------------------|
| `MultiModelBuilder` multi-slot | Lost — theirs holds one `Arc<Model>`. Multi-model switching gone. |
| Idle-eviction / VRAM management | Lost — no `unload_model`/`reload_model` calls at all. |
| `keep_resident` flag           | Lost                                          |
| `load_lock` serial loading     | Lost — concurrent load races possible         |
| `Capabilities` precheck / `Mode` routing | Lost — their `MistralRsProvider` does no capability gating |
| `ResponseGuard`                | Lost — no post-inference checks               |
| `WebSearchOptions`             | Lost — they target 0.7.x which may not have it |
| 0.8.1 ISQ variants (AFQ*, HQQ*, MXFP4) | Lost — would compile against 0.7.x |
| `MultimodalModelBuilder` (0.8.x name) | Lost — their code uses the 0.7.x `VisionModelBuilder` name |

---

## 5. The fork question

`autoagents-mistral-rs` is ~1300 lines across 5 source files (config, conversion, error, models, provider, lib). It is architecturally clean and could be forked.

**Breaking call sites when bumping from 0.7.0 → 0.8.1:**

1. `VisionModelBuilder` → `MultimodalModelBuilder` (rename; ~3 call sites in provider.rs)
2. `TextModelBuilder::new(repo)` argument type may have changed (check 0.8.x docs — low risk)
3. `GgufModelBuilder::new(model_dir, files)` signature is identical
4. `PagedAttentionMetaBuilder` — may need re-check
5. `Model` is now a `MultiModel` wrapper in 0.8.x; `send_chat_request` vs `send_chat_request_with_model` — their code uses `send_chat_request` which still exists as a convenience wrapper defaulting to the first/only model; this compiles fine but loses multi-model support

**Version-skew risk (critical)**: The rest of AutoAgents (`autoagents-llm`, `autoagents`, etc.) is vendored at `=0.3.7` and depends on `autoagents-mistral-rs` at the workspace version. If we fork `autoagents-mistral-rs` to pull in mistralrs 0.8.1, we create a split: `autoagents-mistral-rs`-fork pulls 0.8.1 while the rest of our crates (`oxidemx-agent-local`) also pull 0.8.1 directly — which is actually **fine** since both resolve to the same semver-pinned `=0.8.1`. The risk would only arise if the forked crate re-exported mistralrs types that collide with the ones in our crate. Since our crate imports `mistralrs` directly, and a forked `autoagents-mistral-rs` would also import `mistralrs = "=0.8.1"`, Cargo unifies them to the same crate instance. **No version-skew problem** as long as the fork pins `mistralrs = "=0.8.1"` to match ours.

---

## 6. Recommendation: (A) Keep oxidemx-agent-local, add grammar-constrained decoding directly

**Reasoning:**

mistralrs 0.8.1 — which we already depend on directly at `=0.8.1` — **exposes the full `Constraint` enum** as a public re-export (`pub use mistralrs_core::{Constraint, …}` in lib.rs line 274) and `RequestBuilder::set_constraint` (messages.rs line 749). We can call `rb.set_constraint(Constraint::JsonSchema(schema_value))` in `MistralEngine::generate` with no additional dependency.

The work to add SP2b support to our crate is small:

1. Add a `constraint: Option<Constraint>` field to `EngineRequest`.
2. In `MistralEngine::generate`, apply `rb = rb.set_constraint(c)` when `Some(c)` is set.
3. In `LocalChatProvider::chat_with_tools`, map the `json_schema: Option<StructuredOutputFormat>` parameter (currently ignored with `_`) to a `Constraint::JsonSchema` and set it on the `ChatRequest`.
4. Thread it through `LocalModelManager::chat_with_model` into the `EngineRequest`.

This is ~20 lines of new code and keeps all our unique features (idle-unload, VRAM management, capability gating, ResponseGuard, keep_resident, multi-model, web-search).

**Why not (B) adopt theirs as-is:** We would lose idle-unload, multi-model, VRAM management, and capability gating — and would regress to an older mistralrs (0.7.x) and the wrong `VisionModelBuilder` name.

**Why not (C) fork + bump:** The `Constraint` API we need is already in our direct dependency. Forking adds maintenance overhead for no capability gain.

**Why the decision to depend on mistralrs 0.8.1 directly still holds:** The 0.8.x multi-model / load-unload / `WebSearchOptions` APIs are 0.8.x-only and are core to our VRAM management story. That decision is still correct. The gap is that we never wired the constraint path through our request types — a straightforward addition.
