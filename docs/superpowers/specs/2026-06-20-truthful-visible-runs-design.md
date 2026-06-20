# Truthful & visible background runs — design

Date: 2026-06-20
Status: design (brainstormed + approved in-chat; pending spec review → writing-plans).
Naming reconciled with the connector/Hermes research
(`docs/research/{hermes-agent-architecture,connector-architecture}.md`): the run-* terms
(`RunLauncher`/`run_status`/`RunStatus`) stand; connector framing now uses the locked
`Connector` seam + agentd-as-Gateway (see `docs/design/connector-modularization.md`).
Triggered by the SP1c GUI walkthrough: the agent claimed a flow was "running" when it
never launched (the `run_flow` tool is a stub) and *confabulated* a status (no
`run_status` tool). This slice makes background runs **real, queryable, and visible** —
and bakes in the truthfulness principle that the model is never the source of truth for
verifiable system state. Read `docs/AI-ARCHITECTURE-STATUS.md` first.

## 1. Goal & scope

Three coupled pieces:
1. **Real runs** — the agent's `run_flow` tool actually launches a conductor run and
   returns a real `run_id` (today it returns a stub string the LLM misreads as success).
2. **Queryable status** — `run_status`/`list_runs` tools return ground truth from
   `run_statuses`, so the agent stops imagining status.
3. **Visible + truthful** — the overlay shows a **live activity surface** (floating
   per-run bubbles) rendered from the *real* run status/events, so the user sees truth
   directly, never the model's narration; plus a system-prompt rule + a guard so the
   model doesn't assert unverifiable state.

**In scope:** the `RunLauncher` seam + adapter; real `run_flow` + new `run_status`/
`list_runs` tools; the truthfulness measures (prompt rule; the `ResponseGuard` extension
specced, built as a follow-on if heavy); the overlay activity UI. **Out of scope:** the
full connector modularization (`docs/design/connector-modularization.md` — its own
slice); the SP2d-3 task executor; deep visual polish.

## 2. The truthfulness principle (the spine of this slice)

The model must **never be the authority on a fact the system knows** (run status, file
contents, test/command results) — the same rule as the verifier (`cargo`-pass, not
self-report) and the ledger `CompletionPromise`. Three layers, applied here:
- **Tool coverage** — any reportable state must be *queryable* by a tool (§3). No tool ⇒
  forced guess.
- **Render truth, don't narrate it** — the UI shows real `run_statuses` (§5); for state
  the system tracks, **show it, don't ask the model**.
- **Prompt + guard** — a system-prompt rule ("never state a run/task/file/test/command
  status or result unless a tool call this turn returned it; else call the tool or say
  you can't verify") + extend the existing `ResponseGuard` (grounding / no-new-facts) to
  flag unbacked state-claims (§4).

## 3. `RunLauncher` seam + real tools

- **`RunLauncher` trait** (the seam the user chose): `async fn launch(&self, flow_id, inputs: serde_json::Value) -> Result<String /*run_id*/, RunError>`; `fn status(&self, run_id: &str) -> Option<RunStatus>`; `fn list_runs(&self) -> Vec<RunInfo>`. `RunStatus { Running, Done, Failed, Blocked, … }` (mirrors `run_statuses`). A `RunLauncherAdapter` impl over `AgentService.run_flow` + the `active_runs`/`run_statuses` maps. Threaded into `AgentToolExecutor` as `Arc<dyn RunLauncher>` (alongside `host`). Mock-testable.
- **`run_flow` tool** (replace the stub, `tools/agent.rs`): call `launcher.launch(flow_id, inputs)`; return a TRUTHFUL result — the real `run_id` + "launched flow X (run <id>)". On error, the real error.
- **`run_status` tool** (new; declare in `mode.rs`, dispatch in `tools/agent.rs`): args `run_id` → `launcher.status(run_id)` → the ground-truth status (or "no such run"). **`list_runs` tool**: `launcher.list_runs()` → all known runs + statuses. These give the agent ground truth to answer "status?" instead of confabulating.

## 4. Truthfulness measures (cross-cutting)

- **System-prompt rule** added to the agent's system prompt assembly (`oxidemx-agent-core::mode`/persona): the verbatim "never assert verifiable state without a tool result" rule (§2).
- **`ResponseGuard` extension** (`oxidemx-agent-local::guard`): a check that flags a reply asserting a run/task status/result when no tool call in the turn returned it. Specced here; built as a follow-on if it needs turn-context plumbing (the prompt rule + tool coverage + UI-truth are the immediate fix; the guard is the safety net). Verdict → flag/regenerate, never silently pass a likely-fabricated status.

## 5. Activity UI — the visible, truthful surface (overlay)

- A floating strip of **run bubbles** at the top of the chat, one per known run, fed by
  the `event` signal's `run`-kind events + `list_runs`/`run_status` (the SAME ground
  truth the tools use — the model is not in this path). Each bubble shows the **real**
  status (Running/Done/Failed/Blocked), a **pulse/spinner** while running, and an
  **unread-count badge** (new run events since the user last viewed it).
- Click a bubble → **expand** a panel with that run's event timeline (steps, agent
  messages, partial artifacts) inside the chat; viewing clears the badge. Terminal runs
  collapse to a "recent" affordance.
- Overlay state: `runs: HashMap<run_id, RunView { status, events, unread }>` updated from
  the `run` events; the bubble strip + expansion render it.
- **Visual design (layout, animation, badge placement) to be finalized with the visual
  companion** during implementation — this section is the behavior, not the pixels.

## 6. Connector-agnostic backend (per `docs/design/connector-modularization.md`)

§3–§4 live in agentd's core/tool layer — **connector-neutral**: any future `Connector`
(`TelegramConnector`/`HttpConnector`/…) gets the same real `run_flow`/`run_status` tools
+ the same ground-truth `run_statuses` (agentd is the Gateway hosting them). Only §5's
bubble UI is the `OverlayConnector`'s presentation of the run data. So this slice is
connector-ready by construction; the `Connector` seam itself is a separate slice
(SP-Connectors).

## 7. Testing

- **Backend (headless):** a mock `RunLauncher` (records launch; returns scripted
  statuses); assert `run_flow` tool returns the real run_id (not the stub string),
  `run_status`/`list_runs` return the mock's ground truth, an unknown run_id → "no such
  run" (not a fabrication). The system-prompt rule is a string assertion in the prompt
  assembly test.
- **UI:** build-gate + manual walkthrough — launch a real flow from chat → its bubble
  appears + pulses → "status?" now answers truthfully from the tool → expand → see the
  real timeline → completion flips the bubble to Done.

## 8. Scope / sequencing

Build **backend-first** (RunLauncher + real tools + truthfulness prompt — all
headless-testable), then the **UI** sub-slice (overlay, visual-companion-designed). The
`ResponseGuard` extension and visual polish are follow-ons. Naming reconciled with the
connector/Hermes research before implementation. Distinct from SP2d-3 (task executor)
and the connector slice, though it shares the run-lifecycle + truthfulness patterns.
