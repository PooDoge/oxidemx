# Freya App 2a — Transport Client + Navigable Shell Skeleton — Design

**Status:** Approved for planning (2026-06-22).
**Workspace:** NEW separate workspace `oxide-app/` (own target/lockfile, isolated from the phase1 agentd/overlay/iced workspace). Program docs stay centralized under `oxidemx-phase1/docs/superpowers/`.
**Program context:** First slice of **sub-project 2** (the Freya chat app) of the AI-chat rewrite program.
Sub-project 2 decomposition:
- **2a — transport client + navigable shell skeleton** (this doc): the vertical slice — Freya desktop app talks to the live local agentd over the UDS and streams a real reply, on a reusable-component + per-region-router foundation.
- **2b — full chat UX parity** (artifact cards, the 4 right-rail status "directions", editor, `.oxide` settings, MCP fork, activity bubbles, palette) per the design.
- **2c — Android + remote transport** (`freya-android`, tailnet-TCP + bearer pairing, mobile layout).
- (later) **2d — high-security control plane** (AI settings / skill+flow mgmt / conductor viz) over UDS control routes.
Related memory: [[project_ai_chat_rewrite]], [[reference_design_system]], [[project_connector_architecture]], [[feedback_best_practices_rule]].

**Goal:** Stand up `oxide-app/` (workspace) with `oxide-client` (transport), `oxide-ui` (reusable component library + tokens), and `oxide-freya` (the app) — a minimal Freya **desktop** app that connects to the **live local agentd over the UDS**, renders the design's 3-collapsible-panel shell as independently-navigable routed regions with animated transitions, and streams a real assistant reply end-to-end.

**Architecture:** Three crates. `oxide-client` is transport-only (a `Transport` trait + `UdsTransport`, async agent-protocol methods + an SSE event `Stream` with `Last-Event-ID` reconnect; loose JSON coupling to agentd). `oxide-ui` is a reusable Freya component library (design tokens from the design system + reusable primitives + animated wrappers). `oxide-freya` composes them: a window shell whose left/center/right regions are each an independently-routed, animated-transition navigation outlet, so a sidebar can load a different page without disturbing the active center prompt.

**Tech Stack:** Rust, Freya v0.4.0-rc.23 (cloned `/run/media/system/fastdrive/repos/freya`) — `freya`, `freya-components`, `freya-router` (`Router`/`Outlet`/`use_animated_router`), `freya-animation`, `freya-query` (optional, for transport data hooks), `freya-testing`; `tokio`; `hyper`/`reqwest` (UDS) + `serde`/`serde_json`. Skia via `freya-engine`/`skia-bindings` (build-env TBD — see Open Items).

---

## Global Constraints (bind every task)

- **Rule 0 — field-standard naming.** Transport DTOs mirror the agent-protocol wire shapes 1b ships (`conversations`/`messages`/`events`, the `AgentEvent` `kind`s). UI uses Freya/field-standard component naming (`Outlet`, `Router`, component-per-file). Reusable design-system terms follow `design_handoff_ai_chat_ui/{TOKENS.md,COMPONENTS.md}`.
- **Rule 1 — truthfulness is structural.** The thread renders ONLY transport-delivered content — no optimistic/fabricated assistant text. A sent user turn shows immediately (it IS the user's action); the assistant reply appears only as `delta`/`final` events arrive over SSE. Connection/stream state is shown truthfully (connected / reconnecting / unreachable).
- **Rule 2 — Rust + component quality.** clippy-clean, hand-formatted; trait seam (`Arc<dyn Transport>`) so the UI is testable against a mock; newtypes for ids; `thiserror` errors; **reusable components over copy-paste** (every repeated UI element becomes an `oxide-ui` component with a clear prop interface); no gold-plating (build the seams + the vertical slice, not every 2b component).
- **Rule 3 — process + builds.** `oxide-app/` builds with its own toolchain/target; **resolve the Freya/Skia desktop build recipe in Task 1 before app code** (host vs distrobox vs prebuilt Skia). Isolate work in a git worktree. Brainstorm→spec→plan→subagent-driven build.
- **Foundation-now invariants (the no-refactor insurance, per the 1b lesson):** the **per-region navigation model** and the **reusable-component library boundary** are built in 2a even though 2a ships few pages/components — they are expensive to retrofit. 2a proves them with a minimal real page set; 2b fills pages/components into the existing seams.

---

## Crate 1 — `oxide-client` (transport)

Transport-only; no UI deps. The same crate serves desktop (UDS, 2a) and Android (tailnet-TCP+bearer, 2c) behind one trait.

```rust
#[async_trait]
pub trait Transport: Send + Sync {
    async fn health(&self) -> Result<(), TransportError>;
    async fn list_projects(&self) -> Result<Vec<Project>, TransportError>;
    async fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>, TransportError>;
    async fn create_conversation(&self, project_id: &str, working_dir: Option<&str>) -> Result<Conversation, TransportError>;
    async fn get_history(&self, conversation_id: &str) -> Result<Vec<Turn>, TransportError>;
    async fn send_message(&self, conversation_id: &str, text: &str) -> Result<MessageId, TransportError>;
    /// SSE subscription. Yields normalized events; reconnects with Last-Event-ID on drop.
    fn subscribe(&self, conversation_id: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>>;
}
```
- **`UdsTransport`** — hyper client over a `tokio::net::UnixStream` to `$XDG_RUNTIME_DIR/oxidemx/agentd.sock`. JSON request/response; SSE parsed from the `/v1/conversations/{id}/events` body (incremental line parse: `id:`/`event:`/`data:`), tracking the last seq for reconnect.
- **DTOs** — `Project { id, name, default_working_dir }`, `Conversation { id, project_id, title, working_dir, model, updated_at, … }`, `Turn { role, text, ts }`, `AgentEvent { kind, seq, payload }` with `kind ∈ {delta, final, error, tool_call, run-event kinds, ApprovalRequest, model_status}` (mirrors 1b). Unknown kinds are preserved, not dropped (forward-compat).
- **Errors** — `TransportError` (`thiserror`): `Unreachable` (socket absent/refused), `Http(status)`, `Decode`, `Stream`. Drives the UI's connection state.
- **Reconnect** — `subscribe` re-opens on stream error/EOF with the last-seen `Last-Event-ID`, bounded backoff; surfaces a `reconnecting` signal.

## Crate 2 — `oxide-ui` (reusable component library)

The design system as Freya components. Consumed by `oxide-freya` and (later) 2b/2c/2d.
- **`tokens`** — the palette/type/spacing from `design_handoff_ai_chat_ui/TOKENS.md` + the Collapsible-Panels theme (dark `#05070b`/`#f0f4f8`, Catppuccin-ish `subtext0`/`accent`, accent set cyan/purple/orange/green, Inter + JetBrains Mono). A `Theme` struct + accent switch, provided via context.
- **Reusable primitives** (each its own file, clear prop interface, `freya-testing`-covered): `Panel`/`CollapsiblePanel` (full↔rail + drag handle), `RailButton`, `ListItem` (conversation/project row), `Bubble` (message), `Input` (prompt box), `StatusDot`/`Ring`, `Surface`/`Card`, `IconButton` (+ `oxide-ui` re-exports `freya-icons` set). **Animated wrappers** built on `freya-animation` (`FadeIn`, `SlideIn`, the page-transition shell) so transitions are reused, not re-coded.
- 2a builds only the primitives the vertical slice needs; the crate's module layout reserves the slots 2b fills.

## Crate 3 — `oxide-freya` (the app)

- **Shell** — `WindowFrame` (frameless default per design; native optional) hosting a horizontal layout of three **navigable regions**: left `Sidebar`, center `Main`, right `Context`.
- **Per-region navigation (the headline architecture):** each region mounts its **own** `Router<RegionRoute>` so they navigate **independently** with animated transitions (`use_animated_router`). Left region routes ∈ {`Conversations` (project+conversation list, the default), a placeholder second page to prove independent nav}; Center routes ∈ {`Chat` (the active thread — stays mounted/alive while sidebars navigate), a placeholder page to prove window transitions}; Right region routes ∈ {`StatusRail` placeholder}. Switching a sidebar page does NOT remount the center `Chat`.
- **Collapse** — left + right `CollapsiblePanel` toggle full↔rail (basic toggle in 2a; the velocity-snap drag-handle spring physics is 2b polish — the component interface supports both).
- **Wiring** — the app holds an `Arc<dyn Transport>`; a small `AppState`/signals layer bridges async transport calls + the SSE stream into Freya signals that the `Chat` view renders. Send → `send_message`; the reply streams via the conversation subscription into the thread.

---

## Data flow (exercises the live 1b contract)

Launch → `health` (unreachable ⇒ "can't reach agentd" + retry) → `list_projects` → `list_conversations(personal)` → render left `Conversations`. Select a conversation → `get_history` → render center `Chat` + open `subscribe(conversation_id)`. Type + send → `send_message` (append the user turn immediately) → assistant `delta`s stream into the live bubble → `final` commits it. Independent nav: navigating the left region to its placeholder page (or collapsing it) leaves the center `Chat` + its live subscription untouched.

## Error handling

`Unreachable`/stream drop → the shell shows a truthful banner (`reconnecting…` / `agentd unavailable — retry`) sourced from `TransportError`/the reconnect signal; SSE auto-reconnects with `Last-Event-ID`; no fabricated content. Transport errors never panic the UI (they resolve to a state).

## Testing

- **`oxide-client`** — trait-seam unit tests + a `MockTransport`; **a live integration test against the running local agentd** (health → list_projects has `personal` → create_conversation → send_message → subscribe receives the turn's `final`), mirroring the verified curl walk in Rust. SSE parse + `Last-Event-ID` reconnect unit-tested against a canned byte stream.
- **`oxide-ui`** — `freya-testing` render tests for each primitive (renders, prop variants, the collapse toggle, an animated wrapper mounts).
- **`oxide-freya`** — `freya-testing` smoke: the shell mounts the three regions; a sidebar route change does not remount the center `Chat` (asserts the independent-nav invariant); against a `MockTransport`, sending a message renders the streamed reply.

## Open items (resolved early in the plan, not deferred silently)

- **Task 1 — Freya desktop build recipe.** Determine how `skia-bindings`/`freya-engine` build on this atomic-Fedora box (host with system Skia? distrobox? prebuilt download?). Task 1's deliverable is a building "hello Freya window" before any app logic.
- **Task 1/2 — multi-router feasibility spike.** Validate that multiple independent `Router<R>` instances can coexist in sibling subtrees (per-region nav). Freya exposes nested `Outlet`s + `use_animated_router` + `use_share_router`/`create_global`; if independent parallel routers are NOT supported, the documented fallback is a per-region page-state enum animated directly with `freya-animation` (same UX, no router dependency). Pick the mechanism in Task 2 and record why.

## Out of scope (later slices)

- 2b: artifact cards, activity bubbles, the 4 right-rail status directions, the editor overlay, `.oxide` settings nav, MCP fork, command palette, markdown polish, drag-handle spring physics, real second/third pages per region.
- 2c: Android (`freya-android`), tailnet-TCP + bearer pairing, mobile layout.
- 2d: the high-security control plane.
- Webview (X11-only) stays deferred (external browser).
