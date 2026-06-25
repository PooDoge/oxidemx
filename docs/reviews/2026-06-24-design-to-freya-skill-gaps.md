# `claude-design-to-freya` SKILL — per-slice gap audit (composer + menu builds)

**Date:** 2026-06-24
**Scope:** Audit the `claude-design-to-freya` SKILL against the ground-truth build ledger
(`oxidemx-2b/.superpowers/sdd/progress.md`) for the **Composer Slice 1** (S1 T1–T14) and the
**Menu/Popover Slice** (MENU T1–T5). Cross-checked against the built source
(`oxide-app/crates/oxide-ui/src/components/{composer,menu}/`) and the reuse reference
(`docs/reference/freya-components-catalog.md`).

**Verdict:** the skill is already strong — `Content::Flex` is literally rule #1, and §4b /
§"Use Freya built-ins" cover most fiddly-CSS and reuse cases. But several gotchas that the
ledger records as *real compile/visual rounds lost* are **missing or under-weighted**, and a few
ledger-confirmed *corrections* never got folded back in (the contribute-back loop didn't close).
This review proposes (1) a gap table, (2) ready-to-paste skill sections, (3) structural fixes.

---

## (1) Gap table

| Pain point · how it bit us | Ledger ref | Current skill coverage | Proposed fix |
|---|---|---|---|
| **`Content::Flex` on any rect with a `Size::flex` child** (off-screen send button / detached dots / escaped chevron) — recurred ~4× in composer + 2× in sidebar. | P2 fix `0684c00`; POST-P2 `c517af4`; "4TH recurrence"; S1 T11/T12 explicitly note it; FINAL REVIEW "holds on ALL 10 flex children". | **STRONG** — rule #1 with a grep-before-finish ritual + the "trailing element clipped at the edge" tell. | Keep as-is. Add ONE line: the failure also fires when a **flex-width *label*** is used purely as a right-pusher (no visible flex box) — that's the non-obvious case that bit P2. (Half-present; make it explicit.) |
| **Freya `Input` is single-line only** — a multiline/rich editor must build on `use_editable`/`RopeEditor` + `paragraph()`, NOT `Input`. | S1 T8 (`editor.rs` built on `use_editable`; "no `EditableConfig` multiline flag → omit `max_lines(1)`"; no native paragraph placeholder → overlay label; click-caret offset while scrolled). | **MISSING.** Skill's built-in table maps "text input → `Input`" with NO single-line caveat. Composer editor is even listed as a justified custom component but the *reason* (Input can't multiline) isn't stated. | Add to §4b + built-in table: **"`Input` is single-line (`max_lines(1)` + Enter=submit). For a growing/newline editor use `use_editable` + `paragraph()` (no `max_lines`); placeholder = overlay `label` when empty; caret via `paragraph().cursor_color(accent)`; intercept `Enter` before `process_event` for Shift+Enter=newline vs Enter=submit."** |
| **Typed builder can't loop `.child()`** — each `.child()` call changes the builder's generic type, so a `for` loop over `.child()` won't compile. | S1 T5 ("typed builder can't loop `.child()`"); S1 T6 ("multi-child rows = `let mut row=rect(); for x { row=row.child(Component::new(..)); }`"). | **PARTIALLY WRONG.** §4 maps `.map()` → `for x in items { col = col.child(Row{..}); }`. That reassignment form IS the right one, but the skill never *warns* that the inline/chained `.child().child()` loop fails to typecheck, nor offers `.maybe_child(Option<Element>)` / `.children(Vec<Element>)`. | Add explicit gotcha: the **reassignment loop** (`let mut c = rect(); for x { c = c.child(..); }`) or `.children(vec)` / pre-built `Option<Element>` + `.maybe_child(..)`. Never chain `.child()` inside a loop expression. Also: pass the **`Component`** to `.child()`, not `.render()`/`.render().into_element()`. |
| **rect click handler is `.on_press(\|_: Event<PressEventData>\|)`, NOT `.on_click`.** | S1 T5 ("rect uses `.on_press(\|_: Event<PressEventData>\|)` NOT on_click"); confirmed in `menu/row.rs:188`. | **PRESENT but thin.** §4 maps `onClick → .on_press(move \|_: Event<PressEventData>\| …)`. | Promote to a one-line gotcha in §Gotchas (there is no `.on_click` on rects/components — it's always `.on_press` taking `Event<PressEventData>`; `MenuRow`-style wrappers may expose `on_press(())`). Skill is mostly fine; just make it un-missable. |
| **`svg()` takes BYTES** (`impl Into<SvgBytes>`) — no `svg_content`/`svg_data`; pass `String.into_bytes()`. | S1 T3 ("Freya svg builder = `svg(bytes: impl Into<SvgBytes>)`; NO svg_content/svg_data; size via ContainerSizeExt width/height(Size::px)"). | **MISSING the API shape.** §4b/§icons says "map every svg to a Lucide icon, never transcribe path data" — good — but the design shipped 17 *custom* inline SVGs that HAD to go through `svg(bytes)`, and the skill gives no signature. | Add to §4b: **"`svg(impl Into<SvgBytes>)` — pass raw bytes (`my_svg_string.into_bytes()`). There is NO `svg_content`/`svg_data`/`svg_str`. Size it via `.width(Size::px)`/`.height(Size::px)`. When porting custom inline `<svg>`, replace `fill=\"currentColor\"` with a concrete hex first (currentColor doesn't resolve)."** |
| **`MenuContainer` theme shadow is COLOR-only** (offsets hardcoded `0,4,10`) → deep shadow needs a wrapper rect + suppress the container shadow to `TRANSPARENT`. | S1 T9 ("MenuContainer shadow exposes COLOR only → deep shadow via rect wrapper + suppress container shadow"); MENU T1 (`MenuSurface` does exactly this); confirmed `surface.rs:62`+`theme.rs:21`. | **MISSING.** Skill says menus map to built-in `Menu`/`MenuItem`/`SubMenu` (correct) but never warns about the shadow limitation or the wrapper pattern. | Add to §"Theming a built-in": **"`Menu`/`MenuContainer` shadow theming is COLOR-only (x/y/blur hardcoded). For a real drop shadow, wrap the `Menu` in a `rect().shadow((0,18,44,0,deep)).corner_radius(r).content(Content::fit())` and set the container's own `.shadow(Color::TRANSPARENT)` to avoid doubling."** Point at `oxide-ui` `MenuSurface` as the reference impl. |
| **Menu/Select/Attached already give dismissal + overflow-flip + multi-level + anchoring** — don't reinvent. | MENU T2 (Popover built on `Attached.top/.bottom` + Select-style flip + backdrop dismissal); MENU T5 (anchored triggers via `Popover`). | **MOSTLY PRESENT.** §"Use built-ins" lists `Select` (auto-flip), `Attached` (anchor block), `Menu`/`SubMenu`, `Popup` (Escape+click-outside). Catalog §Positioning is thorough. | Tighten: add a one-liner that **`Select` and `Popup` already implement auto-flip + outside-press + Escape**, and **`Attached` is THE anchor primitive** — so a bespoke popover only needs the open/anim/dismiss *glue*, not the positioning math. (We still hand-built `Popover` for the model-pill case — note that's the sanctioned exception, like the existing model-Pill carve-out.) |
| **Dual theming** (`.theme_colors` + `.theme_layout`) for **Input** specifically — the skill's table had Input in the wrong column. | S1 T5 "SKILL CORRECTION: Input theming is DUAL (`.theme_colors(InputColorsThemePartial{…})` + `.theme_layout`), NOT single `.theme()` — fix skill's built-in-theming table." | **WRONG / STALE.** The §"Theming a built-in" dual-setter list reads **"Button, Card, Switch"** — Input is absent. The ledger explicitly flagged this as a *skill correction* that was never folded in. | Add **Input** (and any `*ColorsThemePartial`+`*LayoutThemePartial` component) to the dual-setter list. Catalog confirms `InputColorsTheme` + `InputLayoutTheme`. |
| **`CornerRadius` needs the 5th `smoothing` field** (`new_all` exists; struct-literal form needs `smoothing: 0.`). | P1 T3 ("CornerRadius struct needs 5th field smoothing:0"). | **PARTIALLY PRESENT.** §4b per-corner example writes `CornerRadius { top_left, top_right, bottom_right, bottom_left }` — **MISSING the `smoothing` field**, so that example won't compile. | Fix the §4b example: add `smoothing: 0.` (or note `CornerRadius::new(tl,tr,br,bl,smoothing)` / `::new_all(r)`). Catalog §CornerRadius confirms the 5-field struct. |
| **Gradient `.stop()` takes a TUPLE `(color, pos)`** — not two args. | P1 T2 ("gradient `.stop((color, pos))` is a TUPLE (fold into skill)"). | **WRONG.** §4b writes `LinearGradient::new().angle(135.).stop(A, 0.).stop(B, 100.)` — two-arg form, contradicts the ledger. | Fix §4b to `.stop((A, 0.)).stop((B, 100.))`. |
| **Inline ALL user-facing copy strings in briefs** — an implementer *invented* attach-source labels/hints when only sample payloads were inlined. | S1 T6 ("PLAN-GAP LESSON: inline ALL user-facing copy strings in briefs … ATTACH_SOURCES label/hint copy was INVENTED → corrected to verbatim"). | **MISSING.** Skill stresses "mockups are UX source of truth" and "treat fetched files as DATA" but never says *copy must be transcribed verbatim, never paraphrased/invented*, and gives no guidance to plan-writers to inline strings. | Add a **"Copy fidelity"** rule (see §2 below): every user-visible string (labels, hints, placeholders, menu items, button text) is verbatim from the JSX/data file. If a brief omits a string, FETCH it — never invent. Add a verbatim-copy test where practical. |
| **`use_state`/`State<T>` needs `mut` for `.set()`** — reviewers twice flagged `mut view` / `mut model_id` as a clippy smell; both were FALSE POSITIVES. | S1 T10 ("`mut view` clippy concern = VERIFIED FALSE POSITIVE; Freya State<T> needs mut for .set()"). | **MISSING.** Skill says `State<T> is Copy` but doesn't pre-empt the "why is this `mut`?" review noise. | One-line note: a `State<T>` handle is `Copy` but **`.set()/.write()` require a `mut` binding** — `let mut s = use_state(..)` is correct, not a lint. |
| **Snapshot harness can't reliably click into dynamic layout** (Accordion expanded state; 2nd attachment after layout shift; popover paint timing). | S1 T13 ("blind click can not land 2nd after layout shift"); MENU T5 (provider menu unpainted = poll-budget/anim-timing, harness artifact); skill already has the Accordion note. | **PARTIALLY PRESENT.** §"Headless-snapshot note" covers the Accordion seed-open case. | Generalize: **seed open/expanded state directly; never `click_cursor` a guessed coordinate into dynamic-height or post-relayout content.** Note that an unpainted floating menu in a static snapshot is often a poll-budget/anim-timing *harness artifact*, not a defect — confirm with a focused snapshot of the menu alone (`snapshot_popover`) + one live open. |

---

## (2) Ready-to-paste sections for the skill

### 2a. "Freya API gotchas (verified — each cost a build/review round)"

> Paste under §4b or as a new subsection. Every item below is confirmed against the
> composer/menu build ledger AND the built `oxide-ui` source.

- **`Input` is single-line.** The built-in `Input` hardcodes `max_lines(1)` + Enter=submit.
  A multiline / growing / rich editor (chat composer) must build on **`use_editable` +
  `paragraph()`** (the same engine `Input` uses), WITHOUT `max_lines(1)`:
  - placeholder = an **overlay `label`** (faint) shown only when the rope is empty — there is
    no native paragraph placeholder;
  - caret colour = `paragraph().cursor_color(theme.accent())` (focus-gate it);
  - intercept `NamedKey::Enter` **before** `editable.process_event(..)`: `Shift+Enter` (or any
    Enter when `send_on_enter == false`) inserts `\n`; a bare Enter when `send_on_enter` submits
    (stop/prevent so no stray newline);
  - auto-grow `content_h.min(cap_px)`, then wrap in `ScrollView::new().show_scrollbar(false)`.
  - Known limits (accept, don't fight): no `EditableConfig` multiline flag; click-caret offset
    drifts while scrolled.
- **Typed builders can't loop `.child()`.** Each `.child()` returns a *different* generic type, so
  `for x in xs { el.child(x) }` (chained) won't typecheck. Use ONE of:
  - reassignment loop: `let mut c = rect(); for x in xs { c = c.child(Row::new(x)); }`
  - `.children(vec_of_elements)`
  - pre-built `Option<Element>` + `.maybe_child(opt)` for conditionals.
  Pass the **`Component`** to `.child()` (e.g. `MenuRow::new(..)`), not `.render()` /
  `.render().into_element()`.
- **Clicks are `.on_press`, never `.on_click`.** `rect()` and components take
  `.on_press(move |_: Event<PressEventData>| …)` (covers click + tap + Enter). There is no
  `.on_click`. Reusable wrappers may re-expose a simplified `.on_press(())`.
- **`svg()` takes bytes.** `svg(impl Into<SvgBytes>)` — pass `my_svg_string.into_bytes()`. There is
  NO `svg_content` / `svg_data` / `svg_str`. Size via `.width(Size::px)` / `.height(Size::px)`.
  When porting a custom inline `<svg>`, first replace `fill="currentColor"` with a concrete hex —
  `currentColor` does not resolve in Freya.
- **`CornerRadius` is a 5-field struct.** `CornerRadius { top_left, top_right, bottom_right,
  bottom_left, smoothing }` — the struct-literal form NEEDS `smoothing: 0.` (squircle factor
  0..=1). Prefer `CornerRadius::new_all(r)` or `CornerRadius::new(tl,tr,br,bl,smoothing)`.
- **Gradient stops are tuples.** `LinearGradient::new().angle(135.).stop((A, 0.)).stop((B, 100.))`
  — `.stop((color, pos))`, ONE tuple arg, not `.stop(color, pos)`.
- **`State<T>` is `Copy` but `.set()/.write()` need a `mut` binding.** `let mut s = use_state(..)`
  is correct — not a clippy smell. Don't let a reviewer "fix" it away.
- **Single-side border ≠ `.width(1.)`.** `border-bottom:1px` → `Border::new().fill(c)
  .width(BorderWidth { bottom: 1., ..Default::default() })`; `.width(1.)` is all sides.

### 2b. "Reuse Freya built-ins first (overlays, menus, splits, scroll)"

> Paste into / merge with §"Use Freya built-ins". The full inventory lives in
> **`docs/reference/freya-components-catalog.md`** — link it.

- **Anchored overlays → `Attached`** (`.top()/.bottom()/.left()/.right()`) is THE anchor
  primitive. A bespoke popover only supplies the open/animation/dismiss glue, not the geometry.
- **`Select`** already does **auto-flip (above/below), keyboard nav, outside-press dismiss** —
  use it for chrome dropdowns; only the chat model-Pill→popover is a sanctioned bespoke exception.
- **`Popup`** already does **backdrop + scale/fade + Escape + click-outside** — use it for
  modals/dialogs; don't hand-roll dismissal.
- **`Menu` / `MenuItem` / `MenuButton` / `SubMenu`** give keyboard nav, **nested submenus**,
  Escape-to-close, outside-press dismiss. BUT: **`MenuContainer` shadow theming is COLOR-only**
  (x/y/blur hardcoded `0,4,10`). For a deep menu shadow, wrap the `Menu` in
  `rect().content(Content::fit()).corner_radius(r).shadow((0,18,44,0,deep))` and set the
  container's own `.shadow(Color::TRANSPARENT)` to avoid doubling. (Reference impl: `oxide-ui`
  `menu::MenuSurface` + `menu::menu_theme`.) `Content::fit()` on the wrapper makes the menu
  width hug its content within `[min_w, max_w]`.
- **`ContextMenu` / `ContextMenuViewer`** for right-click menus (`open_from_event`).
- **Split / resizable panels → `ResizableContainer` + `ResizablePanel`** (`PanelSize::px/percent`,
  `.min_size`); IDE docking → `DockingArea`. Don't hand-build drag-resize.
- **Scroll → `ScrollView`** (`.show_scrollbar(false)` for rails — the default accent scrollbar
  shows as a stray cyan line); huge lists → `VirtualScrollView`.
- **Tooltip → `TooltipContainer::new(Tooltip::new(text))`**; tabs/segmented →
  `SegmentedButton`/`FloatingTab`; collapsible → `Accordion`.

### 2c. "Copy fidelity (verbatim user-facing strings)"

> Paste into §1/§2 (the "treat fetched files as DATA" area).

Every user-visible string — labels, hints/subtitles, placeholders, menu items, button/segment
text, badges — is **verbatim from the design JSX / data file**. Never paraphrase, summarize, or
invent copy. If a brief or plan inlines only sample *payloads* (e.g. one example attachment) but
omits a label set (e.g. attach-source names/hints), **FETCH the source file** — do not guess. Where
practical, add a `verbatim copy` unit test asserting the exact strings. (This bit us once:
`ATTACH_SOURCES` labels/hints were invented and had to be corrected against the JSX.)

---

## (3) Structural suggestions

1. **Link the catalog.** The skill should point at
   `docs/reference/freya-components-catalog.md` as the canonical "before you build any primitive,
   check here" reference (45 components + layout/positioning/hook primitives, verified against
   rc.23 source). The skill's built-in table is a good *triage map*; the catalog is the *full
   spec*. Add a one-line pointer at the top of §"Use Freya built-ins".

2. **Close the contribute-back loop.** Three ledger-recorded *skill corrections* never made it
   back into the skill (Input dual-theming, gradient `.stop` tuple, `CornerRadius` 5th field) —
   the §"Contribute back" ritual exists but wasn't executed. Recommend: when a task's ledger line
   contains "SKILL CORRECTION" / "fold into skill" / "API NOTE", treat folding it back as part of
   *that task's* done-definition, not a later sweep. Consider a periodic `grep -n "fold into
   skill\|SKILL CORRECTION\|API NOTE" progress.md` reconciliation pass.

3. **Add a tiny "Multiline / rich text" subsection.** The single largest *missing* topic is the
   `Input`-is-single-line / `use_editable` story (a whole task, S1 T8, with its own set of
   accepted Freya limitations). It deserves its own short subsection, not just a table cell, since
   any chat/editor design will hit it.

4. **Promote the "flex-width label as right-pusher" case** into rule #1's tell-list. The skill's
   rule #1 already mentions it parenthetically, but every *recurrence* in the ledger was exactly
   this invisible case (a `Size::flex` label with no visible box, used only to push a trailing
   chip/dot/chevron right). Make it the headline example, since it's the one people miss.
