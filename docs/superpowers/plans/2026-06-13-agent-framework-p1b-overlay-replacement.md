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
- [x] `oxidemx-agent/src/provider/mod.rs`: `pub async fn seed_session(&self, id: Option<String>)` and `pub async fn session(&self) -> Option<String>` on `GeminiInteractionsProvider`. Unit test: seed then read round-trips.
- [x] `overlay-rs/Cargo.toml`: add `oxidemx-agent = { path = "../oxidemx-agent" }`, `autoagents = { version = "=0.3.7", default-features = false, features = ["google"] }`, `autoagents-derive = "=0.3.7"`.
- [x] `cargo test -p oxidemx-agent` green. Commit `feat(agent): provider session seed/read accessors`.

### Task 2 (M9): Unify the allowlist (kill the duplicate)
- [x] `overlay-rs/src/agent/commands.rs`: replace the matcher internals (`subcommands`, `strip_wrappers`, `is_allowlisted`, `run`, `cap_output`, constants) with re-exports/thin calls to `oxidemx_agent::allowlist::*`. Keep the config-coupled `allowlist()` + `add_allowlist_entry()`. Keep the test suite (now exercising the shared impl).
- [x] `cargo test -p overlay-rs commands` green. Commit `refactor(agent): overlay allowlist delegates to oxidemx-agent (single source)`.

### Task 3 (M10): The overlay agent runtime
- [x] `StreamSink`: add `#[derive(Debug)]` (tool structs need it). Make `execute_local_tool`, `agent_tool_declarations` reachable (`pub(crate)`), and `StreamSink::send` `pub(crate)`.
- [x] Create `overlay-rs/src/agent_runtime.rs`:
  - `OverlayTool { name, description, schema, sink: Option<StreamSink> }` — impl `ToolT` (name/description/args_schema from fields) + `ToolRuntime` (`execute` → `tools::execute_local_tool(&self.name, args, &self.sink).await` → `Value::String`, errors → `ToolCallError::RuntimeError`). `#[derive(Debug, Clone)]`.
  - `OverlayAgent { tools: Vec<OverlayTool>, system: String, sink: Option<StreamSink> }` — `#[derive(Debug, Clone)]`; impl `AgentDeriveT` (Output=String, `description()`→`&self.system`, `tools()`→boxed clones, name "overlay_agent"); impl `AgentHooks` with `on_turn_start` → Activity("Thinking…").
  - `pub async fn run(mode, model, prompt, session_id, sink, history) -> Result<(String, Option<String>)>`: build `OverlayTool`s from `mode.tools()` (the declarations carry name/description/parameters); build system = `mode.system_instruction(prompt)`; branch on `config.overlay.ai.backend`:
    - Interactions: concrete `GeminiInteractionsProvider::new(key, model).with_delta_sink(tx)`, `seed_session(session_id)`, spawn delta forwarder (String → `sink.send(Delta)`), run DirectAgent, read `session()` back.
    - GenerateContent: `factory::provider_from_config(GenerateContent, model, key)`; seed `SlidingWindowMemory` from `history`; run; return `(reply, None)`. (No live deltas — documented fallback.)
- [x] `overlay-rs/src/main.rs` (or lib root): `mod agent_runtime;`.
- [x] Commit `feat(overlay): AutoAgents-backed agent runtime (tools, streaming, hooks)`.

### Task 4 (M11): Swap ask_ai over; delete the old loop
- [x] `ai_client.rs`: rewrite `ask_ai` body to delegate to `agent_runtime::run(...)`. Add `history: &[(bool, String)]` param (is_user, text) — call site passes `state.chat().history` mapped. Remove `mod sse; use sse::stream_round;` and delete `overlay-rs/src/ai_client/sse.rs`. Keep `RoundOutcome`, `post_interaction`, `blocking_round`, `collect_output_text` (heartbeat/consolidate/grounded_search).
- [x] `app/update.rs`: pass history to `ask_ai`.
- [x] `cargo build -p overlay-rs` clean; `cargo clippy` clean.
- [x] Commit `feat(overlay): replace homegrown agent loop with AutoAgents runtime`.

### Task 5 (M12): Live verification + close-out
- [x] Build + install + relaunch overlay (per the install-and-restart memory). Live-test in the running overlay: a GeneralChat turn with a web search + a memory save; a SettingsCustomizer turn that reads/writes menu config; an off-allowlist command (approval chip); flash/pro toggle; stop mid-stream. Record results.
- [x] Plan checkboxes + Learnings; brainstorm §12 status (both copies); memory. Commit `docs(agent): P1b overlay replacement results`.

---

## Learnings (filled during execution)

Executed 2026-06-13, commits 8f… → close-out on branch agent-framework.
overlay + agent build clean, clippy clean, 68 overlay + 34 agent tests
green. **Headless live verification PASSED 4/4** via the new
`--agent-selftest` entrypoint (drives the real agent_runtime path
against the live Gemini API):
- plain reply + session id threaded across the turn;
- `google_search` → grounded live fact (Rust 1.96.0, matched the box);
- `execute_command` (allowlisted `systemctl --user is-active`) → real
  service status folded into the reply;
- `memory` save → persisted to memories.json (verified bytes grew +
  the phrase written), then cleaned up.

1. **The integration is two seams, exactly as the `building-llm-agents-
   in-rust` skill prescribes** (the skill is distilled from this work):
   our code touches only the provider (wire translation, session) and
   the tools (one `OverlayTool` delegating to the untouched
   `execute_local_tool`). The 80-line hand-rolled ReAct loop is gone;
   the framework owns the loop now.
2. **Streaming from the provider, not the executor, was the key
   simplification.** Attaching a delta sink to `GeminiInteractionsProvider`
   means every `chat_with_tools` round streams SSE text deltas to the
   chat thread while the executor stays in simple non-streaming mode
   (returns a final String). No executor-streaming plumbing.
3. **`description()` = the system message.** ReAct builds a System
   `ChatMessage` from the agent's `description()`; our provider folds
   System messages into the Interactions `system_instruction`. So
   setting `OverlayAgent.description()` to `mode.system_instruction(prompt)`
   carries persona (soul.md/user.md) + the memory injection block
   verbatim — no separate plumbing, and it's re-sent (with fresh
   memory) each round as a system field, which is correct.
4. **Cancel-during-approval is covered by future-drop in the overlay.**
   The skill warns a `CancellationToken`/reqwest-drop won't catch a tool
   parked on the approval dialog. In the overlay, STOP is iced
   `Task::abortable().abort()`, which drops the ENTIRE future tree —
   including a tool awaiting `ask_user_choice()`. So the parked dialog
   IS cancelled; no gap, and no regression (the old loop was inside the
   same abortable Task). The token path matters for agentd (P3), where
   cancellation isn't future-drop.
5. **Denial returns Ok, not Err** — `execute_command_tool` already
   returns `Ok("The user declined…")`, so the model recovers/explains
   instead of the turn aborting. Preserved by delegating to the
   existing dispatcher.
6. **Allowlist unified**: overlay `commands.rs` now re-exports
   `oxidemx_agent::allowlist` (no more duplicate matcher to drift).
7. **NOT installed over the live system binary.** `/usr/local/bin/
   oxidemx-overlay` is Jim's running session built from main; installing
   a feature-branch build there mid-development is disruptive + a
   merge-time action. Interactive UI walk-through (open radial → morph
   to chat → type; watch streaming render, Command/Task/Memory cards,
   the approval chip, mode pills, flash/pro toggle, STOP) is human-gated
   — install with `cargo build --release -p oxidemx-overlay` then copy
   to /usr/local/bin and relaunch when ready to merge.
8. **GenerateContent fallback through the overlay is wired + compiles +
   matches the P1a CLI path** (live-verified there), but its overlay-
   specific history-seeding run is unverified-live (the self-test used
   the default Interactions backend; switching needs a config edit to
   Jim's config.json, which I avoided). Low risk — identical provider +
   identical tool delegation.
