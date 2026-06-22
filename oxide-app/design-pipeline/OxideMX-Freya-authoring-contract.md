# OxideMX → Freya authoring contract (paste into Claude Design / pin as a project instruction)

> Give this to Claude Design when authoring or revising any OxideMX design intended for translation to
> the Freya (Rust) desktop app. Goal: designs that are **deterministically translatable** — the Rust
> agent reads a precise spec instead of guessing from freeform JSX. Every rule below removes a real
> translation ambiguity the `claude-design-to-freya` skill had to work around.

You are authoring the **OxideMX** design system. Each design will be translated into a **Freya v0.4**
Rust app. Produce output that follows these conventions, AND emit the companion machine-readable spec
in Part B — that JSON is the contract the translator consumes; your JSX/HTML is the visual reference.

## Part A — Conventions in the JSX / components

1. **Map to Freya built-ins; name + tag accordingly.** When an element corresponds to a Freya built-in,
   name the component exactly after it and add `data-freya="<Component>"`. Do NOT hand-build these as
   styled `<div>`s — the Rust side uses the real built-in and themes it.
   - Controls: `Button` `Input` `Switch` `Checkbox` `RadioItem` `Slider` `Select` `Chip` `SegmentedButton`.
   - Containers/overlays: `Card` `Accordion` `Popup` `Menu` `Tooltip` `SideBarItem` `FloatingTab`
     `ResizableContainer` `ScrollView` `VirtualScrollView` `Table`.
   - Display/data: `ProgressBar` `CircularLoader` `Skeleton` `Calendar` `ColorPicker` `Link`
     `MarkdownViewer` (markdown bodies) `ImageViewer`.
   - Feature surfaces: `CodeEditor` (any code pane — never a hand-built text grid), `Terminal`,
     `Router`/`Outlet`/`Link` (page navigation), Lucide icons via `icon("<lucide-name>")` (never inline
     raw `<svg>` path data).
   - **Custom is allowed only where no built-in fits** (chat **Bubble**, **StatusDot/ring**, agent
     **AgentCard/ToolCard/ApprovalCard**, the model **Pill→popover**, breadcrumb). Tag these
     `data-freya="custom"` so the translator knows to build them from primitives.

2. **Colors are tokens, never raw hex — translucency is a NAMED token.** Every color references the theme
   object: `T.<token>`. Never inline hex. For translucent fills use a **named** token (`T.accent_15`,
   `T.hair`, `T.peach_20`) — never a hex-alpha suffix like `${T.accent}1a`. Keep one `THEME` object; add
   named alpha variants as needed. (Removes the "decode the hex-alpha suffix" guess.)

3. **Tag layout intent on the main flex containers.** `data-layout="row|col"`. Mark special children:
   `data-region="scroll-body"` (the `flex:1` scrollable area), `data-region="footer"` (a pinned bar),
   `data-region="rail"` with `data-rail-width="<full>,<collapsed>"` (e.g. `"274,60"`). **Avoid**
   `marginLeft:auto` and `maxWidth:"N%"` — instead use `data-align="push-right"` and give caps in px or a
   token. (Removes the flex / `Content::Flex` / alignment / max-width guesswork — the #1 layout bug.)

4. **Express radius / gradient / shadow as structured values, not CSS strings.**
   `data-radius="14,14,4,14"` (TL,TR,BR,BL) · `data-gradient="135; T.mantle 0; T.accent_20 100"`
   (angle; stop pos…) · `data-shadow="0 2 8 T.shadow"`. (Removes per-corner / gradient parsing guesses.)

5. **Named animations only.** Use a small named set and tag `data-anim="pulse|spin|slide|fade|collapse"`
   with `data-anim-ms` and `data-anim-ease`. No bespoke `@keyframes`. (Each name maps 1:1 to a Freya
   `use_animation` shape.)

6. **Mark interactables.** Anything clickable/typeable gets `role` and/or
   `data-action="press|input|toggle|select|navigate"`. (So interactive-vs-decoration is explicit, not inferred.)

## Part B — Companion machine-readable spec (the agent-facing contract)

For each design `Foo.html`, also emit **`Foo.freya.json`** — a structured spec the translator reads
**directly** so it never has to parse your JSX. Keep it in sync with the JSX + `COMPONENTS.md`. Shape:

```json
{
  "design": "OxideMX Freya - Collapsible Panels",
  "theme": {
    "tokens": { "base": "#121418", "accent": "#00d4ff", "accent_15": "rgba(0,212,255,0.15)", "hair": "rgba(255,255,255,0.06)", "...": "..." },
    "accents": { "cyan": "#00d4ff", "violet": "#b388ff", "amber": "#ffab40", "lime": "#7be06a" }
  },
  "components": [
    {
      "name": "Sidebar", "freya": "custom", "role": "region",
      "tokens": ["mantle", "hair"],
      "children": [
        { "name": "ConversationRow", "freya": "SideBarItem", "props": { "active": "bool" }, "tokens": ["row_active", "text"] }
      ]
    },
    {
      "name": "Composer.sendBtn", "freya": "Button", "variant": "primary",
      "props": { "icon": "send" }, "tokens": ["accent", "crust"], "action": "press"
    },
    {
      "name": "PointerSpeed", "freya": "Slider",
      "props": { "min": 0, "max": 100 }, "tokens": ["accent", "surface0"], "action": "input"
    }
  ],
  "layout": {
    "shell": "row",
    "regions": [
      { "id": "left", "kind": "rail", "railWidth": [274, 60], "collapsible": true },
      { "id": "center", "kind": "flex", "contains": ["scroll-body", "footer"] },
      { "id": "right", "kind": "rail", "railWidth": [320, 60], "collapsible": true, "startCollapsed": true }
    ]
  },
  "animations": [
    { "name": "pulse", "kind": "glow-loop", "ms": 1800, "ease": "out", "drives": "status-dot shadow" },
    { "name": "collapse", "kind": "width", "ms": 300, "ease": "expo-out", "drives": "rail width 274->60" }
  ]
}
```

Rules for the JSON: tokens are resolved values (hex/rgba); `freya` is the exact built-in name or `"custom"`;
`props` carries only what the translator needs (variant, icon, min/max, active); `action` marks
interactables; `layout.regions` names the scroll-body / footer / rail so the Rust side gets `Content::Flex`
+ sizing right without inference.

## Part C — What each rule buys (evidence)

Empirical skill tests translating real OxideMX JSX → Freya hit exactly these guesses: hex-alpha suffixes
(rule 2), per-corner radii + gradients as CSS strings (rule 4), `marginLeft:auto`/`maxWidth:%`/the
`Content::Flex` scroll-body+footer layout (rule 3), built-in-vs-custom identity + which built-in (rule 1),
and un-named `@keyframes` (rule 5). The companion JSON (Part B) collapses all of these into a single
deterministic read. The translator's job then narrows to emitting idiomatic Freya from a precise spec.

---

*Reciprocal note for the Rust side:* the `claude-design-to-freya` skill should **trust `Foo.freya.json`
and these annotations over inference** when present, and fall back to inferring from the JSX only for
anything the contract doesn't cover.
