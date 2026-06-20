# Idea — Background-agent activity surface in the chat (user request, 2026-06-20)

Captured from the SP1c GUI walkthrough. After launching a conductor flow ("Rust Agent
Self-Diagnoser") from chat, the agent only said "still running, I'll summarize when
done" — no live visibility. The user wants a richer surface for background + parallel
agent work.

## The vision (user's words, distilled)
- **Floating activity bubbles at the TOP of the chat window** for each background /
  parallel agent run that's in flight.
- A **running/thinking animation** on each (so you can see it's actively working).
- An **unread badge** per bubble — count of new output messages since the user last
  viewed that run.
- **Ability to expand a bubble and view all the output** from that background or
  parallel agent inside the chat (its events/steps/partial results), not just a final
  summary.

## Why it fits
The data already flows: a conductor run emits `RunEvent`s (RunStarted / TaskStarted /
AgentMessage / TaskFinished / RunFinished) → agentd's `RunEventBridge` → the `event`
D-Bus signal (`payload.kind == "run"`) → the overlay's `agent_events` demux (today it
maps them to inline `Activity` text). So this is primarily a **presentation/UX layer**
over events the overlay already receives — plus the per-run grouping + unread tracking.
It pairs naturally with **SP2d-3** (the autonomous run lifecycle: RunTask / TaskStatus /
ResumeTask), which will produce *more* concurrent runs worth surfacing this way.

## Rough shape (to design properly later — likely with the visual companion)
- A `runs: HashMap<run_id, RunView>` in the overlay app state, fed by the `run`-kind
  events (group by `run_id`); each `RunView` holds status (running/done/failed/blocked),
  the event log, and `unread_since_viewed`.
- A floating top strip of pill/bubble widgets (iced) — one per active run — with a
  spinner/pulse animation while `running` and a count badge; click → expand a panel
  showing that run's event timeline; viewing clears the unread count.
- Terminal runs (done/failed) collapse or move to a "recent" affordance.

## Status
Idea captured; not yet brainstormed/specced. Sequencing TBD with the user — natural as a
UI slice paired with or right after SP2d-3 (which drives the runs it visualizes).
