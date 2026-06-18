# Local Model Manager (`oxidemx-agent-local`) — design

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

1. **Crate `oxidemx-agent-local`**, depends on `mistralrs = "=0.8.1"` (exact pin;
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
6. **Capability scoping is mandatory** (§6.1): every call declares a `Mode`; the
   service rejects with `CapabilityUnmet` rather than run a prompt the model can't
   serve — the caller escalates.
7. **A `ResponseGuard` failsafe wraps every generation** (§6.2): lexical-first
   sanity checks with safe defaults (preprocessing/transform never silently passes
   a failed output). Both mechanisms ship in this crate.

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
 oxidemx-agent-local  (lib, no UI)
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

`ChatRequest`/`ChatReply` are small crate-owned types — NOT AutoAgents types, so
the crate stays framework-agnostic. `ChatRequest { messages, mode: Mode (§6.1),
tools?, sampling_override?, system_template?, stream_sink? }`; `ChatReply { text,
usage, verdict: Verdict (§6.2) }` — the verdict always rides back so callers see
confidence. `LocalChatProvider` translates between AutoAgents `ChatMessage`/tools
and these, mapping the AutoAgents call onto a `Mode` (default `Chat`, or `ToolUse`
when tool declarations are present).

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

### 6.1 Capability scoping & request modes

Every call through the service is **scoped to the model's capabilities** so we
never run, say, a tool-requiring prompt on a no-tools model.

- `ModelSpec.capabilities: Capabilities { tools, web_search, vision, code_exec,
  structured_output, ctx_window: u32 }` — what the model + its build support.
- Each `ChatRequest` carries a **`Mode`** preset bundling *required capabilities +
  default sampling + which sanity checks (§6.2) apply*:

  | Mode | Requires | Sampling | Guard emphasis |
  |---|---|---|---|
  | `Classify` | structured_output | temp≈0, short | schema/enum + grounding |
  | `Chat` | — | balanced | non-refusal + grounding |
  | `ToolUse` | tools | balanced | tool-call validity |
  | `WebSearch` | web_search | balanced | grounding + citation present |
  | `Transform` (compress/redact/optimize) | — | low temp | leak / no-new-facts / term-preservation |
  | `Vision` | vision | balanced | grounding |

- **Precheck:** before generating, the service resolves the target model and
  verifies `model.capabilities ⊇ mode.required`. If unmet →
  `LocalError::CapabilityUnmet { needs, have }`. The caller (router / provider
  policy) then **escalates to cloud or selects a capable model** — the crate never
  silently degrades. `Mode` also selects the default `ResponseGuard` config and
  sampling, so callers get safe behavior without hand-configuring every call.

### 6.2 Sanity-check / failsafe pipeline (`ResponseGuard`)

Small local models hallucinate, leak training/context data, and drift off-input.
A `ResponseGuard` wraps the **entire** generation (input → generate → checks →
verdict → action) so a bad local output is never silently trusted. Lexical-first
(deterministic, no extra model load); embedding-based grounding is optional when a
local embedder is available.

**Pre-generation (input):**
- *Ambiguity gate* (Transform/optimize): skip rewriting prompts that are already
  clear/term-precise (rewriting good prompts measurably hurts — nDCG regression).
- *PII/secret pre-scan* (rules-first): flag/redact before the text is used onward.
- *Capability check* (§6.1).

**Post-generation checks** (Mode selects the active set + thresholds):
1. *Non-empty / non-refusal* — reject empty or refusal patterns when a substantive
   answer was expected.
2. *Input grounding / relevance* — output must relate to the input: lexical
   overlap (key-term/Jaccard) ≥ threshold, optional local-embedding cosine ≥
   threshold. Catches off-topic hallucination + context/training leaks.
3. *Leak / no-new-facts* (Transform) — output introduces **no** entities, numbers,
   or URLs absent from the input, and never echoes the system prompt or other
   context verbatim. (This is the guard for the "Python-anchoring" class of bug —
   the model injecting unsupported specifics.)
4. *Repetition / degeneration* — n-gram loop / runaway-repeat detector.
5. *Schema / format* (Classify, StructuredJson) — must parse to the expected
   enum/shape; reject otherwise.
6. *Length bounds* — within the Mode's expected envelope (a classifier returns one
   token; an optimize-rewrite isn't 10× the input).
7. *Term preservation* (optimize) — the input's key terms are retained.

**Verdict → action** (per Mode, configurable):
- Verdict: `Ok | Suspect(reasons) | Failed(reasons)`.
- Action: `Retry { max, adjust }` (re-generate lower-temp / new seed) → then
  `Escalate` (return a signal so the caller routes to cloud) or `Reject`. Signal-
  only roles (cross-model review) get `PassFlagged`.
- **Safe defaults:** preprocessing/transform Modes never silently pass a `Failed`
  output — retry once, then escalate/reject. `Chat` may `PassFlagged` for the UI to
  show a low-confidence hint. Every `ChatReply` carries its `Verdict` so callers
  always see confidence.

`ResponseGuard` is a configurable struct (`checks: Vec<Check>`, thresholds, action
policy); each `Mode` ships a default config, and a caller may override per request.
This is the shared mechanism the §8 roles configure differently.

## 7. Config + settings (in scope)

- `oxidemx-shared`: `LocalModelConfig { download_dir: PathBuf (default
  ~/.config/oxidemx/models), idle_timeout_secs: u64 (default 600), default_model:
  String, models: Vec<ModelSpec> }`, serde-defaulted, additive to `AppConfig`.
- `settings-rs`: a "Local Models" section with the **model download directory**
  field + a **native folder picker** (`rfd`) defaulting to the config folder, and
  an idle-timeout field. The model add/download/select UI is deferred to a later
  sub-project — but the crate API is shaped (registry + status + per-model specs)
  so that UI is additive.

## 8. Consumer roles (fully documented; each its own later SP)

The crate ships the *infrastructure* — the `LocalModelService`, `Mode` presets
(§6.1), and `ResponseGuard` (§6.2). The ROI-ranked roles below are how core/agentd
*use* that infrastructure; each is its own implementation plan after the crate +
agentd land. Documenting them here fixes the Mode/guard/escalation contract so the
crate's API doesn't churn when they arrive.

| Role | ROI | Mode | Req. caps | Guard checks | On guard-fail / objective gate |
|---|---|---|---|---|---|
| **Context/log/diff compression** (between turns — biggest token lever) | High | `Transform` | — | leak/no-new-facts, length-bounds, grounding | reject → send original to cloud (never a lossy/hallucinated compression) |
| **Routing / pre-flight classification** (easy→local, hard/code→cloud) | High | `Classify` | structured_output | schema/enum, grounding | on `Suspect`/`Failed` → default to cloud (escalate generously); calibrate conservatively for code |
| **PII / secret redaction** (before cloud calls) | High | `Transform` | — | leak, term-preservation | rules-first (Presidio-style) is the gate; LLM is edge-case fallback only — a missed entity is a leak |
| **Prompt optimization** (use-case #1) | Conditional | `Transform` | — | ambiguity-gate (pre), term-preservation, grounding, no-new-facts | skip if not ambiguous; on fail keep the original prompt |
| **Cross-model output review** (use-case #2) | Signal-only | `Chat` | — | refusal, schema, leak-detect | `PassFlagged` — a smoke detector (refusals, malformed JSON, missing files, spec contradictions, leaked secrets). **Correctness is decided by compiler + tests + CI, never the small model.** "Pick the best of N" needs an objective selector (tests pass, schema valid), not the 4B's opinion. |
| **Simple Q&A / one-shot web search / comparisons** | — | `Chat` / `WebSearch` | (web_search) | non-refusal, grounding, citation-present | escalate to cloud on fail |

**Won't work — not budgeted:** token-level speculative decoding across cloud APIs
(incompatible tokenizers, no logit-verification API); only the *task-level* cascade
(= routing) is real.

**Governing rule** (binds every role): route on difficulty, escalate generously,
and replace "the small model is confident" with objective verification (tests,
compile, schema, lexical/embedding grounding) wherever a wrong accept is costly.

For THIS sub-project we only **repoint the existing `optimize_prompt` /
`simple_chat`** (in `oxidemx-agent-core`) at the local service via
`LocalChatProvider`, using `Transform`/`Chat` Modes + their guards. The five roles
above are separate, sequenced SPs.

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
- **Capability scoping (no model):** `Mode::ToolUse` against a no-tools `ModelSpec`
  → `CapabilityUnmet`; a capable spec → passes the precheck.
- **`ResponseGuard` (no model):** table-driven over canned (input, output) pairs —
  refusal/empty caught; off-topic output fails grounding; a `Transform` output that
  injects an unseen number/URL fails no-new-facts; a degenerate repeat is caught; a
  non-parsing `Classify` output fails schema; a good output passes. These are pure
  string/heuristic functions — fully unit-testable without any model.

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
- **Guard over-rejection (false positives)** — thresholds too strict block good
  local output and over-escalate (cost) ; too loose lets hallucinations through.
  Mitigate: per-Mode thresholds tuned against the table-driven fixtures, conservative
  defaults (prefer escalate over wrong-accept), and `Verdict` always surfaced so the
  behavior is observable/tunable rather than hidden.
- **Grounding without a cloud call** — input-grounding is lexical-first so the
  failsafe needs no network/extra model; embedding-cosine grounding is opt-in only
  when a local embedder is loaded, so the guard never silently depends on the cloud.

## 11. Out of scope for THIS crate (sequenced later sub-projects)

Documented here (§8) but implemented separately, after the crate + agentd:
agentd hosting + D-Bus lifecycle/status methods (SP1b); the model
add/download/select UI; each §8 role feature (compression, routing, redaction,
prompt-optimization, cross-model review) — they configure this crate's `Mode`s +
`ResponseGuard`, so they're additive. Permanently out of scope: multi-model
*simultaneous* serving (we deliberately keep one-at-a-time) and cross-API
speculative decoding (§8 "won't work").
