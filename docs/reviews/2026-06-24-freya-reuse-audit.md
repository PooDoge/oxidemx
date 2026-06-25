# Freya Reuse + Per-Slice Quality Audit — Composer & Menu slices

**Date:** 2026-06-24
**Scope:** `oxide-app/crates/oxide-ui/src/components/{composer/*, menu/*, resize_grip.rs}`
**Reference:** `docs/reference/freya-components-catalog.md` · Freya source `/run/media/system/fastdrive/repos/freya/crates/freya-components/src/`
**Method:** plain Read/Grep (no Serena — single global active project).

This is analysis only. No code was changed.

---

## 1. Prioritized findings

Priority key: **P1** = clear reuse win, low/med effort, rides upstream fixes; **P2** = worth doing, moderate payoff; **P3** = nice-to-have / idiom polish; **OK** = genuinely custom, leave alone.

| # | Component (file:line) | Finding | Freya built-in to reuse | Effort | Recommendation |
|---|---|---|---|---|---|
| F1 | `composer/toolbar.rs:208-226` (attach btn), `:274-299` (provider pill), `:383-410` (send btn) | Three interactive controls hand-rolled as `rect().on_press()` with **no hover state, no focus ring, no a11y role**. Static bg only. | `Button` (`.flat()` / `.theme_colors`) — gives hover_background, focus_border_fill, press handler, a11y for free | Med | **Adopt Button** for attach + send. Provider pill is borderline (custom inner layout) — see F2. |
| F2 | `composer/toolbar.rs:274-299`; `prediction.rs:127-141`; `attachment.rs:163-176` | "Pill/chip" surfaces (provider pill, prediction chips, attachment chips) all hand-roll the rounded tinted-border-bg pattern with no hover. | `Chip` (`.selected()`, `.on_press()`, hover/selected bg + optional `TickIcon`) | Med | **Prediction chips → `Chip`** (selected==first item; rides hover for free). Attachment + provider pill keep custom (multi-element internal layout Chip can't host cleanly). |
| F3 | `menu/popover.rs` (whole file, 262 lines) | Popover re-implements **Select's exact** open animation (scale 0.9→1, opacity 0→1, slide −8→0, 125ms `Ease::Out`/`Quart`) + auto-flip math + outside-press/Escape dismiss, by hand. | `Select` (animation+flip+dismiss) / `Menu` (`on_close` + outside-press) | High | **Keep custom** but document the divergence. Select hard-requires `MenuItem` children + a `selected_item` button and owns its open-state; our Popover is content-agnostic with caller-owned `open` + arbitrary anchor. Reuse isn't free here. **Action: extract the 3-part anim constants into one shared helper** so a Select upstream tweak is a 1-line sync (F8). |
| F4 | `composer/provider_menu.rs:128-206` (models view), `attach_menu.rs:55-78` | Menu rows built into a plain `rect().children(Vec<Element>)` — no keyboard nav, no Escape-within-menu, no roving focus. Freya's `Menu`/`MenuItem` provide all of it. | `Menu` + `MenuItem` (keyboard nav, focus, a11y) — we already lean on `MenuButton` via `MenuRow` | Med | **Partially adopt:** `MenuSurface` already wraps `Menu`; rows go through `MenuButton`. Gap: the **container is a bare `rect`, not arrow-key navigable**. Consider `MenuItem` for arrow-key/Enter nav. Med value (mouse-first UI). |
| F5 | `composer/provider_menu.rs:208-273` (`model_row`) | Re-implements `MenuRow`'s internals by hand (copies the `MenuButton` + `menu_theme` + `Content::Flex` layer) because `MenuRow` can't host a `RadioItem` leading slot or bold title. Comment block (`:217-263`) admits the duplication. | `MenuRow` (extended) | Low | **Extend `MenuRow`** with `.leading(Option<Element>)` and `.title_bold(bool)` so `model_row` collapses to one `MenuRow` call. Removes ~50 lines of copy-paste + the drift risk the comment flags. |
| F6 | `composer/editor.rs` (whole, 288 lines) | Hand-built multiline editor on `use_editable` + `paragraph()` + manual caret/selection/drag plumbing. | `Input` (single-line only) / `use_editable` (already used) | High | **Keep custom** — justified. `Input` is `max_lines(1)`; no built-in multiline editor exists. The module header documents this correctly. Only nit: F9 (sync-on-render). |
| F7 | `composer/editor.rs:266-269` & `mod.rs:254` | Editor wraps its body in `ScrollView::new().show_scrollbar(false)` purely to clip+scroll an over-cap rope; auto-grow height is computed manually from `on_sized`. | `ScrollView` (already used) — fine. But the **manual height pump** (`content_h` → `on_height` → parent `content_height` → back into `body_height`) round-trips through three signals. | Low | **OK, but see F9.** ScrollView use is correct. The height round-trip is the real cost, not ScrollView. |
| F8 | `menu/popover.rs:118-143` & Freya `select.rs:138-160` | The two animation factories are byte-identical except slide end (`0.` vs Freya's `1.`). No shared source → an upstream easing change silently diverges. | — (internal helper) | Low | **Extract `fn menu_entrance_anim(open: bool) -> (AnimNum,AnimNum,AnimNum)`** in `menu/` and call from Popover (and any future overlay). One place to sync with Select. |
| F9 | `composer/editor.rs:124-129` | External→editor sync runs **every render** with a `String` allocation (`editable.editor().read().to_string()`) and a full `editor.set()` whenever they differ — fires on the parent's own height-signal churn too. | — (idiom) | Low | Gate the sync behind a cheap length/version check, or only when `value` actually changed (track last-synced). Avoids a rope rebuild + history clear on unrelated re-renders. |
| F10 | `composer/icons.rs:31-39` | `svg_string` does `format!` + two `String` allocations + a `.replace()` **per icon, per render**. `icon()` is called dozens of times per Composer frame (toolbar, rows, chips). | — (idiom) | Med | **Cache** the built SVG bytes. Either memoize by `(name,color)` or precompute the static path table once. Hot path: every menu/toolbar render re-stringifies every glyph. |
| F11 | `composer/attach_menu.rs:56-73`, `provider_menu.rs:129-206`, `prediction.rs` | Row/chip `Vec<Element>` built in a loop with **no `.key()` / `DiffKey`** on items. `SegmentedButton` segments DO set `.key(0/1/2)` (`provider_menu.rs:155`), proving the pattern is known — but menu rows and attachment chips omit it. | `KeyExt`/`.key()` on list items | Low | **Add stable keys** (source id / model id / index) to looped rows + chips. Prevents state-reuse bugs if a built-in stateful child (Chip/Switch/MenuButton hover state) is ever reordered. Cheap insurance. |
| F12 | `composer/mod.rs:181`, `:144-153`, `:156-167` | `value.read()` re-derived multiple times per render (`line_count`, `send`, strip, attachment clone). `attachments.read().clone()` clones the whole Vec to pass into `AttachmentRow`. | — (idiom) | Low | Minor: read once into a binding. The `attachments.read().clone()` is needed (component owns `Vec<Attachment>` by value) — acceptable, but could pass a slice/`Arc` if the list grows. |
| F13 | `composer/toolbar.rs:191-204` | `use_animation` plus-rotation (0→45°) is correct + idiomatic. | `use_animation` (already used) | — | **OK** — exemplary use. |
| F14 | `resize_grip.rs` (whole) | Hand-rolled top-edge drag handle via global pointer move/press + `press_y` state. | `ResizableContainer`/`ResizableHandle` | Med | **Keep custom.** `ResizableHandle` resizes *between panels in a `ResizableContainer`*; our grip resizes a single free-standing editor body against a px cap with custom clamp. Adopting ResizableContainer would force the whole composer card into a panel model — disproportionate. Custom is justified; the `clamp_height` unit test is good. |

---

## 2. Per-finding detail

### F1 — Toolbar controls should be `Button` (P1)
`toolbar.rs` builds the attach button (`:209`), provider pill (`:274`), and send button (`:383`) as `rect().background(...).on_press(...)`. None has a hover background, focus border, or `a11y_role`. Freya's `Button` (`crates/freya-components/src/button.rs`) ships `hover_background`, `focus_border_fill`, press handling, `enabled(bool)`, and a11y via `ButtonColorsTheme`/`ButtonLayoutTheme`. The send button's three-state styling (`:362-381`) maps cleanly onto `.theme_colors(...)` per state; `Disabled` maps to `.enabled(false)`. Adopting `Button` for attach + send removes the manual `if on_send.is_some()` press-wiring (`:400-410`) and gives hover/focus for free, riding any upstream Button fixes (e.g. focus-ring a11y).

### F2 — Pill/chip surfaces (P2)
The rounded tinted pill appears 3× with copy-pasted `corner_radius` + `Border::new().fill(alpha)` + static bg and **no hover**:
- prediction chip `prediction.rs:127-134`
- attachment chip `attachment.rs:163-173`
- provider pill `toolbar.rs:274-291`

`Chip` (`chip.rs`) gives `selected`/`hover`/`focus` backgrounds + optional `TickIcon` and `on_press`. **Prediction chips are the clean win**: `is_first` → `.selected(true)`, label as child, hover for free. Attachment chip and provider pill embed a multi-child layout (icon + 2-line text col + remove btn / badge + chevron) that `Chip` (single child line) can't host without fighting it — keep those custom, but factor the shared "tinted surface" rect into one local helper to kill the triplicated border/radius code.

### F3 / F8 — Popover vs Select (P3, keep but share constants)
`popover.rs` is a faithful, content-agnostic re-creation of `Select`'s overlay. The catalog (Popup/Select/Menu entries) and the module's own 40-line doc comment justify *why* Select can't be reused: Select owns its `open` state internally, requires `MenuItem` children, and renders a `selected_item` trigger button — our Popover needs a caller-owned `open` flag and an **arbitrary anchor element** (a +button, a pill). That is a real API mismatch; **reuse is not worth it.** The one cheap improvement: the animation factory (`popover.rs:118-143`) is identical to `select.rs:138-160` (modulo slide end `0.` vs `1.`). Extract it into a shared `menu_entrance_anim(open)` helper so an upstream Select easing change is a one-line sync rather than a silent divergence.

### F4 — Menu containers lack keyboard nav (P2)
`MenuSurface` correctly wraps Freya's `Menu` and rows go through `MenuButton` (via `MenuRow`), so hover theming + press are real. But `attach_menu.rs` and `provider_menu.rs` pour rows into a bare `rect().children(rows)` — the `Menu`'s arrow-key/Enter roving focus only applies to direct `MenuItem` descendants. For a mouse-first composer this is low-severity, but if keyboard nav is a goal, the rows should be `MenuItem`s inside the `Menu`, not a `rect`. Note `provider_menu.rs:16` explicitly (and correctly) rejects `SubMenu` for the models↔settings swap — that reasoning is sound; this finding is only about intra-menu nav.

### F5 — `model_row` duplicates `MenuRow` (P1, low effort)
`provider_menu.rs:208-273` carries a 7-line comment block (`:217-263`) explaining that it copies `MenuRow`'s `MenuButton` + `menu_theme` + `Content::Flex` internals because `MenuRow` can't take a `RadioItem` leading slot or a bold title. The fix is to **extend `MenuRow`** (`menu/row.rs`) with `.leading(Option<Element>)` (mirrors Freya `Tile::leading`) and `.title_bold(bool)`. Then `model_row` becomes a single `MenuRow::new(th).leading(radio).title_bold(true)...` call, deleting ~50 lines and the explicitly-flagged drift risk. This is the highest value-per-effort item.

### F6 / F7 / F9 — Editor (mostly OK)
The multiline editor (`editor.rs`) is genuinely custom and correctly justified: Freya ships no multiline text component (`Input` is `max_lines(1)`), so building on `use_editable` + `paragraph()` is the right call, and the header documents it well. `ScrollView` use is idiomatic. **F9** is the only real defect: the external→editor sync (`:124-129`) runs every render and, on mismatch, allocates a `String`, calls `editor.set()`, clears history, and clears selection. Because the parent pumps a height signal back in (`mod.rs` `content_height`), this can fire on unrelated re-renders. Track a last-synced marker (or compare lengths first) to avoid rope rebuilds on cosmetic re-renders.

### F10 — Icon stringification per render (P2)
`icons.rs:31-44`: every `icon(name,size,color)` call does `format!` of the full `<svg>` + two `String` allocations + `.replace("currentColor", ...)`, then `.into_bytes()`. The Composer calls `icon()` for every toolbar control, menu row, chip, and badge — dozens per frame, re-stringified each render. Cache by `(name, color_hex)` (icons are static paths; only color varies) or precompute the wrapped template once. Clear, measurable hot path.

### F11 — Missing keys on looped children (P3, cheap insurance)
`SegmentedButton` segments set `.key(0/1/2)` (`provider_menu.rs:155-181`) — so the team knows the pattern — but the looped menu rows (`attach_menu.rs:56`, `provider_menu.rs:129-140`), prediction chips, and attachment chips set no `.key()`. Today the lists are static-ish, but `MenuButton`/`Switch`/`Chip` carry internal hover/animation state; without a stable `DiffKey`, a reorder/filter change (e.g. provider filtering) risks state bleeding between items. Add `.key(source.id)` / `.key(model.id)` / `.key(idx)`. Near-zero cost.

### F12 — Redundant reads / Vec clone (P3)
`mod.rs:181,144,156,182` read `value`/`attachments` multiple times per render. Bind once. `attachments.read().clone()` (`:157`) is a full-Vec clone to hand ownership to `AttachmentRow` (which takes `Vec<Attachment>` by value) — acceptable for a handful of attachments; revisit only if the list can grow large.

---

## 3. Genuinely custom — leave alone

- **`editor.rs` (`ComposerEditor`)** — no multiline built-in exists; `use_editable` foundation + Enter-intercept-before-submit is the correct, well-documented approach. (Fix F9 only.)
- **`popover.rs` (`Popover`)** — content-agnostic anchored overlay with caller-owned `open`; Select/Menu can't host an arbitrary anchor. Backdrop-on-`Layer::Relative(-1)` dismissal guard is a genuinely clever, correct solution. (Share constants via F8.)
- **`resize_grip.rs` (`ResizeGrip`)** — single-body px-cap resize; `ResizableContainer` is a multi-panel model that would over-constrain the composer card. Good `clamp_height` test.
- **`prediction.rs` predictor logic** (`predict`, `trailing_word`, `next_for`) — pure domain logic, well unit-tested; nothing in Freya covers it.
- **`config.rs` / `attachment.rs` data tables** (`MODELS`, `ATTACH_SOURCES`, `sample_attachment`) — static registries with copy-drift guard tests; correct.
- **`icons.rs` SVG registry** — bespoke glyph set; keep the table, just cache the render output (F10).
- **`menu/theme.rs` `menu_theme()`** — the `hover_background = surface_hi()` fix is the right override over Freya's light default; correct use of `*ThemePartial`.
- **`toolbar.rs` plus→× `use_animation`** (F13) — exemplary idiomatic animation.

---

## 4. Cross-cutting idiom notes

- **`.into_element()` churn:** the `if let Some(h) = handler { x.on_press(..).into_element() } else { x.into_element() }` pattern repeats in toolbar/attachment/prediction. Harmless but verbose; a small `maybe_on_press` extension trait would dedupe it. P3.
- **Keys discipline:** adopt `.key()` on every looped child as a standing rule (F11) now that stateful built-ins (Switch/Chip/MenuButton) appear inside loops.
- **Anim-constant single-source (F8)** and **MenuRow as the one row primitive (F5)** are the two structural moves that most reduce future drift.
