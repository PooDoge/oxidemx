# Direction — Connector modularization (multi-channel chat I/O) — user request 2026-06-20

The chat is currently bound to ONE front-end: the iced overlay, talking to agentd over
D-Bus (`org.oxidemx.Agent`: `send_message` + the `event` signal). The user wants the
agent reachable through **pluggable connectors** — Telegram, Discord, an **HTTP API**
(handy for an external tool like Claude Code to drive/troubleshoot the agent), etc. —
so the overlay becomes *one connector among many*.

## The principle
The agent core (turn loop, tools, runs, sessions, truthfulness) must be **transport-
agnostic**. A `Connector` is an adapter that (inbound) turns a channel-specific event
into a normalized request to the agent, and (outbound) turns the agent's normalized
events (deltas, tool/run events, final, approval requests) back into channel-specific
output. agentd hosts N connectors over the one agent core.

## Where it lands in our architecture
- agentd already separates the agent core (`AgentService`/`CoreTurnRunner`/sessions)
  from its **one** transport (the D-Bus `#[interface]`). That D-Bus surface is, in
  effect, "the overlay connector." Generalizing means: extract a `Connector` seam, make
  the D-Bus/overlay path one impl, and add HTTP / Telegram / Discord impls.
- A **normalized message/event model** (sender, text, attachments, session/thread id,
  channel capabilities like can-stream / can-render-card) decouples channel payloads
  from the core — the same `send_message`/`event` vocabulary we already have, lifted to
  a channel-neutral type.
- **Capability degradation:** a connector declares what it can do (stream tokens? render
  an approval card? show run bubbles?). Streaming/cards degrade gracefully on channels
  that can't (Telegram → post the final + a text "approve? reply yes/no"; HTTP → SSE).
- **Approvals across channels:** the SP1b `Approver` card must round-trip over whatever
  connector the turn arrived on (overlay card / Telegram inline buttons / HTTP poll).

## HTTP connector (high value, do early)
A small REST + SSE surface (the LangGraph/agent-protocol "threads + runs + messages +
stream" shape) so external tools — notably **Claude Code** — can send a message, stream
the reply, launch/inspect runs, and answer approvals. This doubles as a troubleshooting
and automation surface for the whole agent.

## Status / sequencing
Direction captured; **full design pending the connector research**
(`docs/research/connector-architecture.md` + `hermes-agent-architecture.md`, in flight).
This becomes its own brainstorm → spec → plan slice ("SP-Connectors"). It does NOT block
the truthful-background-runs slice — that slice's BACKEND (RunLauncher + status tools +
truthfulness) is connector-agnostic by construction; only its bubble UI is overlay-
specific (= the overlay connector's presentation). Naming for the seam (Connector vs
Channel vs Client vs Adapter) to be set from the research's field-convention findings.
