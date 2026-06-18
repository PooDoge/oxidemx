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
- ◐ **Step 3 — kill inline styles via typed builders.** In progress: the
  header (segmented switcher + close) now composes `segment` +
  `round_icon_button` instead of hand-written `Style{…}` closures.
  Migrate the remaining inline styles (footer send/stop, threads pill,
  panels) as they're touched. The **full iced `Catalog`/custom-`Theme`
  switch** (style enums resolved by one `Catalog` impl, zero per-site
  closures, app-wide) is the deeper finish — deferred deliberately
  because it touches every app's `iced::application(…).theme(…)` root and
  the `markdown::view` theme; do it attended, not mid-session.
- ⏳ **Step 4 — per-surface adoption.** settings/popup/MC migrate their
  hand-rolled chrome + text glyphs to `icons::icon` + the builders.

## Roadmap — libcosmic-informed patterns (researched 2026-06)

System76's **libcosmic** (vendored at `libcosmic/`) is the reference
iced design system. Patterns worth adopting, in priority order:

1. **Typed component builders with implicit variants** — `button::standard`
   / `suggested` / `destructive` apply the right tokens automatically (no
   per-call color choice). We've started this (`widgets::action_button`'s
   `tone`/`primary`); extend to named variants
   (`button::primary/ghost/danger`) so call-sites never pass raw colors.
   *Evidence: libcosmic `widget/button/text.rs`.*
2. **The iced `Catalog` / `Class` style-resolution pattern** — instead of
   inline `move |_,_| Style{…}` closures per call-site, define a style
   enum (`Button::{Primary,Ghost,Danger,…}`) and one `Catalog` impl that
   computes appearance from tokens + widget state (hover/press/focus) at
   render time. Decouples style from widget code, enables live theme
   switching, kills the ~57 inline style blocks. *libcosmic
   `theme/style/button.rs`.* **This is the highest-leverage refactor.**
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
