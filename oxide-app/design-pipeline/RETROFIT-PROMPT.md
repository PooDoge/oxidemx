# Retrofit prompt — annotate an existing OxideMX design for Freya translation

Paste this into Claude Design (in the OxideMX Design System project). It applies the
[authoring contract](OxideMX-Freya-authoring-contract.md) to an **existing** design, *additively* —
the rendered design must stay pixel-identical; you're only adding machine-readable structure +
annotations the Rust translator reads. First target: **OxideMX Freya - Collapsible Panels**.

---

Apply the OxideMX→Freya authoring contract to the existing design **"OxideMX Freya - Collapsible Panels"**
and its component set: `freya-data.jsx`, `freya2-data.jsx`, `icons.jsx`, `freya-chrome.jsx`,
`freya-sidebar.jsx`, `freya-thread.jsx`, `freya2-rail.jsx`, `freya2-right.jsx`, `freya2-editor.jsx`,
`freya2-shell.jsx`. **Do not change how the design looks** — every edit is additive structure/metadata.

1. **Annotate, don't redesign.** On existing elements, add (no visual change):
   - `data-freya="<Component>"` on anything that maps to a Freya built-in — `Button` `Input` `Switch`
     `Checkbox` `RadioItem` `Slider` `Select` `Chip` `SegmentedButton` `Card` `Accordion` `SideBarItem`
     `FloatingTab` `Popup` `Menu` `Tooltip` `ProgressBar` `Skeleton` `Table` `Calendar` `ColorPicker`
     `ScrollView` `VirtualScrollView` `Link` `ResizableContainer` `MarkdownViewer` `CodeEditor` `Terminal`
     `Router`/`Outlet`. Use `data-freya="custom"` for bespoke widgets with no built-in (chat Bubble,
     StatusDot/ring, AgentCard/ToolCard/ApprovalCard, the model Pill→popover). In particular: the
     `freya2-editor` code view → `CodeEditor`; the model dropdown → `Select`; rail toggles → `Switch`;
     icons (`icon("name")`) stay Lucide names.
   - `data-region` on the layout containers: `scroll-body` (the `flex:1` transcript/list areas),
     `footer` (the pinned composer), `rail` on the collapsible left/right panels with
     `data-rail-width="<full>,<collapsed>"` (left `"274,60"`; right per its design).
   - `data-action="press|input|toggle|select|navigate"` on every interactable.
   - `data-anim="pulse|spin|slide|fade|collapse"` on animated elements (the `fx-pulse`/`fx-spin`/
     `fx-editor-in`/`fx-nav-*` keyframes), with `data-anim-ms` + `data-anim-ease`.

2. **Promote alpha colors to named tokens.** Replace every `${T.x}NN` hex-alpha suffix (e.g.
   `${T.accent}1a`, `${T.yellow}3a`, `${puck.c}14`) with a **named** token added to the THEME object
   (`accent_15`, `accent_20`, `hair`, `peach_20`, …) and reference that token. No raw hex and no
   hex-alpha suffixes left in any `style`.

3. **Structured radius/gradient/shadow**, not CSS strings: `data-radius="14,14,4,14"` (TL,TR,BR,BL),
   `data-gradient="150;T.accent 0;T.accentDim 100"`, `data-shadow="0 4 12 T.accent_30"`.

4. **Emit `OxideMX Freya - Collapsible Panels.freya.json`** — the agent-facing spec:
   `{ theme: { tokens, accents }, components: [{ name, freya, props, tokens, action?, anim?, children }],
   layout: { shell, regions: [{ id, kind, railWidth?, contains? }] }, animations: [{ name, kind, ms, ease,
   drives }] }`. Tokens are resolved values; `freya` is the exact built-in name or `"custom"`; mark
   `scroll-body`/`footer`/`rail` regions so the Rust side gets `Content::Flex` + sizing right.

5. **Keep `COMPONENTS.md` in sync** — each reusable component → its intended Freya built-in (or custom) + props + tokens.

6. **Self-check before finishing:** the emitted JSON must pass the repo validator with zero errors:
   `python3 oxide-app/design-pipeline/validate_freya_spec.py "OxideMX Freya - Collapsible Panels.freya.json" --src <the .jsx/.html files>`.

Report the new/edited file list and confirm the rendered design is visually unchanged.

---

**After this proves out on Collapsible Panels**, generalize the same retrofit to the other OxideMX designs
(chat visual system, component specs, app shells). Then sub-project **2b** implements the annotated design
in `oxide-app` via the `claude-design-to-freya` skill, consuming the `.freya.json` directly.
