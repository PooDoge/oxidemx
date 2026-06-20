# Standalone chat window (sibling binary, in-place reuse) — design

Date: 2026-06-20 (revised same day after a full chat-surface inventory)
Status: design (approved in-chat — Option D). Supersedes the earlier "extract a shared
`oxidemx-chat-ui` crate" plan in this doc's history: the inventory showed the chat is fused
into the overlay (~38 `ai_*` fields in `RadialState`, ~60 of ~75 `Message` variants, ~840
lines of chat `update` arms, all `chat_ui/` views take `&RadialState`, and `ai_morph` /
`chat_focus_pending` / `ai_handoff` are shared with the radial canvas). A clean cross-crate
extraction is a multi-day, high-risk refactor. So we ship the reliable window FIRST by
reusing the overlay's chat **in place**, and defer the extraction (see "Incremental path").

## Problem

The AI chat lives only inside the radial overlay, whose window is frameless / transparent /
always-on-top / `override_redirect` (`oxidemx-window::frameless_topmost`) and positioned by a
GNOME-shell extension — and it currently **won't reliably open** on this Wayland session.
The chat code itself works. So: give the chat its own **normal toplevel window** (system
decorations, normal stacking/focus) via a **sibling binary** that reuses the overlay's chat
in place, launchable from the app menu / a hotkey / the MX button, single-instance.

## Approach: Option D — sibling binary `oxidemx-chat` + a `run_chat_window()` entry

No CLI flag on the overlay (the chat window is **always available as its own binary**). The
overlay crate (`oxidemx-overlay`, dir `overlay-rs`) gains a second binary and one new public
entry point; everything else (state, `chat_ui`, `update`, `agent_events`) is reused unchanged.

### 1. `oxidemx-overlay::run_chat_window()` (new lib entry, mirrors `app::run`)

A sibling of `app::run()` (`overlay-rs/src/app/mod.rs:312`). Differences only:
- **Window:** plain `iced::window::Settings` (decorations on, resizable, `min_size`,
  `application_id = "org.oxidemx.Chat"`, sane default size) instead of
  `oxidemx_window::frameless_topmost(...)`. No transparent/topmost/override_redirect, no
  cursor-helper positioning.
- **Boot:** sets a new `RadialState` field `chat_window_mode: bool = true` (the ONLY state
  addition). Boots the same chat state (threads load, agentd subscription) but does not arm
  the radial/puck/daemon-show path.
- **View:** when `chat_window_mode`, `view()` renders `chat_ui::view(&state, 1.0)` full-window
  and skips the radial canvas / disc / morph. (One branch at the top of the overlay `view()`.)
- **Update:** when `chat_window_mode`, the daemon radial triggers (`MenuRequested`/`HideMenu`
  from `dbus.rs`) and puck/handoff messages are ignored/no-ops; all `Ai*`/`Agentd*` chat
  messages flow through the existing `update.rs` arms unchanged.

### 2. `oxidemx-chat` binary

A `[[bin]]` in the overlay crate (`overlay-rs/src/bin/oxidemx-chat.rs`) whose `main()` calls
`oxidemx_overlay::run_chat_window()`. **Single-instance:** before running, try to own D-Bus
name `org.oxidemx.Chat`; if already owned, call a `Present` method on the running instance
(which raises/focuses via `iced::window::gain_focus`) and exit. If claiming the name fails for
any other reason, fall back to opening a plain window anyway (never exit silently). The
present-request reaches the running app via a small D-Bus listener wired into the chat-window
subscription.

### 3. Launch / trigger glue

- **`.desktop`** (app menu) → `oxidemx-chat`; added to `install.sh` + a desktop entry.
- **GNOME custom keybinding** → `oxidemx-chat` (user-configured; documented in install output).
- **Daemon `ShowChat`** — `oxidemxd` gains a D-Bus signal `ShowChat` (bind an MX button/gesture
  to emit it); `oxidemx-chat`'s single-instance listener presents on it. Small daemon addition.
- All three funnel through the single-instance "open-or-present" path.

## Data flow

Identical to the overlay's chat today (Option D reuses it): user input → `AiSubmitPrompt` →
`update.rs` → (agentd path) `ai_client::ask_ai_remote` → `AgentProxy.send_message` → agentd
streams via the `event` signal → `agent_events::stream()` → `AgentdEvent`/`AiStream` →
`update.rs` appends the assistant bubble; `run_flow`/`run_status` → run cards (the truthful-runs
work). The chat window is agentd-path-focused (`use_agentd=true`), but since it reuses the
shared `update.rs`, the in-proc path remains available until SP1c-T8b deletes it.

## Error handling

- agentd absent / `send_message` D-Bus error → existing error bubble + retry (unchanged).
- Single-instance race / name-claim failure → open a plain (non-unique) window rather than
  exit, so the user always gets a window.

## Testing

- **`run_chat_window` boot:** a headless-feasible check that the chat-window mode boots with
  normal window settings + `chat_window_mode=true` and does not arm the radial path (assert the
  flag + that no `frameless_topmost` is used). Where iced makes a full headless run infeasible,
  cover the mode-branch logic (view selects chat-only; daemon triggers are no-ops in mode) with
  small unit tests on the pure helpers.
- **Single-instance:** a "second launch presents, doesn't duplicate" check via the
  `org.oxidemx.Chat` name path.
- **Overlay regression:** existing overlay tests pass unchanged (D adds a field + branches, does
  not move chat code).
- Backend already covered by `scripts/agentd-smoke.sh`.

## Scope / phasing

1. `run_chat_window()` entry + the `chat_window_mode` field + the view/update branches +
   normal window settings.
2. `oxidemx-chat` binary + single-instance (`org.oxidemx.Chat`, present-on-relaunch).
3. Launch glue: daemon `ShowChat`, `.desktop`, keybinding doc, `install.sh` (binary + entry).

Builds via distrobox (the overlay needs GTK/iced system libs).

## Incremental path to the deferred clean extraction (NOT in this slice)

The clean `oxidemx-chat-ui` crate extraction is deferred (multi-day, high-risk). Two low-risk,
compiler-guided, same-crate refactors get us most of the way there and are queued as the next
slice after this window ships:
- **`ChatState` sub-struct:** move the ~38 `ai_*` fields into a `chat: ChatState` field on
  `RadialState`, leaving the 3 radial-shared fields (`ai_morph`, `chat_focus_pending`,
  `ai_handoff`) behind. This does the hard state-split in place; the eventual cross-crate move
  becomes a near-mechanical lift-and-shift.
- **`app/chat_update.rs`:** move the chat `update` arms (≈`update.rs:675-1630`) into their own
  module (pure code move) to isolate the 840 lines.
Neither is required for the window; both shrink the future extraction's risk.

## Risks

- **Mode-branch leakage:** the `chat_window_mode` branches in `view`/`update` must be
  exhaustive enough that no radial/daemon path runs in the chat window. Mitigation: keep the
  branch at the top of `view()` and guard the daemon/puck/handoff arms in `update()`.
- **iced state-by-tag reset** ([[feedback_iced_tree_state_by_tag]]): keep the chat-window root
  widget type constant; reuse `chat_ui::view`'s existing structure.
