# Live-test fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use `- [ ]`.

**Goal:** Fix 5 bugs found in the live composer test: (1) light Freya chrome on a dark system, (2) menus no longer open / don't dismiss (Popover regression), (3) no padding inside the composer input, (4) editor doesn't follow the caret when typing past the line cap, (5) thread doesn't stick to the bottom when new messages arrive.

**Architecture:** Freya-native fixes. Global dark theme via `use_init_theme(dark_theme)`; menu open/dismiss reworked toward Freya's proven dismissal (the `Select`/`Menu` `on_global_pointer_press` outside-area pattern + correct layering, NOT a `Relative(-1)` backdrop); scroll behaviors via `use_scroll_controller` + `ScrollController` (`ScrollPosition::{Start,End}`, `ScrollRequest`).

**Tech Stack:** Rust, Freya 0.4.0-rc.23, `freya-testing`.

## Global Constraints

- Reuse Freya built-ins / patterns (Rule 0 + Rule 4); confirm any uncertain API against `/run/media/system/fastdrive/repos/freya/crates/` or the catalog `docs/reference/freya-components-catalog.md`.
- **Confirmed APIs:** `use_init_theme(dark_theme)` (global theme; `dark_theme`/`light_theme` fns in `freya::prelude`; ref `freya-devtools-app/src/main.rs:59`). `use_scroll_controller(ScrollConfig)` → `ScrollController`; `ScrollPosition::{Start,End}`; `ScrollController::scroll_to(position, direction)` / `ScrollRequest`; `ScrollView::new().scroll_controller(ctrl)` or `new_controlled(ctrl)` (verify exact builder in `scrollviews/scrollview.rs`).
- No hardcoded hex (Theme accessors). Hand-formatted (no `cargo fmt`); `cargo clippy -p oxide-ui -p oxide-freya` clean (pre-existing `state.rs:280` warning out of scope). Build from `oxide-app/` with `LIBRARY_PATH=/tmp/oxidemx-lib-links`.
- **After the last task, rebuild the binary** (`cargo build -p oxide-freya --bin oxide-freya`).
- Bug fixes stand alone (Rule 2) — no scope creep.

---

### Task 1: Global dark theme

**Files:** Modify `oxide-app/crates/oxide-freya/src/app.rs` (the `shell()` root component) OR `main.rs` `root_app()`.

**Interfaces:** Produces: a root that initializes Freya's global theme to dark, so all built-in components (Menu/Switch/SegmentedButton/RadioItem/ScrollView scrollbars/Input) inherit dark chrome.

- [ ] **Step 1:** at the very top of the root component's `render` (the outermost component that wraps everything — `app::shell()`), add `use_init_theme(dark_theme);` (import `dark_theme` from `freya::prelude`). Confirm `dark_theme` is the correct symbol (`grep -rn "pub fn dark_theme" /run/media/system/fastdrive/repos/freya/crates/`); if it's `DARK_THEME` const instead, use that.
- [ ] **Step 2:** Build the binary + run the shell snapshot (`snapshot_shell`); Read the PNG — confirm no light chrome (scrollbar, any built-in) remains. (The custom rects were already dark; this fixes the framework defaults.)
- [ ] **Step 3:** `cargo test -p oxide-freya --bin oxide-freya` green; clippy clean.
- [ ] **Step 4: commit** `fix(oxide-freya): initialize global dark theme (use_init_theme)`

---

### Task 2: Fix Popover menu open + outside-click dismiss (regression)

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/menu/popover.rs`. Reference `/run/media/system/fastdrive/repos/freya/crates/freya-components/src/{select.rs,menu.rs,popup.rs,attached.rs}`.

**Interfaces:** Public API unchanged (`Popover::new(anchor).open(bool).placement(Placement).on_dismiss(EventHandler<()>).content(Element)`). Behavior fixed: (a) clicking the trigger OPENS the menu adjacent to it in the real nested (deep-layer) context; (b) a press OUTSIDE the menu (anywhere in the app) or Escape DISMISSES it; (c) a press on the menu content does not dismiss; (d) the open-toggling trigger press does not immediately re-dismiss (no flicker).

**Root causes to fix:**
1. The dismiss backdrop is on `Layer::Relative(-1)` — relative to the Popover's (deep) parent, this paints BELOW the rest of the app, so outside-clicks land on app content (higher layer) and never reach the backdrop → dismiss never fires.
2. In the nested context the open path doesn't reliably surface the menu.

**Approach (rework toward Freya's proven dismissal — read `select.rs` first):** Freya's `Select` dismisses via `.on_global_pointer_press(move |_| open.set(false))` on the dropdown root, combined with the content `stop_propagation`ing its own presses, and renders the dropdown on `Layer::Overlay`. Adopt that: drop the `Relative(-1)` backdrop; instead fire `on_dismiss` from an `on_global_pointer_press` on the Popover root that is SUPPRESSED when the press is on the content (content `stop_propagation`s) or the anchor (the anchor's own `on_press` already `stop_propagation`s / the toggle owns it). If a global-press handler also catches the opening trigger press (flicker), guard it the way `Select` does (the press that toggles `open` and the global handler are reconciled by the open-state check + `set_if_modified`, or by ignoring the first global press in the same frame the menu opened). Render content on `Layer::Overlay` via `Attached` as today. Verify the menu paints adjacent and on-screen (Placement::Above from a bottom toolbar).

- [ ] **Step 1: failing test** — a `freya_testing` test that mounts a `Popover` whose anchor is a button nested a few rects deep (mimicking the toolbar), drives a `click_cursor` on the anchor, polls, and asserts the content label renders (open works); then a second `click_cursor` outside the content asserts `on_dismiss` fired (e.g. a flag set / content gone). If a true click-driven open is too harness-fragile, at minimum keep the existing static open/closed tests green AND add a unit-level assertion of the dismissal predicate (press-inside-content vs press-outside).
- [ ] **Step 2: run → FAIL** (or observe current broken behavior).
- [ ] **Step 3: implement** the Select-style dismissal + correct layering; remove the `Relative(-1)` backdrop logic.
- [ ] **Step 4:** run tests → PASS; `snapshot_popover_anchored` + `snapshot_composer_full` re-render; Read PNGs — menu paints adjacent + on-screen. clippy clean.
- [ ] **Step 5: commit** `fix(oxide-ui): Popover opens + dismisses in nested context (Select-style global-press)`

---

### Task 3: Composer input inner padding

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/composer/editor.rs`.

**Interfaces:** unchanged. The editor's text/placeholder gets inner horizontal+vertical padding so text doesn't touch the card edge.

- [ ] **Step 1:** in `editor.rs`'s render, add padding (e.g. `Gaps::new(8., 12., 8., 12.)`) to the rect that wraps the `paragraph()`/placeholder so both the typed text and the placeholder overlay are inset. Keep the placeholder overlay aligned with the text (same inset).
- [ ] **Step 2:** run `snapshot_editor_multiline` + `snapshot_composer_collapsed`; Read PNGs — text + placeholder are padded, not edge-touching.
- [ ] **Step 3:** `cargo test -p oxide-ui composer::editor` green; clippy clean.
- [ ] **Step 4: commit** `fix(oxide-ui): pad composer editor text inset`

---

### Task 4: Editor follows caret past the line cap

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/composer/editor.rs`.

**Interfaces:** unchanged. When content exceeds `cap_px` and the editor body scrolls, typing keeps the caret/last line visible (scrolled to End).

- [ ] **Step 1:** give the editor's inner `ScrollView` a `ScrollController` (`use_scroll_controller(ScrollConfig { default_vertical_position: ScrollPosition::End, ..})`), and on each edit (the key-down / value-change path) request scroll-to-End (`controller.scroll_to(ScrollPosition::End, Direction::Vertical)` or the `ScrollRequest` equivalent — verify the exact call in `use_scroll_controller.rs`). Only when the editor is in the scrollable (over-cap) state.
- [ ] **Step 2:** add/extend a test or the `snapshot_editor_multiline` with >cap lines; Read PNG — the last typed line is visible (scrolled to bottom).
- [ ] **Step 3:** `cargo test -p oxide-ui composer::editor` green; clippy clean.
- [ ] **Step 4: commit** `fix(oxide-ui): editor follows caret to bottom past line cap`

---

### Task 5: Thread sticks to bottom on new messages

**Files:** Modify `oxide-app/crates/oxide-freya/src/regions/main_region.rs` (the thread `ScrollView` at line 49).

**Interfaces:** Produces: the thread scroll pins to the bottom when new turns/streaming text arrive **iff** the user is already at (or near) the bottom; if the user has scrolled up, their position is preserved.

- [ ] **Step 1:** give the thread `ScrollView` a `ScrollController` (`use_scroll_controller`). Track whether the user is at the bottom (read the controller's scroll position / `ScrollPosition`, or a near-bottom threshold). On a change to the turn count or `live_assistant` length (derive a key from `tx.turns.len()` + `tx.live_assistant.len()`; a `use_effect`/`use_memo` on that key), if at-bottom, `scroll_to(End)`; else do nothing.
- [ ] **Step 2:** confirm `ScrollController` exposes current position / a way to detect at-bottom (read `scrollviews/use_scroll_controller.rs`); if not directly, track via the `ScrollView`'s scroll callback or compare scroll offset to max. Implement the closest-correct at-bottom detection; document if approximate.
- [ ] **Step 3:** build + `snapshot_shell`; (can't simulate streaming in a static snapshot — verify it compiles + renders, and live-verify after). `cargo test -p oxide-freya` green; clippy clean.
- [ ] **Step 4:** **Rebuild the live binary** (`cargo build -p oxide-freya --bin oxide-freya`).
- [ ] **Step 5: commit** `fix(oxide-freya): thread sticks to bottom on new messages when at-bottom`

---

## Self-Review

- Each bug → exactly one task; root cause named; Freya-native fix. Task 2 (the regression) is highest-risk and reworks toward Freya's proven `Select` dismissal rather than re-attempting the backdrop. Tasks 4/5 share `use_scroll_controller` but on different ScrollViews (editor vs thread) — no overlap. The `dark_theme`/`scroll_to` exact symbols are flagged to confirm against Freya source before use (not placeholders — concrete with a verification step).
