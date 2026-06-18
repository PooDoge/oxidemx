# OxideMX UI design language

> How the chat redesign is structured as a reusable system, and the path
> to sharing it across the app's other iced surfaces (settings, popup,
> Mission Control). Companion to `ai-chat-ui-spec.md` (inventory) and the
> `design_handoff_ai_chat_ui/` references.

## The problem this solves

Before the design pass, every view hand-rolled its chrome: ad-hoc hex,
ad-hoc Unicode glyphs, button styles copy-pasted per call-site. Changing
"how a pill looks" meant editing N files. The redesign replaces that with
a **layered system** so the same widget is built the same way everywhere
and a token change re-tints the whole surface.

## The four layers (bottom → top)

```
4 · views        header.rs · footer.rs · body.rs · cards.rs · panels…
                 compose layer-3 components; hold no raw style
       ▲
3 · components    overlay-rs/chat_ui/widgets.rs   ← (moving → shared, see roadmap)
                 ghost_icon_button · pill · action_button · card · chip ·
                 status_rule   (encode the recurring chrome ONCE)
       ▲
2 · primitives    oxidemx-widgets::tokens     oxidemx-widgets::icons   ← SHARED
                 spacing/radii/elevation/    28 monochrome SVGs, recolored
                 type ramp (named consts)    via svg::Style.color (cached handles)
       ▲
1 · theme         oxidemx-widgets::palette::Palette   ← SHARED (one parse source)
                 + overlay Kit = Palette + {alpha, pulse} animation context
                 semantic color roles, theme-resolved (crust…red, never a hex)
```

**As of 2026-06-18 layers 1–2 are SHARED** — `tokens` + `icons` moved
into the `oxidemx-widgets` crate (which already held `Palette` + style
closures used by settings / popup / Mission-Control), and the overlay's
`Kit` now sources its colors from `Palette::from_theme` (one hex-parsing
+ fallback path for every surface; the 14 duplicated color literals in
`chat_ui` are gone). `Kit::from_palette(p, alpha, pulse)` is pure, so
static apps can reuse it (alpha=1, pulse=0).

**Rule of thumb:** a view should reach for layer 3 (a `widgets::*`
builder) or, failing that, layer 2 (`tokens::*` + `icons::icon`). A raw
`Color::from_rgb` or a Unicode glyph in a view is a smell — add the role
to `Kit` / a path to `icons.rs` / a builder to `widgets.rs` instead.

## Layer 3 — the component API (`chat_ui/widgets.rs`)

Each builder takes a `Kit` + iced primitives and returns a styled widget:

| Builder | Use |
|---|---|
| `ghost_icon_button(kit, name, size, color, msg)` | inline icon actions (copy, attach, close) — subtle hover tint |
| `pill(kit, icon?, label, active, msg)` | chips, the model pill, "New" — accent when active |
| `action_button(kit, label, tone, primary, msg)` | semantic buttons (Run/Deny, primary fill vs. outline) |
| `card(kit, content)` | the elevated mantle card (r-card, e1) |
| `chip(kit, content)` | small bordered badge (doc/attachment chips) |
| `status_rule(kit, tone)` | the 2.5px left status bar on agent/tool cards |

Adoption is **incremental** — call-sites migrate to these as they're
touched; nothing has to move at once. (Already migrated: bubble
copy/select, footer attach.)

## The composer (input) behavior — a documented pattern

Standard chat inputs (ChatGPT/Claude/etc.): a **1-line minimum**, auto-grow
to a **max** as the user types, then **scroll internally**; attachment
previews stack in a row **above** the input bar, and the composer grows
to fit them — the input row itself never shrinks. OxideMX implements this
in `footer.rs`: the `text_editor` is `Fixed(40)` at 1 line, `Fixed(60)`
at the 2-line cap (scrolls beyond via `line_count()`); `footer_h` is an
exact-fit of (activity + chip + input row + padding) floored at the
painted-arc base, so it grows **upward over the body** rather than
squishing the text. (Fixes the "attaching shrinks the input" bug.)

## Cross-app consolidation — status & next steps

The chat is one of several iced surfaces (`oxidemx-settings`,
`oxidemx-popup`, `oxidemx-mission-control`, overlay). The shared crate is
**`oxidemx-widgets`** (`palette` + `style` closures + composite
`widgets` were already there for the first three; `tokens` + `icons` just
joined; overlay now depends on it).

- ✅ **Step 1 — shared primitives.** `tokens`, `icons` moved to
  `oxidemx-widgets`; overlay's `Kit` re-sourced from `Palette`. One parse
  path, one icon cache, one token scale for every surface.
- ✅ **Step 2 — shared components + render context.** `Kit` moved to
  `oxidemx-widgets::kit` (built via `Kit::from_theme` static /
  `from_palette` animated); the builders moved to
  `oxidemx-widgets::controls` (`ghost_icon_button` / `pill` /
  `action_button` / `segment` / `round_icon_button` / `card` / `chip` /
  `status_rule`). Every iced surface can now build the same control the
  same way. chat_ui re-exports them so `super::*` paths are unchanged.
- ✅ **Step 3 — kill inline styles via typed builders.** The header
  (segmented switcher + close) composes `segment` + `round_icon_button`
  instead of hand-written `Style{…}` closures.
- ✅ **Step 3.5 — the style catalog (`oxidemx-widgets::catalog`).** The
  realization of iced's `Catalog` pattern for *our* color model: a
  **centralized, variant-keyed style catalog** rather than a custom
  `Theme` type. Two enums + two resolver fns:
  - `Surface::{Card, Panel, Chip, CrustWell, Bubble(is_user),
    Tinted{tone,bg_k,border_k,radius}}` → `catalog::surface_style(kit, …)`
  - `Btn::{Ghost, Plain, Fill(tone), Outline(tone), Tinted(tone,k)}` →
    `catalog::button_style(kit, …)`

  A custom `iced::Theme` type would buy nothing here (our colors flow
  through `Kit`/`Palette`, not `iced::Theme`'s palette) and would cost a
  `markdown::Catalog` + highlighter reimpl — so we centralize the style
  *closures* instead. chat_ui call-sites migrated off inline `Style{…}`:
  both message bubbles, the approval card (yellow Tinted), error bubble
  (red Tinted), code preview + io_block (CrustWell), tool-call frame
  (Card + `status_rule`), footer stop button + attachment chip, the
  "↓ Latest" pill (Tinted). Genuinely one-off chrome (hairline, lightbox
  scrim, context menu, the send-button accent glow, `text_input`/
  `text_editor` default-derived styles, the pulsing activity dot) stays
  inline by design — the catalog holds *recurring* variants, not
  singletons.
- ✅ **Step 4 — one render context (the unification).** Before this step
  there were *two* parallel style systems in `oxidemx-widgets`: the older
  `style.rs` (slider/toggler/pick_list/rule + the settings card/sidebar/
  nav chrome) keyed on `&Palette` and used by settings/popup/MC (~600
  call-sites), and the newer `catalog`/`controls`/`kit` keyed on `Kit`
  and used by the overlay. Same job, two context types — neither family
  could use the other's widgets. **Unified onto `Kit`:**
  - `Kit` expanded to carry the full semantic palette it was missing
    (`is_dark`, `base`, `danger`, `hairline`/`_strong`/`_faint`,
    `row_hover`/`row_active`) — now a superset of every role either system
    reads.
  - `style.rs` functions take `impl Into<Kit>` and read from `Kit`;
    `From<&Palette> for Kit` (a static `alpha=1, pulse=0` kit) means the
    ~600 existing `style::card(&palette)` call-sites **compile unchanged**
    while the crate runs on a single context. Zero settings churn, zero
    visual change; the whole UI is now one `Kit`-keyed system.
  - Dead duplicates folded: `controls::card`/`chip` (0 callers, and pure
    value-dups of the catalog) now *delegate* to `catalog::surface_style`
    — one source of truth per surface.
  - `Surface::Row` added (flat list-row card); the chat panels
    (tasks/memories/skills) migrated off their inline row closures.

  **The two style *modules* remain by domain** — `catalog` (chat
  design-language surfaces/buttons, animated via `Kit.alpha/pulse`) and
  `style` (form-widget + settings-surface chrome) — but both now resolve
  from the same `Kit`, so any surface can call either. A re-skin of
  settings onto the chat look is a *separate, opt-in* decision, not
  required by the unification.
- ⏳ **Step 5 — icon adoption (remaining).** settings/MC still use ~40
  Unicode glyphs; migrate them to `icons::icon`. (Two known widget dups
  also remain: `tasks::toggle` ≈ `cards::mini_switch` — fold into one
  `controls::switch` builder when next touched.)

## Roadmap — libcosmic-informed patterns (researched 2026-06)

System76's **libcosmic** (vendored at `libcosmic/`) is the reference
iced design system. Patterns worth adopting, in priority order:

1. **Typed component builders with implicit variants** — `button::standard`
   / `suggested` / `destructive` apply the right tokens automatically (no
   per-call color choice). We've started this (`widgets::action_button`'s
   `tone`/`primary`); extend to named variants
   (`button::primary/ghost/danger`) so call-sites never pass raw colors.
   *Evidence: libcosmic `widget/button/text.rs`.*
2. **The iced `Catalog` / `Class` style-resolution pattern** — ✅ **DONE
   (as `oxidemx-widgets::catalog`, Step 3.5).** Instead of inline
   `move |_,_| Style{…}` closures per call-site, `Surface`/`Btn` enums +
   `surface_style`/`button_style` resolvers compute appearance from
   `Kit` (tokens + roles) and widget state at render time. We chose a
   centralized *style catalog* over a custom `iced::Theme` type
   deliberately — our colors live in `Kit`/`Palette`, so a custom Theme
   would add a `markdown::Catalog` + highlighter reimpl for zero color
   benefit. *libcosmic `theme/style/button.rs` is the inspiration.*
3. **Density / Roundness as orthogonal config** — enums that transform
   the *whole* spacing / radii table (Compact/Standard/Spacious ×
   Round/Square), independent of color theme. A future user setting.
   *libcosmic `cosmic-theme/src/model/{spacing,corner}.rs`.*
4. **Layered semantic surfaces** — model UI as Background/Primary/
   Secondary layers; components query the current container instead of
   hardcoding a surface color. Richer than our flat `Palette`; adopt if
   nesting depth grows. *libcosmic `widget/layer_container.rs`.*
5. **Color math for derived tones** — derive hover/pressed/disabled via
   `palette` crate compositing (`over()`), not hand-tuned values. Our
   `Palette` already pre-mixes a few (`accent_06/15/40`); generalize.

Anti-patterns (libcosmic avoids, so do we): per-call-site color
constants, mixed spacing units, `widget.hover_color()`-style state
setters (let the Catalog compute state), hardcoded "if in sidebar use X"
(use layer context).

## What stays OUTSIDE the design language

The **canvas/shader layer is a different system.** The radial menu's
slices, the 3D framing shaders, and the chat shell's painted caps
(`chat_shell::CapsPainter`, the footer arc, the page puck) are drawn on
`iced::canvas` / wgpu — not from tokens/widgets. The design language
governs the **widget chrome layered over** that canvas; the shader stack
is modularized on its own terms (per-effect WGSL passes). Keep the seam
clean: widgets don't know about shaders, and the canvas doesn't consume
`widgets::*`.

## What stays OUTSIDE the design language

The **canvas/shader layer is a different system.** The radial menu's
slices, the 3D framing shaders, and the chat shell's painted caps
(`chat_shell::CapsPainter`, the footer arc, the page puck) are drawn on
`iced::canvas` / wgpu — not from tokens/widgets. The design language
governs the **widget chrome layered over** that canvas; the shader stack
([[project_3d_shader_stack]]) is modularized on its own terms (per-effect
WGSL passes). Keep the seam clean: widgets don't know about shaders, and
the canvas doesn't consume `widgets::*`.
