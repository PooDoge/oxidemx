# Agent Framework Re-architecture — coding-first, agentd-hosted

Date: 2026-06-18
Status: design (brainstormed + approved section-by-section; pending user spec review → writing-plans)
Supersedes (renamed `*_OLD.md`, kept as the research quarry — do not implement from them):
`docs/plans/agent-framework-integration-brainstorm_OLD.md`,
`agent-feature-roadmap_OLD.md`, `agent-features-implementation_OLD.md`.
Carries forward: `docs/plans/phase{1,2,4}-*.md` (local LLM gateway, session manager,
hybrid router — all built on branch `phase1-local-llm-gateway`).

## 1. Goal & framing

A **coding-first** personal AI agent framework (personal-assistant features ride
the same substrate, secondary), driven entirely from the **Radial overlay + AI
Chat window**. Concretely it must be excellent at: repo-aware coding (read/edit/run/
test with diffs), longer autonomy with checkpoint/undo safety, cost-controlled
model use (strong cloud for reasoning, local mistral.rs for cheap work), and
multi-step flows (planner→coder→reviewer) — all runnable in the background.

Two product shapes, one substrate:
- **Single-loop agent** — a tight ReAct loop for quick edits/questions in chat.
- **Flows** — the conductor's multi-agent pipelines for big multi-step tasks.

## 2. Decisions locked in brainstorm

1. **Coding-first**, personal follows.
2. **Both shapes** (single-loop default + flows for big tasks), shared substrate.
3. **agentd now** — a persistent process; flows/tasks survive the overlay closing.
4. **Single backend** — the overlay is a pure D-Bus client. No in-process agent
   path, no fallback, no feature flag. We delete the overlay's in-proc execution
   rather than preserve it.
5. **Extract a shared core lib first**, then host it in agentd.
6. **Tools: native core + optional-WASM third-party** — core/built-in tools are
   native Rust; only untrusted third-party tools are WASM-sandboxed.

## 3. Reassessment of the old plans (coding-first lens)

**Keep (built, still right):** hybrid router (Phase 4), session manager, provider
factory + local LLM (mistral.rs) + multi-provider, the whole `oxidemx-conductor`
(FlowDoc/DAG/supervisor/approval/roster/events), allowlist + approval gate,
semantic memory, skills system (the evolved "recipes").

**Build (researched, unbuilt, high value for coding):** agentd + `org.oxidemx.Agent`
D-Bus; per-step/role **provider policy** (generalize the per-turn router); native
**coding toolset** (file edit with diffs, run/test, repo context); **checkpoint/undo**;
**approval-card actions** (edit-&-approve, reject-with-reason); **sub-agent
provenance + depth caps**; **Mission Control wired live**.

**Drop / defer:** WASM *core* tools → dropped (native core; WASM only for 3rd-party);
SQLite-episodic + triple-graph memory → defer (JSON + vector cosine is adequate;
revisit only if recall proves weak); guardrails crate → defer (single-user coding);
actor/Topic transport for in-process flows → don't adopt (JoinSet is fine);
external-worker federation (kowalski §15.2) → defer, but **reserve the D-Bus
`RegisterWorker` seam** (that's where "multi-agent transport" extends later);
visual flow wizard (Tier-B) → defer (`compose_flow` tool already exists).

## 4. Architecture (single backend)

```
 oxidemx-agent-core  (lib, NO ui/dbus deps)  ── the agent brain
   providers: factory · session manager · ProviderPolicy (router unified)
   turn loop: single-loop ReAct (lifted from overlay agent_runtime)
   tools:     native core set + dispatcher (+ MCP proxy, + WASM 3rd-party host)
   memory:    semantic + v2 · persona
   depends on: oxidemx-conductor (flows)
   I/O boundary (traits, see §4.1)
        │  hosted by
        ▼
 agentd  (bin, org.oxidemx.Agent)  ── the ONE runtime
   persistent tokio runtime · owns sessions/runs · the sole impl of the
   core I/O boundary, bridged to D-Bus (signals out, methods in)
        │  D-Bus (session bus)
        ▼
 clients (thin, render-only):
   overlay-rs (chat + radial) · settings-rs (Agents tab) · mission-control · indicator
```

### 4.1 Core's I/O boundary (the in-proc-trait / D-Bus-later seam)

Core never depends on D-Bus or iced. It expresses its boundary as small traits;
**agentd provides the single production implementation** (tests use mocks):

- `EventSink` — emit the unified event stream (turn/stream-delta/tool/card/flow/
  usage). Unify with the conductor's existing `EventSink`. agentd → D-Bus `Event`.
- `Approver` — request approval / ask a question, await a verdict. agentd →
  `ApprovalRequested` signal + `RespondApproval` method.
- `HostCapability` — request a UI-only capability (vision/screenshot, current
  window, clipboard). agentd → D-Bus call to a connected overlay; none ⇒
  structured "unavailable".

This is the same pattern as the already-shipped `SessionStore` seam (Phase 2).

### 4.2 What moves, what's deleted

- **Moves into `oxidemx-agent-core`:** overlay `agent_runtime.rs` (run/route_turn/
  simple_chat/optimize_prompt/classify), the agent assembly in `ai_client.rs`
  (AgentMode/system-prompt/tools()), and the `agent/` modules (persona, memory,
  memory_semantic, skills, heartbeat, tasks) — none are UI-bound.
- **Deleted:** the overlay's in-process execution path (no dual backend). The
  overlay keeps only: input capture → `SendMessage`; event subscription → render;
  approval UI → `RespondApproval`; `HostCapability` impl (compositor side).
- **`oxidemx-agent` + `oxidemx-conductor`:** already libraries; folded under /
  depended on by core. `oxidemx-agent` may merge into core or stay a sub-crate.

## 5. `org.oxidemx.Agent` D-Bus surface

**Methods (clients → agentd):**
- `SendMessage(thread_id, text, attachments) → turn_id` (async; output via signals)
- `CancelTurn(thread_id)` · `CancelRun(run_id)`
- `RespondApproval(request_id, verdict)` — verdict ∈ `allow` | `deny(reason)` |
  `always` | `edit(args)`
- `RunFlow(flow_id, inputs) → run_id` · `ListFlows()` · `ValidateFlow(id)` ·
  `RunStatus(run_id)` · `ListRuns()` · `ListAgents()`
- `OptimizePrompt(text) → text`
- `GetTranscript(thread_id)` — big payloads pulled by method, never pushed
- `Checkpoint(thread_id) → ckpt_id` · `Revert(ckpt_id)` (SP3; reserve now)

**Signals (agentd → all clients):**
- `Event(json)` — one unified stream; agentd stamps `ts` + `thread_id`/`run_id`
  (AutoAgents events carry none). Folds today's `StreamSink` + conductor
  `EventSink` into one bus feed.
- `ApprovalRequested(request_id, card_json)`

**Rules (from old learnings):** events carry summaries + artifact **paths**, capped
~48 KB; multiple clients subscribe (agentd is the single source of truth); session
bus + unix sockets only (no TCP), identity from peer credentials; file-edit events
carry **diffs**. This bus IS the process-level multi-agent transport; the deferred
external-worker `RegisterWorker(id, capabilities)` is a later extension here.

## 6. Provider policy (SP2 — overview; own spec later)

Generalize the Phase-4 per-turn router into a **ProviderPolicy** that, given a task
context (single-loop turn | flow step | agent role | classified complexity),
returns a provider fingerprint, which the session provider-cache reuses:
- **Flow steps**: explicit `model`/`provider` per step + `[defaults]` (FlowDoc
  already supports this) + role defaults in the roster.
- **Single-loop chat**: the hybrid classifier (Phase 4) — heuristic + local-SLM
  → local/cloud.
- **Coding-first defaults**: planning/reasoning → strong cloud (Claude/Gemini Pro);
  mechanical edits / classification / prompt-opt / sanitization → local; reviewer/
  critic → cloud. All overridable.

## 7. Tool boundary (§ from brainstorm Section 3)

1. **Native core tools** (in core, run in agentd): file read/edit/write (diffs),
   `run_command` (allowlist-gated), run-tests, search/list_dir, memory, persona,
   schedule, `run_flow`/`compose_flow`, parse_document, brave_search, MCP-proxied.
2. **UI-capability tools** (vision/screenshot, current-window, clipboard) via the
   `HostCapability` seam → overlay; unavailable ⇒ structured result, not error.
3. **Optional-WASM third-party tools** via AutoAgents wasmtime `ToolRuntime`,
   distributed like widget plugins.

All mutating tools pass the `ApprovalGate` (allowlist fast-path + per-tool policy
`always|allowlist|autonomous` + structured denial). Coding adds edit-&-approve /
reject-with-reason / checkpoint.

## 8. Decomposition (each sub-project gets its own spec → plan)

- **SP1 — agentd foundation (this spec's deep target).** Extract
  `oxidemx-agent-core` with the §4.1 seams; build `agentd` hosting it + the §5
  D-Bus surface; convert the overlay to a thin client and **delete** its in-proc
  path. *Exit: chat + run_flow work end-to-end over D-Bus; agentd survives overlay
  close; old in-proc path gone.*
- **SP2 — Provider policy** (§6): per-step/role declaration unified with the router.
- **SP3 — Coding toolset + checkpoint/undo + approval-card actions** (diffs,
  run/test, repo context, edit-&-approve, reject-with-reason, Revert).
- **SP4 — UI**: Mission Control wired to `Event`; radial agent presence; approval
  surfacing across overlay/indicator (+ optional MX4 haptics).
- **SP5+** — external-worker transport (`RegisterWorker`), trigger engine
  (schedule/D-Bus), heartbeat memory-consolidation, Tier-B flow wizard.

## 9. SP1 migration steps (extract-core-first, single-path cutover)

1. **Carve core**: create `oxidemx-agent-core`; move the §4.2 modules in; define
   `EventSink`/`Approver`/`HostCapability`; keep everything compiling with a
   temporary in-overlay adapter so the app still runs. (Pure refactor; behavior
   unchanged; Phases 1-4 intact.)
2. **Build agentd**: bin hosting core + the persistent runtime; implement the
   seams as the D-Bus surface (§5). Headless-testable.
3. **Thin the overlay**: replace `ai_client::ask_ai`/`agent_runtime` call sites
   with D-Bus calls; implement `HostCapability` (vision/clipboard) and the event/
   approval rendering against the bus. **Delete** the in-proc execution path.
4. **Supervisor unit**: agentd joins the existing supervisor (like daemon/overlay)
   — autostart, restart, single-instance.
5. Settings "Agents" tab + Mission Control repointed at agentd (read path) — minimal
   in SP1, expanded in SP4.

## 10. Testing

- Core: unit tests with mock `EventSink`/`Approver`/`HostCapability` (the turn
  loop, ProviderPolicy, tool dispatch, conductor already has its own).
- agentd: integration test driving the D-Bus surface (SendMessage → Event stream;
  RunFlow → run-layer events; RespondApproval round-trip) with a mock provider.
- Live: a coding turn (read→edit→diff→approve) against a real provider; a flow
  (research-digest) end-to-end over the bus; agentd-survives-overlay-close check.

## 11. Risks

- **Big cutover (no fallback)** — accepted per the single-backend decision; mitigate
  by keeping SP1 step 1 a pure refactor (green throughout) and only ripping the
  in-proc path in step 3 once agentd reaches parity headlessly.
- **D-Bus payload/latency** — summaries+paths + 48 KB cap + transcripts by method;
  `try_send`/drop-oldest for UI event fanout, blocking only for the supervisor's
  own consumption.
- **HostCapability when no overlay up** — structured "unavailable"; vision tools
  simply degrade.
- **AutoAgents pre-1.0** — unchanged: vendored & pinned; our code touches trait
  surfaces only.
- **Scope** — strictly SP1 here; SP2-SP5 are separate specs to avoid a mega-PR.
