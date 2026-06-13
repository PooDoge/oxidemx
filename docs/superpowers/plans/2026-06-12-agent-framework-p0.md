# Agent Framework P0 Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the AutoAgents integration end-to-end: a custom `GeminiInteractionsProvider` (our v1beta/interactions transport behind AutoAgents' `LLMProvider` traits), the `execute_command` tool bridged with our allowlist, and a ReAct agent running in a CLI harness.

**Architecture:** New workspace crate `oxidemx-agent` (lib + `oxidemx-agent-cli` bin). The provider keeps Interactions' server-side session (`previous_interaction_id`) internally and sends only the *newest* message per call — the framework believes it ships history; the server replays it. One provider instance per agent/conversation (documented constraint). SSE folding is ported from `overlay-rs/src/ai_client/sse.rs` with its tests. Allowlist matcher ported from `overlay-rs/src/agent/commands.rs` (unification into a shared crate is P1; for P0 the port lives in `oxidemx-agent` and overlay-rs is untouched).

**Tech Stack:** Rust 1.96 (edition-2024 capable), `autoagents = "=0.3.7"` + `autoagents-derive = "=0.3.7"` (default features — none — so no provider backends), reqwest, tokio, serde_json, futures-util, tokio-util (CancellationToken).

**Spec:** `docs/plans/agent-framework-integration-brainstorm.md` Part I §3.2–3.3 and Part II §12 (P0).

**Worktree:** `mx-master-4-linux/oxidemx-agent-framework`, branch `agent-framework` off `rust-gtk4-overlay`.

**Verified API ground truth used below:**
- `ChatProvider::chat_with_tools(&[ChatMessage], Option<&[Tool]>, Option<StructuredOutputFormat>) -> Result<Box<dyn ChatResponse>, LLMError>` is the single required method (`AutoAgents/crates/autoagents-llm/src/chat/mod.rs:449+`); everything else has default impls except `CompletionProvider::complete`, `EmbeddingProvider::embed`, `ModelsProvider::list_models`.
- `ChatResponse` needs `text() -> Option<String>`, `tool_calls() -> Option<Vec<ToolCall>>`, plus `Debug + Display`.
- `ToolCall { id, call_type: "function", function: FunctionCall { name, arguments: String } }` (`autoagents-llm/src/lib.rs:111-139`).
- `ChatMessage { role: ChatRole, message_type: MessageType, content: String }`; tool results arrive as `MessageType::ToolResult(Vec<ToolCall>)` where each call's `arguments` holds the *result* JSON string.
- AutoAgents `Tool { tool_type, function: FunctionTool { name, description, parameters } }` → Interactions wants the **flat** shape `{type:"function", name, description, parameters}` (verified `overlay-rs/src/ai_client.rs:274-288`).
- Interactions request: `{model, input, tools, system_instruction, previous_interaction_id?, stream?, store?}`; `input` = prompt string OR `{type:"function_result", call_id, name, result:[{type:"text",text}]}` (verified `ai_client.rs:559-625`).
- Tool defined via `#[tool(name, description, input = ArgsT)]` + `impl ToolRuntime { async fn execute(&self, args: Value) -> Result<Value, ToolCallError> }`; agent via `#[agent(name, description, tools=[...])] #[derive(Clone, AgentHooks)]` + `AgentBuilder::<_, DirectAgent>` (verified `AutoAgents/examples/basic/src/simple.rs`).

**Open risk checkpoints (resolve live in Task 6):**
1. Multiple tool calls in one round → we send `input` as an *array* of function_result objects; unverified against the live API (our current client only ever returns one).
2. `store:false` + no `previous_interaction_id` (fully stateless) is proven for nested search calls; the session-id path is proven by the overlay. Both shapes exist in production code, so risk is low.

---

### Task 1: Crate scaffold (`oxidemx-agent`) — MILESTONE M1

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `oxidemx-agent/Cargo.toml`
- Create: `oxidemx-agent/src/lib.rs`

- [ ] **Step 1: Add workspace member** — add `"oxidemx-agent"` to `members` in root `Cargo.toml`.

- [ ] **Step 2: Crate manifest**

```toml
# oxidemx-agent/Cargo.toml
[package]
name = "oxidemx-agent"
version = "0.1.0"
edition = "2021"
description = "AutoAgents-based agent runtime for OxideMX (P0 spike: Gemini Interactions provider + execute_command tool + ReAct CLI harness)"

[dependencies]
autoagents = { version = "=0.3.7", default-features = false }
autoagents-derive = "=0.3.7"
async-trait = "0.1"
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "time", "sync"] }
tokio-util = "0.7"
futures-util = "0.3"
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "0.8"
thiserror = "1"
tracing = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }

[[bin]]
name = "oxidemx-agent-cli"
path = "src/bin/cli.rs"
required-features = []
```

(If crates.io fetch fails offline, fall back to `path = "/run/media/system/fastdrive/Games/AutoAgents/crates/autoagents"` dependencies and note it in the commit message.)

- [ ] **Step 3: lib.rs skeleton**

```rust
//! OxideMX agent runtime on AutoAgents (P0 spike).
pub mod allowlist;
pub mod provider;
pub mod tools;
```

(Comment out modules not yet created; uncomment per task.)

- [ ] **Step 4: Verify** — Run: `cargo check -p oxidemx-agent`. Expected: clean (empty lib).

- [ ] **Step 5: Commit** — `feat(agent): scaffold oxidemx-agent crate pinned to autoagents 0.3.7`

### Task 2: Port the allowlist module — MILESTONE M2 (part 1)

**Files:**
- Create: `oxidemx-agent/src/allowlist.rs`

- [ ] **Step 1: Port** `subcommands`, `strip_wrappers`, `is_allowlisted`, `cap_output`, `run`, constants and ALL tests verbatim from `overlay-rs/src/agent/commands.rs:38-327`, **minus** the config-coupled `allowlist()`/`add_allowlist_entry()` (the CLI harness loads config itself; persistence stays overlay-side until P1 unification). Header comment must say it is a port and name the source file.

- [ ] **Step 2: Verify** — Run: `cargo test -p oxidemx-agent allowlist`. Expected: 13 tests pass (same suite as overlay-rs).

- [ ] **Step 3: Commit** — `feat(agent): port command allowlist matcher + runner from overlay-rs`

### Task 3: Interactions wire translation — MILESTONE M2 (part 2)

**Files:**
- Create: `oxidemx-agent/src/provider/wire.rs` (module `provider/mod.rs` declares `mod wire;`)

- [ ] **Step 1: Write failing tests** for the three translations:

```rust
#[test] fn tools_translate_to_flat_interactions_shape() { /* AutoAgents Tool{function:FunctionTool{..}} -> {"type":"function","name":..,"description":..,"parameters":..} */ }
#[test] fn newest_user_message_becomes_string_input() { /* [System, User("hi")] -> input json!("hi"), system text captured separately */ }
#[test] fn tool_results_become_function_result_array() { /* MessageType::ToolResult(vec![call]) -> [{"type":"function_result","call_id":..,"name":..,"result":[{"type":"text","text":<arguments>}]}] ; single result still an array? NO — single stays bare object to match proven shape */ }
```

- [ ] **Step 2: Implement** `pub fn tools_to_interactions(&[Tool]) -> Vec<Value>`, `pub fn newest_input(&[ChatMessage]) -> (Value, Option<String>)` returning `(input, system_instruction)`: walk from the end; `ToolResult` → function_result object (array iff >1); else last non-system message content as JSON string; collect all `System` contents joined as system_instruction.

- [ ] **Step 3: Verify** — `cargo test -p oxidemx-agent wire`. Expected: PASS.

- [ ] **Step 4: Commit** — `feat(agent): AutoAgents<->Interactions wire translation`

### Task 4: Port SSE folding — MILESTONE M2 (part 3)

**Files:**
- Create: `oxidemx-agent/src/provider/sse.rs`

- [ ] **Step 1: Port** `split_sse_events`, `FnCallFold`, `apply_sse_event`, `stream_round` and all 5 tests from `overlay-rs/src/ai_client/sse.rs`, with two changes: (a) `StreamSink` becomes `tokio::sync::mpsc::Sender<String>` text-delta sender (Option), (b) `stream_round` takes `cancel: &CancellationToken` and wraps the bytes-stream loop in `tokio::select!` against `cancel.cancelled()` → returns `LLMError::Generic("cancelled")`-equivalent error. `RoundOutcome { id: Option<String>, status: String, text: String, calls: Vec<(String,String,Value)> }` moves here.

- [ ] **Step 2: Verify** — `cargo test -p oxidemx-agent sse`. Expected: 5 ported tests pass.

- [ ] **Step 3: Commit** — `feat(agent): port Interactions SSE folding with cancellation support`

### Task 5: `GeminiInteractionsProvider` — MILESTONE M3

**Files:**
- Create: `oxidemx-agent/src/provider/mod.rs`

- [ ] **Step 1: Provider struct + blocking round**

```rust
pub struct GeminiInteractionsProvider {
    client: reqwest::Client,
    api_key: String,
    pub model: String,
    session: Mutex<Option<String>>,       // previous_interaction_id (hybrid mode a)
    store: bool,                          // false => stateless worker mode (b)
    pub cancel: CancellationToken,
}
```

`async fn round(&self, body: &Value) -> Result<RoundOutcome, LLMError>` = blocking JSON POST (port of `ai_client.rs` `blocking_round`), honoring `cancel` via `select!`.

- [ ] **Step 2: `impl ChatProvider`** — `chat_with_tools`: build request from `wire::newest_input` + `wire::tools_to_interactions` + session lock; send; update session from `outcome.id`; wrap in `InteractionsResponse`. `InteractionsResponse(RoundOutcome)` implements `ChatResponse` (`text()` = non-empty text, `tool_calls()` = calls mapped to `ToolCall{id, call_type:"function", function:FunctionCall{name, arguments: args.to_string()}}`), plus `Debug`/`Display`.
  Also override `chat_stream` to use `sse::stream_round` (text deltas only — tool-call streaming integration with `StreamChunk` is P1; ReAct's non-stream path is what the harness exercises).

- [ ] **Step 3: Stub siblings** — `CompletionProvider::complete` → `Err(LLMError::Generic("completion not supported"))`; `EmbeddingProvider::embed` → same; `ModelsProvider::list_models` → returns the two known model ids; `impl LLMProvider for GeminiInteractionsProvider {}`.

- [ ] **Step 4: Unit tests (no network)** — response adapter mapping (RoundOutcome with calls → ChatResponse::tool_calls), session id threading (mock by constructing provider and calling the private body-builder), `requires_action` status maps to tool_calls present.

- [ ] **Step 5: Verify** — `cargo test -p oxidemx-agent provider`. Expected: PASS. `cargo clippy -p oxidemx-agent -- -D warnings` clean.

- [ ] **Step 6: Commit** — `feat(agent): GeminiInteractionsProvider implementing AutoAgents LLMProvider`

### Task 6: `execute_command` tool + ReAct CLI harness — MILESTONE M4

**Files:**
- Create: `oxidemx-agent/src/tools.rs`
- Create: `oxidemx-agent/src/bin/cli.rs`

- [ ] **Step 1: Tool**

```rust
#[derive(Serialize, Deserialize, ToolInput, Debug)]
pub struct ExecuteCommandArgs {
    #[input(description = "The shell command to execute")]
    command: String,
}

#[tool(name = "execute_command", description = "Run a shell command on the user's system. Only allowlisted commands will execute.", input = ExecuteCommandArgs)]
pub struct ExecuteCommand { pub allowlist: Vec<String> }

#[async_trait]
impl ToolRuntime for ExecuteCommand {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let a: ExecuteCommandArgs = serde_json::from_value(args)?;
        if !crate::allowlist::is_allowlisted(&a.command, &self.allowlist) {
            return Ok(json!({"error": "denied: command not on the allowlist", "command": a.command}));
        }
        let (output, code) = crate::allowlist::run(&a.command).await;
        Ok(json!({"output": output, "exit_code": code}))
    }
}
```

(Denial returns a tool *result*, not an error — the model should explain, mirroring overlay behavior. No interactive approval in the CLI; the D-Bus approval chip is P1.)

- [ ] **Step 2: Harness** — `cli.rs`: clap-free arg parse (`args().nth(1)` = prompt; `--model`, `--allow <entry>` repeatable). API key from `GEMINI_API_KEY` else `~/.config/oxidemx/gemini.key` (same lookup as `ai_client.rs:124-149`). Allowlist = config `overlay.ai.command_allowlist` defaults (`brightnessctl`, `wpctl`, `systemctl --user`) + `--allow` extras. Assemble:

```rust
#[agent(name = "shell_agent", description = "...ReAct system prompt...", tools = [/* injected via constructor */])]
```

Per the McpAgent pattern, tools carrying state need a manual `AgentDeriveT` impl instead of the macro (tools = constructor-built `ExecuteCommand{allowlist}`); build with `AgentBuilder::<_, DirectAgent>::new(agent).llm(provider).memory(SlidingWindowMemory::new(10)).build().await?`, run `Task::new(prompt)`, print `ReActAgentOutput.response` + executed tool calls.

- [ ] **Step 3: Verify offline** — `cargo build -p oxidemx-agent`. Expected: builds; `cargo test -p oxidemx-agent` all green.

- [ ] **Step 4: Live smoke test** (needs key + network):
`./target/debug/oxidemx-agent-cli "Check the status of the oxidemx daemon user service and summarize it in one sentence."`
Expected: one `execute_command("systemctl --user status …")` round, allowlisted, model summarizes. Record transcript + any wire-format surprises (risk checkpoints 1–2) in the learnings section below.

- [ ] **Step 5: Commit** — `feat(agent): execute_command tool + ReAct CLI harness (P0 spike complete)`

### Task 7: Close out

- [ ] Update this plan's checkboxes; append **Learnings** section with live-test results.
- [ ] Update `docs/plans/agent-framework-integration-brainstorm.md` §12 P0 exit-criteria status.
- [ ] Commit: `docs(agent): P0 spike results + plan close-out`

---

## Learnings (filled during execution)

- (none yet — appended as each milestone lands)
