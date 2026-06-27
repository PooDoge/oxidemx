# OxideTooltip + TooltipGroup — Design (Slice D of the UI-improvements program)

**Date:** 2026-06-27
**Branch / worktree:** new branch off `2b-collapsible-panels` / `oxidemx-2b`
**Goal:** A reusable hover tooltip for the OxideMX Freya UI — used first on the **collapsed sidebar
+ right-panel rail items** — with a >1s show delay, an "instant after the first" warm behaviour
across a group, configurable placement + offset, and a text OR detailed-content variant.

## Why our own (modeled on Freya's `TooltipContainer`)

Freya blog/0.4 has `TooltipContainer` (`freya-components/src/tooltip.rs`): it wraps a child, shows a
`Tooltip` after a configurable `.delay()`, positions via `AttachedPosition` (Top/Bottom/Left/Right),
and animates (scale 0.9→1 + opacity 0→1, 150ms Expo). We **reuse that proven pattern** but build our
own because Freya's lacks three things in our spec: (a) **detailed content** (Freya's `Tooltip` is a
single-line text label only), (b) a configurable **offset**, and (c) the **"instant after the first"**
group behaviour (Freya's delay is per-container). `AttachedPosition` and `Attached` are public, so we
reuse them for positioning.

## Architecture

### `OxideTooltip` (`oxide-ui/src/components/tooltip.rs`, new)
A wrapper component (Freya builder + `ChildrenExt` style):
```rust
OxideTooltip::text("Conversation title")              // or
OxideTooltip::detailed(rect()...content...)           // arbitrary rich element
    .placement(AttachedPosition::Right)               // default Bottom
    .offset(8.0)                                       // extra gap beyond Freya's default 5px; default 0
    .delay(Duration::from_millis(1000))               // default 1000ms
    .child(<trigger element>)                          // the rail icon it wraps
```
- `enum TooltipBody { Text(Cow<'static, str>), Detailed(Element) }` (private; set by `text()`/`detailed()`).
- Render (port of `TooltipContainer::render`): `use_state(is_hovering)` + `use_state(delay_task: Option<TaskHandle>)`; `use_animation` scale(0.9→1)+opacity(0→1) 150ms Expo, reversed when not hovering; `on_pointer_over` → cancel any pending task, spawn `Timer::after(effective_delay)` → `is_hovering = true` (+ mark the group warm); `on_pointer_out` → cancel task, `is_hovering = false` (+ start the group cooldown). The floating body is `Attached::new(trigger).position(placement)` with a `maybe_child(is_visible.then(|| rect().opacity(scale_opacity).scale(scale).padding(placement_pad + offset).child(body)))`. `is_visible = opacity > 0.` (we do NOT use Freya's `ContextMenu`, so its `is_open()` suppression is inapplicable; if a tooltip-while-our-context-menu-open issue ever appears, gate on `OxideCtxMenu`'s open state instead — out of scope here). Text body = a themed `rect`+`label` (Freya `Tooltip` look: padding (4,10), 1px border, radius 8, `max_lines(1)`); detailed body = the caller's element wrapped in the same shadowed surface (a small `tooltip_surface` helper in this file: `rect().background(panel).border(hairline_strong).corner_radius(8).shadow(soft)`).
- `interactive(Interactive::No)` on the floating body (tooltips never capture pointer).

### `TooltipGroup` (same file) — the warm/"instant after first" behaviour
A context provider that wraps a set of `OxideTooltip`s (one per rail):
```rust
TooltipGroup::new().child(<the rail with OxideTooltips inside>)
```
- Provides `TooltipGroupState { warm: State<bool>, cold_task: State<Option<TaskHandle>> }` via
  `use_provide_context`.
- `OxideTooltip` reads it with `use_try_consume::<TooltipGroupState>()` (None ⇒ standalone, always uses
  full delay). When present:
  - **effective_delay** = `if *warm.read() { Duration::ZERO } else { self.delay }`.
  - `on_pointer_over`: **cancel `cold_task`** (we're still inside the group).
  - on show (delay elapsed): `warm = true`.
  - `on_pointer_out`: spawn `cold_task = Timer::after(COOLDOWN ~400ms)` → `warm = false`. Moving to
    another group item cancels this via that item's `on_pointer_over`, so warm persists between items;
    leaving the group entirely lets it fire → cold again.
- Net: first item waits the full second; moving straight between items is instant; idle/leave resets.

### `Side`/placement
Reuse Freya's `AttachedPosition` directly (Top/Bottom/Left/Right) — no new enum. `offset` adds to the
position-specific padding (Freya hard-codes 5px per side; we add the configured offset).

## Integration (this slice)
- **Sidebar collapsed rail** (`oxide-freya/src/regions/sidebar.rs`): wrap the rail in a `TooltipGroup`;
  wrap the project dot in `OxideTooltip::text(project_name).placement(Right)`, and each per-conversation
  icon in `OxideTooltip::detailed(<title + model + worktree>).placement(Right)`.
- **Right-panel direction rail** (`oxide-freya/src/regions/context/mod.rs`): wrap the rail in a
  `TooltipGroup`; wrap each direction icon in `OxideTooltip::text(direction_label).placement(Left)`
  (the right rail's tooltips point left, into the window).
- Expanded panels are NOT wrapped (their labels are already visible).
- `oxide-ui/src/components/mod.rs`: `pub mod tooltip; pub use tooltip::{OxideTooltip, TooltipGroup};`.

## Data flow
```
pointer over a rail item ──(delay OR 0 if group warm)──▶ show tooltip (fade/scale in) + group.warm=true
move to next item ──cancel cold_task, warm still true──▶ instant show
pointer leaves all items ──cold_task fires after ~400ms──▶ group.warm=false (next hover waits full delay)
ContextMenu open ──▶ tooltips suppressed
```

## Error / edge handling
- No `TooltipGroup` present ⇒ `OxideTooltip` works standalone with its full delay (warm logic inert).
- Task cancellation is idempotent (`.take()` + `cancel()`), no leaks on rapid hover in/out.
- Detailed content is an owned `Element`; missing data renders an empty-but-valid body (no unwrap).
- Tooltip body `interactive(No)` so it never blocks clicks on what's beneath.

## Testing
- **Unit** (`tooltip.rs`): `TooltipGroupState` transitions — cold→(delay)→warm; warm ⇒ effective_delay
  = 0; cooldown ⇒ warm=false; re-enter cancels cooldown. (Pure logic on the signals; no timing.)
- **Dark snapshots** (render + read-back): a `text` tooltip and a `detailed` tooltip shown at each of
  Top/Bottom/Left/Right (force `is_hovering=true` in the harness, poll past 150ms); a collapsed sidebar
  rail with one conversation tooltip visible.
- **Live** (after green snapshots): hover a collapsed-rail icon >1s → tooltip; sweep across icons →
  instant; leave + return → delayed again; placement/offset correct; right rail points left.
- Headless can't test the hover *timing*, so the delay/warm logic is unit-tested + live-verified.

## File structure
| File | Responsibility |
|------|----------------|
| `oxide-ui/src/components/tooltip.rs` | `OxideTooltip` + `TooltipGroup` + `TooltipGroupState` + `tooltip_surface` |
| `oxide-ui/src/components/mod.rs` | register + re-export |
| `oxide-freya/src/regions/sidebar.rs` | wrap collapsed-rail items in a `TooltipGroup` + `OxideTooltip`s |
| `oxide-freya/src/regions/context/mod.rs` | wrap direction-rail icons (placement Left) |

## Out of scope (other slices in this program)
- A — "+" icon button inline with search.
- B — menu open/close animation (Select-style) in the shared Popover/MenuSurface.
- C — menu interrupt-closure + first-class toggle/radio/keep-open rows.
- Tooltips on EXPANDED panels, the composer, or arbitrary elsewhere (the component is reusable for
  those later; this slice only wires the two collapsed rails).
