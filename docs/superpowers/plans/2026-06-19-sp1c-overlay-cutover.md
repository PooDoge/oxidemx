# SP1c — agentd turn-path completion + overlay cutover — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make agentd run a REAL chat turn (native tools + transcript history + streamed events) and cut the overlay over to be a thin `org.oxidemx.Agent` D-Bus client, deleting its in-process agent path.

**Architecture:** Backend first (T1–T5, headless + TDD): bridge core's `StreamSink` and the conductor's `EventSink` into agentd's `EventEmitter`→`event` signal; migrate `execute_local_tool`'s native tools into an agentd `AgentToolExecutor` (host-bound few via `HostCapability`); wire run_flow/run_status/cancel; compose it all into `CoreTurnRunner`. Then overlay (T6–T8, build-gate + manual): implement `HostCapability`, switch the chat send/receive path to `AgentProxy` + the `event` signal, delete the in-proc path on the final flip.

**Tech Stack:** Rust, zbus 5, tokio, AutoAgents; crates `agentd`, `oxidemx-agent-core`, `oxidemx-conductor`, `oxidemx-agent-local`, `oxidemx-agent-proxy`, `overlay-rs`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-19-sp1c-overlay-cutover-design.md`. Worktree `../oxidemx-phase1`, branch `phase1-local-llm-gateway`. Commit per task.
- **The overlay in-proc path (`ai_client::ask_ai`→`route_turn`, `OverlayToolExecutor`) stays fully working until Task 8 (the flip).** No behavior change to live chat before T8.
- Tools run **agentd-native** in the project cwd; only host-bound capabilities go through `HostCapability`. Native set: `read_file`, `list_dir`, `search_file`, `parse_document`, `execute_command`, `list_system_apps`, `google_search`, `compose_flow`, `run_flow`, `use_skill`, `memory`, `persona`, `schedule_task`. Host-delegated: `ask_multiple_choice_question`, menu-config live-apply, and the capability hooks `screenshot`/`vision`/`clipboard`/`current-window`.
- **Token deltas best-effort** (keep `BusEmitter` `try_send` drop-on-full); **final assistant message authoritative + persisted** to the transcript.
- agentd default build excludes `mistralrs` (behind `mistral` feature); `cargo tree -p agentd | grep -i mistralrs` empty on default. `oxidemx-agent-proxy` stays light (zbus+serde).
- `#![forbid(unsafe_code)]`; no `unwrap`/`expect` outside tests; poison-recovery locks (`.lock().unwrap_or_else(|e| e.into_inner())`); never hold a lock/guard across `.await`; thiserror; `clippy -D warnings`; pristine build. Backend tasks: `cargo test -p agentd` green each task. Overlay tasks: `cargo build -p overlay-rs` + a manual walkthrough.
- Migrate tools **behavior-preserving** — the existing `overlay-rs/src/ai_client/tools.rs` body is the oracle for each tool.

## Real signatures (verified — use verbatim)

- `oxidemx_agent_core::runtime::route_turn(mode: AgentMode, model_hint: &str, prompt: &str, sink: Option<StreamSink>, history: &[(bool,String)], image: Option<(String,Vec<u8>)>, session_id: &str, exec: &Arc<dyn ToolExecutor>) -> Result<(String, Option<String>), BoxError>`.
- `oxidemx_agent_core::events::StreamEvent`: `Delta(String)`, `Activity(String)`, `Card(AgentCardData)`, `Usage{prompt:u32,completion:u32}`, `Command{..}`, `Task{..}`, `Memory{..}`, `Flow{..}`. `StreamSink` at `events.rs:94` (inspect its constructor — it wraps a sender of `StreamEvent`).
- `oxidemx_conductor::{run_flow, RunHandle, RunOptions, RunOutcome}`; `run_flow(plan: &FlowPlan, opts: RunOptions, sink: Arc<dyn EventSink>) -> RunOutcome`. `RunOptions{run_id, inputs: BTreeMap<String,String>, workdir: PathBuf, roster: Roster, factory: Arc<dyn ProviderFactory>, cancel: CancellationToken, approval: ApprovalPolicy, allowlist: Vec<String>}`. `RunHandle{run_id, cancel}`. `ConfigFactory` implements `ProviderFactory`. `RunEvent`: `RunStarted{flow_id,run_id,steps}`, `TaskAssigned{step,agent}`, `TaskStarted{step}`, `AgentMessage{step,message}`, `TaskFinished{step,..}`, `TaskError{step,error}`, `StepRetrying{step,attempt}`, `StepSkipped{step,reason}`, `ApprovalRequested{step,card}`, `RunFinished{run_id,..}`, `RunFailed{run_id,..}`.
- Overlay send path today: `overlay-rs/src/main.rs:102` and `overlay-rs/src/app/update.rs:795` call `ai_client::ask_ai(mode,&model,&prompt,sink,&history,image,&session_id)` → `ai_client.rs:227 ask_ai` → `route_turn`. Tool dispatch: `ai_client/tools.rs:77 execute_local_tool(name,args,sink)`.

## File structure

```
agentd/src/stream_bridge.rs   # T1: StreamSink that forwards StreamEvents -> EventEmitter; usage capture
agentd/src/tools/mod.rs       # T2/T3: AgentToolExecutor (impl core::tool::ToolExecutor)
agentd/src/tools/fs.rs        # T2: read_file/list_dir/search_file/parse_document/execute_command/list_system_apps/google_search
agentd/src/tools/agent.rs     # T3: compose_flow/run_flow/use_skill/memory/persona/schedule_task + host-delegated dispatch
agentd/src/run_bridge.rs      # T4: conductor EventSink -> EventEmitter; run handle/cancel map
agentd/src/interface.rs       # T4/T5: wire run_flow/run_status/cancel; CoreTurnRunner uses real exec+stream bridge
overlay-rs/src/agent/host.rs  # T6: HostCapability impl + registration
overlay-rs/src/ai_client.rs   # T7: ask_ai -> AgentProxy.send_message; T8 delete in-proc
overlay-rs/src/app/agent_events.rs # T7: event-signal subscriber -> UI messages
```

---

### Task 1: `StreamSink` → `EventEmitter` bridge (+ usage capture)

**Files:** Create `agentd/src/stream_bridge.rs`; Modify `agentd/src/lib.rs`.

**Interfaces:**
- Consumes: `oxidemx_agent_core::events::{StreamSink, StreamEvent}`; `seams::{EventEmitter, AgentEvent, RecordingEmitter}`.
- Produces: `pub struct StreamBridge` with `pub fn new(project: String, thread: String, emitter: Arc<dyn EventEmitter>) -> (StreamBridge, StreamSink)` — returns a bridge handle + a `StreamSink` to hand to `route_turn`; a spawned task drains the sink's channel and emits one `AgentEvent` per `StreamEvent` (`payload.kind`: `Delta`→`"delta"` with `{text}`, `Activity`→`"activity"`, `Card`→`"card"`, `Flow`→`"flow"`, `Command`/`Task`/`Memory`→`"tool"`). `Usage{prompt,completion}` is NOT emitted as an event but accumulated; `pub async fn finish(self) -> (u64,u64)` joins the drain task and returns `(prompt,completion)` totals for the journal.

- [ ] **Step 1: Inspect `StreamSink`'s constructor** in `oxidemx-agent-core/src/events.rs:94-130` — find how to build a `StreamSink` from a sender (e.g. `StreamSink::new(tx)` or a channel). The bridge creates that channel, keeps the receiver, hands the `StreamSink` out.

- [ ] **Step 2: Write the failing test**
```rust
#[tokio::test]
async fn bridge_forwards_deltas_and_captures_usage() {
    use oxidemx_agent_core::events::StreamEvent;
    let em = std::sync::Arc::new(crate::seams::RecordingEmitter::default());
    let (bridge, sink) = StreamBridge::new("proj".into(), "t1".into(), em.clone());
    sink.send(StreamEvent::Delta("Hel".into())).await;
    sink.send(StreamEvent::Delta("lo".into())).await;
    sink.send(StreamEvent::Usage { prompt: 10, completion: 3 }).await;
    drop(sink);
    let (p, c) = bridge.finish().await;
    assert_eq!((p, c), (10, 3));
    let evs = em.events();
    let deltas: Vec<_> = evs.iter().filter(|e| e.payload["kind"] == "delta").collect();
    assert_eq!(deltas.len(), 2);
    assert_eq!(deltas[0].payload["text"], "Hel"); // ordered
    assert!(evs.iter().all(|e| e.payload["kind"] != "usage")); // usage not an event
}
```

- [ ] **Step 3: Run → FAIL** (`cargo test -p agentd bridge_forwards`).
- [ ] **Step 4: Implement** `stream_bridge.rs` per the Interfaces block; declare `pub mod stream_bridge;`. Drain loop maps + emits; accumulates Usage; `finish()` awaits the join handle. No lock across await.
- [ ] **Step 5: Run → PASS.**
- [ ] **Step 6: Commit** — `feat(agentd): StreamSink->EventEmitter bridge with usage capture`

---

### Task 2: `AgentToolExecutor` + native fs/shell/web tools

**Files:** Create `agentd/src/tools/mod.rs`, `agentd/src/tools/fs.rs`; Modify `agentd/src/lib.rs`. Oracle: `overlay-rs/src/ai_client/tools.rs`.

**Interfaces:**
- Produces: `pub struct AgentToolExecutor { paths: ProjectPaths, host: Arc<dyn HostCapability> }` implementing `oxidemx_agent_core::tool::ToolExecutor` (`async fn execute(&self, name: &str, args: Value, sink: &Option<StreamSink>) -> Result<String, String>`). This task implements the cwd-scoped tools: `read_file`, `list_dir`, `search_file`, `parse_document`, `execute_command`, `list_system_apps`, `google_search`. Unknown name → `Err("Unknown tool: …")` (T3 fills the rest).
- Path-scoping: file/dir/search/parse/command resolve relative paths against `paths.cwd`; reject escapes above cwd (canonicalize + prefix-check) — coding-first safety. `google_search`/`list_system_apps` unchanged from the oracle.

- [ ] **Step 1: Failing tests** (one per tool, temp-cwd fixtures). Example:
```rust
#[tokio::test]
async fn read_file_reads_within_cwd() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.txt"), "hello").unwrap();
    let exec = test_executor(d.path());      // AgentToolExecutor over a ProjectPaths at d
    let out = exec.execute("read_file", serde_json::json!({"path":"a.txt"}), &None).await.unwrap();
    assert!(out.contains("hello"));
}
#[tokio::test]
async fn read_file_rejects_escape() {
    let d = tempfile::tempdir().unwrap();
    let exec = test_executor(d.path());
    assert!(exec.execute("read_file", serde_json::json!({"path":"../../etc/passwd"}), &None).await.is_err());
}
#[tokio::test]
async fn execute_command_runs_in_cwd() {
    let d = tempfile::tempdir().unwrap();
    let exec = test_executor(d.path());
    let out = exec.execute("execute_command", serde_json::json!({"command":"pwd"}), &None).await.unwrap();
    assert!(out.contains(d.path().file_name().unwrap().to_str().unwrap()));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `tools/mod.rs` (the executor + dispatch) and `tools/fs.rs` (the 7 tool bodies, ported from the oracle, cwd-scoped + escape-guarded). Reuse `oxidemx-agent-core` helpers if the oracle delegated to them. Declare `pub mod tools;`.
- [ ] **Step 4: Run → PASS** (`cargo test -p agentd tools::`).
- [ ] **Step 5: Commit** — `feat(agentd): AgentToolExecutor + native fs/shell/web tools (cwd-scoped)`

---

### Task 3: remaining native tools + host-delegated dispatch

**Files:** Create `agentd/src/tools/agent.rs`; Modify `agentd/src/tools/mod.rs`. Oracle: `overlay-rs/src/ai_client/tools.rs`.

**Interfaces:**
- Extends `AgentToolExecutor::execute` to handle `compose_flow`, `run_flow`, `use_skill`, `memory`, `persona`, `schedule_task` (native, ported from the oracle; `use_skill`/`memory`/`persona` resolve under the project store + `merged_skill_roots()`), and the host-delegated `ask_multiple_choice_question` + menu-config live-apply via `self.host.invoke(cap, args)` (returns the host's reply or a clean "host unavailable" error).
- `run_flow`/`compose_flow` tools call the conductor loader/`run_flow` (the supervisor) directly OR delegate to a shared helper introduced in Task 4 — keep the tool body thin.

- [ ] **Step 1: Failing tests** — e.g. `memory` save→list round-trips under a temp store; `ask_multiple_choice_question` calls `host.invoke("ask_multiple_choice_question", …)` (assert via a mock `HostCapability` that records the call and returns a canned choice); `use_skill` reads a skill placed under the project `.oxidemx/skills`.
```rust
#[tokio::test]
async fn ask_multiple_choice_delegates_to_host() {
    let host = Arc::new(RecordingHost::with_reply(serde_json::json!({"choice":"B"})));
    let exec = test_executor_with_host(tempfile::tempdir().unwrap().path(), host.clone());
    let out = exec.execute("ask_multiple_choice_question",
        serde_json::json!({"question":"x","options":["A","B"]}), &None).await.unwrap();
    assert!(host.calls().iter().any(|c| c == "ask_multiple_choice_question"));
    assert!(out.contains("B"));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `tools/agent.rs` + extend the dispatch. Add a `#[cfg(test)] RecordingHost` implementing `HostCapability`.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** — `feat(agentd): remaining native tools + host-delegated dispatch (HostCapability)`

---

### Task 4: conductor `EventSink` bridge + run_flow/run_status/cancel wiring

**Files:** Create `agentd/src/run_bridge.rs`; Modify `agentd/src/interface.rs`, `agentd/src/lib.rs`.

**Interfaces:**
- Produces: `pub struct RunEventBridge` implementing `oxidemx_conductor::EventSink` (`async fn emit(&self, e: RunEvent)`), forwarding each `RunEvent` to the `EventEmitter` as an `AgentEvent` (`payload.kind="run"`, with `run_id`/`step`/variant). Also updates a shared run-status table (`Arc<Mutex<HashMap<run_id,String>>>`).
- `AgentService` gains `active_runs: Mutex<HashMap<String, RunHandle>>` and `run_statuses: Mutex<HashMap<String,String>>`. `run_flow` builds `RunOptions{run_id, inputs, workdir: paths.runs_dir(), roster: load_roster, factory: Arc::new(ConfigFactory…), cancel: token.clone(), approval, allowlist}`, registers the `RunHandle`, spawns `supervisor::run_flow(&plan, opts, Arc::new(RunEventBridge…))`, returns `run_id`. `run_status` reads the table (no longer "not yet wired"). `cancel_run(run_id)` cancels the token; `cancel_turn` cancels the turn's token (see T5).

- [ ] **Step 1: Failing test** (mock factory + a trivial 1-step flow, or a `FixedFactory`):
```rust
#[tokio::test]
async fn run_flow_streams_run_events_and_status() {
    let env = TestEnv::new();                  // AgentService::for_test, RecordingEmitter
    let run_id = env.svc.run_flow(env.cwd_str(), "<test-flow-id>", "{}").await.unwrap();
    // wait briefly for the spawned run
    for _ in 0..50 { if env.svc.run_status(&run_id).await.is_ok() { break } tokio::time::sleep(ms(20)).await; }
    assert!(env.emitter.events().iter().any(|e| e.payload["kind"] == "run"));
    assert!(env.svc.run_status(&run_id).await.is_ok());     // table populated
}
#[tokio::test]
async fn cancel_run_cancels_token() {
    let env = TestEnv::new();
    let run_id = env.svc.run_flow(env.cwd_str(), "<test-flow-id>", "{}").await.unwrap();
    assert!(env.svc.cancel_run(&run_id).await.is_ok());
}
```
(Use a minimal test flow fixture under the test project's flows dir, or a `FixedFactory` that completes one step. If wiring a real flow is heavy, assert the `RunEventBridge` mapping directly: feed it `RunEvent`s, assert emitted AgentEvents + status-table updates — that's the unit under test; the supervisor integration can be the live path.)
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `run_bridge.rs` + the `AgentService` run wiring. Cancellation tokens via `tokio_util::sync::CancellationToken` (already a conductor dep). No lock across await (clone handles out of the guard).
- [ ] **Step 4: Run → PASS** (`cargo test -p agentd run_`).
- [ ] **Step 5: Commit** — `feat(agentd): conductor EventSink bridge + run_flow/run_status/cancel wiring`

---

### Task 5: compose real turn in `CoreTurnRunner` + headless end-to-end test

**Files:** Modify `agentd/src/interface.rs`; Modify `agentd/tests/live_bus.rs`.

**Interfaces:**
- `CoreTurnRunner` now builds a `StreamBridge` (T1) per turn, passes its `StreamSink` into `route_turn`, uses the real `AgentToolExecutor` (T2/T3) as `exec`, and writes the bridge's `finish()` usage into `JournalEntry::Turn`. `AgentService::new` constructs the `AgentToolExecutor` over the per-turn `ProjectPaths` + the host. (Real provider still built from config in `route_turn` — exercised only by the live path, not unit tests.)
- The headless proof (mock path): extend `MockTurnRunner` so it (a) calls `exec.execute(...)` for a configured tool and (b) pushes `StreamEvent::Delta`s through a `StreamBridge` sink, so an `AgentService`-level test asserts a multi-turn, tool-using, streamed conversation: turn 2 receives turn-1 history; deltas arrive ordered; the native tool ran; the final message is persisted to the transcript.

- [ ] **Step 1: Failing test** (AgentService-level, mock provider via MockTurnRunner exercising exec+stream):
```rust
#[tokio::test]
async fn multi_turn_tool_using_streamed_conversation() {
    let env = TestEnv::with_tool_mock();   // MockTurnRunner that streams 2 deltas + calls read_file once
    std::fs::write(env.cwd().join("note.txt"), "data").unwrap();
    env.svc.send_message(env.cwd_str(), "t1", "turn one: read note.txt", "").await.unwrap();
    env.svc.send_message(env.cwd_str(), "t1", "turn two", "").await.unwrap();
    let evs = env.emitter.events();
    assert!(evs.iter().filter(|e| e.payload["kind"]=="delta").count() >= 2);   // streamed
    assert!(evs.iter().any(|e| e.payload["kind"]=="tool"));                     // tool ran
    let turns = env.svc.get_transcript(env.cwd_str(), "t1").await.unwrap();
    assert!(turns.len() >= 4);                                                  // 2 user + 2 assistant persisted
    // turn 2 saw turn-1 history: the MockTurnRunner records the history arg len it received
    assert!(env.last_history_len() >= 2);
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the `CoreTurnRunner` composition + extend `MockTurnRunner`/`TestEnv`. Update the `#[ignore]` `live_bus.rs` test to drive a tool-using streamed turn through `AgentProxy` (documented run: `cargo test -p agentd --test live_bus -- --ignored`).
- [ ] **Step 4: Run → PASS** (`cargo test -p agentd`); confirm `cargo build -p agentd --features mistral` + `cargo tree -p agentd | grep -i mistralrs` empty.
- [ ] **Step 5: Commit** — `feat(agentd): real streamed tool-using turn in CoreTurnRunner + headless e2e test`

---

### Task 6: overlay `HostCapability` impl + host registration

**Files:** Create `overlay-rs/src/agent/host.rs`; Modify `overlay-rs/src/agent/mod.rs`, the overlay startup (where it connects to D-Bus). Reference: existing compositor/screenshot/clipboard code in overlay + `daemon` signals for menu apply.

**Interfaces:**
- The overlay registers as agentd's host on startup. **Mechanism (chosen):** the overlay exposes a tiny `org.oxidemx.AgentHost` zbus interface (methods `screenshot`, `vision`, `clipboard`, `current_window`, `ask_multiple_choice_question`, `apply_menu_config` — each takes a json arg, returns a json String) at `/org/oxidemx/AgentHost`; agentd's `HostCapability` production impl (added here, in agentd) is a proxy to `org.oxidemx.AgentHost` that returns "unavailable" if the name isn't owned. (This keeps agentd calling OUT to the host, matching the seam.)
- Each host method is backed by the overlay's existing compositor/clipboard/screenshot code and, for `apply_menu_config`, writes the file + emits the daemon-watched signal.

- [ ] **Step 1:** Implement the `org.oxidemx.AgentHost` interface in `overlay-rs/src/agent/host.rs` wired to existing capability code; serve it on the overlay's session-bus connection at startup.
- [ ] **Step 2:** In `agentd`, add the production `HostCapability` impl (a proxy to `org.oxidemx.AgentHost`) used by `AgentService::new` (replacing `UnavailableHost`); on no-owner → clean "unavailable" error. Add an `#[ignore]` integration note (real bus).
- [ ] **Step 3: Gate** — `cargo build -p overlay-rs` + `cargo build -p agentd` clean; `cargo clippy` clean. Manual: launch overlay + agentd, confirm agentd's `ask_multiple_choice_question` tool surfaces a prompt in the overlay.
- [ ] **Step 4: Commit** — `feat(overlay,agentd): HostCapability via org.oxidemx.AgentHost`

---

### Task 7: overlay send/receive over `AgentProxy` (in-proc path still present)

**Files:** Modify `overlay-rs/src/ai_client.rs`, create `overlay-rs/src/app/agent_events.rs`; Modify `overlay-rs/src/app/update.rs`, `overlay-rs/src/main.rs`. Use `oxidemx-agent-proxy::AgentProxy`.

**Interfaces:**
- Add a NEW send path `ask_ai_remote(...)` that calls `AgentProxy::send_message(project=cwd, thread=session_id, text=prompt, model_hint=model)` and an `event`-signal subscriber (`agent_events.rs`) that demuxes `payload.kind` (`delta`→stream into the active bubble; `tool`/`flow`/`run`→activity UI; `final`→commit; `approval_requested`→approval card; `model_status`→status) into the existing iced `Message`s. History on open/reconnect via `list_threads`+`get_transcript`.
- Behind a runtime switch (`AiConfig` flag `use_agentd`, default FALSE this task) so the in-proc path remains default until Task 8. This lets T7 be tested live without flipping default behavior.

- [ ] **Step 1:** Implement `ask_ai_remote` + the `event` subscriber → iced messages; wire history load. Gate the chat send on the `use_agentd` flag.
- [ ] **Step 2: Gate** — `cargo build -p overlay-rs` clean; clippy clean. Manual (flag ON via config): send a message → streamed reply from agentd; run a tool; run a flow (`run`-events render); trigger + answer an approval; reconnect → history restored. Flag OFF → unchanged in-proc behavior.
- [ ] **Step 3: Commit** — `feat(overlay): agentd client send/receive path behind use_agentd flag`

---

### Task 8: flip default + delete the in-proc path

**Files:** Modify `overlay-rs/src/ai_client.rs` (delete `ask_ai`/`route_turn` call), delete `overlay-rs/src/agent/tool_exec.rs` (`OverlayToolExecutor`) and the migrated tool bodies in `overlay-rs/src/ai_client/tools.rs`; Modify `oxidemx-shared` to default `use_agentd=true` (or remove the flag and make agentd the only path); prune now-dead `overlay-rs/src/agent/*` shims.

- [ ] **Step 1:** Make the agentd client the only chat path (remove the `use_agentd` flag / default it true and delete the in-proc branch). Delete `OverlayToolExecutor` + the tool bodies now living in agentd (`read_file`…`schedule_task`); keep any `agent/*` shim still referenced by settings/other code, delete the rest. Remove now-unused imports/deps.
- [ ] **Step 2: Gate** — `cargo build -p overlay-rs` + workspace build clean; clippy clean; `cargo test -p agentd` still green. **Manual walkthrough (the point of no return — do before commit):** full chat (stream, tool, flow, approval, reconnect-history) works end-to-end through agentd with the in-proc path gone.
- [ ] **Step 3: Commit** — `feat(overlay): cut over to agentd; delete in-proc agent path`

---

## Self-Review

- **Spec coverage:** §4 event unification → T1 (StreamSink) + T4 (conductor EventSink); §3 tool split → T2 (native fs/shell/web) + T3 (rest + host-delegated); §5 turn completion → T4 (run_flow/status/cancel) + T5 (compose real turn, history, usage, streaming); §6 overlay thin client → T6 (HostCapability) + T7 (send/receive) + T8 (delete in-proc); §8 testing → T1–T5 TDD + T5 headless e2e + live_bus, T6–T8 build-gate+manual. All covered.
- **Placeholder scan:** the `<test-flow-id>` in T4 is the one symbolic value — the implementer supplies a minimal test flow fixture or asserts the `RunEventBridge` mapping directly (both spelled out). No "TODO"/"add error handling"/code-free steps. GUI tasks (T6–T8) are honestly build-gate+manual (iced UI, no automated GUI test — consistent with the project).
- **Type consistency:** `StreamBridge`/`AgentToolExecutor`/`RunEventBridge`/`HostCapability`/`AgentService` flow consistently; `route_turn`/`run_flow`/`RunOptions`/`RunEvent`/`StreamEvent` used per the verified signatures; T5 consumes T1+T2+T3; T7 consumes the T5-completed backend + the proxy from SP1b.
- **Sequencing safety:** in-proc path untouched until T8 (Global Constraint + T7's `use_agentd=false` default); T8 is the single point of no return with a mandatory manual walkthrough before commit.
