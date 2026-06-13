# Agent Framework P1b: Replace the overlay's homegrown agent loop

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** The overlay chat runs on the AutoAgents runtime (via `oxidemx-agent`), not the hand-rolled `ask_ai` SSE loop. No feature flag — straight replacement. Zero feature loss: streaming text, all tools (execute_command w/ interactive approval, schedule_task, memory, persona, google_search, get/set_menu_config, list_system_apps, ask_multiple_choice_question), both modes, persona + memory injection, session continuity, the flash/pro toggle, stop/abort. Honor the config-selected backend (Interactions default, GenerateContent fallback).

**Architecture / key findings (verified in AutoAgents 0.3.7 source):**
- ReAct builds a `ChatRole::System` message from `task.system_prompt` else the agent's `description()` (`turn_engine.rs:585`). Our provider's `newest_input` folds System messages into Interactions `system_instruction`. ⇒ the overlay agent's `description()` IS the mode system instruction (persona + memory injection).
- Our `GeminiInteractionsProvider.round()` streams over SSE whenever a `delta_sink` is attached — **even when the executor runs non-streaming**. ⇒ attach a delta sink, keep the executor in simple non-stream mode, still get live text deltas. No executor-streaming complexity.
- Stateful tools: McpAgent pattern — agent holds the tools, returns them from `AgentDeriveT::tools()`. One generic `OverlayTool { name, description, schema, sink }` per declaration, `execute()` delegates to the EXISTING `ai_client::tools::execute_local_tool(name, args, sink)`. Zero tool logic rewritten.
- Activity "Thinking…" between rounds: hand-impl `AgentHooks::on_turn_start` on the overlay agent (holds the sink) → `StreamEvent::Activity("Thinking…")`.
- Session: provider holds `previous_interaction_id` internally; add `seed_session`/`session` accessors. Overlay builds the concrete provider (keeps an `Arc<GeminiInteractionsProvider>`), seeds the thread's session_id, runs, reads it back. GenerateContent fallback = stateless (no session; single-turn in the overlay — documented).
- Cancellation: the iced `Task::abortable()` already drops the future on stop. Provider CancellationToken is a bonus, not required here.

**Scope boundary:** keep `load_api_key`, `run_heartbeat`, `consolidate_memories`/`auto_consolidate_if_due`, `grounded_search`, and the raw `post_interaction`/`blocking_round`/`collect_output_text` helpers — they are one-shot nested API calls, not "the agent loop." DELETE: `ai_client/sse.rs` (`stream_round`), the `ask_ai` ReAct loop body. Keep all public types (`AgentMode`, `AgentCardData`, `StreamEvent`, `StreamSink`, `PendingQuestion`, `DEFAULT_MODEL`, `PRO_MODEL`, channels) so the rest of the overlay is untouched.

---

### Task 1 (M8): Provider session accessors + Cargo wiring
- [ ] `oxidemx-agent/src/provider/mod.rs`: `pub async fn seed_session(&self, id: Option<String>)` and `pub async fn session(&self) -> Option<String>` on `GeminiInteractionsProvider`. Unit test: seed then read round-trips.
- [ ] `overlay-rs/Cargo.toml`: add `oxidemx-agent = { path = "../oxidemx-agent" }`, `autoagents = { version = "=0.3.7", default-features = false, features = ["google"] }`, `autoagents-derive = "=0.3.7"`.
- [ ] `cargo test -p oxidemx-agent` green. Commit `feat(agent): provider session seed/read accessors`.

### Task 2 (M9): Unify the allowlist (kill the duplicate)
- [ ] `overlay-rs/src/agent/commands.rs`: replace the matcher internals (`subcommands`, `strip_wrappers`, `is_allowlisted`, `run`, `cap_output`, constants) with re-exports/thin calls to `oxidemx_agent::allowlist::*`. Keep the config-coupled `allowlist()` + `add_allowlist_entry()`. Keep the test suite (now exercising the shared impl).
- [ ] `cargo test -p overlay-rs commands` green. Commit `refactor(agent): overlay allowlist delegates to oxidemx-agent (single source)`.

### Task 3 (M10): The overlay agent runtime
- [ ] `StreamSink`: add `#[derive(Debug)]` (tool structs need it). Make `execute_local_tool`, `agent_tool_declarations` reachable (`pub(crate)`), and `StreamSink::send` `pub(crate)`.
- [ ] Create `overlay-rs/src/agent_runtime.rs`:
  - `OverlayTool { name, description, schema, sink: Option<StreamSink> }` — impl `ToolT` (name/description/args_schema from fields) + `ToolRuntime` (`execute` → `tools::execute_local_tool(&self.name, args, &self.sink).await` → `Value::String`, errors → `ToolCallError::RuntimeError`). `#[derive(Debug, Clone)]`.
  - `OverlayAgent { tools: Vec<OverlayTool>, system: String, sink: Option<StreamSink> }` — `#[derive(Debug, Clone)]`; impl `AgentDeriveT` (Output=String, `description()`→`&self.system`, `tools()`→boxed clones, name "overlay_agent"); impl `AgentHooks` with `on_turn_start` → Activity("Thinking…").
  - `pub async fn run(mode, model, prompt, session_id, sink, history) -> Result<(String, Option<String>)>`: build `OverlayTool`s from `mode.tools()` (the declarations carry name/description/parameters); build system = `mode.system_instruction(prompt)`; branch on `config.overlay.ai.backend`:
    - Interactions: concrete `GeminiInteractionsProvider::new(key, model).with_delta_sink(tx)`, `seed_session(session_id)`, spawn delta forwarder (String → `sink.send(Delta)`), run DirectAgent, read `session()` back.
    - GenerateContent: `factory::provider_from_config(GenerateContent, model, key)`; seed `SlidingWindowMemory` from `history`; run; return `(reply, None)`. (No live deltas — documented fallback.)
- [ ] `overlay-rs/src/main.rs` (or lib root): `mod agent_runtime;`.
- [ ] Commit `feat(overlay): AutoAgents-backed agent runtime (tools, streaming, hooks)`.

### Task 4 (M11): Swap ask_ai over; delete the old loop
- [ ] `ai_client.rs`: rewrite `ask_ai` body to delegate to `agent_runtime::run(...)`. Add `history: &[(bool, String)]` param (is_user, text) — call site passes `state.chat().history` mapped. Remove `mod sse; use sse::stream_round;` and delete `overlay-rs/src/ai_client/sse.rs`. Keep `RoundOutcome`, `post_interaction`, `blocking_round`, `collect_output_text` (heartbeat/consolidate/grounded_search).
- [ ] `app/update.rs`: pass history to `ask_ai`.
- [ ] `cargo build -p overlay-rs` clean; `cargo clippy` clean.
- [ ] Commit `feat(overlay): replace homegrown agent loop with AutoAgents runtime`.

### Task 5 (M12): Live verification + close-out
- [ ] Build + install + relaunch overlay (per the install-and-restart memory). Live-test in the running overlay: a GeneralChat turn with a web search + a memory save; a SettingsCustomizer turn that reads/writes menu config; an off-allowlist command (approval chip); flash/pro toggle; stop mid-stream. Record results.
- [ ] Plan checkboxes + Learnings; brainstorm §12 status (both copies); memory. Commit `docs(agent): P1b overlay replacement results`.

---

## Learnings (filled during execution)

- (none yet)
