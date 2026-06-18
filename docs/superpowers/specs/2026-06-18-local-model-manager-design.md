# Local Model Manager (`oxidemx-localmodel`) — design

Date: 2026-06-18
Status: design (brainstormed + approved in-chat; pending spec review → writing-plans)
Part of the agent re-architecture (see
`docs/superpowers/specs/2026-06-18-agent-framework-rearchitecture-design.md`).
This crate is the EMBEDDED local-LLM path; it **revises** that spec's
"mistral.rs-as-an-HTTP-server" assumption — we embed the mistral.rs Rust SDK
in-process (in agentd) instead.

## 1. Goal

A standalone crate that owns local LLM inference via the mistral.rs Rust SDK:
load/unload models on demand with explicit VRAM control, keep at most one model
loaded at a time, auto-unload on idle, expose live status, and surface models to
AutoAgents + the rest of the app through a model-agnostic service trait and a
no-HTTP `ChatProvider` adapter. Hosted by the persistent **agentd** process
(heavy inference stays out of the GUI).

## 2. Decisions (locked in brainstorm)

1. **Crate `oxidemx-localmodel`**, depends on `mistralrs = "=0.8.1"` (exact pin;
   `docs.rs` fails to build it — use local `cargo doc` + `mistralrs/examples/`).
   Native accelerators behind features (`cuda`/`metal`/`accelerate`). NO UI deps;
   does NOT link `autoagents-mistral-rs` (0.7.0) — two `mistralrs` versions cannot
   co-link.
2. **Own `ChatProvider` adapter** against 0.8.1 (the 0.7.0-pinned
   `autoagents-mistral-rs` is reference-only for conversion logic).
3. **Auto load-on-demand + idle timeout**; explicit `unload_model`/`reload_model`
   for VRAM (never Arc-drop).
4. **One model loaded at a time**; the `default` small model is the hot-path
   resident, specialists evict it.
5. **Hosted by agentd only** (SP1b). Built + live-tested now via an example/CLI
   bin; NOT wired into the overlay/GUI process.

## 3. mistral.rs 0.8.1 API surface used (from SDK research; verify via local cargo doc)

- Per-model builders: `TextModelBuilder` / `MultimodalModelBuilder` (the latter
  renamed from `Vision*`), async `.build()` → `Model` (newtype over
  `Arc<MistralRs>`); `MistralRsBuilder` under `mistralrs::core::`. `Model` and
  `MistralRs` are `Send + Sync` + tokio — fit a long-running process.
- One engine, many models: `MultiModelBuilder::add_model()` /
  `add_model_with_alias(alias, …)` / `with_default_model(id)`. Request methods have
  `_with_model` variants (`send_chat_request_with_model(req, Some("optimizer-8b"))`).
- Lifecycle: `unload_model(id)` (frees VRAM, keeps config) / `reload_model(id)` /
  `remove_model(id)` (drops the model entirely). Status:
  `list_models_with_status → ModelStatus::{Loaded, Unloaded, Reloading}`.
- Tuning: `set_sampler_*` (13 `SamplingParams` incl. `repetition_penalty`);
  `with_isq(IsqType)` / `with_auto_isq(IsqBits)`; web search via
  `with_search(SearchEmbeddingModel)`. Streaming + tool-calling exposed.

## 4. Architecture

```
 oxidemx-localmodel  (lib, no UI)
   LocalModelManager  ── owns one mistralrs engine (MultiModelBuilder) +
                          registry + active-state + idle timer + status
   trait LocalModelService  ── model-agnostic seam (chat / lifecycle / status)
       └ MistralLocalService : LocalModelService   (the 0.8.1-backed impl)
   LocalChatProvider : autoagents ChatProvider  ── the "no-HTTP" adapter,
                          wraps Arc<dyn LocalModelService>
   examples/cli.rs    ── live harness (load → chat → status → unload)
        │ hosted by (SP1b)
        ▼
 agentd  ── holds the singleton manager; exposes lifecycle/status over D-Bus;
            the core's provider factory uses LocalChatProvider (no HTTP)
```

### 4.1 `LocalModelService` (the seam)

```rust
#[async_trait]
pub trait LocalModelService: Send + Sync {
    /// Chat against the active model (auto-loads default if none ready).
    async fn chat(&self, req: ChatRequest) -> Result<ChatReply, LocalError>;
    /// Chat against a specific registered alias (loads/evicts as needed).
    async fn chat_with_model(&self, alias: &str, req: ChatRequest) -> Result<ChatReply, LocalError>;
    /// Ensure `alias` is Ready (evicting the current model if different).
    async fn ensure_loaded(&self, alias: &str) -> Result<(), LocalError>;
    /// Free VRAM for `alias` (keeps its registry config).
    async fn unload(&self, alias: &str) -> Result<(), LocalError>;
    /// Make `alias` the active/default target.
    async fn set_active(&self, alias: &str) -> Result<(), LocalError>;
    /// Live status of every registered model.
    fn status(&self) -> Vec<ModelStatusInfo>;
}

pub struct ModelStatusInfo { pub alias: String, pub state: ModelState, pub last_used: Option<u64> }
pub enum ModelState { Unloaded, Loading, Ready, Busy, Error(String) }
```

`ChatRequest`/`ChatReply` are small crate-owned types (messages, optional tools,
optional sampling/system-template override, stream sink) — NOT AutoAgents types,
so the crate stays framework-agnostic. `LocalChatProvider` translates between
AutoAgents `ChatMessage`/tools and these.

### 4.2 LocalChatProvider (no-HTTP adapter)

Implements AutoAgents `ChatProvider` (+ stub the rest of `LLMProvider` like the
Claude-Code provider does) by calling `LocalModelService::chat_with_model`. Plugs
into the core's `provider_from_config` as the `MistralRs` provider variant —
replacing today's OpenAI-HTTP-to-localhost path. `optimize_prompt`/`simple_chat`
in core then run on the embedded model with no server.

## 5. Lifecycle & one-at-a-time policy

- **Registry** (`LocalModelConfig.models`): each `ModelSpec { alias, source:
  Hf{repo,rev}|Gguf{dir,files}, capabilities{web_search,tools,code_exec}, sampling,
  isq, keep_resident }`. One entry is `default` (hot-path small model, e.g.
  Qwen3-4B q8_0).
- **Serialized load path:** all loads/chats go through an async mutex so
  one-at-a-time holds and loads can't race.
- **ensure_loaded(X):** if `X` is `Unloaded` → `unload_model(current_loaded)` then
  `reload_model(X)` (or first-time `add_model` + load); state
  `Unloaded → Loading → Ready`. A specialist request evicts the resident default
  (can't hold 4B+14B in 12 GB); when the hot path resumes, the `default` is
  reloaded.
- **Auto load-on-demand:** `chat` ensures the target (or `default`) is `Ready`,
  awaiting `Loading`; callers/UI observe `state` via `status()`.
- **Idle timeout:** a background task unloads the active model after
  `idle_timeout_secs` (default 600) of no requests; `keep_resident` models use a
  longer/never timeout (config). Next request reloads.
- **Errors:** load/inference failures set `ModelState::Error(msg)` and return
  `LocalError`; the manager stays usable (next request retries the load).

## 6. Capabilities & sampling

Per `ModelSpec`, mapped at build time: `web_search` → `with_search(...)`; `tools`
→ tool-calling on (the `ChatProvider` passes tool declarations through);
`code_exec` → a flag honored by the executing tool layer (wire to mistral.rs's
interpreter if 0.8.1 exposes one, else our `run_command` tool). Sampling via
`set_sampler_*`; quantization via `with_isq`/`with_auto_isq`. The two optimizer
system prompts (coding vs general) are **request templates** against the loaded
model — no reload to switch.

## 7. Config + settings (in scope)

- `oxidemx-shared`: `LocalModelConfig { download_dir: PathBuf (default
  ~/.config/oxidemx/models), idle_timeout_secs: u64 (default 600), default_model:
  String, models: Vec<ModelSpec> }`, serde-defaulted, additive to `AppConfig`.
- `settings-rs`: a "Local Models" section with the **model download directory**
  field + a **native folder picker** (`rfd`) defaulting to the config folder, and
  an idle-timeout field. The model add/download/select UI is deferred to a later
  sub-project — but the crate API is shaped (registry + status + per-model specs)
  so that UI is additive.

## 8. Intended consumers (downstream roles — NOT in this crate)

The crate ships the service; the ROI-ranked roles live in core/agentd and reuse
it (their own later sub-projects):
- **High ROI:** between-turn context/log/diff **compression**; **routing /
  pre-flight classification** (route easy → local, hard/code → cloud);
  **PII/secret redaction** (rules-first, LLM edge-case fallback).
- **Conditional:** **prompt optimization** — gate on an "is this ambiguous?"
  check; never rewrite already-good/term-precise prompts (measurable retrieval
  harm); ground every output against the input.
- **Signal, never gate:** **cross-model output review** — a smoke detector
  (refusals, malformed JSON, missing files, spec contradictions, leaked secrets);
  correctness is decided by compiler + tests + CI, not the small model.
- **Won't work:** token-level speculative decoding across cloud APIs (incompatible
  tokenizers); only the task-level cascade (covered by routing) is real.

Governing rule (carried into those SPs): route on difficulty, escalate generously,
replace "the small model is confident" with objective verification.

For THIS sub-project we only **repoint the existing `optimize_prompt` /
`simple_chat`** at the local service (via `LocalChatProvider`); the richer roles
are separate features.

## 9. Testing

- **Unit (no model):** the state machine against a **mock `LocalModelService`** /
  a fake engine trait — load→Ready, specialist eviction (one-at-a-time), idle
  unload, error→recover, status transitions, serialization under concurrent
  requests.
- **Live (feature-gated / `#[ignore]`):** `examples/cli.rs` loads a small local
  GGUF, runs a chat, asserts `status()` flips Ready→…→Unloaded across idle, then
  reloads. Manual (needs a model file + VRAM); the 0.8.1 native build is heavy.
- Keep the engine calls behind a thin internal trait so unit tests don't link the
  native engine.

## 10. Risks

- **Heavy native build** (`mistralrs` 0.8.1 + candle + accelerators) — feature-gate
  per platform; long compile; isolate to this crate + agentd.
- **`docs.rs` build failure** for 0.8.1 — rely on local `cargo doc` + `examples/`;
  pin exactly; vendor if churn bites.
- **Version isolation** — never co-link `mistralrs` 0.8.1 with `autoagents-mistral-rs`
  0.7.0; this crate owns its own `ChatProvider`.
- **VRAM OOM / contention** — one-at-a-time + explicit unload + `Error` state;
  document that an external mistral.rs server competing for VRAM will fail loads.
- **Pre-1.0 mistral.rs churn** — the `LocalModelService` trait insulates consumers;
  a version bump is localized to `MistralLocalService`.

## 11. Out of scope (later sub-projects)

agentd hosting + D-Bus lifecycle/status methods (SP1b); model add/download/select
UI; the compression/routing/redaction/review role features; multi-model
*simultaneous* serving (we deliberately keep one-at-a-time).
