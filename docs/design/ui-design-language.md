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
3 · components    chat_ui/widgets.rs
                 ghost_icon_button · pill · action_button · card · chip ·
                 status_rule   (encode the recurring chrome ONCE)
       ▲
2 · primitives    chat_ui/tokens.rs        chat_ui/icons.rs
                 spacing/radii/elevation/   28 monochrome SVGs, recolored
                 type ramp (named consts)   via svg::Style.color
       ▲
1 · theme         chat_ui/mod.rs :: Kit
                 semantic color roles resolved from the active theme
                 palette at view time (crust…red, never a literal hex)
```

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

## Cross-app path — a shared crate

The chat is one of several iced surfaces (`settings-rs`, `popup-rs`/
`oxidemx-popup`, `oxidemx-mission-control`). Today each has its own
chrome. To make the design language app-wide:

1. **Lift layers 1–3 into a shared crate** — `oxidemx-widgets` already
   exists; move `Kit` (or a palette trait it implements), `tokens`,
   `icons`, and `widgets` there. They depend only on `iced` + a theme
   palette, so the lift is mechanical.
2. **One palette source** — `Kit::from_state` reads
   `oxidemx_shared::theme`; expose that as the shared palette so every
   app re-tints from the same theme switch.
3. **Adopt per surface** — settings/popup/MC import the crate and
   migrate their chrome to the builders incrementally.

This is a deliberate, separate effort (it touches four crates); the chat
module is the proving ground. Do it once the builder API has settled.

## What stays OUTSIDE the design language

The **canvas/shader layer is a different system.** The radial menu's
slices, the 3D framing shaders, and the chat shell's painted caps
(`chat_shell::CapsPainter`, the footer arc, the page puck) are drawn on
`iced::canvas` / wgpu — not from tokens/widgets. The design language
governs the **widget chrome layered over** that canvas; the shader stack
([[project_3d_shader_stack]]) is modularized on its own terms (per-effect
WGSL passes). Keep the seam clean: widgets don't know about shaders, and
the canvas doesn't consume `widgets::*`.
