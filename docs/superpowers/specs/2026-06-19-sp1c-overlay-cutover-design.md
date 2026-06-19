# SP1c — agentd turn-path completion + overlay D-Bus cutover

Date: 2026-06-19
Status: design (brainstormed + approved in-chat; pending spec review → writing-plans)
Part of the agent re-architecture (`docs/superpowers/specs/2026-06-18-agent-framework-rearchitecture-design.md`).
Completes the single-backend migration: agentd runs a REAL turn (tools + history +
streaming), then the overlay drops its in-process agent path and becomes a thin
`org.oxidemx.Agent` D-Bus client. Builds directly on SP1b
(`docs/superpowers/specs/2026-06-18-agentd-design.md`). Read
`docs/AI-ARCHITECTURE-STATUS.md` first.

## 1. Goal & scope

SP1b stood up agentd with a mock-tested turn path. SP1c makes that turn path REAL
and cuts the overlay over to it — in ONE effort (user decision), with the overlay's
existing in-process path kept fully working until the final cutover task flips it
(no mid-effort chat regression). Worktree-isolated.

**In scope:**
1. **Event unification** — fold core's `StreamSink` (chat token deltas/tool markers)
   and the conductor's `EventSink` (`RunEvent`s) into agentd's `EventEmitter` → the
   `event` signal.
2. **agentd-native tools** — migrate `execute_local_tool`'s logic out of the overlay
   into an agentd-hosted `ToolExecutor`; host-bound capabilities route via
   `HostCapability`.
3. **Turn-path completion** — feed `TranscriptStore` history into `route_turn`; wire
   the real `ToolExecutor`; plumb token usage; complete `run_flow`/`run_status`/
   `cancel_turn`/`cancel_run`.
4. **Overlay thin client** — overlay calls the `oxidemx-agent-proxy` client, renders
   from the `event` signal, loads history via `get_transcript`/`list_threads`,
   DELETES the in-proc `route_turn` path + `OverlayToolExecutor`, implements
   `HostCapability` over its compositor code.

**Out of scope (later SPs):** settings/mission-control client migration; SP2 coding
tools depth; SP-Learn; SP-Research; the in-process fleets/federation SPs.

## 2. Decisions (locked in brainstorm)

1. **Single combined SP1c** (turn-path + cutover together), in-proc path alive until
   the final flip.
2. **Tools run agentd-native** (server-side, in the project cwd; work headless), with
   only genuinely host-bound capabilities delegated to the client via `HostCapability`.
3. **Token deltas are best-effort** (keep the `BusEmitter` `try_send` drop-on-full
   policy — a dropped delta only degrades live typing); the **final assistant message
   is authoritative and persisted** to the transcript, so a client that misses deltas
   still gets the correct result via `get_transcript`.
4. **Menu-config "apply live"** is host-delegated: agentd reads/writes the menu config
   file directly, but applying it to the *running* radial menu emits a signal the
   daemon/overlay already watch (the live-apply step needs the GUI/daemon).

## 3. Tool split (from the SP1c audit of `overlay-rs/src/ai_client/tools.rs`)

**agentd-native** (run in the project cwd; no GUI needed):
`read_file`, `list_dir`, `search_file`, `parse_document`, `execute_command` (shell),
`compose_flow`, `run_flow`, `use_skill`, `memory`, `persona`, `schedule_task`,
`list_system_apps`, `google_search`.

**host-delegated via `HostCapability`** (need the GUI/user session):
`ask_multiple_choice_question` (prompts the user), the menu-config *live-apply* step,
plus the capability-style (non-tool) hooks `screenshot`/`vision`/`clipboard`/
`current-window`.

Migration moves the native tool bodies into an agentd-side module (preferred: a new
`tools` module in `agentd`, or in `oxidemx-agent-core` if it stays UI-free) behind an
`AgentToolExecutor: oxidemx_agent_core::tool::ToolExecutor`. Per-tool host calls go
through `Arc<dyn HostCapability>`. Project-scoped tools (`read_file`, `execute_command`,
`use_skill`, `memory`, `persona`) resolve paths from the turn's `ProjectPaths` +
`merged_skill_roots`/`merged_mcp`/`project_config` (this is where SP1b's project-merge
finally gets consumed).

## 4. Event unification

- **`StreamSink` → `EventEmitter`.** `route_turn`/`run` take an `Option<StreamSink>`.
  agentd builds a `StreamSink` whose backing forwards each `StreamEvent` (delta,
  tool-call-start/finish, error) to the `EventEmitter` as an `AgentEvent`
  (`payload.kind` ∈ `delta|tool|error|final`), stamped with project + thread. The
  drain task already maps `AgentEvent` → the `event` signal (best-effort, §2.3).
- **conductor `EventSink` → `EventEmitter`.** Implement an agentd `EventSink` (the
  "mpsc forwarder" its doc anticipates) that maps each `RunEvent` to an `AgentEvent`
  (`payload.kind="run"`, run_id, step) → `event` signal. `run_flow` passes this sink
  to `supervisor::run_flow`.
- **`approval_requested`** continues to fire from the drain task (SP1b final-review
  C1 fix) when `payload.kind=="ApprovalRequest"`.
- One unified `event` stream out of agentd; clients demux on `payload.kind`.

## 5. Turn-path completion (agentd)

- `CoreTurnRunner::run_turn` gains: (a) `history` already wired in SP1b's final fix
  (`TranscriptStore` → `(is_user,text)`); (b) the real `AgentToolExecutor` (§3)
  instead of the no-op; (c) the `StreamSink`→`EventEmitter` bridge (§4) so deltas
  stream; (d) token usage from the turn result into `JournalEntry::Turn`.
- `run_flow` → `supervisor::run_flow(plan, RunOptions{workdir: project.runs_dir(), …},
  agentd_event_sink)` with a real `ProviderFactory` (the `ConfigFactory`); populate
  the run-status table from `RunEvent`s; `run_status` returns it.
- `cancel_turn`/`cancel_run` → retain a cancel token / `RunHandle` per active
  turn/run (a map in `AgentService`) and signal it; emit a cancellation `event`.
- `EventEmitter` ↔ conductor/core unification (rearch §4.1) is satisfied by §4's two
  adapters — no third event abstraction.

## 6. Overlay thin client

- **Send path:** the chat UI calls `oxidemx_agent_proxy::AgentProxy` methods
  (`send_message(project=cwd, thread, text, model_hint)`, `run_flow`,
  `respond_approval`, `optimize_prompt`, model controls) instead of `ask_ai`/
  `route_turn`. `project` = the overlay's current working directory (a session
  default for the personal-assistant case; a real cwd for coding sessions).
- **Receive path:** subscribe to the `event` signal once; demux on `payload.kind`
  (`delta`→append to the streaming bubble; `tool`→tool-activity UI; `run`→flow
  progress UI; `final`→commit the message; `model_status`→status). Replace the
  current `StreamSink`-driven rendering.
- **History:** on chat open / reconnect, `list_threads` + `get_transcript` rebuild the
  view; stop holding authoritative history in the UI.
- **Delete:** the in-proc `route_turn` call site, `OverlayToolExecutor`
  (`agent/tool_exec.rs`), and the now-unused overlay tool bodies migrated to agentd.
  Keep the overlay `agent/*` `pub use` shims only where settings/other code still
  needs them; delete the rest (no feature-flag phase-out — clean removal).
- **`HostCapability` impl:** the overlay registers as agentd's host (a D-Bus method or
  signal handshake — e.g. the overlay calls a `RegisterHost`-style method, or agentd
  invokes a `host_*` method on the overlay's own small interface; pick the simpler in
  the plan) and services `screenshot`/`vision`/`clipboard`/`current-window`/
  `ask_multiple_choice_question`/menu-live-apply from its existing compositor +
  daemon-signal code.
- **Approvals:** the overlay renders `approval_requested` as its approval card and
  calls `respond_approval`.

## 7. Architecture / data flow

```
overlay (thin client)                     agentd (org.oxidemx.Agent)
  chat UI ──send_message──────────────────▶ AgentService.send_message
  event-signal subscriber ◀──event──────── EventEmitter ◀─ StreamSink bridge ◀ route_turn
                                                          ◀─ EventSink bridge  ◀ conductor
  HostCapability impl ◀──host call──────── AgentToolExecutor (host-bound tools)
                                            AgentToolExecutor (native tools) → project cwd
  transcript view ◀──get_transcript─────── TranscriptStore (per project/thread)
```

## 8. Testing

- **Headless agentd = the proof surface.** Extend the agentd suite + the live-bus
  test to assert a **multi-turn, tool-using, streamed** conversation end-to-end
  through `AgentProxy`: a mock provider that emits deltas + requests a (mock) native
  tool + a host-delegated tool (mock `HostCapability`) across two turns; assert
  ordered `delta` events arrive, the tool runs, the transcript has the full
  multi-turn history fed back on turn 2, and the final message is persisted. A
  `run_flow` test asserts `run`-kind events stream and `run_status` reflects them.
- **Overlay** is iced GUI → build-gate + a documented manual chat walkthrough
  (send a message, see streaming, run a tool, run a flow, approve an approval,
  reconnect and see history). No automated GUI test (consistent with the project).

## 9. Risks

- **Chat regression during cutover** — mitigated by keeping the in-proc path until
  the final flip task; that task is the point of no return and gets the manual
  walkthrough before commit.
- **Tool migration surface** — `tools.rs` is 851 lines; moving it risks behavior
  drift. Mitigate: migrate tool-by-tool with the existing behavior as the oracle;
  the agentd tool tests pin each.
- **Delta ordering vs drop policy** — accepted (§2.3): deltas best-effort, final
  authoritative. The test asserts the *final* + transcript, and that deltas are
  ordered when not dropped.
- **HostCapability handshake races** — no host connected ⇒ host-bound tools return a
  clean "unavailable" (SP1b behavior); the overlay registers on startup.
- **`project` for the personal-assistant case** — default to a stable per-user
  "home" project when the overlay isn't in a coding cwd; coding sessions pass the
  real directory.

## 10. Out of scope / sequencing

After SP1c the single-backend migration is complete (overlay = thin client, agentd =
the one backend). Next: SP2 (coding tools depth), then the fleets/federation SPs, then
SP-Learn / SP-Research (seams already reserved in SP1b). Settings + mission-control
client migration is a small follow-up, not blocking.
