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

## Locked naming + seam (from research 2026-06-20)

Research done: `docs/research/connector-architecture.md` (elizaOS/Goose/LangGraph/Letta/
OpenHands/Rig survey) + `docs/research/hermes-agent-architecture.md` (Nous Hermes).
Hermes uses **Gateway** (host process) + per-channel **adapters**; the survey found
Service/Extension/Client/(unnamed) and recommends **Connector**. Decisions:

- **agentd = the Gateway/host** — owns a **connector registry**, routing, and session
  continuity; the agent core (`AgentService`/`CoreTurnRunner`) stays transport-agnostic
  and is reused unchanged across connectors (exactly Hermes's one-`AIAgent`-everywhere).
- **`Connector`** = the per-channel seam (the user's term; composes: `OverlayConnector`,
  `TelegramConnector`, `HttpConnector`):
  ```rust
  trait Connector: Send + Sync {
      fn name(&self) -> &str;
      fn caps(&self) -> ConnectorCaps;                              // can_stream, can_card, …
      async fn start(&self, tx: mpsc::Sender<InboundEvent>) -> Result<()>;  // own poll/server loop
      async fn send(&self, reply: OutboundMessage) -> Result<()>;          // route reply back
      async fn stop(&self) -> Result<()>;
  }
  ```
- **Normalized inbound:** `InboundEvent { session_id, connector: String, sender: SenderInfo,
  content: EventContent, capabilities: ConnectorCaps }` — `caps.can_stream` decides
  stream-vs-buffered reply (degradation, e.g. Telegram → buffered).
- **Outbound:** `OutboundMessage` (delta / final / tool-or-run event / approval-request).
- **HTTP connector shape** (agent-protocol / LangGraph convergent): `POST /conversations`
  → session; `POST /conversations/{id}/messages/stream` → SSE (`token`/`updates`/`done`,
  reconnect via `Last-Event-ID`); `POST /conversations/{id}/messages` → blocking. Minimum
  for Claude Code to drive + troubleshoot the agent.
- **Approvals:** Hermes confirms the decision stays in core (our `ApprovalClassifier` +
  `GatedToolExecutor`) while the *prompt rendering* is per-channel — which is exactly our
  existing `ApproverPrompt` seam. The overlay renders a card; Telegram renders inline
  buttons; HTTP exposes an approve endpoint. No core rework.
- **OverlayConnector migration:** wrap today's D-Bus `send_message` → `InboundEvent` and
  the `event` signal → `OutboundMessage::send`, fixed session UUID. The registry replaces
  the current direct D-Bus loop; no other agentd logic changes.

## Status / sequencing
Naming + seam **locked** (above). This becomes its own brainstorm → spec → plan slice
("SP-Connectors"), post the truthful-runs slice. It does NOT block truthful-runs — that
slice's BACKEND (RunLauncher + status tools + truthfulness) is connector-agnostic by
construction; only its bubble UI is the OverlayConnector's presentation.
