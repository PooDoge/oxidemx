# Right-Panel Shell + 3 Tabs — Design (Slice 1 of the Collapsible-Panels right panel)

**Date:** 2026-06-27
**Branch / worktree:** new branch off `2b-collapsible-panels` / `oxidemx-2b`
**Design source (authoritative):** Claude Design project `OxideMX Design System`
(`686a723e-0412-4e94-870e-b4e32ae465f2`) — `freya2-right.jsx`, `freya2-shell.jsx`,
`OxideMX Freya - Collapsible Panels.{html,freya.json}`. Translate **pixel-faithfully**
via the `claude-design-to-freya` skill, using the `.freya.json` as the authoring contract.

## Why this slice exists (correction)

The right panel was previously built as four content tabs **Spec/Mission/Workbench/Ambient**
showing project metadata. **That was wrong** — it doesn't exist in the design. In the real
design the four "directions" are a **Tweaks preference** (`TweakSelect "Rail status"`) selecting
one of four *visual styles* for how running agents render in the rail (rings / bubbles / spine /
orbs). The actual right panel is a **control plane** with **three tabs: Run | Worktree | .oxide**.
This slice rebuilds the right panel to the correct structure. The rail agent-status styles +
popovers, and each tab's real backend data, are explicit follow-on slices.

## Scope (this slice)

Build the right panel's **shell + tab structure** with available/placeholder data:
- Resizable panel: **348px full / 60px rail**, `DragHandle` (collapse toggle; drag-resize if
  `oxide-ui/.../resize_grip.rs` fits cleanly, else click-toggle + defer drag), collapse anim
  **340ms expo-out** (design timing; replaces the current 180ms). Keep the shell's `Content::Flex`.
- **Full panel:** `SegmentedButton` header — **Run / Worktree / .oxide** (active = `accent` text +
  `with_alpha(accent,0x16)` fill + `with_alpha(accent,0x33)` border, per design) over a `ScrollView`.
- **Rail (collapsed):** the bottom nav `RailButton`s — Run / Worktree / .oxide / Open editor — each
  expands the panel + selects its tab (editor button is a no-op stub this slice). The live-log
  button + agent-status cues are slice 2 (omit, no placeholder needed).
- **Tab bodies (structure + data we already have):**
  - **Run** → uppercase section label + "No active run" empty state (real flow stages = slice 3).
  - **Worktree** → the active conversation's `worktree` (branch + path, already in the DTO) in the
    design's changed-file row style, + a faint "changed files coming" line (real git diff = slice 4).
  - **.oxide** → the `SettingsRoot` nav-list **structure** (rows = `SideBarItem`-style: icon tile +
    label + count + inheritance-source `Chip`) with placeholder categories Hooks/MCP/Permissions/Env
    and source chips (inherited/local/merged). Rows are inert this slice (real `.oxide` resolution +
    Hooks/MCP editors = slice 5).

## Responsive breakpoints (added per user request)

The design is **one app across four size classes** (`freya2-shell.jsx`): `wide ≥1180` (3 surfaces) ·
`compact 920–1179` (both side panels forced to **icon rails**) · `tablet 600–919` (overlay drawers,
slim app bar) · `phone <600` (full-bleed Android app). This slice builds the **size-class
infrastructure + the desktop breakpoints (wide ↔ compact)**; the tablet-drawer + phone-Android
shells are slice 6.

- `SizeClass { Wide, Compact, Tablet, Phone }` derived from the **logical window width**, measured
  with a full-window global probe rect (`Position::new_global()` + `width(fill).height(fill)` +
  `on_sized`) — the LOGICAL-window technique from `[[feedback_freya_logical_vs_physical_px]]`
  (`Platform::root_size` is PHYSICAL; do not use it). Thresholds: ≥1180 Wide · 920–1179 Compact ·
  600–919 Tablet · <600 Phone. Held in `AppState` (`pub size_class: State<SizeClass>`), updated by
  the probe's `on_sized`.
- **Compact behavior (this slice):** when `size_class` is `Compact` (or narrower), the shell forces
  **both** the sidebar and the right panel to their 60px rails regardless of the user's collapse
  signals (the design's "compact forces icon rails"). The user's collapse signals are preserved and
  restored when the window widens back to `Wide`. Implement as a derived "effective collapsed" =
  `user_collapsed || size_class <= Compact`, computed in `shell()` and passed down.
- **Tablet / Phone (this slice):** detected + stored, but render the same desktop shell (clamped) —
  a faint dev affordance is acceptable; the real drawer/Android layouts are slice 6. No crash at
  any width.
- Resizing is continuous: the probe re-measures on every `on_sized`, so dragging the window across a
  threshold flips the rails live (with the existing 340ms collapse animation).

## Architecture

### State (`oxide-freya/src/state.rs`)
- Add `RightTab { Run, Worktree, Settings }` (`#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]`,
  default `Run`) + `pub right_tab: State<RightTab>` on `AppState` (add to `new` + `PartialEq`).
- `StatusDirection` stays (it becomes the slice-2 rail-status *style* preference) but is **no longer
  wired to content**. Rename is deferred to slice 2 to keep this diff focused.
- Keep `context_collapsed` (drives rail/full) + the `OnCreation::Finish` width settle.
- Add `SizeClass { Wide, Compact, Tablet, Phone }` (`Copy, PartialEq, Eq, Debug, Default=Wide`) with
  `pub fn from_logical_width(w: f32) -> SizeClass` (≥1180 Wide / ≥920 Compact / ≥600 Tablet / else
  Phone) + a `<=`-style helper (`is_compact_or_narrower()`); `pub size_class: State<SizeClass>` on
  `AppState`. Both `Sidebar` and `ContextRegion` take an explicit **`collapsed: bool` prop** = the
  effective value `user signal || size_class.is_compact_or_narrower()`, computed in `shell()` and
  used for RENDER (rail vs full). Their own collapse/expand buttons still write the *user* signal
  (`sidebar_collapsed` / `context_collapsed`) via `state`; `shell()` recomputes the effective prop —
  so widening back to Wide restores the user's pre-compact choice. (Re-adds the `Sidebar.collapsed`
  prop dropped in the chat-shell slice.)

### Right panel (`oxide-freya/src/regions/context.rs` rewrite)
- `data-freya="ResizableContainer"`: `rect().background(panel()).border_left(hairline())`,
  width animated `CONTEXT_FULL_W=348` ↔ `CONTEXT_RAIL_W=60` (scoped to `context_collapsed`,
  `OnCreation::Finish`, 340ms — match the design ease as close as Freya's `Function` allows;
  document the chosen ease).
- Full vs rail chosen by `context_collapsed`.
- `DragHandle` on the panel's inner (left) edge: click toggles `context_collapsed`.

### Tab bodies (`oxide-freya/src/regions/context/{run,worktree,settings}.rs`, new dir module)
- `mod.rs` dispatches `RightTab` → `RunTab` / `WorktreeTab` / `SettingsTab` (each `{ state: AppState }`).
- Use Freya built-ins per Rule 4: `SegmentedButton`, `ScrollView`, `Card`, `Chip`. Map design
  `data-freya` names → these. Where a built-in doesn't fit (the settings nav row), compose a small
  `rect`-based row matching the design's `SideBarItem`.

### Remove
- `oxide-freya/src/regions/directions/{mod,spec,mission,workbench,ambient}.rs` (wrong content tabs)
  and the `regions/directions` module registration. `DirectionPanel` usage in `context.rs` goes away.

## Token mapping (design → `oxide-ui` Theme) — for pixel fidelity
`T.mantle`→`panel()` · `T.crust`→`bg_deep()` · `T.hair`→`hairline()` ·
`T.hairStrong`→`hairline_strong()` · `T.surface0/1/2`→`surface()/surface_hi()/surface_max()` ·
`T.text`→`text()` · `T.subtext1`→`subtext_hi()` · `T.subtext0`→`subtext()` · `T.faint`→`faint()` ·
`T.accent`→`accent()` · `T.accentDim`→`accent_dim()` · `T.green/yellow/red/blue/mauve`→ same-named ·
`T.<tone>_NN` (e.g. `accent_16`, `green_3a`) → `Theme::with_alpha(<tone>(), 0xNN)` (NN is the hex
alpha byte). Source chips: inherited=`blue`, local=`green`, merged=`mauve`.

## Data flow
```
rail RailButton press ──set right_tab + context_collapsed=false──▶ panel expands to that tab
SegmentedButton press ──set right_tab──▶ ScrollView body swaps
DragHandle click ──toggle context_collapsed──▶ rail ↔ full (340ms)
Tab bodies read AppState (active conversation worktree) — no new agentd endpoints this slice
```

## Error / edge handling
- No active conversation → Worktree shows "No conversation"; no `unwrap`.
- `right_tab` always valid (enum); rail buttons set it before expand.
- Empty `.oxide` nav placeholder list renders without panic.

## Testing
- Unit: `RightTab` default = `Run`; setting `right_tab` from a rail button. `SizeClass::from_logical_width`
  thresholds (1180/920/600 boundaries) + `is_compact_or_narrower`.
- Dark snapshots (render + read-back, per the snapshot-dark-theme rule): rail (nav buttons),
  full panel each tab (Run empty / Worktree with worktree / .oxide nav list); full shell at a
  **Wide** width (panel expanded, 348px, `Content::Flex` reserves it) AND at a **Compact** width
  (both side panels forced to 60px rails).

## File structure
| File | Responsibility |
|------|----------------|
| `state.rs` | `RightTab` + `SizeClass` enums + `right_tab`/`size_class` signals (+ `new`/`PartialEq`) |
| `regions/context.rs` | rewrite: ResizableContainer + DragHandle + SegmentedButton + rail nav |
| `regions/context/{mod,run,worktree,settings}.rs` | tab dispatcher + 3 tab bodies |
| `regions/directions/*` | **deleted** |
| `regions/mod.rs` | drop `directions`, add `context` submodule |
| `app.rs` | size-class probe (full-window logical-size rect) → `size_class`; compute effective-collapsed for both side panels |

## Out of scope (follow-on slices)
2. Rail agent-status visual styles (spec rings / mission bubbles / workbench spine / ambient orbs) +
   "Rail status" preference + live-log + agent **popovers** (pin/drag).
3. Run tab → conductor/agentd run state (stages/steps/status).
4. Worktree tab → real git diff (changed files + adds/dels).
5. .oxide tab → real config resolution + Hooks toggles + MCP fork-to-edit flow + warning modal.
6. Code editor (EditorView / EditorPill) + the **tablet-drawer** and **phone-Android** shells
   (the size-class detection + desktop wide↔compact rails ARE in this slice; only the distinct
   tablet/phone *layouts* — overlay drawers, slim app bar, Android nav variants — are deferred here).
