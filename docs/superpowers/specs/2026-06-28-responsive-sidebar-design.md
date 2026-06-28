# Responsive Sidebar (expand-below-breakpoint + drawer + auto-collapse-on-blur) — Design

**Date:** 2026-06-28
**Branch / worktree:** new branch off `2b-collapsible-panels`
**Goal:** Below the responsive breakpoint the user can still manually expand a sidebar (today they
can't), the expanded sidebar floats over content as a drawer (instead of squishing it), and it
auto-collapses when the user clicks outside it. Applies to BOTH the left conversations sidebar and
the right context panel. Frontend-only (`oxide-freya` shell + `oxide-ui`).

## Root cause

`app.rs:36-39` computes `collapsed = *state.<x>_collapsed.read() || force_rail`, where
`force_rail = sc.is_compact_or_narrower()` (= **not `Wide`**, i.e. logical width **< 1180**). That
`|| force_rail` is a **continuous override**: below 1180 the value is always `true`, so the moment the
user presses » (expand) the next render recomputes `false || true = true` and re-collapses. The expand
button fires, but the layout ignores it.

## Behavior

- **Wide (≥1180):** unchanged — inline resizable panels; user toggles freely (collapse/expand persist).
- **Narrow (<1180):** each panel shows its **60px rail inline** (content layout is stable). Pressing »
  on a rail **expands it as a drawer** that floats over the content (`Layer::Overlay`), content stays
  put. Clicking **outside** the open drawer (or **Escape**) collapses it back to the rail.
- **Crossing the breakpoint:** shrinking past it **auto-collapses** both panels; growing back past it
  **restores the wide defaults** (left = expanded, right = collapsed). Between crossings the manual
  toggle is authoritative.

## A. Sole-truth signal + edge-triggered responsive default (`app.rs`, `state.rs`)

- **Drop the `|| force_rail`**: render from the raw signals.
  ```rust
  let sidebar_collapsed = *state.sidebar_collapsed.read();
  let context_collapsed = *state.context_collapsed.read();
  let narrow = sc.is_compact_or_narrower();
  ```
- **Edge-triggered effect** in `shell()` (a `use_state` previous-class tracker + `use_side_effect`):
  ```rust
  let mut prev_sc = use_state(|| crate::state::SizeClass::Wide); // start "Wide" so a narrow
                                                                 // first-render counts as a down-crossing
  {
      let mut sidebar_c = state.sidebar_collapsed;
      let mut context_c = state.context_collapsed;
      let size_class_sig = state.size_class;
      use_side_effect(move || {
          let now = *size_class_sig.read();
          let was = *prev_sc.peek();                       // peek: don't subscribe to prev
          let was_wide = matches!(was, crate::state::SizeClass::Wide);
          let now_wide = matches!(now, crate::state::SizeClass::Wide);
          if was_wide != now_wide {
              if now_wide {                                // grew above → restore wide defaults
                  sidebar_c.set(false);                    // left expanded
                  context_c.set(true);                     // right collapsed (its default)
              } else {                                     // shrank below → auto-collapse both
                  sidebar_c.set(true);
                  context_c.set(true);
              }
          }
          prev_sc.set(now);
      });
  }
  ```
  (Read each signal into a local before any `.set()` to avoid the if-let/read-guard borrow panic.)
- A pure helper for unit testing the transition decision lives in `state.rs`:
  ```rust
  /// What a breakpoint crossing should do to a panel's `collapsed` flag.
  /// `None` = no change (no crossing). For a crossing: returns the new collapsed value.
  pub fn crossing_collapse(was_wide: bool, now_wide: bool, wide_default_collapsed: bool) -> Option<bool> {
      if was_wide == now_wide { None }
      else if now_wide { Some(wide_default_collapsed) } // grew → wide default
      else { Some(true) }                               // shrank → collapse
  }
  ```
  Left panel `wide_default_collapsed = false`; right panel `= true`. The effect uses this helper.

This alone makes manual expand-below-breakpoint stick (no continuous re-collapse).

## B. Drawer overlay for narrow + expanded (`app.rs`)

When `narrow && !collapsed`, the panel renders in TWO parts:
1. the **60px rail inline** (so content width is unchanged whether the drawer is open or not), AND
2. a **drawer overlay**: a `Layer::Overlay` + `Position::new_global()` full-window **backdrop**
   (transparent-to-dim; `on_press` → set that panel's `collapsed = true`; `on_global_key_down` Escape →
   same), with the **full panel** (`Sidebar`/`ContextRegion` with `collapsed: false`) as a child,
   anchored to the correct edge (left drawer: `position top(0).left(0)`, fixed width ~274px, full
   height, deep shadow; right drawer: anchored right, ~348px). Backdrop sized with
   `Size::window_percent(100.)` (NOT `fill`, which would size to the shell parent).

Hit-testing: the panel is the deepest node, so clicks inside it hit its controls; clicks on the
backdrop (outside the panel) hit the backdrop → collapse. This is the proven `ConfirmDialog` pattern.

Layout decision table (per panel):
| width | collapsed | render |
|-------|-----------|--------|
| Wide  | true      | inline 60px rail (today) |
| Wide  | false     | inline resizable panel (today) |
| Narrow| true      | inline 60px rail |
| Narrow| false     | inline 60px rail **+ drawer overlay** |

So narrow never uses the inline resizable panel; it's rail + optional drawer. Wide is unchanged.

## C. Auto-collapse on blur

Implemented by B's backdrop: while a drawer is open (narrow + !collapsed), the full-window backdrop
behind the panel converts an outside-press (or Escape) into `collapsed = true`. No separate
focus-tracking needed — the drawer is only mounted while open, so the opening » press (on the rail,
before the drawer mounts) can't reach the backdrop and self-close. (Same mount-while-open reasoning
as the menus.)

## Data flow
```
resize ─▶ probe on_sized ─▶ size_class ─▶ edge effect ─▶ (crossing?) set collapsed defaults
» on rail (narrow) ─▶ collapsed=false ─▶ drawer overlay mounts (panel floats over content)
click outside drawer / Escape ─▶ backdrop ─▶ collapsed=true ─▶ drawer unmounts (rail remains)
wide: collapse/expand toggles drive the inline resizable layout (unchanged)
```

## Error / edge handling
- Both panels expanded at once on narrow (rare): two independent drawers + backdrops; each collapses on
  its own outside-press. Left drawer is left-anchored, right is right-anchored; backdrops overlap but
  each only collapses its own panel. Acceptable; not optimized.
- A deliberate wide-collapse of the left sidebar is reset to expanded after a shrink→grow round-trip
  (the chosen "restore wide default" behavior). Documented, intended.
- The probe + size_class is unchanged; the effect only reacts to its transitions.

## Testing
- **Unit (pure):** `crossing_collapse` truth table — no-crossing → None; shrink → Some(true) for both;
  grow → Some(false) left / Some(true) right.
- **Snapshot (oxide-freya, harness with a fixed width):** at a narrow width with the left panel
  expanded, assert the drawer overlay rect (Layer::Overlay) is present AND the content area still
  starts at x≈60 (rail width) — i.e. the drawer floats, not pushes. (Mount the shell body helper at a
  narrow size; assert via node areas.)
- **Live (controller):** below 1180 — » expands the drawer over content; click outside collapses it;
  Escape collapses; resizing across 1180 auto-collapses on shrink and restores expanded-left on grow;
  wide behavior unchanged; right context panel behaves symmetrically.

## File structure
| File | Responsibility |
|------|----------------|
| `oxide-freya/src/state.rs` | `crossing_collapse` pure helper (+ test) |
| `oxide-freya/src/app.rs` | drop `\|\| force_rail`; edge effect; narrow rail+drawer layout for both panels |
| (reuse) `oxide-ui` `point_in_rect`, `Sidebar`, `ContextRegion` | drawer content + (if needed) bounds; no new component unless a `Drawer` wrapper proves worth extracting |

## Decomposition / sequencing (2 tasks)
1. **Sole-truth + edge effect** (`state.rs` helper + test; `app.rs` drop the `||`, add the effect).
   Deliverable: manual expand-below-breakpoint sticks (inline layout); crossings auto-collapse/restore.
2. **Drawer overlay + auto-collapse-on-blur** (`app.rs` narrow rail+drawer for both panels; backdrop
   outside-press/Escape → collapse). Deliverable: expanded-narrow floats over content + blur-collapses.
   Final live verify.

## Out of scope
- Animating the drawer slide (could be a later polish; not required).
- Remembering a per-width manual preference across round-trips (we restore the fixed wide default).
- Changing the breakpoint values or the rail width.
- Touching the resizable-panel drag behavior at wide widths.
