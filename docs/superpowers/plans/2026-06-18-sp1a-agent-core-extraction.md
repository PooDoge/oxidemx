# SP1a — Extract `oxidemx-agent-core` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the agent "brain" out of `overlay-rs` into a new UI-free
`oxidemx-agent-core` library crate, behind a `ToolExecutor` seam, with the app
still building/running/passing tests unchanged (in-process) the whole way.

**Architecture:** Pure-data types and config-driven modules move verbatim into
`oxidemx-agent-core`; the one coupling that needs an interface — the agent loop
calling the overlay's `execute_local_tool` — becomes a `ToolExecutor` trait the
overlay implements. Each task keeps every crate compiling by leaving a `pub use`
re-export shim in the overlay at the old path, so consumers (`app/update.rs`,
`chat_ui/*`) are untouched. No behavior changes; no D-Bus yet (that's SP1b).

**Tech Stack:** Rust, cargo workspace, AutoAgents 0.3.7 (vendored, pinned),
tokio, async-trait, serde. The new crate must NOT depend on iced/wgpu/zbus.

## Global Constraints

- New crate `oxidemx-agent-core` must have **no UI deps** (`iced`, `wgpu`, `zbus`,
  `rfd`, `notify`, image-processing). Allowed: `oxidemx-shared`, `oxidemx-agent`,
  `oxidemx-conductor`, `autoagents`/`autoagents-derive` (workspace-pinned `=0.3.7`),
  `reqwest`, `tokio`, `tokio-util`, `async-trait`, `futures-util`, `serde`,
  `serde_json`, `tracing`, `url`, `percent-encoding`, `once_cell`.
- After EVERY task: `cargo build --workspace` is green and `cargo test -p
  oxidemx-agent -p oxidemx-agent-core` is green. The overlay keeps `pub use`
  re-export shims at every moved path so external call sites never change in SP1a.
- Work in the existing worktree `../oxidemx-phase1` on branch
  `phase1-local-llm-gateway`. Commit after each task.
- Submodules `pop_os_iced`/`libcosmic` are already checked out; do not touch them.

---

### Task 1: Scaffold the `oxidemx-agent-core` crate

**Files:**
- Create: `oxidemx-agent-core/Cargo.toml`
- Create: `oxidemx-agent-core/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Produces: an empty library crate `oxidemx-agent-core` that builds.

- [ ] **Step 1: Add the crate to the workspace members**

In the root `Cargo.toml`, under `[workspace] members = [ ... ]`, add the line
`"oxidemx-agent-core",` next to `"oxidemx-agent",`.

- [ ] **Step 2: Write `oxidemx-agent-core/Cargo.toml`**

```toml
[package]
name = "oxidemx-agent-core"
version = "0.0.1"
edition = "2021"

[dependencies]
oxidemx-shared = { path = "../oxidemx-shared" }
oxidemx-agent = { path = "../oxidemx-agent" }
oxidemx-conductor = { path = "../oxidemx-conductor" }
autoagents = { version = "=0.3.7", default-features = false, features = ["google", "openai", "anthropic", "ollama"] }
autoagents-derive = "=0.3.7"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "stream"] }
tokio = { version = "1", features = ["rt", "macros", "time", "fs", "process"] }
tokio-util = "0.7"
async-trait = "0.1"
futures-util = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
url = "2"
percent-encoding = "2"
once_cell = "1"
```

- [ ] **Step 3: Write the empty `src/lib.rs`**

```rust
//! oxidemx-agent-core — the UI-free agent brain (providers, turn loop, tools,
//! memory, persona), hostable in-process today and by `agentd` (SP1b) later.
```

- [ ] **Step 4: Build it**

Run: `cargo build -p oxidemx-agent-core`
Expected: PASS (`Finished`), no warnings about missing crate.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml oxidemx-agent-core/Cargo.toml oxidemx-agent-core/src/lib.rs
git commit -m "feat(core): scaffold oxidemx-agent-core crate"
```

---

### Task 2: Move pure-data event types + define the `ToolExecutor` seam

**Files:**
- Create: `oxidemx-agent-core/src/events.rs`
- Create: `oxidemx-agent-core/src/tool.rs`
- Modify: `oxidemx-agent-core/src/lib.rs`
- Modify: `overlay-rs/src/ai_client.rs` (replace type defs with re-exports)

**Interfaces:**
- Produces:
  - `oxidemx_agent_core::events::{StreamEvent, StreamSink, AgentCardData, FlowStep, PendingQuestion}` — verbatim copies of the overlay types (all `#[derive(serde::Serialize, serde::Deserialize)]` where they already are; `StreamSink { pub thread: usize, pub tx: tokio::sync::mpsc::Sender<(usize, StreamEvent)> }`).
  - `oxidemx_agent_core::tool::ToolExecutor` — `#[async_trait] pub trait ToolExecutor: Send + Sync { async fn execute(&self, name: &str, args: serde_json::Value, sink: &Option<StreamSink>) -> Result<String, String>; }`
- Consumes: nothing new.

- [ ] **Step 1: Write `oxidemx-agent-core/src/events.rs`**

Copy the definitions of `StreamEvent` (ai_client.rs:30-50), `AgentCardData`
(57-93), `FlowStep` (97-101), `PendingQuestion` (12-16), `StreamSink` (113-132
including the `for_thread` impl and `StreamEventTx` alias), and the `send` helper
on `StreamSink`, verbatim, into this module. Use `tokio::sync::mpsc`. Make every
moved item `pub`. Do not move `STREAM_TX`/`QUESTION_TX`/`CONFIG_CHANGED_TX`
(those stay in the overlay).

- [ ] **Step 2: Write `oxidemx-agent-core/src/tool.rs` with the seam + a mock**

```rust
use async_trait::async_trait;
use serde_json::Value;

use crate::events::StreamSink;

/// The seam between the core agent loop and whoever runs the tools. The overlay
/// implements this by delegating to its `execute_local_tool` dispatcher; agentd
/// (SP1b) implements it natively. Returns the tool's raw output string (JSON or
/// plain text) — the caller wraps it for the provider.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        name: &str,
        args: Value,
        sink: &Option<StreamSink>,
    ) -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoExec;
    #[async_trait]
    impl ToolExecutor for EchoExec {
        async fn execute(&self, name: &str, args: Value, _sink: &Option<StreamSink>) -> Result<String, String> {
            Ok(format!("{name}:{args}"))
        }
    }

    #[tokio::test]
    async fn executor_trait_dispatches_by_name() {
        let e = EchoExec;
        let out = e.execute("ping", serde_json::json!({"x":1}), &None).await.unwrap();
        assert_eq!(out, "ping:{\"x\":1}");
    }
}
```

- [ ] **Step 2b: Run the seam test to verify it fails (module not wired yet)**

Run: `cargo test -p oxidemx-agent-core tool::tests::executor_trait_dispatches_by_name -v`
Expected: FAIL — `events`/`tool` modules not declared in lib.rs yet (compile error).

- [ ] **Step 3: Declare the modules in `lib.rs`**

Add to `oxidemx-agent-core/src/lib.rs`:
```rust
pub mod events;
pub mod tool;
```

- [ ] **Step 4: Run the seam test to verify it passes**

Run: `cargo test -p oxidemx-agent-core tool::tests::executor_trait_dispatches_by_name -v`
Expected: PASS.

- [ ] **Step 5: Replace the overlay type defs with re-exports**

In `overlay-rs/src/ai_client.rs`, delete the `StreamEvent`, `AgentCardData`,
`FlowStep`, `PendingQuestion`, `StreamSink`, `StreamEventTx` definitions and the
`for_thread`/`send` impls, and add near the top:
```rust
pub use oxidemx_agent_core::events::{
    AgentCardData, FlowStep, PendingQuestion, StreamEvent, StreamEventTx, StreamSink,
};
```
Keep `STREAM_TX`, `QUESTION_TX`, `CONFIG_CHANGED_TX` statics where they are (they
reference the now-imported types). Add `oxidemx-agent-core` to
`overlay-rs/Cargo.toml` `[dependencies]`:
`oxidemx-agent-core = { path = "../oxidemx-agent-core" }`.

- [ ] **Step 6: Build the workspace**

Run: `cargo build --workspace`
Expected: PASS. If errors reference a moved field/method, it's an import gap — fix
by importing from `oxidemx_agent_core::events::*` at that site.

- [ ] **Step 7: Commit**

```bash
git add oxidemx-agent-core/ overlay-rs/src/ai_client.rs overlay-rs/Cargo.toml
git commit -m "feat(core): move stream/card event types + add ToolExecutor seam"
```

---

### Task 3: Move config-driven modules — persona, memory, memory_semantic

**Files:**
- Create: `oxidemx-agent-core/src/{persona.rs, memory.rs, memory_semantic.rs}`
- Modify: `oxidemx-agent-core/src/lib.rs`
- Modify: `overlay-rs/src/agent/{persona.rs, memory.rs, memory_semantic.rs}` → shims
- Modify: `overlay-rs/src/agent/mod.rs` (if it re-exports)

**Interfaces:**
- Produces: `oxidemx_agent_core::{persona, memory, memory_semantic}` with the exact
  public fns from the coupling map (e.g. `memory::injection_block_for_async(query:
  &str) -> Option<String>`, `persona::soul_block(mode: &str) -> Option<String>`).
- Consumes: nothing overlay-specific (these are `std::fs` + `oxidemx_agent::embed`).

- [ ] **Step 1: Move the three files into core**

```bash
git mv overlay-rs/src/agent/persona.rs oxidemx-agent-core/src/persona.rs
git mv overlay-rs/src/agent/memory.rs oxidemx-agent-core/src/memory.rs
git mv overlay-rs/src/agent/memory_semantic.rs oxidemx-agent-core/src/memory_semantic.rs
```

- [ ] **Step 2: Fix intra-core references**

In the three moved files, rewrite any `crate::agent::memory_semantic::*` /
`crate::agent::memory::*` references to `crate::memory_semantic::*` /
`crate::memory::*`. They already use `oxidemx_agent::embed` (unchanged). Declare in
`oxidemx-agent-core/src/lib.rs`:
```rust
pub mod memory;
pub mod memory_semantic;
pub mod persona;
```

- [ ] **Step 3: Leave re-export shims in the overlay**

Replace each `overlay-rs/src/agent/<m>.rs` with a one-line shim so existing call
sites (`crate::agent::memory::…`) keep working:
```rust
// overlay-rs/src/agent/memory.rs
pub use oxidemx_agent_core::memory::*;
```
(same for `persona.rs`, `memory_semantic.rs`).

- [ ] **Step 4: Build + run existing memory tests**

Run: `cargo build --workspace && cargo test -p oxidemx-agent-core memory`
Expected: PASS (the moved memory unit tests now run under the core crate).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): move persona + memory + semantic-memory into core"
```

---

### Task 4: Move skills, commands, heartbeat, tasks

**Files:**
- Create: `oxidemx-agent-core/src/{skills.rs, commands.rs, heartbeat.rs, tasks.rs}`
- Modify: `oxidemx-agent-core/src/lib.rs`
- Modify: `overlay-rs/src/agent/{skills,commands,heartbeat,tasks}.rs` → shims

**Interfaces:**
- Produces: `oxidemx_agent_core::{skills, commands, heartbeat, tasks}` with the
  public fns from the coupling map (e.g. `commands::add_allowlist_entry`,
  `skills::render_command`, `tasks::create`, `heartbeat::checklist`).
- Consumes: `oxidemx_agent::allowlist` (commands re-exports `is_allowlisted`, `run`).

- [ ] **Step 1: Move the four files into core**

```bash
git mv overlay-rs/src/agent/skills.rs oxidemx-agent-core/src/skills.rs
git mv overlay-rs/src/agent/commands.rs oxidemx-agent-core/src/commands.rs
git mv overlay-rs/src/agent/heartbeat.rs oxidemx-agent-core/src/heartbeat.rs
git mv overlay-rs/src/agent/tasks.rs oxidemx-agent-core/src/tasks.rs
```

- [ ] **Step 2: Declare modules + fix references**

In `oxidemx-agent-core/src/lib.rs` add:
```rust
pub mod commands;
pub mod heartbeat;
pub mod skills;
pub mod tasks;
```
Fix any `crate::agent::*` references inside the moved files to `crate::*`.
`heartbeat.rs` calls `deliver_alert` via `notify-send` (a subprocess, fine in
core — no `notify` crate). `commands.rs` reads `oxidemx_shared::AppConfig` (fine).

- [ ] **Step 3: Leave overlay shims**

Replace each `overlay-rs/src/agent/<m>.rs` with `pub use oxidemx_agent_core::<m>::*;`.

- [ ] **Step 4: Build + test**

Run: `cargo build --workspace && cargo test -p oxidemx-agent-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): move skills + commands + heartbeat + tasks into core"
```

---

### Task 5: Move `AgentMode` + system-instruction assembly

**Files:**
- Create: `oxidemx-agent-core/src/mode.rs`
- Modify: `oxidemx-agent-core/src/lib.rs`
- Modify: `overlay-rs/src/ai_client.rs` (move `AgentMode` + assembly out; re-export)

**Interfaces:**
- Produces: `oxidemx_agent_core::mode::AgentMode` with `system_instruction_async(&self,
  query: &str) -> String`, `tools(&self) -> Vec<serde_json::Value>`, `label`,
  `tool_count`. The assembly calls `crate::{persona, memory, skills}` (now in core).
- Consumes: `persona`, `memory`, `skills` from Task 3/4.

- [ ] **Step 1: Move `AgentMode` and its `impl` block into `mode.rs`**

Cut `AgentMode` (ai_client.rs:274-281) and its full `impl AgentMode { … }`
(system_instruction_base/_async, tools, label, tool_count, plus the
`available_skills_block` helper it uses) into `oxidemx-agent-core/src/mode.rs`.
Rewrite `crate::agent::persona::…` → `crate::persona::…`,
`crate::agent::memory::…` → `crate::memory::…`,
`crate::agent::skills::…` → `crate::skills::…`. Declare `pub mod mode;` in lib.rs.

- [ ] **Step 2: Re-export from the overlay**

In `overlay-rs/src/ai_client.rs` add `pub use oxidemx_agent_core::mode::AgentMode;`
so `crate::ai_client::AgentMode` still resolves for `app/*` and `chat_ui/*`.

- [ ] **Step 3: Build the workspace**

Run: `cargo build --workspace`
Expected: PASS. Fix any remaining `crate::agent::*` references at call sites inside
the moved assembly by pointing them at `crate::*` (core) paths.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(core): move AgentMode + system-instruction assembly into core"
```

---

### Task 6: Move the agent loop (`agent_runtime`) into core behind `ToolExecutor`

**Files:**
- Create: `oxidemx-agent-core/src/runtime.rs`
- Modify: `oxidemx-agent-core/src/lib.rs`
- Modify: `overlay-rs/src/agent_runtime.rs` → re-export shim

**Interfaces:**
- Produces: `oxidemx_agent_core::runtime` with `run`, `route_turn`, `simple_chat`,
  `optimize_prompt`, `summarize`, `new_session_id`, `SESSIONS`. Each turn-running
  fn gains a trailing parameter `executor: &std::sync::Arc<dyn crate::tool::ToolExecutor>`
  (replacing the hard call to `execute_local_tool`).
- Consumes: `crate::tool::ToolExecutor`, `crate::events::*`, `crate::mode::AgentMode`,
  `oxidemx_agent::{session, factory, keys}`.

- [ ] **Step 1: Move `agent_runtime.rs` to `oxidemx-agent-core/src/runtime.rs`**

```bash
git mv overlay-rs/src/agent_runtime.rs oxidemx-agent-core/src/runtime.rs
```
Declare `pub mod runtime;` in `oxidemx-agent-core/src/lib.rs`. Rewrite imports:
`use crate::ai_client::{AgentMode, StreamEvent, StreamSink};` →
`use crate::events::{StreamEvent, StreamSink}; use crate::mode::AgentMode;`.

- [ ] **Step 2: Replace the `execute_local_tool` call with the seam**

`OverlayTool` (now in runtime.rs) holds the executor. Change its struct + execute:
```rust
#[derive(Clone)]
struct CoreTool {
    name: String,
    description: String,
    schema: Value,
    sink: Option<StreamSink>,
    exec: std::sync::Arc<dyn crate::tool::ToolExecutor>,
}

#[async_trait]
impl ToolRuntime for CoreTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        match self.exec.execute(&self.name, args, &self.sink).await {
            Ok(text) => Ok(serde_json::from_str::<Value>(&text)
                .unwrap_or_else(|_| serde_json::json!({ "output": text }))),
            Err(e) => Err(ToolCallError::RuntimeError(e)),
        }
    }
}
```
(Keep the `ToolT` impl identical, renamed to `CoreTool`. Manually `impl Debug` or
drop `#[derive(Debug)]` since `Arc<dyn ToolExecutor>` isn't `Debug`.)
Thread `exec: Arc<dyn ToolExecutor>` through `build_tools`, `run`, and `route_turn`
(add it as the last parameter), and into `simple_chat`/`optimize_prompt` only if
they build tools (they don't — they're tool-free; leave their signatures, they
just won't take `exec`).

- [ ] **Step 3: Leave an overlay re-export shim**

Replace `overlay-rs/src/agent_runtime.rs` content with:
```rust
//! Re-export shim — the agent loop now lives in oxidemx-agent-core.
pub use oxidemx_agent_core::runtime::*;
```

- [ ] **Step 4: Build core only (overlay will break at call sites — expected)**

Run: `cargo build -p oxidemx-agent-core`
Expected: PASS. (The overlay won't build until Task 7 passes the executor; that's
fine — Task 6 and Task 7 are one logical change split for review. If you need the
workspace green at every commit, defer the Task 6 commit and do 6+7 together.)

- [ ] **Step 5: Commit (core-only green)**

```bash
git add -A
git commit -m "feat(core): move agent loop into core behind ToolExecutor seam"
```

---

### Task 7: Overlay implements `ToolExecutor`; wire call sites; workspace green

**Files:**
- Create: `overlay-rs/src/agent/tool_exec.rs`
- Modify: `overlay-rs/src/ai_client.rs` (`ask_ai` passes the executor)
- Modify: `overlay-rs/src/app/update.rs` (call sites already use `ask_ai` — unchanged)

**Interfaces:**
- Consumes: `oxidemx_agent_core::tool::ToolExecutor`, `runtime::{run, route_turn}`.
- Produces: `OverlayToolExecutor` (implements `ToolExecutor` by delegating to
  `crate::ai_client::tools::execute_local_tool`), and a process-global
  `Arc<dyn ToolExecutor>` the overlay passes into core.

- [ ] **Step 1: Write `overlay-rs/src/agent/tool_exec.rs`**

```rust
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;
use oxidemx_agent_core::events::StreamSink;
use oxidemx_agent_core::tool::ToolExecutor;

/// Overlay-side tool execution: delegates to the existing dispatcher (which owns
/// approval chips, activity, cards, and the UI-coupled tools).
pub struct OverlayToolExecutor;

#[async_trait]
impl ToolExecutor for OverlayToolExecutor {
    async fn execute(&self, name: &str, args: Value, sink: &Option<StreamSink>) -> Result<String, String> {
        crate::ai_client::tools::execute_local_tool(name, args, sink).await
    }
}

/// Shared executor handle the overlay hands to core for every turn.
pub fn executor() -> Arc<dyn ToolExecutor> {
    use once_cell::sync::Lazy;
    static EXEC: Lazy<Arc<dyn ToolExecutor>> = Lazy::new(|| Arc::new(OverlayToolExecutor));
    EXEC.clone()
}
```
Declare `pub mod tool_exec;` in `overlay-rs/src/agent/mod.rs`.

- [ ] **Step 2: Pass the executor from `ask_ai`**

In `overlay-rs/src/ai_client.rs`, update `ask_ai` to forward the executor:
```rust
pub async fn ask_ai(
    mode: AgentMode, model: &str, prompt: &str, sink: Option<StreamSink>,
    history: &[(bool, String)], image: Option<(String, Vec<u8>)>, session_id: &str,
) -> Result<(String, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    oxidemx_agent_core::runtime::route_turn(
        mode, model, prompt, sink, history, image, session_id,
        &crate::agent::tool_exec::executor(),
    ).await
}
```
(Confirm `route_turn`'s new signature ends with `executor: &Arc<dyn ToolExecutor>`
per Task 6 Step 2.)

- [ ] **Step 3: Fix the headless selftest + in-file test call sites**

`overlay-rs/src/main.rs` `--agent-selftest` calls `ask_ai(...)` — unchanged
(executor added inside `ask_ai`). The moved `image_vision_live` test (now in
`oxidemx-agent-core/src/runtime.rs`) calls `super::run(...)`; give it a tiny mock
executor:
```rust
struct NoTools;
#[async_trait::async_trait]
impl crate::tool::ToolExecutor for NoTools {
    async fn execute(&self, _n: &str, _a: serde_json::Value, _s: &Option<crate::events::StreamSink>) -> Result<String, String> {
        Ok("{}".into())
    }
}
// ... run(..., "test-vision", &std::sync::Arc::new(NoTools)).await
```

- [ ] **Step 4: Build the whole workspace**

Run: `cargo build --workspace`
Expected: PASS. Fix any straggling `crate::agent_runtime::X` references — they
resolve via the Task-6 shim, but `SESSIONS`/`new_session_id` used in
`app/update.rs` must still work (they're re-exported by the shim).

- [ ] **Step 5: Run the full test suite**

Run: `cargo test -p oxidemx-agent -p oxidemx-agent-core -p oxidemx-overlay`
Expected: PASS (session, factory, heuristic, memory tests all green under their
new homes).

- [ ] **Step 6: Live smoke — the app still works in-process**

Run (mistral.rs at :1234, throwaway config):
```bash
TMP=$(mktemp -d); mkdir -p "$TMP/oxidemx"
printf '%s' '{ "overlay": { "ai": { "provider": "mistral_rs", "model": "default", "local_endpoint": "http://localhost:1234/v1/" } } }' > "$TMP/oxidemx/config.json"
XDG_CONFIG_HOME="$TMP" cargo run -q -p oxidemx-overlay --bin oxidemx-overlay -- --agent-selftest "what is a hash map?"
rm -rf "$TMP"
```
Expected: a correct answer printed (turn runs through core + the OverlayToolExecutor).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(overlay): implement ToolExecutor; route turns through core"
```

---

### Task 8: Guardrails + cleanup

**Files:**
- Modify: `overlay-rs/src/agent/mod.rs` (drop now-empty re-export clutter if any)
- Test: a grep-based no-UI-deps assertion (manual)

**Interfaces:** none new.

- [ ] **Step 1: Assert core has no UI deps**

Run: `cargo tree -p oxidemx-agent-core -e normal | grep -E '\biced\b|\bwgpu\b|\bzbus\b' || echo "clean: no UI deps in core"`
Expected: prints `clean: no UI deps in core`.

- [ ] **Step 2: Verify the overlay shims are minimal**

Run: `wc -l overlay-rs/src/agent_runtime.rs overlay-rs/src/agent/{persona,memory,memory_semantic,skills,commands,heartbeat,tasks}.rs`
Expected: each ≤ 3 lines (pure `pub use` shims). These shims are intentional for
SP1a; SP1c deletes them when call sites move to D-Bus.

- [ ] **Step 3: Full workspace build + test once more**

Run: `cargo build --workspace && cargo test -p oxidemx-agent -p oxidemx-agent-core`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore(core): verify no-UI-deps + minimal overlay shims (SP1a done)"
```

---

## Self-Review notes

- **Spec coverage:** SP1a == spec §9 step 1 ("carve core with the seams; temporary
  in-overlay adapter so the app still runs"). The `EventSink`/`Approver`/
  `HostCapability` triad from §4.1: SP1a lands the event types + the `ToolExecutor`
  seam (the tool half of `Approver`); the full `Approver`/`HostCapability` D-Bus
  traits are SP1b (agentd) — noted, not silently dropped.
- **No placeholders:** every new file's code is shown; moves are `git mv` + explicit
  import rewrites verified by `cargo build`.
- **Type consistency:** `ToolExecutor::execute(&self, name, args, sink)` is defined
  once (Task 2) and consumed identically in Task 6 (`CoreTool.exec`) and Task 7
  (`OverlayToolExecutor`, `executor()`). `route_turn`'s new trailing
  `executor: &Arc<dyn ToolExecutor>` param is introduced in Task 6 and supplied in
  Task 7.
- **Known soft spot:** Tasks 6+7 are one logical change split for review; Task 6
  leaves the workspace red (core-only green). If per-commit workspace-green is
  required, do 6 and 7 in one commit.
