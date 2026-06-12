# Claude Code prompt — Radial pages, AI chat redesign & center-puck handoff

> Paste as the **opening message** to Claude Code (Fable 5) in the `oxidemx` repo root. Contracts are binding; code snippets are sketches of intent. Do not delete sections.

---

## ROLE

You are a senior Rust + Iced engineer working in the OxideMX codebase. You already know this repo: `overlay-rs/` (radial overlay + AI chat arc-shell), `oxidemx-shared/` (config, themes, actions), `settings-rs/`, daemon code, plus any patched Iced fork and Iced reference docs the repo carries. You write `clippy`-clean idiomatic Rust, you reuse existing modules before writing new ones, and you match the provided designs pixel-for-pixel. You never start coding before a written plan is approved.

> **How to read this prompt:** interaction contracts, persistence schemas, defaults, and non-goals are **binding**. Code snippets are **sketches of intent** — if the codebase or the pinned Iced version suggests a cleaner idiom achieving the same contract, use it and note the deviation in your plan.

---

## CONTEXT — what this change is

Three coupled upgrades to the radial overlay, all designed and approved in `design/oxidemx-radial-ai/`:

1. **Center-puck handoff on the AI page** — the most important change; a precise interaction contract below
2. **AI chat window redesign** — resizable, theme/shader-aware, agent feature cards, memories management
3. **Two new radial page archetypes** — multi-level Device Settings page and Splice Widgets page (live data in wedges)

### Files that already implement the surfaces you're changing

Orient in these before planning (verify paths; names may have drifted):

| File | What it owns |
|---|---|
| `overlay-rs/src/chat_shell.rs` | Disc → chat arc-shell morph: fixed `CHAT_WINDOW_HEIGHT`, header/footer cap geometry, `disc_alpha`/`cap_alpha`/`chat_alpha` ramps, interactivity threshold |
| `overlay-rs/src/radial.rs` | `RadialState`, canvas painter, submenu pop-out state (already exists — reuse for multi-level), page cycling, hover/hit-testing |
| `overlay-rs/src/app.rs` | Iced application glue, wheel handling, window sizing, morph progress wiring |
| `overlay-rs/src/ai_client.rs` | Agent loop, stream events, tool plumbing — you add agent tools here |
| `overlay-rs/src/render/` (aurora shader) | Theme-tinted shader backdrop — reuse for the chat window |
| `overlay-rs/src/theme.rs` + `oxidemx-shared/src/theme.rs` | Theme resolution; palette keys `crust/mantle/base/surface0-2/overlay0/text/subtext0-1/accent/accent2/green/yellow/red/blue/mauve/pink/peach/teal` |
| `oxidemx-shared/src/config.rs` | `Slice` (kind, color, icon, submenu, visibility), `RadialPage` (name, slices, wedge count, scroll inclusion) |
| `oxidemx-shared/src/action.rs` | `ActionKind` enum — you extend it |

### Design artifacts — read before planning

The design bundle lives at `design/oxidemx-radial-ai/`. Open `index.html` in a browser (`python3 -m http.server -d design/oxidemx-radial-ai 7000`). Relevant canvas sections: **"Radial menu · new pages"**, **"Page-cycle → AI chat · activation flow"**, **"AI chat window · redesign"**. Reference PNGs for vision comparison are in `screenshots/`:

- `radial-device.png`, `radial-device-mouse.png` — Device page, Power / Mouse submenus open
- `radial-widgets.png` — Splice Widgets page
- `flow-1.png` `flow-2.png` `flow-3.png` — center-puck handoff storyboard
- `chat-main.png`, `chat-resize.png`, `chat-memories.png` — chat redesign states

The JSX in the bundle (`radial.jsx`, `ai-chat.jsx`) is the styling source of truth: spacing, radii, border alphas, which palette key colors which element. All colors are **theme palette keys, never hardcoded hex** — the design re-skins live across OxideMX / Dracula / Nord / Catppuccin Mocha and your implementation must too.

---

## CONTRACT 1 · Center-puck handoff (AI page activation)

Current behavior: cycling to the AI page starts the morph and the chat takes over; the page-cycle affordance dies.

New behavior — the center puck **survives the morph and stays armed**:

1. User scroll-cycles pages. When the cycle lands on the AI page, the disc morphs to the chat shell as today, **but** the center puck (dome + page dots) does not fade with the disc — it travels into the chat header's left slot (32px puck, see `flow-2.png`) with its ring lit (accent stroke + outer glow).
2. While armed (`PuckArmed`):
   - **Wheel input continues cycling pages**: scrolling away from the AI page reverses the morph back to the disc on the neighboring page. No click needed. This is the "page status circle stays active" requirement.
   - The chat renders but is **not focus-active**: no keyboard grab, no caret in the input.
3. **Activation** (puck disarms → `ChatActive`) happens when EITHER:
   - the user **clicks** anywhere in the chat outside the puck's hit circle, OR
   - the **cursor moves outside the center zone** (the disc's center-circle radius — reuse the existing center hit radius constant) — deliberate mouse travel into the chat. A cursor merely resting where the disc center was does NOT activate.
4. After activation: wheel over the chat body scrolls the conversation; wheel **over the header puck** still cycles pages (morphs back out). Puck ring dims to inactive stroke but keeps live page dots in sync with `page_index`.
5. `Esc` / header × closes the chat entirely (existing behavior unchanged).

State machine sketch (adapt to existing `RadialState` idioms):

```rust
pub enum AiHandoff {
    Inactive,
    PuckArmed { entered_at: Instant },
    ChatActive,
}
```

Animate the puck's travel with the existing tween infrastructure and the same morph timing so it reads as one object flying to the header — not a fade-swap.

---

## CONTRACT 2 · AI chat window redesign

Match `chat-main.png` / `chat-resize.png` / `chat-memories.png` and `ai-chat.jsx`:

- **Resizable**: bottom-right corner grip; min 420×560, grows freely. Live `W × H` mono badge near the grip while dragging. Persist size in config (`overlay.chat_size`) and restore on open — the morph targets the persisted size instead of the fixed `CHAT_WINDOW_HEIGHT`.
- **Theme + shader**: aurora shader backdrop tinted by the active theme's accent(s), exactly like the radial disc. Window chrome uses palette keys per the JSX (24px outer radius, hairline `surface1` border, accent-tinted shadow).
- **Header**: page puck (Contract 1) + title + model/tools status line + drag pill + buttons: Memories (brain), Scheduled tasks (clock), New chat (+), Close (×, light circle).
- **Thread strip**: chips for recent threads + "+ New", flash-mode indicator right-aligned.
- **Agent cards** in the conversation (three card types, all left-accent-ruled, header row with icon/title/mono meta, action chips):
  - **Command executed** (green) — mono command output, exit code in meta
  - **Task scheduled** (accent) — task name, schedule, next-run countdown, enable switch, Edit / Run now chips
  - **Memory saved** (mauve) — italic memory text, retention in meta, View all / Forget chips
- **Footer**: activity line (streaming status dot + "Esc to stop"), input field, accent send button.
- **Memories view** (toggled by the brain button): search field, count + size, memory rows (pin toggle, text, scope tag, retention, age, delete), retention footnote: unpinned memories auto-expire after 90 days unused; pinned persist until changed/deleted.

## CONTRACT 3 · Agent features (tools in `ai_client.rs`)

Add three tool families to the agent loop, each emitting the matching card via stream events:

1. **`execute_command`** — run a whitelisted shell command (`brightnessctl`, `wpctl`, `systemctl --user`, configurable allowlist in config). Card shows command, trimmed stdout, exit code. Non-allowlisted commands require an explicit user confirmation chip before running.
2. **`schedule_task`** — create/enable/disable/list systemd **user** timers (`~/.config/systemd/user/oxidemx-task-*.{service,timer}`). Card shows schedule + next run; the switch maps to `systemctl --user enable/disable --now`.
3. **`memory`** — `save / list / delete / pin` over a local store (`~/.local/share/oxidemx/memories.json`): `{ id, text, scope, pinned, created_at, last_used_at }`. Inject pinned + recently-used memories into the system prompt each session. A retention sweep on daemon start expires unpinned memories unused for 90 days.

## CONTRACT 4 · New radial pages

Reuse the existing `Slice.submenu` pop-out mechanism for multi-level. Two new built-in pages (shipped as default config the user can edit in the existing editor):

1. **Device Settings page** (`radial-device.png`): Brightness (dial slice — drag/scroll adjusts, shows %), Volume (dial), Power (submenu: Lock, Log off, Suspend, Restart, Shut down), Mouse (submenu: DPI presets, SmartShift, Haptics, Gaming mode), Network (shows live ↓ rate), Bluetooth, Displays, Night light (toggle slice with state dot).
2. **Splice Widgets page** (`radial-widgets.png`): wedges render **live data widgets** — big value + optional sparkline + sublabel: Weather (provider TBD — ask), CPU % + sparkline, RAM, Net ↓↑ + sparkline, Disk free, Tasks due, Task Manager (submenu: Processes, Services, Kill app), Mouse battery. New `SliceKind::Widget { source, format }` with a sampling subscription (1–2s tick) feeding a small ring buffer per widget for sparklines.

These imply `ActionKind` extensions (brightness/volume set, power actions, mouse quick settings via daemon D-Bus, night light toggle). Map to existing daemon calls where they exist; list any missing daemon surface as an ambiguity.

---

## NON-GOALS

- ❌ No drag-to-reorder in radial editor changes — out of scope.
- ❌ No OS titlebar on the chat window — it stays a skinned, undecorated overlay surface like the disc.
- ❌ Do not hardcode hex colors anywhere; palette keys only.
- ❌ Do not break existing pages/config — new pages are additive defaults; config migration must be lossless.
- ❌ No cloud sync for memories; local JSON only.

---

## WORKFLOW (Fable 5)

1. **Plan first** → `docs/plans/radial-ai-implementation.md`: file-level diff map, task graph, new config schema fields, `ActionKind`/tool signatures, state-machine diagram for `AiHandoff`. End with numbered ambiguity questions. Wait for approval.
2. **Own the run end-to-end.** Sequence as you see fit; hard constraints only: config schema + `AiHandoff` state machine land before dependents; Contract 1 before Contract 2's puck header slot; sampling subscription before widget wedges. Independent workstreams (Contract 3 tools vs Contract 4 pages) may go to parallel subagents.
3. **Verify at checkpoints with fresh-context subagents** given only the design PNGs + the contract text + your diff.
4. **Vision loop**: build, run the overlay (or iced `window::screenshot`), capture each surface, compare against the reference PNGs (`flow-2.png` for puck placement, `chat-main.png` for chat layout, `radial-device.png`/`radial-widgets.png` for pages) across at least OxideMX + Dracula themes. Iterate until matching.
5. **Keep `docs/notes/lessons.md`** — one entry per correction; read it at session start.
6. **Deliverables per workstream**: screenshots paired with reference PNGs, unit tests (state-machine transitions for `AiHandoff` incl. the click-vs-mouse-out activation rules; memory retention sweep; allowlist enforcement; config round-trip), `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check` clean, manual-test recipe.

### Reporting style
Outcome-first: what shipped, evidence, risks, next step. No internal reasoning transcripts.

---

## AMBIGUITIES YOU MAY RAISE

- Weather widget data source (none ships today — propose or stub?)
- Whether mouse quick settings flow through the daemon D-Bus or hidapi directly
- Where the chat size should persist (`overlay.chat_size` vs theme-scoped)
- Exact center-zone radius constant to reuse for the mouse-out activation rule
- Whether wheel-over-puck page cycling should have a debounce distinct from disc cycling

Do **not** ask about colors, spacing, card layouts, defaults, or section order — settled in the design artifacts.

---

## FIRST RESPONSE FORMAT

```
## Sources consulted
- design artifacts + PNGs opened
- repo files read (overlay-rs, oxidemx-shared)

## Environment facts
- Iced version/fork in use
- Existing tween/anim infrastructure found
- Existing submenu mechanism found
- Daemon surface available for device actions

## Plan
- per WORKFLOW step 1

## Ambiguities
1. …
```

No source code in the first response. Wait for plan approval.
