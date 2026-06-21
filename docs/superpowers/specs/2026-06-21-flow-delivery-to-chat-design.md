# Flow Delivery to Chat — Design (S1)

**Status:** Approved for planning (2026-06-21).
**Branch:** phase1-local-llm-gateway.
**Program context:** First of four specs decomposed from the "flow output UX + bugs" batch
(S-bug ✅ done → **S1 (this doc)** → S2 live streaming inspector → S3 model listing/selection).
Resume anchor: `docs/notes/2026-06-21-flow-output-ux-and-bugs.md`. Related memories:
project_flow_schema_v2, project_agent_activity_bubbles, feedback_best_practices_rule (Rule 0–3).

**Goal:** When a chat-launched flow finishes, automatically deliver its result — richly — into the
*originating* conversation, with truthful status, per-conversation run state, and inline artifact
cards. Eliminate the "result never posts to chat / stuck-thinking / false-failed" failures.

**Architecture (one sentence):** Standardize chat flow execution on **agentd** (embedded conductor,
authoritative in-process run events), thread a `conversation_id` end-to-end so run events route back
to the launching thread, scope per-turn run state per conversation, and render the delivered result
as markdown + artifact cards.

**Tech Stack:** Rust, iced 0.14 (markdown + `iced_highlighter`), agentd D-Bus, oxidemx-conductor,
oxidemx-agent-core, zbus.

---

## Global Constraints (bind every task)

- **Rule 0 — field-standard naming + current best practices.** Use `conversation_id` consistently
  (the agentd thread id is the conversation id; `ChatThread.session_id` is its server handle). Tool
  context object is idiomatic — name it `ToolContext`.
- **Rule 1 — truthfulness is structural.** Flow status MUST derive from the authoritative run
  outcome (`RunFinished` / `run.json`), NEVER inferred from scraping subprocess stdout. The model
  must be able to query authoritative status (`run_status`) before asserting a run is done.
- **Rule 2 — Rust quality bar.** clippy-clean, hand-formatted (no repo-wide `cargo fmt`), no
  gold-plating, `?` over unwrap, trait seams for anything mocked. TDD on pure/seam logic;
  compile-wire + live-test the config-built live paths.
- **Rule 3 — process + builds.** agentd / oxidemx-agent-core / oxidemx-conductor build **host-side**
  (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target`, rustup); the overlay builds in the
  `claude_development` distrobox. Never mix toolchains over one `target/`. Isolate implementation in
  a git worktree (Jim edits the main checkout concurrently). Install affected bins + verify the
  RUNNING process is the new build before declaring test-ready ("merged ≠ deployed").

---

## Background — current behaviour and the four failures observed

1. **Result never auto-posts.** On `RunFinished`, `handoff_markdown` (the FULL `[delivery].root`
   artifact text, capped 48 KB — not a summary) reaches the activity bubble's Transcript button only.
   Nothing posts it into the chat thread. The Transcript button posts to `ai_threads[ai_active]`
   (whatever thread is *currently* active), not the originating one
   (overlay-rs/src/app/update.rs `Message::RunTranscript`).
2. **Terse final reply.** Because the rich handoff never posts, the user sees only the agent's
   "it's in ANSWER.md" one-liner.
3. **Stuck "working" state + no conversation scoping.** `ai_loading` is a **window-global** field
   on `RadialState` ("only one request can be in flight"), so switching conversations shows the same
   status. In in-proc mode `run_flow_tool` reads the conductor subprocess stdout to EOF then
   `child.wait()` (overlay-rs/src/ai_client/tools.rs) — the tool BLOCKS for the whole flow, holding
   the turn open and `ai_loading=true` indefinitely.
4. **False "failed" status (truthfulness bug).** In-proc `run_flow_tool` decides success only by
   parsing a `run_finished` line off subprocess stdout; it ignores the authoritative `run.json`.
   A real `doc-digest` run produced `run.json {"success": true, 4 artifacts}` yet the tool reported
   `[flow run failed: doc-digest — 0/0 steps]` (zero events parsed → `steps=[]`, `success=false`).
   The model then confabulated around the wrong status. Compounding it, `run_status` (the
   self-diagnose tool) is **agentd-only** — not wired in the in-proc dispatch — so the model's
   "ALWAYS call run_status before claiming done" instruction hits "Unknown tool" in-proc.

**Decision (approved):** chat flows execute **agentd-only**. agentd embeds the conductor (no
subprocess, no PATH resolution, no stdout scraping), emits authoritative run events via
`RunEventBridge`, and has `run_status`. The brittle in-proc subprocess+scrape `run_flow_tool` is
retired for chat. In-proc conductor remains a CLI/dev tool, not a chat path.

---

## Components & data flow

### A. Run → conversation linkage (server-authoritative)

The agentd thread id (= `conversation_id`) exists at turn start (agentd/src/interface.rs
`run_turn(thread, …)`) but is dropped before the tool executor. Carry it through:

```
send_message(thread) → run_turn(thread)
  → tool executor receives a ToolContext { conversation_id }          [oxidemx-agent-core]
    → run_flow tool → RunLauncher.launch(project, flow_id, inputs, conversation_id)  [agentd]
      → RunOptions.conversation_id                                     [oxidemx-conductor]
        → RunEventBridge { conversation_id } → AgentEvent payload carries conversation_id
        → write_run_json adds "conversation_id"                        [durability/recovery]
```

- **`ToolContext`** (new, oxidemx-agent-core): a small struct carrying `conversation_id`
  (extensible later). The `ToolExecutor::execute` seam gains the context. This is our crate, not
  external; tools-receive-a-context is the field-standard shape.
- **`RunLauncher::launch`** gains `conversation_id: &str`. `ConductorRunLauncher` stores it on the
  `RunEventBridge` and in `RunOptions`.
- **`RunOptions.conversation_id`** + **`write_run_json`** record it (oxidemx-conductor/supervisor.rs)
  → recovery + future Mission-Control "which chat launched this".
- **`AgentEvent` payload** gains `conversation_id` (agentd/src/run_bridge.rs `do_emit`). The
  `thread_or_run` envelope field stays the run_id (other consumers depend on it); the conversation id
  rides in the payload.

### B. Overlay: route + auto-deliver + per-conversation state

- **`RunEventView`** gains `conversation_id`; `agent_events.rs::demux_event` passes it through.
- **Per-conversation run state.** Move per-turn state out of the window-global `ai_loading` into
  per-conversation state keyed by `conversation_id` (on `ChatThread`, or a
  `HashMap<conversation_id, ConvRunState>` on `RadialState`). Fields: `loading`/working, in-flight
  run ids. The send/stop button + thinking animation read the *active* conversation's state.
  Switching threads shows that thread's real status; multiple conversations can work concurrently.
- **Auto-deliver on `RunFinished`:** look up the thread by `conversation_id`; append the result as an
  assistant message (Section C); clear that conversation's working state. The existing
  `RunTranscript` button stays as a manual re-post.
- **`RunFailed`/`RunCancelled`:** post a failure/cancel notice to the originating thread and clear
  its working state (a stalled/failed flow can never wedge the UI). The failure text comes from the
  authoritative outcome error, not a guess.
- **Conversation-scoped bubbles:** the activity dock filters clusters by the active
  `conversation_id` (closes the window-global gap in project_agent_activity_bubbles). Bubbles for
  thread A stop showing while viewing thread B.

### C. Delivery format (terseness fix)

- Post `handoff_markdown` (the full delivery artifact) as a normal assistant **markdown** message via
  the existing render path (iced markdown + `iced_highlighter`), prefixed with a compact result
  header: `flow-name · ✓/✗ · N steps · run <id>`.
- Data-driven: deliver the artifact text, not a model re-summary (avoids terse paraphrase). The model
  may still add its own commentary in its turn, but the authoritative content is always posted.

### D. Inline artifact cards

When the delivered message has artifact paths (from `RunFinished.artifacts` — no text scraping):
- **Card per artifact.** Title = filename. Right-aligned icon buttons: **open file**, **open folder**
  (`xdg-open` the dir), **copy path** (`RunOpenArtifact` already opens; add folder + copy).
- **Body** = file contents, **truncated** (iced `text` `Ellipsis`); **click body to expand/collapse**
  full contents.
- **Markdown** artifacts render via the markdown path; **code** via `iced_highlighter`
  (streaming-capable).
- **Density-aware:** many artifacts → default collapsed (title + buttons only); few → longer preview.

### E. Truthful status + self-diagnosis (Rule 1)

- **Retire in-proc stdout-scraping** for chat; status is the authoritative `RunFinished`/`run.json`
  outcome.
- **`run_status` available wherever flows run** (agentd) so the model verifies before asserting done.
- **Run self-introspection:** the agent can read a run's authoritative status + `run.json` +
  artifacts/`debug/` to answer "did it work?" truthfully and explain a discrepancy. (Implementation:
  extend `run_status` to optionally include artifact listing / a `read_run_artifact` tool — final
  shape decided in the plan; keep minimal.)

### F. `use_agentd` config-revert fix (prerequisite)

Flows now require agentd mode. The `overlay.ai.use_agentd` key keeps reverting to absent. Find the
cause (serialize default-skip? a settings round-trip dropping the key? a default() overwrite path)
and make it persist; default it on for flow use, with a graceful "agentd required for flows" path
(auto-enable + ensure the daemon is up) so a missing daemon surfaces clearly instead of silently
falling back to the broken in-proc path.

---

## Build order (single shippable cut — cards included, per approval 2026-06-21)

Cards ship in the first cut. Implementation order follows the dependency chain (each task ends with
an independently testable deliverable):

1. **`use_agentd` revert fix (F)** — prerequisite; flows require agentd.
2. **`ToolContext` + `conversation_id` end-to-end (A)** — the linkage foundation.
3. **Per-conversation run state (B)** — retire window-global `ai_loading`.
4. **Auto-deliver on `RunFinished` + clear state + failure/cancel notices (B/C)** — fixes
   missing-delivery, stuck-state, terseness.
5. **Authoritative status + `run_status` reachable; retire in-proc scrape (E)** — fixes false-failed
   (Rule 1).
6. **Inline artifact cards (D)** — open/folder/copy, truncate/expand, markdown vs `iced_highlighter`,
   density-aware.
7. **Conversation-scoped bubbles (B) + result-header polish (C) + run self-introspection (E).**

## Out of scope (future specs)

- General self-diagnostic agent that debugs the framework end-to-end (build on seeded
  `rust-agent-self-diagnose` / `system-doctor`). S1 only adds truthful run status + run
  introspection.
- Streaming inspector (thinking/tool-progress) = **S2**. Model listing/selection = **S3**. Phase 2a
  typed I/O = the conductor track (project_flow_schema_v2).

## Testing strategy

- **Pure/seam (mock):** `conversation_id` propagation through `RunLauncher`/`RunOptions`/`RunEventBridge`
  (assert the emitted `AgentEvent` payload carries it); `write_run_json` records it; per-conversation
  state reducer (apply `RunFinished` for conv A while conv B is active → A updates, B untouched);
  auto-deliver posts to the conv matched by `conversation_id`, not `ai_active`; `RunFailed` clears
  state + posts notice.
- **Authoritative status:** a flow whose `run.json` says success → status reported success even if
  the (now-removed) event scrape would have differed; `run_status` returns ground truth.
- **Live-wire:** agentd run end-to-end (mock provider) → overlay receives `RunFinished` with
  `conversation_id` → message lands in the right thread; `use_agentd` persists across a settings
  round-trip + restart.
- **UI (manual):** artifact card open/folder/copy, truncate/expand, markdown vs code render,
  density-aware default; bubbles scoped to active conversation; second conversation works while first
  is mid-run.
