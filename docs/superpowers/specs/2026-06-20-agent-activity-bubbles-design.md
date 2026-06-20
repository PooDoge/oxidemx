# Agent Activity Bubbles — Design Spec

**Date:** 2026-06-20
**Status:** Approved (scope), pending spec review
**Supersedes the vision in:** `docs/design/background-agent-activity-ui.md`
**Visual contract:** `agent-bubbles-app.jsx` in the *OxideMX Design System* project
(claude.ai/design, projectId `686a723e-0412-4e94-870e-b4e32ae465f2`) — fetch via the
`DesignSync` MCP (`get_file`). Every size / state / animation below is extracted from it.
**Token contract:** `docs/design/agent-bubbles/TOKENS.md` → `oxidemx-widgets::Palette` / Kit.

---

## Goal

In the standalone `oxidemx-chat` window, surface background/parallel conductor runs as a
**floating corner cluster of agent bubbles** — each bubble a running sub-agent (flow step)
with a live animation, an unread-count badge, and a click-to-peek popover showing that
step's live log tail, progress, status, and actions. Driven entirely by the run-kind events
already flowing `conductor RunEvent → agentd RunEventBridge → "event" D-Bus signal →
overlay agent_events`. Honors Rule 1 (truthfulness): every bubble's state comes from a real
event / `run_statuses`, never a model claim.

> **Prerequisite (verified 2026-06-20):** the dock is fed by the agentd **bus** `event` signal,
> and that subscriber is gated on `overlay.ai.use_agentd` (subscriptions.rs). The feature shows
> bubbles only when the chat runs in **agentd mode** (`config.json` → `overlay.ai.use_agentd:
> true`), which also routes the agent's own `run_flow` tool through the daemon's bus-emitting
> `ConductorRunLauncher`. In the default in-proc path (`use_agentd: false`) there is no bus
> subscription and the agent's runs never emit run-kind events, so no bubbles appear. This
> matches the agentd-as-Gateway direction (CLAUDE.md Rule 0). Confirmed end-to-end: RunStarted/
> TaskAssigned/TaskStarted → `dock_view` renders the cluster.

## Non-goals (deferred — explicitly OUT of v1)

- **Inline per-step approval** (the design's Deny / Always / Run-it chips). Conductor flows
  run `ApprovalPolicy::Autonomous`; per-step gating is real backend work. The *preferred*
  home for approvals is a **separate unified approval system** (desktop notification + a
  pending-approvals view on radial-menu open) — its own design exists in Claude Design and
  gets its own spec. See §10. Bubbles in approval state are **not** rendered in v1.
- **The "+" spawn-a-task button** — needs a "spawn background agent" backend that does not
  exist. Drop the button.
- **The Tweaks panel** (corner / speed / accent / reduce-motion dev tool) — mockup-only. Drop.
  Accent + all colors come from the live `Palette`, never hardcoded hex (the mockup's cyan
  is a mockup value).

---

## §1 · The design contract (what it looks like + does)

Extracted from `agent-bubbles-app.jsx`. **Sizes are the design's; colors are `Palette`
field names**, not the mockup hex.

### Bubble (one per flow step / sub-agent) — `AgentBubble`
- A **52×52 circular orb**: radial-gradient fill from `tone @ 32%` → `crust`, a `1.5px`
  `tone` border, drop shadow (elevation e1) + a `tone` glow. Centered 20px line-icon.
- **Tone** = the step's agent archetype color, mapped to a `Palette` slice color
  (researcher→`blue`, shell→`peach`, writer→`mauve`, summarizer→`teal`, browser→`green`,
  coordinator/default→`accent`). Mapping keyed off `TaskAssigned.agent` (§2).
- **State drives color + decoration** (state machine in §2):
  - *working* → tone color; a **spinning activity arc** (26/120 dash circle) + a **pulse**
    glow ring; the orb **bobs** gently.
  - *done* → `green` border + a one-shot **burst** ring; a green **check badge**.
  - *failed* → `red` border + a `!` badge.
  - (*approval* state exists in the design but is OUT of v1 — never produced.)
- **Badge** (top-right, ≥19px pill, `bump` animation on change): unread-count while working
  (`unread>9 → "9+"`), check when done, `!` when failed. Unread resets to 0 when its peek
  is open.
- **Dismiss ×** (top-left, 17px): always shown on done/failed, hover-only while working.
  Working dismiss = cancel the run; done/failed dismiss = remove the bubble.

### Peek popover (click a bubble) — `AgentPeek`
- A **286px** panel, opens toward window center (`left`/`right` of the bubble per corner),
  `crust @ 97%` + blur, `tone` left-border, elevation e2. Contents top→bottom:
  1. **Header:** 28px tone orb + icon · step/agent name (mono) · state label (uppercase,
     tone) · elapsed `m:ss` (mono, right).
  2. **Task line** + a **3px progress bar** (`tone`, glow). Progress = steps-done / steps-total
     for the run (§2) — coarse, no sub-step %.
  3. **Live log tail:** the step's last ~6 `AgentMessage` lines in `Font::MONOSPACE` (10.5px),
     newest highlighted with a blinking cursor `▌` while working; `$`-prefixed lines render
     as commands. Auto-scrolls to newest. Container `crust`, hairline border, max-height ~108px.
  4. **Action chips** (per state):
     - *working* → **Cancel** (calls cancel_run) + a "● live" indicator. (Design's "Watch"
       chip is dropped — the open peek *is* watching.)
     - *done* → **Transcript** (post the run's handoff/event-log into the chat thread),
       **Open artifact** (xdg-open `TaskFinished.artifact`), **Dismiss**.
     - *failed* → **Retry** (re-launch the run) + **Dismiss**.

### Cluster / dock — `AgentDock` + `CollapsedBubble`
- Lives in a **window corner** (default bottom-right; the chat composer sits above it).
  A floating `Stack` layer over the chat body — does not push chat content.
- **Collapsed (default):** one **cluster-orb per active run** (icon `agents`, status color =
  `accent` while running, `green` when all steps done), with a **count badge** = that run's
  step count. A run with exactly one step
  shows that step's solo orb instead (design's single-agent case). Multiple runs = a small
  vertical stack of cluster-orbs.
- **Expanded (click a cluster-orb):** the design's **header pill** — a status dot (pulse
  while running) + `"{n} running"` / `"all done"` + the flow name + a **collapse ×** — above
  a **column of that run's step-bubbles** (each entrance uses a one-shot `popin`). Spawn "+"
  button dropped (§non-goals).
- **Collapse on focus loss:** any pointer-down outside the dock collapses it (design's
  `pointerdown` capture). Esc also collapses.

### Animations (design names → §6 for the iced approach)
`bob` (orb float), `pulse` (glow ring), `spin` (activity arc), `popin` (bubble entrance),
`bump` (badge change), `burst` (done one-shot), `blink` (log cursor), `peekin` (popover),
`tailin` (new log line). Reduce-motion = all off.

---

## §2 · Data model — RunEvents → cluster state

The conductor emits these (confirmed in `oxidemx-conductor/src/event.rs`), the bridge
forwards them as `payload.kind=="run"` with `variant` + `details` + `run_id`:

| RunEvent (variant) | details | Effect on state |
|---|---|---|
| `RunStarted` | `flow_id`, `steps:[String]` | Create `RunCluster{run_id, flow_id, steps: one AgentBubble per step id, status: Running}`. Bubbles start *pending*. |
| `TaskAssigned` | `step`, `agent` | Set that bubble's **agent name → tone/icon** (the archetype mapping). |
| `TaskStarted` | `step` | Bubble → *working*; start its elapsed clock. |
| `AgentMessage` | `step`, `message` | **Append `message` to that bubble's log tail**; bump unread if its peek is closed. |
| `TaskFinished` | `step`, `success`, `artifact?`, `summary` | Bubble → *done* (success) / *failed*; store `artifact`+`summary`; recompute run progress. |
| `TaskError` | `step`, `error` | Bubble → *failed*; append error to log. |
| `StepRetrying` | `step`, `attempt` | Bubble → *working*; log "retry attempt N". |
| `StepSkipped` | `step`, `reason` | Bubble → *skipped* (greyed, not failed); log reason. |
| `RunFinished` | `artifacts`, `handoff_markdown` | Cluster → Finished; store artifacts + handoff (for Transcript). |
| `RunFailed` | `reason`, `step?` | Cluster → Failed. |
| `RunCancelled` | — | Cluster → Cancelled; remove after the recent-tray timeout. |
| `ApprovalRequested` | `step`, `card` | **v1: ignore** (no inline approval). |

`run_statuses` (the truthful-runs ground-truth table) remains authoritative for the
cluster status chip; the per-bubble states are derived from the step events.

**Progress** = `bubbles.filter(done||failed||skipped).count / bubbles.len()`.
**Elapsed** per bubble = wall-clock from its `TaskStarted` (events carry agentd-stamped `ts`).

### The one backend gap — `run_id` on step events
Today the bridge emits step-level events with `do_emit("", …)` (empty `run_id`,
`run_bridge.rs:107–153`), so the UI cannot group a step under its run. **Fix:** every emitted
`run`-kind event must carry its owning `run_id` in `payload.run_id`. Recommended mechanism
(implementer to confirm against how `ConductorRunLauncher` instantiates the bridge): if the
bridge is **one-per-run**, give it a `run_id: String` field set at construction (or captured
from the first `RunStarted`) and pass it in every `do_emit`. If a single bridge is shared
across concurrent runs, thread the run_id via a per-run sink wrapper instead. **Acceptance:
every emitted `run` event has a non-empty `payload.run_id` matching its run.** (`AgentMessage`
is *already* forwarded — no other backend change needed.)

---

## §3 · Architecture — where it lives + module shape

Reuses the standalone chat window built in `2026-06-20-standalone-chat-window-design.md`.
**Gated on `chat_window_mode`** — the floating corner cluster is for the decorated chat
window, not the cramped radial overlay. All new state/widgets no-op when `!chat_window_mode`
(same discipline as the rest of that slice).

Crate: `oxidemx-overlay` (overlay-rs). New module tree under `src/activity/`:

- `src/activity/mod.rs` — re-exports; the `ActivityState` struct (the run-cluster store) and
  its **event reducer** `apply_run_event(&mut self, RunEventView)` (pure, unit-tested).
- `src/activity/model.rs` — `RunCluster`, `AgentBubble`, `BubbleState` enum, `AgentTone`
  mapping (`agent name → Palette slice color + icon`), progress/elapsed helpers.
- `src/activity/dock.rs` — the `view` functions: `dock_view(&ActivityState, &Palette, …) ->
  Element` (collapsed cluster-orbs / expanded header-pill + bubble column), `bubble_view`,
  `peek_view`. Pure view code; consumes `Palette`/Kit only.
- `src/activity/anim.rs` — the per-bubble animation clocks (`Tween`s) + the reduce-motion gate
  (§6). Keyed by `(run_id, step)` so bubbles animate independently.

Wiring into the existing app:
- **State:** add `activity: ActivityState` to `RadialState` (only populated in chat_window_mode).
- **Demux:** replace the flat mapping at `agent_events.rs:192` (`AgentdInner::Activity(
  "{variant}{step}")`) with a structured parse → `Message::RunEvent(RunEventView)` carrying
  `{run_id, variant, step, agent, message, success, artifact, summary, steps, flow_id, ts}`.
  `update` calls `state.activity.apply_run_event(view)`.
- **View:** in `chat_window_view`, wrap the chat in a `Stack` and overlay `dock_view` in the
  configured corner (a `container` aligned bottom-right, padding clear of the composer).
- **Messages:** new variants — `RunEvent(RunEventView)`, `ActivityExpand(run_id)`,
  `ActivityCollapse`, `BubblePeekToggle(run_id, step)`, `BubbleDismiss(run_id, step)`,
  `RunCancel(run_id)`, `RunRetry(run_id)`, `RunOpenArtifact(path)`, `RunTranscript(run_id)`.
  All no-op when `!chat_window_mode`.
- **Tick:** the chat window's animation tick already gates on activity (perf fix from the
  chat-window slice). Extend the `needs_tick` predicate to also fire while any cluster has a
  *working* bubble (and for the bounded duration of one-shot `burst`/`bump`/`popin`), so
  idle CPU stays low and the dock animates only while runs are live.

### Lifecycle — the "recent tray" (user-chosen)
A finished/failed/cancelled cluster stays a live bubble briefly (flip to done/failed +
burst), then after **~6s** slides into a compact **recent tray** (a small collapsed affordance
that still expands to the run's final bubbles + Transcript/Open-artifact). Dismiss removes it
immediately. The tray caps at the last ~5 runs; older ones drop (logged, not silently).

---

## §4 · Backend changes (summary)

Only one, in `agentd/src/run_bridge.rs`: **stamp `run_id` on every `run`-kind event** (§2).
Add/extend a bridge test asserting `payload.run_id` is non-empty for `TaskStarted` /
`AgentMessage` / `TaskFinished` (today's test at `run_bridge.rs:248` asserts they emit, but
not the run_id). Build host-side (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target`, per Rule 3).

`cancel_run` / `retry` reuse existing agentd run controls if present; if `cancel_run` is not
yet exposed on the bus, the Cancel chip calls whatever the truthful-runs slice exposed
(`run_status`/`list_runs` are confirmed; the implementer verifies the cancel path and, if
absent, the Cancel chip is disabled with a tooltip rather than lying — Rule 1).

---

## §5 · Theming (the alignment the user asked for)

The "align coloring to Claude Design" ask is satisfied **structurally**: the design system
*is* `oxidemx-widgets::Palette` (TOKENS.md). So:
- **Never hardcode hex.** Every color is a `Palette` field (`crust`/`base`/`surface0-2`/
  `text`/`subtext0`/`accent`/`accent_15`/`accent_40`/`green`/`red`/slice colors). The bubbles
  match whatever theme is active.
- **Tone mapping:** agent archetype → slice color (table in §1). Status: working=tone,
  done=`green`/`success`, failed=`red`/`danger`.
- **Scales** (TOKENS.md): radii — orb is a pill (999), peek panel 16, log/control 6;
  spacing on the 4px grid; type — mono for names/logs/elapsed, Inter for the task line;
  elevation e1 (orb), e2 (peek). Focus ring = `accent_40`.

---

## §6 · Animations — CSS keyframes → iced

iced 0.14 has no CSS keyframes; rebuild with the overlay's `Tween` + the gated tick
(the standalone-chat-window slice established both). Per-bubble clocks live in
`activity/anim.rs`, keyed by `(run_id, step)`:
- **pulse** (glow ring opacity 1→.3→1) + **bob** (translateY ±5px) + **blink** (cursor) —
  continuous sine/triangle `Tween`s driven by the tick; cheap.
- **spin** (activity arc) — a rotating dash arc; draw as an iced `Canvas` rotated by a
  monotonically advancing angle, OR (preferred, matches the existing shader stack) a tiny
  fragment in the status-shader family. Implementer picks; Canvas is the low-risk default.
- **popin / peekin / tailin / bump / burst** — one-shot, time-boxed: a `Tween` that runs once
  on mount/change then holds at rest (the design's `MountIn` pattern — apply for one cycle,
  then drop so re-renders don't re-pin `opacity:0`). `burst` may simplify to a single
  expanding ring or be dropped if it fights the tick budget.
- **Reduce-motion:** a single flag (config or `prefers-reduced-motion` if available) disables
  all of the above (static orbs, no tick escalation). Match the design's `rm` behavior.

Match the *intent* faithfully; pixel-exact keyframe parity is not required (noted to the user).

---

## §7 · Scope (v1)

**IN:** the floating corner cluster (collapsed cluster-orbs ↔ expanded header-pill + step
bubbles), per-step bubbles with working/done/failed states + tone + unread badge + dismiss,
the peek popover (header, progress, **live log tail from AgentMessage**, Cancel/Transcript/
Open-artifact/Retry/Dismiss actions), the recent-tray lifecycle, animations per §6, the
`run_id` backend fix, full `Palette` theming. Chat-window-only (`chat_window_mode`).

**OUT (deferred):** inline approval (→ §10 unified approval feature), spawn "+" button,
tweaks panel, per-tool-call log granularity beyond what `AgentMessage` already provides,
multi-run *merged* clustering beyond per-run orbs.

---

## §8 · Testing

**Unit (pure, host-side):**
- `apply_run_event` reducer: a scripted event sequence (RunStarted→TaskAssigned→TaskStarted→
  AgentMessage×N→TaskFinished, then RunFinished) produces the expected cluster: bubble states,
  tones, log tails, unread counts, progress, status. Edge cases: out-of-order events, unknown
  step id, TaskError/StepSkipped, RunFailed/RunCancelled, ApprovalRequested-ignored.
- `AgentTone` mapping: every archetype → its slice color + icon; unknown agent → accent/default.
- unread logic: increments while peek closed, resets to 0 on open.
- `run_bridge` (agentd): `run_id` non-empty on step events.

**GUI-verified (by Jim — can't headless-test):** the visuals, the corner placement clear of
the composer, animation feel, collapse-on-click-away, the recent-tray slide, theme match
against the Claude-design reference.

---

## §9 · Open decisions (resolve in plan, none block)

1. **Corner default** — bottom-right (above the composer). Confirmable later; make it a const.
2. **Tie bubbles to the launching chat turn?** v1: clusters are window-global (a corner dock),
   not anchored to a specific chat message. Simpler + matches the design. (A future "jump to
   the message that launched this run" is a nice-to-have.)
3. **Transcript action** — v1 posts the run's `handoff_markdown` (or collected event log if
   empty) into the chat thread as an assistant message, reusing existing markdown rendering.
4. **Open artifact** — `xdg-open` the path. If multiple `RunFinished.artifacts`, open the first
   / show a tiny list. v1: first artifact.
5. **Cancel/Retry availability** — gated on the real agentd capability (Rule 1: disable, don't
   fake, if the bus path is absent).

---

## §10 · Deferred companion feature — unified approvals (separate spec)

Per the user: inline bubble approvals are not the right model. Instead, a **unified approval
system**: when any agent/run needs off-allowlist approval (`ApprovalRequested` /
`GatedToolExecutor`), raise a **desktop notification**, and surface **pending approvals in a
view shown on radial-menu open** (a Claude-design mockup exists). This is connector-agnostic
(decision in core per Rule 0: `ApprovalClassifier`/`GatedToolExecutor`; rendering per
connector). Gets its own brainstorm → spec when prioritized. The bubbles here deliberately
do **not** handle approvals so the two systems don't diverge.
