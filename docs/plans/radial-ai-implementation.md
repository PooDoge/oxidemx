# Radial pages, AI chat redesign & center-puck handoff — Implementation Plan

> Status: **awaiting approval** — no code written yet.
> Spec: `design/oxidemx-radial-ai/CLAUDE_CODE_PROMPT_RADIAL_AI.md` (contracts binding).
> Styling truth: `design/oxidemx-radial-ai/{radial,ai-chat}.jsx` + `screenshots/*.png`.

**Goal:** center puck survives the disc→chat morph and stays armed for page cycling; chat window becomes resizable/theme-shader-skinned with agent cards + memories; two new built-in radial pages (Device Settings, Splice Widgets).

**Architecture:** one new state machine (`AiHandoff`) gates input routing during/after the morph; the existing `ai_morph` tween drives puck travel via the same `cap_rects`-style lerp helpers; agent features ride the existing `StreamEvent`/`execute_local_tool` plumbing with one new `Card` event; new pages reuse `Slice`/`RadialPage`/submenu mechanics with additive serde-default fields so config round-trips losslessly.

**Tech stack facts (verified):** iced **0.14 from crates.io** (no fork), wgpu 27.0 + bytemuck 1.x pinned to iced_wgpu; zbus daemon at `org.oxidemx.Daemon`; Gemini v1beta Interactions API (`previous_interaction_id` sessions; built-in `google_search` cannot mix with custom functions → nested-interaction pattern already exists in `grounded_search()`).

---

## 1 · File-level diff map

### Modified

| File | Change |
|---|---|
| `oxidemx-shared/src/config.rs` | Add `OverlayConfig` section (`chat_size`), `AiConfig` (command allowlist), `Slice.widget: Option<WidgetConfig>`, `Slice.dial: Option<DialKind>` — all `#[serde(default)]` + `skip_serializing_if`, lossless round-trip |
| `oxidemx-shared/src/action.rs` | New `ActionKind` variants: `Widget`, `Dial`, `Power`, `NightLight`, `MouseSetting` (payloads stay in `Slice.command`/new fields, matching the existing `EasySwitch`-in-`command` idiom — deviation from the `SliceKind::Widget { source, format }` sketch, noted §4) |
| `oxidemx-shared/default-config.json` | Append two built-in pages (Device, Widgets) — fresh seeds only; existing configs untouched |
| `overlay-rs/src/radial.rs` | `RadialState.ai_handoff: AiHandoff`; arm/disarm transitions in `sync_ai_morph()`/`cycle_page()`; suppress `chat_focus_pending` while `PuckArmed`; widget sample state (`WidgetSamples` ring buffers); `ChatMessage.card: Option<AgentCardData>` |
| `overlay-rs/src/chat_shell.rs` | `puck_geom(t, win_w) -> (Point, f32)` lerp (disc center/45 px → header-left slot/16 px); `CapsPainter` draws the travelling puck + ring (lit when armed, dim stroke when active) + live page dots; wheel/click hit rules per handoff state; header redesign geometry (52 px header per `ai-chat.jsx`) |
| `overlay-rs/src/app.rs` | Handoff-aware wheel/click routing; `Message` variants for resize drag, memories view, card chips, thread chips; window resize request on morph (targets persisted `chat_size`); morph target geometry from config; gut `build_ai_panel()` → delegate to new `chat_ui` module |
| `overlay-rs/src/ai_client.rs` | `StreamEvent::Card(AgentCardData)` + dynamic `Activity(String)`; three tool families in `mode.tools()` (both modes; GeneralChat's built-in search switches to the existing nested-custom pattern); dispatch in `execute_local_tool()`; memory injection into `system_instruction()` |
| `overlay-rs/src/render/slices.rs` | Extract shared `draw_puck(...)` (dome + ring + dots) used by radial painter (t=0) and `CapsPainter` (t>0); widget wedge rendering (big value, sparkline polyline, sublabel, uppercase label per `radial.jsx` proportions); dial wedge (% readout) |
| `overlay-rs/src/actions.rs` | Dispatch for `Power` (logind/gnome-session), `NightLight` (gsettings), `Dial` set, `MouseSetting` (daemon D-Bus via `haptic_client`) |
| `overlay-rs/src/haptic_client.rs` | Add blocking wrappers: `set_dpi`, `set_wheel_mode`, `set_haptics_enabled`, `set_gaming_mode` (daemon methods already exist) |
| `overlay-rs/src/input.rs` | Center-zone + puck-circle hit helpers shared with handoff rules |

### Created

| File | Responsibility |
|---|---|
| `overlay-rs/src/handoff.rs` | `AiHandoff` enum + transition fns + unit tests (pure, no iced types beyond `Point`/`Instant`) |
| `overlay-rs/src/chat_ui/mod.rs` (+ `header.rs`, `threads.rs`, `cards.rs`, `footer.rs`, `memories.rs`) | The redesigned chat view, split out of `app.rs` — header (puck slot, title, status line, drag pill, brain/clock/+/× buttons), thread chip strip + flash-mode badge, three agent card types, footer (activity line, input, send), memories management view. All colors via `ThemeColors::lookup` keys |
| `overlay-rs/src/agent/mod.rs` (+ `commands.rs`, `tasks.rs`, `memory.rs`) | Tool implementations: allowlisted command runner; systemd user-timer CRUD (`~/.config/systemd/user/oxidemx-task-*.{service,timer}`); memory store (`~/.local/share/oxidemx/memories.json`) + retention sweep + system-prompt injection |
| `overlay-rs/src/sampler.rs` | 1 s iced subscription sampling CPU (`/proc/stat`), RAM (`/proc/meminfo`), net ↓↑ (`/proc/net/dev`), disk free (`statvfs`), mouse battery (daemon `DeviceStateChanged` signal + `GetBatteryStatus`), weather (TBD §6.1) → `Message::WidgetSample`; ring buffers (30 samples) live in `RadialState` |
| `overlay-rs/src/resize.rs` (or inline in `chat_ui`) | Corner-grip drag state, min 420×560 clamp, live `W × H` mono badge, persist to `overlay.chat_size` on release |

Not touched: daemon (its D-Bus surface already covers every Mouse-submenu action; no daemon changes needed), gnome-extension, settings-rs (editor reuse is out of scope per NON-GOALS).

---

## 2 · `AiHandoff` state machine

```rust
pub enum AiHandoff {
    Inactive,
    PuckArmed { entered_at: Instant, armed_cursor: Option<Point> },
    ChatActive,
}
```

```
                 cycle lands on AI page (sync_ai_morph → target 1.0)
   Inactive ──────────────────────────────────────────────► PuckArmed
      ▲                                                        │ │
      │  wheel cycles away (any position while armed;          │ │ click in chat outside
      │  morph reverses to neighbour page)                     │ │ puck hit circle
      ◄────────────────────────────────────────────────────────┘ │      OR
      ▲                                                          │ CursorMoved lands outside
      │  Esc / header × (close — existing path)                  │ center zone after real travel
      │                                                          ▼
      ◄──────────────────────────────────────────────────── ChatActive
      ▲                                                          │
      └── wheel over header puck hit circle cycles away ─────────┘
```

Binding rules encoded as pure functions (unit-testable without iced):

- **Arm:** entering the AI page records `armed_cursor` from the last known cursor position (`None` if unknown).
- **Mouse-out activation:** a `CursorMoved` event activates iff `dist(pos, disc_center) > CENTER_ZONE_RADIUS` **and** cumulative travel since arming ≥ 8 px. A cursor *resting* outside the zone at arm time never activates (no event ⇒ no transition; the travel floor kills jitter).
- **Click activation:** any press inside the window, outside the puck's hit circle (header puck radius 16 px + 4 px slop).
- **Wheel:** `PuckArmed` ⇒ wheel anywhere cycles pages (reuses `cycle_page` + existing `cycle_debounce_ms`). `ChatActive` ⇒ wheel cycles only inside the puck hit circle; elsewhere falls through to the conversation scrollable.
- **Focus:** `chat_focus_pending` is only honoured in `ChatActive` (set on transition), so the armed chat never grabs keys / shows a caret.
- **Page dots** stay bound to `page_index` in every state; ring stroke = accent + glow while armed, `overlay0`-ish dim stroke when active (per `ai-chat.jsx` puck spec).

Puck travel: `puck_geom(t, win_w)` lerps center `(242, 242) → (8 + 14 + 16, 8 + 26)` (header-left slot per `ai-chat.jsx`: 8 px window margin, 14 px header left padding, 32 px puck in a 52 px header) and radius `45 → 16`, eased by the same `ai_morph` tween — one object flying, no fade-swap. Drawn by `CapsPainter` above the cap layer for all t > 0; the radial painter stops drawing center dome/dots once `ai_morph > 0` (shared `draw_puck` keeps the two pixel-identical at the boundary).

---

## 3 · Config schema additions (all additive, serde-default, lossless)

```rust
// oxidemx-shared/src/config.rs
pub struct OverlayConfig {                  // new AppConfig.overlay section
    pub chat_size: Option<(u32, u32)>,      // persisted on resize-grip release; None → 484×640 legacy
    pub ai: AiConfig,
}
pub struct AiConfig {
    pub command_allowlist: Vec<String>,     // default ["brightnessctl", "wpctl", "systemctl --user"]
}

// on Slice (both Option + skip_serializing_if, so existing configs byte-round-trip)
pub struct WidgetConfig { pub source: WidgetSource, pub format: Option<String> }
pub enum WidgetSource { Weather, Cpu, Memory, Network, Disk, TasksDue, MouseBattery }
pub enum DialKind { Brightness, Volume }
```

`ActionKind` additions (unit variants; parameter rides `command` like `EasySwitch` does):

| Variant | `command` payload | Executor |
|---|---|---|
| `Widget` | – (display-only wedge; `slice.widget` holds source) | none (hover/click no-op unless submenu) |
| `Dial` | – (`slice.dial`) | `brightnessctl set N%` / `wpctl set-volume @DEFAULT_AUDIO_SINK@ N%` on drag/scroll |
| `Power` | `lock` \| `logoff` \| `suspend` \| `restart` \| `shutdown` | `org.freedesktop.login1` (suspend/reboot/poweroff), `org.gnome.ScreenSaver` Lock, `gnome-session-quit --logout` |
| `NightLight` | – | gsettings `org.gnome.settings-daemon.plugins.color night-light-enabled` toggle; state dot reads same key |
| `MouseSetting` | `dpi:1600` \| `smartshift` \| `haptics` \| `gaming` | daemon D-Bus: `SetDpi`, `SetWheelMode`, `SetHapticsEnabled`, `SetGamingMode` (all exist, `daemon/src/dbus/interface.rs`) |

Migration: no version field exists; schema stays forward/backward compatible exactly as today (no `deny_unknown_fields`, every new field defaulted + skipped when empty). Round-trip unit test: deserialize current `default-config.json` → serialize → byte-compare semantics (serde_json::Value equality).

---

## 4 · Agent tool signatures (Contract 3)

```rust
// ai_client.rs — new stream event
pub enum StreamEvent {
    Delta(String),
    Activity(String),               // was &'static str; now dynamic ("Scheduling task — writing systemd unit…")
    Card(AgentCardData),
}
pub enum AgentCardData {
    Command { command: String, stdout: String, exit_code: i32 },
    Task    { name: String, unit: String, schedule: String, next_run: Option<String>, enabled: bool },
    Memory  { id: String, text: String, retention: String },
}
```

Declared to the Interactions API (custom functions, both `AgentMode`s; GeneralChat's built-in `google_search` becomes the nested-custom pattern already used by SettingsCustomizer, since built-ins can't mix with customs):

1. `execute_command(command: string)` — split + match head against `overlay.ai.command_allowlist`. Allowlisted → run via `sh -c`, 10 s timeout, trim stdout to ~2 KB. Not allowlisted → reuse the `QUESTION_TX`/`PendingQuestion` confirmation plumbing as a Run/Deny chip; deny returns a refusal `function_result`. Emits `Card(Command{..})`.
2. `schedule_task(name, on_calendar, command, action: create|enable|disable|list|delete)` — writes `~/.config/systemd/user/oxidemx-task-<slug>.{service,timer}`, then `systemctl --user daemon-reload` + `enable|disable --now`; `next_run` parsed from `systemctl --user list-timers --output=json`. Card switch ↔ enable/disable; Edit/Run-now chips map to `schedule_task` calls / `systemctl --user start`.
3. `memory(action: save|list|delete|pin, text?, scope?, id?)` — JSON store `~/.local/share/oxidemx/memories.json`, entries `{ id, text, scope, pinned, created_at, last_used_at }`. Pinned + recently-used (≤10) injected into `system_instruction()` each session; `last_used_at` touched on injection. Retention sweep (unpinned, unused > 90 d) runs at overlay launch (deviation from "daemon start" — daemon never touches this file; flagged §6.6).

Cards persist: `ChatMessage` gains `card: Option<AgentCardData>` (serde-default ⇒ old `ai-chats.json` loads unchanged).

---

## 5 · Task graph & sequencing

Hard order constraints (per spec): **T1 → everything**; **T2 (Contract 1) → T3 (Contract 2 header)**; **T5a sampler → T5b widget wedges**. T4 (tools) ∥ T5 (pages) may run as parallel subagent workstreams.

```
T1 foundations: config schema + ActionKind + AiHandoff (pure) + unit tests
 ├─► T2 puck handoff: draw_puck extraction → puck_geom travel → input routing → focus gating → transition tests
 │     └─► T3 chat redesign: window resize + persisted morph target → aurora backdrop reuse →
 │            chat_ui module split (header/threads/cards/footer/memories) → resize grip + badge
 ├─► T4 agent tools (parallelizable): memory store+sweep+injection → execute_command+allowlist →
 │            schedule_task → StreamEvent::Card wiring → card persistence round-trip
 └─► T5 pages (parallelizable): T5a sampler subscription → T5b widget/dial wedge rendering →
              T5c action executors (Power/NightLight/Dial/MouseSetting) → T5d default-config pages
T6 verification: fresh-context subagent contract checks → vision loop (OxideMX + Dracula) →
   clippy -D warnings + fmt → manual-test recipe → lessons.md
```

Each task lands with its tests and a clean `cargo clippy --all-targets -- -D warnings` before the next starts. Unit tests required by spec: `AiHandoff` transitions incl. click-vs-mouse-out rules (rest-at-center must NOT activate; travel+exit must; click on puck must not; wheel in each state), memory retention sweep (90 d unpinned vs pinned), allowlist enforcement (head-match, not substring), config round-trip.

**Vision loop (T6):** build, show each surface, capture via `iced::window::screenshot` (or grim on the live overlay), compare against `flow-2.png` (puck slot), `chat-main.png`, `chat-resize.png`, `chat-memories.png`, `radial-device.png`, `radial-device-mouse.png`, `radial-widgets.png`, across OxideMX + Dracula; iterate to match.

---

## 6 · Risks

- **Window resize on Wayland** — the window is deliberately created once at 484×640 and never resized (wgpu surface-sync with fractional scaling). Contract 2 requires real resizing. Primary approach: `iced::window::resize` request at morph start / grip drag, disc stays anchored top-left. Fallback if unstable: pre-create at persisted chat size (current pattern, just bigger). Will be the first thing T3 spikes.
- **Aurora behind a rect window** — the shader is a full-window triangle (conic around center), so reuse is direct; visual parity with the JSX's three edge-anchored radial gradients may need a "rect mode" uniform variant. Treated as polish inside T3, not a blocker.
- **GeneralChat search regression** — moving GeneralChat from built-in to nested-custom search changes its grounding path; existing `grounded_search()` covers it but needs a regression pass.

## 7 · Ambiguities — please answer before implementation

1. **Weather provider**: propose **Open-Meteo** (keyless HTTP, fits `http_get`-style fetch; location from a new `overlay.weather_location` config) — or ship the wedge stubbed ("—") until you pick one?
2. **Mouse quick settings transport**: propose **daemon D-Bus** (`SetDpi`/`SetWheelMode`/`SetHapticsEnabled`/`SetGamingMode` all already exported) — hidapi-direct would duplicate the daemon's device ownership. Confirm?
3. **Chat size persistence**: propose global `overlay.chat_size` in `config.json` (new `overlay` section, §3), not theme-scoped. Confirm?
4. **Center-zone radius**: reuse `CENTER_ZONE_RADIUS = 45.0` (`overlay-rs/src/geometry.rs:15`) for the mouse-out rule, with an 8 px travel floor for "deliberate" (§2). Confirm constant + floor?
5. **Wheel-over-puck debounce**: propose reusing `cycle_debounce_ms` unchanged; add a distinct knob only if testing shows accidental double-cycles on the 16 px puck. OK?
6. **Retention sweep location**: spec says "on daemon start", but `memories.json` is overlay-owned (ai_client) and the daemon never reads it — propose sweeping at overlay launch instead. OK?
7. **Tasks-due widget source**: no task backend exists in the repo. Stub ("—" + label) or integrate something (Evolution/khal/todo.txt)?
8. **New built-in pages for existing users**: shipped via `default-config.json` they only appear on fresh installs (lossless-migration constraint). Acceptable, or should `normalize_pages()` append them once when absent (risks resurrecting user-deleted pages)?
9. **Tool availability per mode**: propose all three tool families in **both** GeneralChat and SettingsCustomizer (GeneralChat search moves to nested-custom). Confirm?
10. **`SliceKind::Widget { source, format }` sketch**: implemented as `ActionKind::Widget` unit variant + `Slice.widget: Option<WidgetConfig>` field to keep the existing tag-string serde shape and lossless config compat. Confirm deviation?
