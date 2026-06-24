# OxideMX Composer — implementation plan (architecture)

> This is the **architecture plan** the composer prompt asks for (component tree, token
> model, editor strategy, overlay approach, animation mapping). After approval it becomes a
> bite-sized `superpowers:writing-plans` task plan, then a subagent-driven build. Contract:
> `design/composer/OxideMX - Composer.freya.json` (the `.freya.json`). JSX is a behavior
> tiebreaker only.

## Goal

Replace the bottom-of-thread `PromptInput` (a styled single-line `Input` wrapper) with the
full **Composer** chassis from the design study: a multi-line rich editor, prediction strip,
attach menu, grouped provider menu, attachment chips, send/working/disabled states, and an
activity status line — faithful to the `.freya.json` spacing, radii, accent rules, and
animation timings.

## Environment facts (verified this session)

- **Freya** `0.4.0-rc.23` (path dep `/run/media/system/fastdrive/repos/freya/crates/freya`).
  Builder API (`rect().child()`), NOT `rsx!`.
- **Existing token module:** `crates/oxide-ui/src/tokens.rs` — `Theme` (flat accessors) +
  `Accent{Cyan,Violet,Amber,Lime}` + `Theme::with_alpha(base,u8)`. Already carries every
  composer token EXCEPT the backdrop trio `bg0/bg1/bg2` and `shadowDeep`. Minor additions
  only.
- **Mount point:** `crates/oxide-freya/src/regions/main_region.rs:50-53` — the footer
  `.child(PromptInput::new(input.into_writable()).on_submit(...))` of the Vertical
  `Content::Flex` column. The new `Composer` replaces this child.
- **Freya built-ins available** (map 1:1 to the `.freya.json` `freya` fields):
  `Button` (`.style_variant`/`.theme_colors(ButtonColorsThemePartial)`/`.on_press`),
  `Card` (`.theme_colors(CardColorsThemePartial)`), `Chip` (`.selected`/`.on_press`/`.theme`),
  `Menu`+`MenuButton`+`SubMenu` (overlay, Escape/outside-press close),
  `Select` (`.selected_item` + `MenuItem` children), `SegmentedButton`+`ButtonSegment`,
  `RadioItem`, `Switch` (`.toggled`/`.on_toggle`/`.theme_colors`), `Tooltip`+`TooltipContainer`,
  `ScrollView` (`.show_scrollbar(false)`), `Attached` (anchored overlay — for floating menus),
  `Popup`+`PopupBackground` (centered overlay / bottom-sheet substitute), `Portal` (animated
  overlay), `Loader`, `Slider`, `Tile`.
- **THE editor constraint (load-bearing):** the `Input` *component* is **single-line, plain
  text** — no multiline, no caret-color control, no per-run inline styling, no
  contenteditable. BUT `freya-edit` (`use_editable` → `RopeEditor` + `EditableEvent` +
  `EditorHistory`) is the real multiline editing engine behind `Input` and the
  `freya-code-editor` crate; it supports multiline content, a controllable cursor, and
  per-span styled rendering (`paragraph` + `text` spans + a cursor reference). Example:
  `freya/examples/text_editing.rs`. **The composer editor is a custom widget on
  `use_editable`, not a config of `Input`.** This is the single biggest piece of work.

## Token model

Extend `Theme` (don't restructure — Rule 2, no gold-plating):
- Add backdrop accessors `bg0()=#060a12`, `bg1()=#0a1018`, `bg2()=#070b11`, and
  `shadow_deep()=argb(0x9e,0,0,0)` (≈ rgba(0,0,0,0.62)).
- Per-attachment **tone** tokens are dynamic (`T.<tone>_16` fill / `T.<tone>_33` border for
  tone ∈ blue/green/peach/teal/mauve/accent). Add a `Theme::tone(&self, Tone) -> Color` +
  reuse `with_alpha(tone, 0x16|0x33)`. Define a small `enum Tone` in `oxide-ui`.
- Accent alpha ramp (`accent_10/14/1a/33/50`) stays computed via `with_alpha(accent, NN)`;
  no new stored fields. Changing `Accent` recolors every `accent_*`-derived surface for free
  (already true) — satisfies the "changing accent recolors everything" acceptance criterion.

## Component / file tree (new `oxide-ui` components)

```
crates/oxide-ui/src/components/
  composer/mod.rs            Composer  — orchestrates state + lays out the card stack
  composer/editor.rs         ComposerEditor — use_editable multiline editor (THE big one)
  composer/prediction.rs     PredictionStrip + PredictionChip + predict() data
  composer/attachment.rs     AttachmentRow + AttachmentChip (tone-tinted, removable)
  composer/toolbar.rs        Toolbar — AttachButton · ProviderPill · OptimizerChip · LineHint · SendButton
  composer/attach_menu.rs    AttachMenu (6 sources) on Attached/Menu
  composer/provider_menu.rs  ProviderMenu (models | settings views) on Attached/Menu
  composer/activity_line.rs  ActivityLine (model · thinking · status · '/ for commands')
  composer/config.rs         ComposerConfig (tweaks) + ProviderModel/Thinking/Tone enums
  resize_grip.rs             ResizeGrip (top-edge pointer-drag)
```
`Composer::new(value, config).on_submit(...).on_attach(...).theme(...)` is the public API the
mount calls. Internal sub-components stay private to the module. Keep files focused (Rule 2).

## Editor strategy (`composer/editor.rs`)

Built on `use_editable(|| String::new(), || EditableConfig::new().with_...)`:
- **Line blocks:** the `RopeEditor` is multiline; render each visual line as a `rect`
  (`.cline`) containing a `paragraph` of styled `text` spans. Caret = a 1px accent `rect`
  positioned from the editor's cursor row/col (cursor metrics from the editable cursor attr).
- **Caret color = accent:** the custom cursor rect is `th.accent()` (solves Input's missing
  caret-color control).
- **Auto-grow then scroll:** measured content height grows to `capPx = lineCap*23 + 6`
  (lineCap default 5), then the editor body becomes a `ScrollView` (`.show_scrollbar(false)`).
  `manualHeight` (from the grip) overrides, clamped `[capPx, 520]`.
- **Keys** (via `on_global_key_down` / the editable event pump):
  - `Shift+Enter` (or `Enter` when `send_on_enter=false`) → insert newline at caret (split
    the current line block).
  - `Enter` when `send_on_enter=true` → submit (reset editor + attachments + manualHeight).
  - `Tab` → accept first prediction / ghost suffix.
- **Live markdown on space** (`markdown=true`): on space keypress, scan the current line:
  line-start `#`/`##`/`###` → block style `md-h1/2/3`; inline `*x*`→bold, `_x_`→italic,
  `` `x` ``→code, `~~x~~`→strike — **markers consumed** (removed from the rope, the run
  re-styled). Implemented as a tokenizer over the active line that rewrites the rope + the
  per-span style map.
- **Ghost** (`prediction=ghost`): render a faded `contenteditable=false`-equivalent suffix
  span (non-selectable `text`, `faint`) of the top completion at line end; Tab accepts.

**Phasing within the editor (see Phasing):** multiline + caret + Shift+Enter split +
auto-grow + grip is the MVP; live-markdown-on-space + ghost are the second editor pass.

## Menus & overlay approach

- **Desktop:** floating menus via **`Attached`** anchored to the trigger button, placed
  `.top()` (the spec's `placement: above`). `AttachMenu`/`ProviderMenu` render their rows as
  `MenuButton`/`RadioItem`/`SegmentedButton`/`Switch`. Dismiss = outside-press/Escape (Menu
  built-in). The `ComposerCard` border switches to `accent_33` while any menu is open; the
  `AttachButton` glyph rotates 0→45° on open.
- **ProviderMenu** has two views (`models | settings`) toggled by internal state; groups
  Gemini / Claude / Local-LLM via `ProviderGroup` headers; selection is a `RadioItem` + check;
  thinking level is a `SegmentedButton` (Low/Med/High); prompt-optimizer + send-on-enter are
  `Switch` rows; "Composer settings" navigates to the settings sub-view.
- **Touch (responsive):** the same menu *content* re-hosts in a **`Popup`/bottom-sheet** for
  tablet/phone. Built behind the size-class so desktop ships first.

## Prediction data

Port the JSX's static `COMPLETIONS`/`NEXT` maps into `predict(partial, prevWord) -> Vec<Cand>`
(mode `complete` | `next`). The strip shows ≤3 chips (first accent-tinted), `Tab` accepts the
first. This is a faithful canned demo predictor (no model call) gated by the `prediction`
tweak; real model-driven prediction is out of scope for this slice.

## Send / working / disabled states (`SendButton`)

- **disabled** (empty editor & no attachments): `surface1` bg, `faint` glyph, no shadow.
- **enabled:** `accent` bg, `crust` glyph, `shadow 0 4 12 accent_50`.
- **working:** `surface2` bg, `red` stop glyph (swaps `send`→`stop` icon).

## Animation table → Freya mapping

| `.freya.json` anim | drives | Freya impl |
|---|---|---|
| `slide` 140ms ease-out | AttachMenu/ProviderMenu enter | `use_animation` opacity 0→1 + translateY 6→0 (or `Portal` ease) |
| `fade` 120ms | PredictionStrip appearance | `use_animation` opacity 0→1 |
| `rotate` 140ms | AttachButton glyph 0→45° | `use_animation` rotate on `open` |
| `slide` 160ms | Switch knob translateX | Freya `Switch` built-in animates this |
| `pulse` 1400ms | ThinkingBubble dot | (thread-side; not composer) |

(`freya::prelude` animation hooks; per `reference_freya_dev` — `use freya::animation::*` if
not in prelude.)

## Responsive classes (the `.freya.json` `responsive` block)

- `wide >=1180`: full composer; provider pill shows name + thinking badge; strip inline.
- `compact 920–1179`: provider pill → icon + thinking badge (name hidden); line hint hidden.
- `tablet 600–919`: full-width; attach/provider menus → bottom sheets.
- `phone <600`: full-bleed; prediction strip horizontally scrolls; menus → sheets; resize
  grip hidden (auto-expand only); send target 44px.

This composer's responsive work folds into **2b-P4** (the responsive desktop phase), reusing
the size-class seam established there.

## Testing

Per-surface headless snapshots in `oxide-freya` (the `render_to_file` harness already used for
`snapshot_shell`/`snapshot_thread_p1`/`snapshot_sidebar_p2`): `snapshot_composer_collapsed`,
`_with_attachments`, `_attach_menu_open`, `_provider_menu`, `_multiline`. Plus `oxide-ui` unit
tests for `predict()`, the markdown tokenizer (markers consumed), send-state resolution, and
`capPx`/clamp math. **Recurring gotcha guard:** every styled row with a `Size::flex` child
gets `.content(Content::Flex)` (FREYA-PATTERNS #1); side rails get `.show_scrollbar(false)`.

## Phasing (recommended)

- **Slice 1 — Chassis + functional editor:** token additions; `Composer` layout; activity
  line; attachment chips; toolbar; attach menu; provider menu (both views); prediction strip
  (chips + Tab-accept, static data); send/working/disabled; **editor = multiline + caret +
  Shift+Enter split + auto-grow + resize grip** on `use_editable`. Desktop floating menus.
  Mount it (keep `PromptInput` until parity, then swap). Headless-snapshot each surface.
- **Slice 2 — Editor rich text:** live-markdown-on-space (consume markers, block + inline
  styling) + ghost completion. Hardest text-transform work, isolated.
- **Responsive:** folds into 2b-P4 (size classes + bottom-sheet menus).

## Open ambiguities (resolve before locking the executable plan)

1. **Editor fidelity sequencing** — Slice-1 ships a working multiline editor but defers
   live-markdown-render + ghost to Slice 2. (Recommended.) Or build full editor fidelity in
   one slice? Or chassis-only with a plain field?
2. **Worktree/branch placement** — fold as a dedicated phase in the existing `oxidemx-2b`
   worktree (shares `oxide-ui`/theme) vs. its own branch/worktree off phase1. (Recommend: a
   phase in 2b.)
3. **Prediction** — ship the canned static `COMPLETIONS`/`NEXT` predictor from the JSX
   (faithful, demoable), behind the `prediction` tweak. (Recommended.) Or stub the strip with
   no data?
4. **Tweaks surface** — plumb tweaks as a `ComposerConfig` with the documented defaults
   (prediction=chips, lineCap=5, markdown=true, activity=true, accent=cyan); no separate
   tweaks-panel UI this slice. (Recommended.) Or build a tweaks panel?
5. **Responsive timing** — confirm the composer's bottom-sheet/size-class work folds into
   2b-P4 rather than shipping in this composer slice. (Recommended.)
