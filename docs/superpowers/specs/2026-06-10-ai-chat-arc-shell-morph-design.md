# AI Chat "Arc Shell" Morph — Design

**Date:** 2026-06-10 · **Status:** Approved (brainstormed with visual mockups; user selected "Arc shell" end-state, "× closes / wheel goes back" semantics, and the canvas-cap-morph implementation strategy)

## Summary

When the radial menu's active page becomes the **AI Assistant** page, the round disc animates into a chat window: the disc splits in half horizontally, the two halves travel apart while flattening — the top half becomes a slim **header arc** (page title, drag surface, × button), the bottom half becomes the **input footer** backdrop — and the conversation fills the space between. The window is draggable by the header arc while chat is active. Reverse animation plays when leaving the page.

## Interaction model

- **Enter:** wheel-cycling onto the AI page, or app-context auto-selection of it, triggers the morph. Toggle mode is forced on (keyboard needed for chat). Releasing the gesture button while on the AI page enters toggle mode instead of dispatching a slice.
- **Leave:** wheel scroll reverse-morphs back to the disc on the neighboring page (existing `cycle_page` semantics).
- **Close:** **×** in the header arc dismisses the overlay entirely — same code path as Escape / right-click / click-outside, all of which keep working.
- **Drag:** mouse-down on the header arc (outside the × hit area) issues `iced::window::drag(id)` (verified present in iced 0.14.0: `iced_runtime-0.14.0/src/window.rs:299`). Window stays where dropped until the next Show (which repositions to cursor as today).
- **Persistence:** `ai_history`, `ai_session_id`, `ai_pending_question` persist across morphs for the overlay process lifetime (unchanged).

## Morph animation (~480 ms, one new `Tween`)

Driven by `RadialState::ai_morph: Tween` (0.0 = disc, 1.0 = chat), stepped by the existing 16 ms Tick.

| window | what happens |
|---|---|
| 0–90 ms | canvas slices + all shader layers fade out; two painted semicircle **caps** (theme-gradient, drawn by a new canvas layer) fade in over the disc footprint |
| 60–420 ms | caps translate to window top/bottom edges while flattening (scaleY 1.0 → ~0.35), spring-eased |
| 280–480 ms | chat region (history scrollable + input row + pending-question buttons) fades/slides in; input auto-focused |

Exit is the exact reverse. Wheel events mid-flight retarget the tween (no stuck states). Timings/easing ship as a `menu.ai_morph` enter/exit block using the existing `ElementAnimation`/track config schema.

## Window management

- Base window stays **484×484**. On morph start: a single `iced::window::resize(id, 484×760)` batched with one `MoveOverlay(x, y−138)` D-Bus call so the disc center is unchanged (window is transparent/frameless — the resize is invisible). All animation afterward is drawing-space only.
- On reverse-completion or dismiss/hide: resize back to 484×484 and restore y (+138) (skip reposition when hidden; next Show repositions anyway).
- Morph state resets on Hide so every Show starts as a clean disc.

## Code shape

- **New:** `overlay-rs/src/chat_shell.rs` — pure morph geometry (cap rects/corner radii/alpha as functions of progress; hit-tests for × and the drag region) + a `canvas::Program` painter for the caps. Unit-tested.
- **Changed:** `overlay-rs/src/radial.rs` — `ai_morph` tween + `is_ai_page()`/morph-phase accessors; retarget in `cycle_page`, `apply_focused_class`, `show`, reset in `hide`.
- **Changed:** `overlay-rs/src/app.rs` — new messages `ChatHeaderPressed`, `ChatClose`; resize/reposition tasks at morph start/end; view composition: global fade on disc layers from the tween, cap painter layer, chat region built from the existing `build_ai_panel` internals restyled to fill the middle (replacing the 260 px sidebar overlay).
- **Changed:** `oxidemx-shared` animation config — `ai_morph` element defaults.

## Edge cases

Config reload mid-chat rebuilds pages but preserves chat state and morph progress; Escape dismisses even with input focused (current window-event path); debounced wheel still applies during chat; drag-mode wheel onto AI page + release → toggle mode, no dispatch.

## Testing

Unit tests: cap geometry at t = 0 / 0.5 / 1, hit-test boundaries, tween retarget mid-flight. Manual checklist (live-test recipe): enter/exit morph, ×, Escape, drag, wheel-during-morph, drag-mode entry, config reload during chat.
