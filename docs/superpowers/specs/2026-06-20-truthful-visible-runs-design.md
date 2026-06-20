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

Re-evaluated against agent best practices + the Claude Code system prompts
(`docs/research/claude-code-system-prompt-patterns.md`), which enforce truthfulness
**structurally** — requiring tool output to exist before any status claim, never merely
exhorting honesty ("Report outcomes accurately…"; "Read, search, and investigate freely —
looking is not acting"). Same discipline as our verifier (`cargo`-pass, not self-report)
and the ledger `CompletionPromise`. The invariant is **grounded narration** — not
silencing the model. Layers, primary first:

- **Grounded narration (invariant — ALL connectors).** The model may state verifiable
  system state (run/task/file/test/command status or result) **only from a tool result in
  the current turn**; otherwise it calls the tool or abstains ("I haven't checked"). This
  is connector-agnostic — it must hold on Telegram/HTTP where there is no UI to read.
- **Action vs. outcome (the testable line).** The model MAY narrate its own *actions*
  ("I launched the flow, run `<id>`" — grounded in the tool's return); it MUST NOT assert
  *outcomes it did not observe* ("still running" / "succeeded") without a status tool
  result. "Looking is not acting": investigation is always free; assertion needs evidence.
- **Tool coverage (enables the invariant).** Any reportable state must be *queryable* by a
  tool (§3). No tool ⇒ the model is forced to guess.
- **Render truth directly (defense-in-depth — UI connectors only).** The overlay shows
  real `run_statuses` from source (§5): a second channel of truth + the authoritative
  tiebreaker if model text and UI ever diverge. NOT a substitute for the invariant — there
  is nothing to render on a headless connector.
- **Verify (backstop).** Extend the existing `ResponseGuard` (grounding / no-new-facts) to
  flag a state-claim unbacked by a turn observation — prompting alone never fully
  eliminates confabulation (§4).

## 3. `RunLauncher` seam + real tools

- **`RunLauncher` trait** (the seam the user chose): `async fn launch(&self, flow_id, inputs: serde_json::Value) -> Result<String /*run_id*/, RunError>`; `fn status(&self, run_id: &str) -> Option<RunStatus>`; `fn list_runs(&self) -> Vec<RunInfo>`. `RunStatus { Running, Done, Failed, Blocked, … }` (mirrors `run_statuses`). A `RunLauncherAdapter` impl over `AgentService.run_flow` + the `active_runs`/`run_statuses` maps. Threaded into `AgentToolExecutor` as `Arc<dyn RunLauncher>` (alongside `host`). Mock-testable.
- **`run_flow` tool** (replace the stub, `tools/agent.rs`): call `launcher.launch(flow_id, inputs)`; return a TRUTHFUL result — the real `run_id` + "launched flow X (run <id>)". On error, the real error.
- **`run_status` tool** (new; declare in `mode.rs`, dispatch in `tools/agent.rs`): args `run_id` → `launcher.status(run_id)` → the ground-truth status (or "no such run"). **`list_runs` tool**: `launcher.list_runs()` → all known runs + statuses. These give the agent ground truth to answer "status?" instead of confabulating.

## 4. Truthfulness measures (cross-cutting)

- **System-prompt rule** (in `oxidemx-agent-core::mode`/persona assembly), worded as the §2 invariant + action/outcome line: *"State a run/task/file/test/command status or result ONLY from a tool result in this turn; otherwise call the tool or say you haven't checked. Narrate your actions, never unobserved outcomes. Looking is not acting — investigate freely."* Phrased structurally (evidence-gated), per the Claude Code prompts.
- **`ResponseGuard` extension** (`oxidemx-agent-local::guard`): a check that flags a reply asserting a run/task status/result when no tool call in the turn returned it. Specced here; built as a follow-on if it needs turn-context plumbing (the prompt rule + tool coverage + UI-truth are the immediate fix; the guard is the safety net). Verdict → flag/regenerate, never silently pass a likely-fabricated status.
- **Broader system-prompt rules** (from `docs/research/claude-code-system-prompt-patterns.md` — fold into the prompt assembly as a separate system-prompt pass, beyond this slice): evidence-gated outcome reporting; approval-scope non-transferability ("approved once ≠ approved always / adjacent context" — pairs with our `ApprovalClassifier`); reversibility gate (read freely, confirm before destructive/outward-facing); no gold-plating. Tracked, not built here.

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
