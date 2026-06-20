# Standalone chat window + shared chat-ui crate — design

Date: 2026-06-20
Status: design (brainstormed + approved in-chat; pending spec review → writing-plans).

## Problem

The AI chat lives only inside the radial overlay (`oxidemx-overlay`), a frameless /
transparent / always-on-top / `override_redirect` iced window positioned by a GNOME-shell
extension. That window is fragile and currently **won't reliably open** on this Wayland
session (it shows then vanishes / never appears). Rather than debug the overlay's
windowing, give the chat its **own normal toplevel window** — system decorations, normal
stacking/focus — that opens reliably from the app menu, a hotkey, or the MX button, and
**reuses the exact same agentd surfaces and chat UI**. Decision recap (in-chat): extract a
**shared chat crate** (both overlay and the new app consume it — no drift); **agentd-only**
transport; launch via **menu + GNOME keybinding + daemon trigger**, single-instance.

## Current structure (from recon)

- Overlay windowing is already plain **iced xdg-shell** (no layer-shell); fragility comes
  from `oxidemx-window::frameless_topmost` (`decorations:false, transparent, AlwaysOnTop,
  override_redirect:true`) + cursor-helper positioning (`oxidemx-window/src/cursor_helper.rs`).
- Chat UI is **baked into the overlay**: `overlay-rs/src/chat_ui/` (`mod.rs` entry
  `view(&RadialState, alpha) -> Element<Message>` ~`mod.rs:50`; submodules `body/cards/footer/
  header/memories/palette/skills/tasks/threads`). State lives in `RadialState`
  (`overlay-rs/src/radial/mod.rs:49`): `ai_threads: Vec<ChatThread>`, `ai_loading`,
  `ai_activity`, `ai_palette`, etc. Chat *update* logic is woven into the overlay's `Message`
  enum (`app/mod.rs`) + `app/update.rs` (`Ai*`/`Agentd*` arms; `AgentProxy::new` at
  `update.rs:1597`). Signal subscription: `app/agent_events.rs::stream()`.
- **Reusable as-is:** `oxidemx-agent-proxy` (zbus `AgentProxy` for `org.oxidemx.Agent`:
  `send_message`, `run_flow`, `run_status`, `list_threads`, `get_transcript`,
  `respond_approval`, …; signals `event`/`approval_requested`/`model_status_changed`; zero
  overlay deps), `oxidemx-widgets` (tokens/Kit/catalog/icons), `oxidemx-agent-core`
  (`StreamEvent`, `AgentCardData`, `PendingQuestion`, `AgentMode`).

## Components

### 1. `oxidemx-chat-ui` (new shared crate)

The chat, extracted. Owns:
- **`ChatState`** — the AI fields lifted from `RadialState`: `threads: Vec<ChatThread>`,
  current-thread index, input buffer, `loading`/`activity` flags, and the panel state for
  `palette`/`skills`/`tasks`/`memories`/`threads`. (`ChatThread` and the panel types move
  here or to a shared location they already permit.)
- **`ChatMessage`** — the `Ai*`/`Agentd*` variants trimmed out of the overlay `Message`.
- **`pub fn view(state: &ChatState, alpha: f32) -> Element<'_, ChatMessage>`** — the moved
  `chat_ui/` view, signature changed `&RadialState` → `&ChatState`.
- **`pub fn update(state: &mut ChatState, msg: ChatMessage) -> Task<ChatMessage>`** — the
  chat-handling logic lifted from `update.rs`: send via `AgentProxy.send_message`; apply
  `event`-signal stream deltas; `run_flow`/`run_status` cards; approvals via
  `respond_approval`.
- **`pub fn subscription(state: &ChatState) -> Subscription<ChatMessage>`** — the moved
  `agent_events::stream()` (agentd `event`/`approval_requested`/`model_status_changed`).
- **Deps:** `oxidemx-agent-proxy`, `oxidemx-widgets`, `oxidemx-agent-core`, `iced`. **No**
  `oxidemx-agent` (in-proc) — agentd-only.
- **Boundary:** everything in `chat_ui/` + the `ai_*` state + agentd signal plumbing moves
  here. `radial/`, `render/` (shaders), `chat_shell.rs` (puck/morph), `handoff.rs`,
  daemon `dbus.rs`, `widget_host.rs`, `tray.rs`, cursor positioning **stay in the overlay**.

### 2. `oxidemx-chat` (new binary)

A thin iced **normal-window** app:
- `iced::application(...)` with default-ish `window::Settings` (decorations on, resizable,
  sane `min_size`, `application_id = "org.oxidemx.Chat"`). No frameless/transparent/topmost/
  override_redirect, no cursor-helper. This is the whole reason it opens reliably.
- `App { chat: ChatState }`; `update` → `chat_ui::update`; `view` → `chat_ui::view`;
  `subscription` → `chat_ui::subscription` + a present-request channel.
- **Single-instance:** at startup, attempt to own D-Bus name `org.oxidemx.Chat`. If already
  owned, call a `Present` method on the running instance and exit; the running instance
  raises/focuses (`iced::window::gain_focus`). Menu + keybinding + daemon-trigger all become
  "open-or-present."

### 3. Overlay rewire

`RadialState` gains `chat: ChatState`; the overlay `Message` keeps a `Chat(ChatMessage)`
wrapper; `update.rs` routes those to `chat_ui::update`; the chat-shell `view` calls
`chat_ui::view(&state.chat, alpha)`. The overlay keeps its puck/handoff shell but the chat
content+logic is the shared crate. Overlay drops in-proc `oxidemx-agent` chat calls
(agentd-only — finishing SP1c-T8b for the chat path). The overlay's non-chat behavior is
unchanged.

### 4. Launch / trigger

- **`.desktop`** entry (app menu) → `oxidemx-chat` (added to `install.sh`).
- **GNOME custom keybinding** → `oxidemx-chat` (user-configured; documented).
- **Daemon `ShowChat`** — `oxidemxd` gains a D-Bus signal (or method) `ShowChat`; bind an MX
  button/gesture to emit it; `oxidemx-chat` subscribes and presents. Small daemon addition.
- All three funnel through the single-instance open-or-present path.

## Data flow

User input → `ChatMessage::Send` → `chat_ui::update` appends optimistic user bubble + spawns
`AgentProxy.send_message` task → agentd streams via the `event` signal → `chat_ui::subscription`
forwards deltas as `ChatMessage` → `update` appends/extends the assistant bubble; `run_flow`
tool → a run card; `run_status` → ground-truth status (the truthful-runs work). Identical
semantics in the overlay and the standalone window because both call the same crate.

## Error handling

- agentd absent / `send_message` D-Bus error → an error bubble ("agentd unavailable") +
  retry, exactly as today. The systemd unit means agentd is normally up; a transient restart
  surfaces the error rather than hanging.
- Single-instance race: if owning `org.oxidemx.Chat` fails, fall back to launching a normal
  (non-unique) window rather than exiting silently, so the user always gets a window.

## Testing

- **`oxidemx-chat-ui`:** unit-test `ChatState::update` transitions against a **mock
  `AgentProxy`/event source**: send → optimistic user bubble; `event` delta → assistant text
  appended; `run_flow` result → run card; unknown/`run_status` → ground-truth status; approval
  request → pending card. (First real unit coverage for chat logic, previously buried in
  `update.rs`.)
- **`oxidemx-chat`:** a launch + single-instance "second launch presents, doesn't duplicate"
  check (headless-feasible via the D-Bus name path).
- **Overlay:** existing overlay tests must still pass after the rewire (regression gate).
- Backend already covered by `scripts/agentd-smoke.sh`.

## Scope / phasing (for the plan)

Sizable slice — the extraction + overlay rewire is the bulk (chat is deeply woven into
`update.rs`). Phases, each independently testable:
1. **Extract + rewire:** create `oxidemx-chat-ui` by moving the chat out of the overlay;
   rewire the overlay to consume it; overlay still builds + its tests pass.
2. **Standalone app:** `oxidemx-chat` binary (normal window, single-instance) on top of the
   crate; opens reliably + chats via agentd.
3. **Launch glue:** daemon `ShowChat`, `.desktop`, keybinding doc, single-instance present;
   add binary + `.desktop` to `install.sh`.

Out of scope: the activity-UI run bubbles (separate slice), the install-GUI (separate slice),
debugging the radial overlay's own open bug (this sidesteps it; the overlay keeps working via
the shared crate but its frameless-window fragility is a separate, now-lower-priority issue).

## Risks

- **Extraction depth:** `RadialState` is entangled; pulling `ChatState` + the chat `update`
  arms cleanly is the main effort/risk. Mitigation: move incrementally, keep the overlay
  compiling at each step; the `chat_ui/` files are already separate.
- **iced state-by-tag reset** ([[feedback_iced_tree_state_by_tag]]): keep the standalone
  app's root widget type constant; reuse the chat view's existing structure.
