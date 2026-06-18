# oxidemx-agent-local Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A standalone `oxidemx-agent-local` crate that owns embedded local-LLM inference via the mistral.rs Rust SDK — load/unload with explicit VRAM control, one model at a time, auto-load + idle-unload, capability-scoped requests, and a `ResponseGuard` sanity failsafe — exposed via a `LocalModelService` trait and a no-HTTP AutoAgents `ChatProvider`.

**Architecture:** All control logic (registry, capabilities, modes, guard, manager state machine, provider) is pure and unit-tested against an internal `InferenceEngine` trait with a `MockEngine`. The real `MistralEngine` (mistralrs 0.8.1) is the only thing behind that trait that touches the native engine, gated behind a `mistral` cargo feature (default-off) so the common build/test path never compiles mistralrs. Config types live in `oxidemx-shared`.

**Tech Stack:** Rust, cargo workspace, `mistralrs = "=0.8.1"` (optional/feature-gated), `bitflags`, `thiserror`, `serde`, `tokio`, `async-trait`, AutoAgents 0.3.7 (vendored), `rfd` (settings folder picker).

## Global Constraints

- Crate `oxidemx-agent-local`: **no UI deps**; **never co-link `autoagents-mistral-rs`** (0.7.0) with our `mistralrs = "=0.8.1"`.
- `mistralrs = "=0.8.1"` is **optional, behind feature `mistral`** (default features `[]`); accelerators behind `cuda`/`metal`/`accelerate` features that imply `mistral`. `docs.rs` fails to build mistralrs — verify its API via local `cargo doc` + `mistralrs/examples/`.
- **Capabilities `required` defaults to `Capabilities::empty()`** — a request asking nothing is unconstrained (`Mode::default()` == `Chat`). `CapabilityUnmet` only on a declared, unmet requirement.
- One model loaded at a time; explicit `unload_model`/`reload_model` (never Arc-drop). `status()` is non-blocking (separate `RwLock`), distinct from the load/inference `tokio::sync::Mutex`.
- Lib hygiene: `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]` on public API, no `unwrap`/`expect` outside tests, `thiserror` `#[non_exhaustive] LocalError` (no `anyhow` in public API), `clippy -D warnings`, pristine build.
- Naming (Rust API Guidelines + AI norms): `ChatRequest`/`ChatResponse`; `messages`/`model`/`tools`/`usage{prompt_tokens,completion_tokens}`/`stream`; `ModelState::Failed { reason }` (not `Error`); builders `MistralLocalService::builder()`.
- Spec: `docs/superpowers/specs/2026-06-18-local-model-manager-design.md`. Work in worktree `../oxidemx-phase1` on branch `phase1-local-llm-gateway`. Commit after each task. `cargo build/test -p oxidemx-agent-local` (default features) green after every task.

## File structure

```
oxidemx-shared/src/config.rs   (+ Capabilities, ModelSource, SamplingConfig, ModelSpec, LocalModelConfig)
oxidemx-agent-local/
  Cargo.toml
  src/lib.rs          # crate attrs + module decls + re-exports
  src/error.rs        # LocalError (thiserror, non_exhaustive)
  src/types.rs        # Message, Role, ChatRequest, ChatResponse, Usage, ModelState, ModelStatusInfo
  src/mode.rs         # Mode + required()/sampling()/guard_config()
  src/guard.rs        # Verdict, Reason, Check, GuardConfig, check fns, ResponseGuard
  src/engine.rs       # internal InferenceEngine trait + EngineRequest/EngineReply + MockEngine(cfg test)
  src/service.rs      # LocalModelService trait
  src/manager.rs      # LocalModelManager : LocalModelService (state machine over InferenceEngine)
  src/mistral.rs      # MistralEngine : InferenceEngine  (cfg feature "mistral")
  src/provider.rs     # LocalChatProvider : autoagents ChatProvider
  examples/cli.rs     # live harness (feature "mistral")
settings-rs/src/tabs/ai.rs + main.rs   (Local Models section)
```

---

### Task 1: Scaffold the crate

**Files:**
- Create: `oxidemx-agent-local/Cargo.toml`, `oxidemx-agent-local/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Produces: an empty library crate `oxidemx-agent-local` that builds with default features (no mistralrs).

- [ ] **Step 1: Add to workspace members**

In root `Cargo.toml`, under `[workspace] members`, add `"oxidemx-agent-local",`.

- [ ] **Step 2: Write `oxidemx-agent-local/Cargo.toml`**

```toml
[package]
name = "oxidemx-agent-local"
version = "0.0.1"
edition = "2021"

[features]
default = []
mistral = ["dep:mistralrs"]
cuda = ["mistral", "mistralrs/cuda"]
metal = ["mistral", "mistralrs/metal"]
accelerate = ["mistral", "mistralrs/accelerate"]

[dependencies]
oxidemx-shared = { path = "../oxidemx-shared" }
autoagents = { version = "=0.3.7", default-features = false }
async-trait = "0.1"
thiserror = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt", "sync", "time", "macros"] }
tracing = "0.1"
mistralrs = { version = "=0.8.1", optional = true }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "test-util"] }
```

- [ ] **Step 3: Write `src/lib.rs`**

```rust
//! oxidemx-agent-local — embedded local-LLM inference (mistral.rs 0.8.1) with
//! lifecycle, capability scoping, and a sanity-check failsafe. Hosted by agentd.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
```

- [ ] **Step 4: Build (default features — no mistralrs)**

Run: `cargo build -p oxidemx-agent-local`
Expected: PASS, and `cargo tree -p oxidemx-agent-local | grep -i mistralrs` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml oxidemx-agent-local/
git commit -m "feat(local): scaffold oxidemx-agent-local crate (mistralrs optional)"
```

---

### Task 2: Shared config + `Capabilities` (in `oxidemx-shared`)

**Files:**
- Modify: `oxidemx-shared/src/config.rs`
- Modify: `oxidemx-shared/Cargo.toml` (add `bitflags = "2"` if absent)

**Interfaces:**
- Produces (all in `oxidemx_shared::config`):
  - `bitflags! { pub struct Capabilities: u8 { const TOOLS=1; const WEB_SEARCH=2; const VISION=4; const CODE_EXEC=8; const STRUCTURED_OUTPUT=16; } }` with serde.
  - `pub enum ModelSource { Hf { repo: String, revision: Option<String> }, Gguf { dir: String, files: Vec<String> } }`
  - `pub struct SamplingConfig { pub temperature: Option<f32>, pub top_p: Option<f32>, pub top_k: Option<u32>, pub max_tokens: Option<u32> }` (serde default)
  - `pub struct ModelSpec { pub alias: String, pub source: ModelSource, #[serde(default)] pub capabilities: Capabilities, #[serde(default)] pub sampling: SamplingConfig, pub isq: Option<String>, #[serde(default)] pub keep_resident: bool, #[serde(default)] pub ctx_window: Option<u32> }`
  - `pub struct LocalModelConfig { pub download_dir: PathBuf (default ~/.config/oxidemx/models), pub idle_timeout_secs: u64 (default 600), pub default_model: String, pub models: Vec<ModelSpec> }` (serde defaults) + field on `AppConfig`/`OverlayConfig` (`#[serde(default)] pub local_models: LocalModelConfig`).

- [ ] **Step 1: Write the failing test**

Add to `oxidemx-shared/src/config.rs` tests:
```rust
#[test]
fn capabilities_subset_and_serde() {
    use super::Capabilities;
    let model = Capabilities::TOOLS | Capabilities::WEB_SEARCH;
    assert!(model.contains(Capabilities::empty()));   // non-mandatory default
    assert!(model.contains(Capabilities::TOOLS));
    assert!(!model.contains(Capabilities::VISION));
    let json = serde_json::to_string(&model).unwrap();
    assert_eq!(Capabilities::from_bits_truncate(serde_json::from_str::<u8>(&json).unwrap()), model);
}

#[test]
fn local_model_config_defaults() {
    let c: super::LocalModelConfig = serde_json::from_str("{\"default_model\":\"qwen\",\"models\":[]}").unwrap();
    assert_eq!(c.idle_timeout_secs, 600);
    assert!(c.download_dir.ends_with("oxidemx/models"));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p oxidemx-shared capabilities_subset_and_serde local_model_config_defaults`
Expected: FAIL (types not defined).

- [ ] **Step 3: Implement the types**

Add `bitflags = "2"` to `oxidemx-shared/Cargo.toml`. Define `Capabilities` via `bitflags!` with `#[derive(Serialize, Deserialize)]` (serialize as the `u8` bits via `#[serde(transparent)]` on a wrapper, or `bitflags` serde feature — use `bitflags = { version = "2", features = ["serde"] }` and serialize as bits). Define `ModelSource`, `SamplingConfig` (`#[derive(Default)]`), `ModelSpec`, `LocalModelConfig` with `fn default_idle()->u64{600}` / `fn default_models_dir()->PathBuf` (HOME-based, falling back to `.config/oxidemx/models`), and add `#[serde(default)] pub local_models: LocalModelConfig` to the existing AI/overlay config struct + its `Default`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p oxidemx-shared capabilities_subset_and_serde local_model_config_defaults`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add oxidemx-shared/
git commit -m "feat(shared): LocalModelConfig + ModelSpec + Capabilities bitflags"
```

---

### Task 3: `error` + `types`

**Files:**
- Create: `oxidemx-agent-local/src/error.rs`, `oxidemx-agent-local/src/types.rs`
- Modify: `oxidemx-agent-local/src/lib.rs`

**Interfaces:**
- Produces:
  - `#[non_exhaustive] pub enum LocalError { ModelNotFound(String), CapabilityUnmet { needs: Capabilities, have: Capabilities }, LoadFailed { alias: String, reason: String }, Inference(String), GuardRejected { reasons: Vec<String> } }` (thiserror `Display`).
  - `pub enum Role { System, User, Assistant, Tool }`; `pub struct Message { pub role: Role, pub content: String }`.
  - `pub struct ChatRequest { pub messages: Vec<Message>, pub mode: Mode, pub tools: Vec<serde_json::Value>, pub sampling_override: Option<SamplingConfig>, pub system_template: Option<String> }` (`Mode` from Task 4 — declare the module so this compiles; if ordering is awkward, define `Mode` first).
  - `pub struct Usage { pub prompt_tokens: u32, pub completion_tokens: u32 }`
  - `pub struct ChatResponse { pub text: String, pub usage: Usage, pub verdict: Verdict }` (`Verdict` from Task 5).
  - `pub enum ModelState { Unloaded, Loading, Ready, Busy, Failed { reason: String } }`; `pub struct ModelStatusInfo { pub alias: String, pub state: ModelState, pub last_used: Option<u64> }`.

> Implementation note: Tasks 3–5 have mutual references (`ChatRequest.mode`, `ChatResponse.verdict`). Declare `pub mod mode; pub mod guard; pub mod types; pub mod error;` together in `lib.rs` and let the type bodies reference each other; the build gate is the whole group compiling. If a reviewer prefers, fold Tasks 3–5 into one commit — they form one coherent "core types" deliverable.

- [ ] **Step 1: Failing test** (`types.rs`)
```rust
#[test]
fn model_state_is_a_state_not_an_error() {
    let s = ModelState::Failed { reason: "oom".into() };
    assert!(matches!(s, ModelState::Failed { .. }));
}
```
- [ ] **Step 2: Run → FAIL** `cargo test -p oxidemx-agent-local model_state_is_a_state_not_an_error` (undefined).
- [ ] **Step 3: Implement** `error.rs` + `types.rs` with the types above; `pub use` them from `lib.rs`. Derive `Debug, Clone` where sensible; `Serialize/Deserialize` on `Message`/`Usage`/`ModelState`/`ModelStatusInfo`.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** `feat(local): LocalError + core request/response/state types`

---

### Task 4: `mode`

**Files:** Create `oxidemx-agent-local/src/mode.rs`; modify `lib.rs`.

**Interfaces:**
- Produces: `pub enum Mode { Classify, Chat, ToolUse, WebSearch, Transform, Vision }` with `impl Default for Mode { fn default()->Self { Mode::Chat } }` and methods `pub fn required(&self) -> Capabilities`, `pub fn sampling(&self) -> SamplingConfig`, `pub fn guard_config(&self) -> GuardConfig` (`GuardConfig` from Task 5).

- [ ] **Step 1: Failing test**
```rust
#[test]
fn mode_required_caps_and_default() {
    assert_eq!(Mode::default(), Mode::Chat);
    assert_eq!(Mode::Chat.required(), Capabilities::empty());      // non-mandatory
    assert_eq!(Mode::ToolUse.required(), Capabilities::TOOLS);
    assert_eq!(Mode::WebSearch.required(), Capabilities::WEB_SEARCH);
    assert_eq!(Mode::Vision.required(), Capabilities::VISION);
    assert_eq!(Mode::Classify.required(), Capabilities::STRUCTURED_OUTPUT);
    assert_eq!(Mode::Transform.required(), Capabilities::empty());
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the enum + `required()` per the table; `sampling()` returns low-temp for `Classify`/`Transform`, balanced otherwise; `guard_config()` returns the per-mode `GuardConfig` (Task 5) — until Task 5 lands, return `GuardConfig::default()` and refine in Task 5.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** `feat(local): request Modes + required-capability mapping`

---

### Task 5: `guard` — ResponseGuard sanity failsafe

**Files:** Create `oxidemx-agent-local/src/guard.rs`; modify `lib.rs`, `mode.rs` (wire `guard_config()`).

**Interfaces:**
- Produces:
  - `pub enum Verdict { Ok, Suspect(Vec<Reason>), Failed(Vec<Reason>) }`; `pub struct Reason { pub check: &'static str, pub detail: String }`.
  - `pub enum Action { Retry { max: u8 }, Escalate, Reject, PassFlagged }`.
  - `pub struct GuardConfig { pub checks: Vec<Check>, pub action_on_fail: Action }` (`Default` = grounding+refusal, `Escalate`).
  - `pub enum Check { NonEmptyNonRefusal, Grounding { min_overlap: f32 }, NoNewFacts, Repetition, Schema(SchemaKind), LengthBounds { max_ratio: f32 }, TermPreservation { min_keep: f32 } }`; `pub enum SchemaKind { OneOf(Vec<String>), Json }`.
  - `pub fn evaluate(cfg: &GuardConfig, input: &str, output: &str) -> Verdict` — runs each `Check` as a pure fn over `(input, output)`.
  - Pure helpers (all `pub(crate)`, individually testable): `is_refusal(&str)->bool`, `lexical_overlap(input,output)->f32`, `introduces_new_specifics(input,output)->Vec<String>`, `has_repetition(&str)->bool`, `matches_schema(output,&SchemaKind)->bool`, `term_preservation(input,output)->f32`.

- [ ] **Step 1: Failing tests (table-driven, one assertion per check)**
```rust
#[test] fn refusal_detected() { assert!(is_refusal("I cannot help with that.")); assert!(!is_refusal("Paris.")); }
#[test] fn grounding_flags_offtopic() {
    assert!(lexical_overlap("capital of France?", "Paris is the capital of France") > 0.3);
    assert!(lexical_overlap("capital of France?", "Bananas grow in the tropics") < 0.2);
}
#[test] fn no_new_facts_catches_injected_specifics() {
    // Transform must not introduce a number/URL absent from the input.
    let extra = introduces_new_specifics("Summarize: the build failed", "It failed in 3.14 seconds at http://x");
    assert!(extra.iter().any(|s| s == "3.14") && extra.iter().any(|s| s.contains("http://x")));
    assert!(introduces_new_specifics("retry 3 times", "retry 3 times please").is_empty());
}
#[test] fn repetition_caught() { assert!(has_repetition("go go go go go go go go")); assert!(!has_repetition("a normal sentence")); }
#[test] fn schema_oneof() { let k = SchemaKind::OneOf(vec!["SIMPLE".into(),"COMPLEX".into()]); assert!(matches_schema("SIMPLE", &k)); assert!(!matches_schema("maybe", &k)); }
#[test] fn evaluate_fails_offtopic_classify() {
    let cfg = GuardConfig { checks: vec![Check::Schema(SchemaKind::OneOf(vec!["SIMPLE".into(),"COMPLEX".into()]))], action_on_fail: Action::Escalate };
    assert!(matches!(evaluate(&cfg, "classify this", "definitely simple, I think"), Verdict::Failed(_)));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the pure helpers + `Check`/`Verdict`/`GuardConfig`/`evaluate`. `lexical_overlap` = Jaccard over lowercased word sets (ignore stopwords). `introduces_new_specifics` = numbers/URLs/Capitalized-entities in output not present in input. `is_refusal` = match against a small refusal-phrase set. `has_repetition` = any 3-gram repeated > N times. Wire `Mode::guard_config()` to real per-mode configs (Classify→Schema+Grounding; Transform→NoNewFacts+TermPreservation+LengthBounds; Chat→NonEmptyNonRefusal+Grounding, `PassFlagged`).
- [ ] **Step 4: Run → PASS** (`cargo test -p oxidemx-agent-local guard`).
- [ ] **Step 5: Commit** `feat(local): ResponseGuard sanity checks (grounding/leak/schema/repetition)`

---

### Task 6: `engine` — internal `InferenceEngine` seam + `MockEngine`

**Files:** Create `oxidemx-agent-local/src/engine.rs`; modify `lib.rs`.

**Interfaces:**
- Produces:
  - `pub(crate) struct EngineRequest { pub messages: Vec<Message>, pub sampling: SamplingConfig, pub tools: Vec<serde_json::Value> }`; `pub(crate) struct EngineReply { pub text: String, pub usage: Usage }`.
  - `#[async_trait] pub(crate) trait InferenceEngine: Send + Sync { async fn load(&self, spec: &ModelSpec) -> Result<(), LocalError>; async fn unload(&self, alias: &str) -> Result<(), LocalError>; async fn generate(&self, alias: &str, req: &EngineRequest) -> Result<EngineReply, LocalError>; }`
  - `#[cfg(test)] pub(crate) struct MockEngine { … }` — records load/unload calls, returns a scripted `EngineReply` (settable per test), can be told to fail a load.

- [ ] **Step 1: Failing test**
```rust
#[tokio::test]
async fn mock_engine_records_and_replies() {
    let m = MockEngine::new().with_reply("pong");
    let spec = test_spec("q");                // helper builds a ModelSpec
    m.load(&spec).await.unwrap();
    let r = m.generate("q", &EngineRequest{messages:vec![],sampling:Default::default(),tools:vec![]}).await.unwrap();
    assert_eq!(r.text, "pong");
    assert_eq!(m.loaded(), vec!["q".to_string()]);
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the trait + `MockEngine` (interior `Mutex<Vec<String>>` for load log, `Mutex<Option<&str>>` for fail-next, scripted reply). `test_spec` helper in a `#[cfg(test)]` mod.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** `feat(local): InferenceEngine seam + MockEngine for tests`

---

### Task 7: `service` trait + `manager` state machine

**Files:** Create `oxidemx-agent-local/src/service.rs`, `oxidemx-agent-local/src/manager.rs`; modify `lib.rs`.

**Interfaces:**
- Consumes: everything from Tasks 2–6.
- Produces:
  - `#[async_trait] pub trait LocalModelService: Send + Sync { async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LocalError>; async fn chat_with_model(&self, alias: &str, req: ChatRequest) -> Result<ChatResponse, LocalError>; async fn ensure_loaded(&self, alias: &str) -> Result<(), LocalError>; async fn unload(&self, alias: &str) -> Result<(), LocalError>; async fn set_active(&self, alias: &str) -> Result<(), LocalError>; fn status(&self) -> Vec<ModelStatusInfo>; }`
  - `pub struct LocalModelManager { /* registry, engine: Arc<dyn InferenceEngine>, load_lock: tokio::sync::Mutex<()>, states: std::sync::RwLock<HashMap<String, (ModelState, Option<u64>)>>, active: Mutex<Option<String>>, idle_timeout: Duration, default_model: String */ }` + `pub fn new(models: Vec<ModelSpec>, default_model: String, idle_timeout: Duration, engine: Arc<dyn InferenceEngine>) -> Self`.
  - `impl LocalModelService for LocalModelManager`.

- [ ] **Step 1: Failing tests (with `MockEngine`)**
```rust
#[tokio::test]
async fn ensure_loaded_sets_ready_and_one_at_a_time() {
    let mgr = mgr_with(["a","b"], "a");                  // helper: manager over MockEngine
    mgr.ensure_loaded("a").await.unwrap();
    assert_eq!(state(&mgr,"a"), ModelState::Ready);
    mgr.ensure_loaded("b").await.unwrap();               // must evict a
    assert_eq!(state(&mgr,"a"), ModelState::Unloaded);
    assert_eq!(state(&mgr,"b"), ModelState::Ready);
}

#[tokio::test]
async fn capability_unmet_when_mode_needs_more() {
    let mgr = mgr_with_caps("a", Capabilities::empty());  // model has no caps
    let req = ChatRequest{ mode: Mode::ToolUse, ..req("hi") };
    assert!(matches!(mgr.chat_with_model("a", req).await, Err(LocalError::CapabilityUnmet{..})));
}

#[tokio::test]
async fn guard_failure_escalates() {
    let mgr = mgr_reply("a", "Bananas");                  // off-topic vs a grounded Chat
    let req = ChatRequest{ mode: Mode::Chat, ..req("capital of France") };
    // Chat guard PassFlagged -> Suspect verdict surfaced, not an error:
    let resp = mgr.chat_with_model("a", req).await.unwrap();
    assert!(matches!(resp.verdict, Verdict::Suspect(_) | Verdict::Failed(_)));
}

#[tokio::test(start_paused = true)]
async fn idle_unloads_after_timeout() {
    let mgr = mgr_with(["a"], "a");
    mgr.ensure_loaded("a").await.unwrap();
    mgr.run_idle_sweep_once(Instant::now() + Duration::from_secs(601));  // test hook
    assert_eq!(state(&mgr,"a"), ModelState::Unloaded);
}

#[tokio::test]
async fn status_is_nonblocking_snapshot() {
    let mgr = mgr_with(["a"], "a");
    assert_eq!(mgr.status().len(), 1);                    // reads RwLock, not the load mutex
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the manager:
  - `ensure_loaded(X)`: acquire `load_lock`; if X already `Ready`, return; else set X `Loading` (RwLock), `unload` the current active (engine.unload + state Unloaded), `engine.load(spec_X)`, set X `Ready` + active=X; on engine error set `Failed{reason}` + return `LoadFailed`.
  - `chat_with_model(alias, req)`: look up spec (`ModelNotFound` else); **capability precheck** `spec.capabilities.contains(req.mode.required())` else `CapabilityUnmet`; `ensure_loaded(alias)`; build `EngineRequest` (mode.sampling merged with override); set `Busy`, `engine.generate`, set `Ready`, update `last_used`; run `evaluate(req.mode.guard_config(), last_user_msg, text)` → apply `Action` (Retry up to N with adjusted sampling; Escalate→`Err(GuardRejected)` carrying reasons; Reject→`Err`; PassFlagged→return `ChatResponse` with the `Suspect`/`Failed` verdict). Return `ChatResponse{ text, usage, verdict }`.
  - `chat(req)`: `chat_with_model(active.or(default), req)`.
  - `set_active`/`unload` straightforward; `status()` reads the RwLock into `Vec<ModelStatusInfo>`.
  - Provide a `run_idle_sweep_once(now)` test hook + a `spawn_idle_task()` that calls it on a `tokio::time::interval`; `keep_resident` specs skip unload.
- [ ] **Step 4: Run → PASS** (`cargo test -p oxidemx-agent-local manager`).
- [ ] **Step 5: Commit** `feat(local): LocalModelManager — lifecycle, capability scoping, guard, idle`

---

### Task 8: `provider` — `LocalChatProvider` (no-HTTP AutoAgents adapter)

**Files:** Create `oxidemx-agent-local/src/provider.rs`; modify `lib.rs`.

**Interfaces:**
- Consumes: `LocalModelService`, AutoAgents `ChatProvider`/`LLMProvider`/`ChatMessage`/`ChatResponse`(trait)/`LLMError`.
- Produces: `pub struct LocalChatProvider { service: Arc<dyn LocalModelService>, alias: String }` + `pub fn new(service, alias) -> Self`; `impl ChatProvider` (real `chat_with_tools`) + empty `CompletionProvider`/`EmbeddingProvider`/`ModelsProvider` + `impl LLMProvider` (stub like `ClaudeCodeProvider`).

- [ ] **Step 1: Failing test (mock `LocalModelService`)**
```rust
#[tokio::test]
async fn provider_maps_messages_and_returns_text() {
    let svc = Arc::new(StubService::returning("pong"));   // impl LocalModelService in test
    let p = LocalChatProvider::new(svc, "a".into());
    let msgs = [autoagents::llm::chat::ChatMessage::user("ping")];
    let resp = p.chat_with_tools(&msgs, None, None).await.unwrap();
    assert_eq!(resp.text().unwrap_or_default().trim(), "pong");
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `chat_with_tools`: map `&[ChatMessage]` → `Vec<Message>`, choose `Mode::ToolUse` if `tools.is_some()` else `Mode::Chat`, call `service.chat_with_model(alias, req)`, wrap the `ChatResponse.text` in a `Box<dyn autoagents ChatResponse>` adapter (mirror `oxidemx-agent/src/claude_code.rs`). Map `LocalError` → `LLMError`. Stub the other provider traits.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** `feat(local): LocalChatProvider AutoAgents adapter (no HTTP)`

---

### Task 9: `mistral` — real `MistralEngine` (feature-gated, API-discovery-bound)

**Files:** Create `oxidemx-agent-local/src/mistral.rs`; modify `lib.rs` (`#[cfg(feature="mistral")] mod mistral;`).

**Interfaces:**
- Produces: `#[cfg(feature="mistral")] pub struct MistralEngine { /* Model (Arc<MistralRs>) + alias→loaded map */ }` + `pub async fn new(download_dir, specs) -> Result<Self, LocalError>` (registers models via `MultiModelBuilder` without loading), implementing `InferenceEngine`.

> **This task is API-discovery-bound.** `docs.rs` cannot build mistralrs 0.8.1, so the implementer MUST generate local docs first and verify exact signatures:
> `cargo doc -p mistralrs --no-deps --open` (or read `~/.cargo/registry/src/*/mistralrs-0.8.1/` + the crate's `examples/`). The `InferenceEngine` trait (Task 6) is the stable contract; only this file changes if the 0.8.1 API differs from the notes below.

- [ ] **Step 1: Verify the API locally**

Run: `cargo doc -p mistralrs --no-deps 2>&1 | tail -5` (under `--features mistral`), then inspect the registry source for: `TextModelBuilder`, `MultiModelBuilder::{add_model_with_alias, with_default_model}`, `Model::{unload_model, reload_model, list_models_with_status}`, `send_chat_request_with_model`, `set_sampler_*`, `with_isq`, `with_search`. Record the real signatures in the task report.

- [ ] **Step 2: Implement `MistralEngine` against the verified API**

`new`: build a `MultiModelBuilder`, `add_model_with_alias` for each `ModelSpec` (mapping `ModelSource::Hf`→`TextModelBuilder::new(repo)`, `Gguf`→GGUF builder; apply `with_isq`, `with_search` when `capabilities` includes `WEB_SEARCH`), `.build()` → store the `Model`. `load(spec)` = `reload_model(alias)`; `unload(alias)` = `unload_model(alias)`; `generate(alias, req)` = build a chat request (messages + sampling via `set_sampler_*`), `send_chat_request_with_model(req, Some(alias))`, map response text + usage into `EngineReply`. Map all mistralrs errors → `LocalError`. Keep ALL mistralrs imports confined to this file.

- [ ] **Step 3: Build under the feature**

Run: `cargo build -p oxidemx-agent-local --features mistral`
Expected: PASS (long native build; CUDA/Metal via `--features cuda`/`metal`). No live model needed to compile.

- [ ] **Step 4: Confirm default build still excludes mistralrs**

Run: `cargo build -p oxidemx-agent-local && cargo tree -p oxidemx-agent-local | grep -i mistralrs || echo "mistralrs absent by default ✓"`
Expected: `mistralrs absent by default ✓`.

- [ ] **Step 5: Commit** `feat(local): MistralEngine (mistralrs 0.8.1, feature-gated)`

---

### Task 10: `examples/cli.rs` — live harness

**Files:** Create `oxidemx-agent-local/examples/cli.rs`.

**Interfaces:** Consumes `LocalModelManager` + `MistralEngine` (feature `mistral`).

- [ ] **Step 1: Write the example**

A `#[tokio::main]` bin (gated `#![cfg(feature="mistral")]` body) that: reads a `ModelSpec` from argv (alias + GGUF path or HF repo), builds `MistralEngine::new` + `LocalModelManager`, prints `status()`, runs `chat_with_model` on a prompt, prints the `ChatResponse` (text + verdict), calls `run_idle_sweep_once(far_future)`, prints `status()` (expect `Unloaded`), then `ensure_loaded` again. Plain `println!` output; no test assertions (it's a manual harness).

- [ ] **Step 2: Build it**

Run: `cargo build -p oxidemx-agent-local --features mistral --example cli`
Expected: PASS.

- [ ] **Step 3: Document how to run** (in the example's top doc-comment): `cargo run -p oxidemx-agent-local --features metal --example cli -- <alias> <gguf-path-or-hf-repo> "prompt"`. Live run needs a model + VRAM; not part of CI.

- [ ] **Step 4: Commit** `feat(local): live CLI example for load/chat/status/idle`

---

### Task 11: Settings — "Local Models" section (download dir + folder picker)

**Files:** Modify `settings-rs/src/tabs/ai.rs`, `settings-rs/src/main.rs`.

**Interfaces:** Consumes `oxidemx_shared::config::LocalModelConfig`.

- [ ] **Step 1: Add messages**

In `settings-rs/src/main.rs` `Message`: `AiModelDirChanged(String)`, `AiModelDirPick`, `AiIdleTimeoutChanged(String)`.

- [ ] **Step 2: Add handlers**

`AiModelDirChanged(s)` → set `state.config.overlay.local_models.download_dir = PathBuf::from(s)`, `touch()`. `AiModelDirPick` → `Task::perform(async { rfd::AsyncFileDialog::new().pick_folder().await }, |o| Message::AiModelDirChanged(folder_or_current))`. `AiIdleTimeoutChanged(s)` → parse u64, set `idle_timeout_secs`, `touch()`. (Mirror the existing `AiLocalEndpointChanged` handler shape.)

- [ ] **Step 3: Add the view section**

In `tabs/ai.rs`, a `section_block(state, "Local Models", local_models_panel(state))` with: a `text_input` for `download_dir` (`on_input(AiModelDirChanged)`) + a "Choose folder…" `button(on_press = AiModelDirPick)`, and an idle-timeout `text_input`. Add `rfd` to `settings-rs/Cargo.toml` if absent (it's used elsewhere — verify).

- [ ] **Step 4: Build**

Run: `cargo build -p oxidemx-settings`
Expected: PASS.

- [ ] **Step 5: Commit** `feat(settings): Local Models section — model dir + folder picker + idle timeout`

---

## Self-Review

- **Spec coverage:** §3 API → Task 9 (with local-doc verification); §4 architecture/service/provider → Tasks 6,7,8; §4.1 trait + non-blocking status → Task 7; §4.2 provider → Task 8; §5 lifecycle/one-at-a-time/idle → Task 7; §6 caps/sampling → Tasks 2,4,9; §6.1 capability scoping + non-mandatory default → Tasks 2,4,7; §6.2 ResponseGuard → Task 5 (+ applied in Task 7); §7 config + settings → Tasks 2,11; §9 testing (mock-based, no native) → Tasks 6,7 + feature-gated 9/10; §12 conventions → Global Constraints + applied throughout. Roles (§8) are out of scope (later SPs) — correctly not tasked.
- **Placeholder scan:** none — pure-logic tasks carry real test + impl code; Task 9 is explicitly API-discovery-bound with a verification step (not a placeholder — the trait contract is concrete and the verification command is exact).
- **Type consistency:** `ChatRequest`/`ChatResponse`/`Usage`/`ModelState::Failed`/`Capabilities`/`Mode`/`Verdict`/`GuardConfig`/`InferenceEngine`/`LocalModelService`/`LocalModelManager`/`LocalChatProvider`/`MistralEngine` used consistently across tasks; `Mode::required()`/`guard_config()` defined in Task 4 and consumed in Task 7; `evaluate`/`Check` defined in Task 5 and consumed in Task 7.
- **Note:** Tasks 3–5 inter-reference (request carries `Mode`, response carries `Verdict`); the plan flags they may be one commit if a reviewer prefers — each is still independently testable.
