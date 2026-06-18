# agentd (SP1b) — design

Date: 2026-06-18
Status: design (brainstormed + approved in-chat; pending spec review → writing-plans)
Part of the agent re-architecture (`docs/superpowers/specs/2026-06-18-agent-framework-rearchitecture-design.md`).
Implements that spec's §4 (single backend) + §5 (D-Bus) + §9 (migration step 2),
**now project-aware**, and hosts the embedded local model
(`docs/superpowers/specs/2026-06-18-local-model-manager-design.md`).
Read `docs/AI-ARCHITECTURE-STATUS.md` first for what's current.

## 1. Goal & scope

Stand up **agentd** — a persistent, project-aware process that hosts the agent
brain (`oxidemx-agent-core`) + orchestration (`oxidemx-conductor`) + the embedded
local model (`oxidemx-agent-local`), exposed over the `org.oxidemx.Agent` D-Bus
surface, and headless-testable. Heavy inference + long-running flows live here, out
of the GUI; flows/sessions survive the overlay closing.

**In scope (SP1b):** the agentd process + systemd/D-Bus activation; the
`org.oxidemx.Agent` surface; project-awareness (per-project sessions/transcripts/
logs/skills/mcp); agentd-owned persisted transcripts; the `EventSink`/`Approver`/
`HostCapability` seam impls; per-project decision/event **journaling** (the seam
SP-Learn consumes); local-model lifecycle controls; headless tests with mock
providers + `MockEngine`; the systemd unit.

**Out of scope (separate SPs):** SP1c overlay cutover (overlay → thin D-Bus client,
delete in-proc path, implement `HostCapability`, repoint chat UI at agentd
transcripts); **SP-Learn** (self-improving/preference-learning loop, §10); **SP-
Research** (local-LLM web-grounded research offload, §10). SP1b only *reserves the
seams* for these.

## 2. Decisions (locked in brainstorm)

1. **systemd user service + D-Bus activation** (`oxidemx-agentd.service` + a
   `org.oxidemx.Agent` D-Bus `.service` file). Bus auto-starts agentd on first
   call; systemd restarts on crash; survives the overlay.
2. **agentd owns per-thread transcripts** (persisted), so a reconnecting client can
   watch/replay — not client-ships-history.
3. **Project = working directory.** **Hybrid layout (like Claude Code):**
   project-local `.oxidemx/` (skills/mcp/project-config, merged OVER global
   `~/.config/oxidemx`) + central `~/.local/share/oxidemx/projects/<key>/`
   (transcripts, logs, learned-data).
4. **Single backend** — SP1c deletes the overlay in-proc path; SP1b reaches headless
   parity first (the running app is untouched until then).
5. **zbus 5**, mirroring the daemon's `#[interface]` server + `#[proxy]` client
   patterns (`daemon/src/dbus/interface.rs`, `overlay-rs/src/dbus.rs`).

## 3. Process & lifecycle

- New `agentd` bin crate (`agentd/`). One persistent multi-thread tokio runtime.
- On first activation it constructs once: the provider factory (local path wired to
  `oxidemx_agent_local::LocalChatProvider`), the `LocalModelService` singleton, the
  conductor, and the project registry. These are process-global (`Arc`), shared
  across D-Bus calls.
- Single-instance via `connection.request_name("org.oxidemx.Agent")` (NameTaken ⇒
  exit, like the overlay).
- Ships `oxidemx-agentd.service` (systemd user unit, `Type=dbus`,
  `BusName=org.oxidemx.Agent`, `Restart=on-failure`) + `org.oxidemx.Agent.service`
  (D-Bus activation file). Installed alongside the existing units.

## 4. Project model (project-aware)

- A **project** is identified by its absolute working directory. `project_key` =
  a stable slug+hash of the canonicalized path (collision-safe, human-recognizable),
  e.g. `myrepo-9f3a2c`.
- **Resolution / merge (project-local over global):**
  - **skills:** global `~/.claude` + `~/.gemini` + `~/.config/oxidemx/skills`, then
    project-local `<cwd>/.oxidemx/skills` (+ the existing `<cwd>/.claude/skills`),
    project overrides on name collision. (Extends `oxidemx-agent-core::skills`,
    which already scans some of these.)
  - **mcp:** global `~/.config/oxidemx/mcp.toml` merged with
    `<cwd>/.oxidemx/mcp.toml` (project servers added; name-collision = project wins).
  - **project-config:** `<cwd>/.oxidemx/config.toml` (per-project model/approval/
    default-flow overrides) layered over the global `AiConfig`.
- **Central per-project store** `~/.local/share/oxidemx/projects/<key>/`:
  `transcripts/`, `runs/`, `journal.jsonl` (decisions/events — SP-Learn seam),
  `learned/` (SP-Learn output), `meta.json` (the real cwd path + last-seen).
- Every D-Bus turn/flow call carries the **project cwd**; agentd resolves (and
  caches) that project's merged skills/mcp/config + opens its transcript/journal.
  A `ListProjects` method enumerates known projects.

## 5. `org.oxidemx.Agent` D-Bus surface (zbus 5)

**Methods (clients → agentd)** — all chat/flow methods take a leading
`project: &str` (cwd):
- `SendMessage(project, thread_id, text, attachments) → turn_id` (async; output via
  signals)
- `CancelTurn(project, thread_id)` · `CancelRun(run_id)`
- `RespondApproval(request_id, verdict)` — `allow | deny(reason) | always | edit(args)`
- `RunFlow(project, flow_id, inputs) → run_id` · `ListFlows(project)` ·
  `ValidateFlow(project, flow_id)` · `RunStatus(run_id)` · `ListRuns(project)` ·
  `ListAgents(project)`
- `OptimizePrompt(project, text) → text`
- `GetTranscript(project, thread_id)` · `ListThreads(project)` · `ListProjects()`
  (big payloads pulled by method, never pushed)
- **Local-model controls:** `LoadModel(alias)` · `UnloadModel(alias)` ·
  `SetActiveModel(alias)` · `ListModels()` (alias + state)

**Signals (agentd → all clients):**
- `Event(json)` — one unified stream; agentd stamps `ts` + `project` + `thread_id`/
  `run_id`; FIFO envelope-dedup. Folds the core `StreamSink` + conductor `EventSink`.
- `ApprovalRequested(request_id, card_json)`
- `ModelStatusChanged(alias, state)`

Rules (from the rearch spec): summaries + paths, ~48 KB signal cap; session bus +
unix sockets only, peer-credential identity; multiple clients subscribe (agentd is
the source of truth).

## 6. Sessions & transcripts (agentd-owned)

- A session is `(project_key, thread_id)`; agentd owns its transcript, persisted to
  `projects/<key>/transcripts/<thread_id>.jsonl`. `SendMessage` appends the user
  turn + the assistant reply; the per-conversation provider (the Phase-2
  `SessionManager`, now hosted in agentd) is keyed by `(project_key, thread_id)`.
- A reconnecting client calls `ListThreads`/`GetTranscript` to rebuild the view; the
  `Event` stream carries live deltas. History is no longer shipped by the client.
- The conductor's runs persist under `projects/<key>/runs/` (it already writes run
  records); agentd scopes them per project.

## 7. Seams wired + journaling (SP-Learn seam)

The §4.1 traits get their agentd impls:
- `EventSink` → the `Event` signal (ts/project-stamped, deduped). **Also appended to
  `projects/<key>/journal.jsonl`** (every turn/tool/flow event).
- `Approver` → `ApprovalRequested` signal + `RespondApproval` method (blocks the tool
  on a oneshot). **Each approval decision (allow/deny/edit + reason) is journaled.**
- `HostCapability` (vision/screenshot/clipboard/current-window) → a request to a
  connected client; **no client ⇒ structured "unavailable"** (the overlay impl is
  SP1c).

The `journal.jsonl` (turns, tool calls, approval decisions, outcomes, per project)
is the **raw material SP-Learn consumes** — SP1b just produces it faithfully; no
learning logic here.

## 8. Local-model controls

The model methods/signal wrap `oxidemx_agent_local::LocalModelService` (the
singleton agentd holds). `LoadModel`/`UnloadModel`/`SetActiveModel`/`ListModels` map
to the service; `ModelStatusChanged` mirrors `ModelState`. The provider factory's
local path returns a `LocalChatProvider` over this service (no HTTP). Per-project
config may set a preferred local model (resolved in §4).

## 9. Crate structure

```
agentd/                         # new bin crate
  src/main.rs                   # activation, request_name, runtime, construct singletons
  src/interface.rs             # #[interface(name="org.oxidemx.Agent")] impl (methods+signals)
  src/projects.rs              # project registry: key, resolve/merge skills/mcp/config, paths
  src/sessions.rs              # agentd-owned transcripts + SessionManager hosting
  src/seams.rs                 # EventSink/Approver/HostCapability agentd impls + journaling
  src/models.rs                # local-model controls bridge
oxidemx-agent-proxy/  (or in a shared crate)  # #[proxy] trait + small client types,
                              # so overlay/settings/mission-control (SP1c+) reuse one client
dist/systemd/oxidemx-agentd.service + dbus/org.oxidemx.Agent.service
```
Deps: `oxidemx-agent-core`, `oxidemx-conductor`, `oxidemx-agent-local` (with the
`mistral` feature for real local inference), `zbus = "5"`, `tokio`, `serde`. The
`#[proxy]` client lives in its own small crate (no heavy deps) so clients don't pull
core/mistralrs.

## 10. Reserved seams for later SPs (documented, NOT built here)

- **SP-Learn — self-improving loop.** Learns the user's practices, journals +
  reviews decisions, adapts/optimizes flows over time (gets better with use). It
  consumes SP1b's per-project `journal.jsonl` + transcripts + approval decisions,
  and writes to `projects/<key>/learned/` (e.g. distilled preferences, decision
  rubrics, flow tweaks) that the core's system-prompt assembly + the router read
  back. SP1b's contract for it: faithful, structured per-project journaling +
  stable `learned/` read path. Heavy use of the **local model** (cheap, offline) for
  the review/distillation passes — never a correctness gate (objective verification
  rules from the local-model spec apply).
- **SP-Research — local-LLM web-grounded research offload.** A consumer role
  (already in the local-model spec §8): the local model researches small,
  verifiable "best-practice / decision" questions with **web-search grounding** to
  spare the cloud. Uses `oxidemx-agent-local` `Mode::WebSearch` + the `ResponseGuard`
  (grounding/citation checks). SP1b hosts the model; the role logic is its own SP.

Both are added to the rearch roadmap (§8 of that spec) after SP1c.

## 11. Testing (headless)

A **test D-Bus client** drives `org.oxidemx.Agent` against agentd built with **mock
providers + `MockEngine`** (no GPU, no overlay, a private/throwaway bus or a test
session):
- `SendMessage(project,…)` → `Event` stream carries the deltas + final; transcript
  file written under the project store.
- `RunFlow` → run-layer events; run record under `projects/<key>/runs/`.
- `RespondApproval` round-trip unblocks a gated mock tool; the decision is journaled.
- `LoadModel`/`UnloadModel` → `ModelStatusChanged`.
- Project resolution unit tests: project-local `.oxidemx/skills|mcp|config` merges
  over global; two cwds get distinct `project_key` + isolated transcripts/journal.
- Journaling unit test: a turn + an approval produce the expected `journal.jsonl`
  lines (the SP-Learn contract).

## 12. Risks

- **D-Bus payload/latency** — summaries + paths + 48 KB cap + `GetTranscript`;
  `try_send`/drop-oldest for the `Event` fanout, blocking only for agentd's own
  consumption.
- **Project key collisions / moved repos** — key = slug + path-hash; `meta.json`
  stores the real path; re-resolve on cwd change.
- **Activation races / single-instance** — `request_name` guard; D-Bus activation
  serializes the first start.
- **HostCapability with no client** — structured "unavailable"; vision degrades.
- **Transcript ownership migration** — SP1b agentd owns transcripts; the overlay
  still has its own chat store until SP1c repoints it (no data migration in SP1b —
  agentd starts fresh per project; SP1c decides whether to import the overlay's
  history).
- **Heavy crate** — agentd pulls `oxidemx-agent-local` with `mistral`; keep the
  `#[proxy]` client crate light so clients don't.

## 13. Out of scope / sequencing

SP1c (overlay thin client + delete in-proc + HostCapability impl + transcript UI),
then SP-Learn (§10), SP-Research (§10). The rearch spec §8 roadmap is updated to
append SP-Learn + SP-Research after SP1c.
