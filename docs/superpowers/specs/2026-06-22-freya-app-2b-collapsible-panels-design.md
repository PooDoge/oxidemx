# Freya App 2b — Collapsible Panels (visual implementation) — Design

**Status:** Approved for planning (2026-06-22).
**Program:** sub-project 2 (the Freya chat app) of the AI-chat rewrite. 2a (transport + navigable shell,
live-agentd-connected) is MERGED to `phase1-local-llm-gateway` (`ec3d1b9`). 2b restyles that working shell
to match the Claude Design **"OxideMX Freya - Collapsible Panels"** mockup.

**Source of truth:** the validated `oxide-app/design-pipeline/` artifacts — the design's companion spec
`OxideMX Freya - Collapsible Panels.freya.json` (92 components, 86 tokens, 5 named animations, 3 layout
regions; validator-clean) — consumed via the **`claude-design-to-freya`** skill (run it + `freya-gui-framework`
+ read `oxide-app/FREYA-PATTERNS.md` before implementing). Build method: per the skill, map `theme.tokens` →
the `oxide-ui` theme, map each component → a Freya built-in (themed via `define_theme!` partial) or a custom
`oxide-ui` component; render headless snapshots each phase and compare 1:1 to the design.

## Scope (locked)

**In — the functionally-backed shell + the right-rail silhouette**, styled to the design:
- Full **theme** (the design palette).
- **Thread** (center): header (title + status puck + worktree chip + actions), styled user/assistant bubbles,
  the composer (input + send).
- **Sidebar** (left): project switcher, search, new-conversation, conversation rows (state dot + worktree
  chip), the collapsed rail variant.
- **WindowFrame chrome** (frameless titlebar + desktop backdrop) + the **right-panel collapsed status rail**
  (so the 3-panel "Collapsible Panels" silhouette is complete).

**Deferred (NOT 2b)** — unbacked surface / overlaps the future 2d control-plane, or needs new reducer work:
- Right-panel **expanded** internals: the 4 status "directions" (spec/mission/workbench/ambient), `.oxide`
  settings nav, MCP fork flow, pinned/draggable popovers.
- The built-in **code editor** overlay (`EditorView`/`EditorPill`).
- The rich **Activity / Delivery / Artifact** cards — these render `activity`/`run`/`card` SSE events the 2a
  transcript reducer ignores; rendering them is a *functional* addition (reducer + event handling), tracked
  separately, not 2b styling.

## Architecture / code layout (all in `oxide-app`)

- **`oxide-ui/src/tokens.rs`** — expand the `Theme` to the design palette, **keeping flat names** (per
  decision) + an alpha helper (so the `accent_NN`/`yellow_NN` ramp needs no 80 named fields):
  - surfaces: `bg` (base `#121418`), `bg_deep` (crust `#0a0c10`), `panel` (mantle `#0f1117`),
    `surface` (surface0 `#1a1d24`), `surface_hi` (surface1 `#242832`), `surface_max` (surface2 `#2e3440`),
    `overlay` (overlay0 `#404654`).
  - text: `text` (`#f0f4f8`), `subtext_hi` (subtext1 `#c8d0dc`), `subtext` (subtext0 `#9aa5b5`),
    `faint` (`#5d6675`).
  - accent: `accent` (`#00d4ff`), `accent_hi` (accent2 `#0abdc6`), `accent_dim` (`#0891a8`).
  - semantic: `green` `#00e676`, `yellow` `#ffd54f`, `red` `#ff5252`, `blue` `#4a9eff`, `mauve` `#b388ff`,
    `pink` `#ff80ab`, `peach` `#ffab40`, `teal` `#0abdc6`.
  - hairlines: `hairline` (white@.06), `hairline_strong` (white@.10).
  - **alpha helper:** `pub fn with_alpha(base: Color, a: u8) -> Color` (compute `Color::from_argb(a, r,g,b)`);
    the design's `accent_1a` → `Theme::with_alpha(theme.accent(), 0x1a)`. Keep the `Accent` enum
    {Cyan, Violet, Amber, Lime} → `(accent, accent_hi, accent_dim)`.
- **`oxide-ui` components** — restyle existing primitives (Bubble, ListItem, PromptInput, RailButton,
  StatusDot, CollapsiblePanel) to the design + add the reusable design bits: `StatusPuck`/`StatusDot`
  (state→tone, pulse), `Avatar` (gradient sphere + sparkle), `WorktreeChip` (accent-tint pill), `Chip`
  variants, the chrome `WindowFrame`/`Titlebar`. Reach for Freya built-ins where the `freya.json` maps them
  (Button, Select, Input, SideBarItem, Card, ScrollView, Tooltip, CircularLoader, ProgressBar) themed via
  their `define_theme!` partials (the skill's "theming a built-in" table gives the per-component shape).
- **`oxide-freya` regions** — restyle `Sidebar`, `MainRegion` (Thread), `ContextRegion` (right rail), and add
  the `WindowFrame` chrome wrapper around the 3-region shell.

## Phase 1 — Theme + Thread (the immediate visible win)

The center column the user already sees, restyled. Components (freya.json → impl):
- **Theme:** the full palette + alpha helper above (oxide-ui). All P1+ components pull from it.
- **ThreadHeader** (`freya.json: Thread.ThreadHeader`, custom): conversation title (15/600, `text`,
  ellipsis) + working-dir mono line + **WorktreeChip** (Chip: `accent` text, `with_alpha(accent,0x14)` fill,
  `with_alpha(accent,0x33)` border, pill) + **StatusPuck** (custom pill: state→tone
  working=`yellow`/delivered=`green`/failed=`red`/idle=`overlay`, dot + glow, `pulse` anim when working) +
  header action buttons (Button, icon clock/brain/kebab).
- **UserBubble** (custom): right-aligned (`cross_align(End)`), `with_alpha(accent,0x1a)` fill +
  `with_alpha(accent,0x33)` border, per-corner radius `14,14,4,14`, `text` body, 13/1.5, max-width ~78%.
- **AssistantBubble** (custom): left-aligned, gradient **Avatar** (26px, `linear-gradient(150deg, accent,
  accent_dim)` via `.background_linear_gradient`, crust sparkle) + bubble (`surface` fill, `hairline` border,
  per-corner `14,14,14,4`). Body text plain (markdown rich-rendering deferred).
- **Composer** (`freya.json: Thread.Composer`, custom footer): the bordered input box (`bg_deep` fill,
  `surface_max` border, radius 12) wrapping the built-in **Input** (placeholder "Ask, or type / for a flow…")
  + a model/worktree mono chip row; the **SendButton** (Button, 42px, `accent` fill + `crust` icon, accent
  glow shadow; `surface_max`+`red` stop state while working). Wired to the existing `AppState::send`.
- **Layout:** the existing `Content::Flex` column (header / `Size::flex(1.0)` scroll thread / pinned composer)
  from the 2a fix — keep it; restyle its children.

**P1 acceptance:** headless snapshot of the thread (seeded mock transcript: user + assistant turns) matches
the design's center column — accent-tinted bubbles with tails, gradient avatar, status puck, styled composer.
Truthfulness intact (assistant text only from delta/final). Live run shows the real conversation restyled.

## Phase 2 — Sidebar

`freya.json: Sidebar`. Project switcher (built-in **Select**, `surface`/`hairline_strong` → `accent` on
open), search (built-in **Input**, `bg_deep`/`surface_max`), New-conversation (**Button**, accent fill +
glow), conversation rows (**SideBarItem**: active=`with_alpha(accent,0x14)` fill + `with_alpha(accent,0x3a)`
border, radius 9; **StatusDot** per conversation state; **WorktreeChip** when present), and the **collapsed
rail** variant (icon column). Wired to the existing `conversations` signal + `open_conversation`.

## Phase 3 — Chrome + right rail

`freya.json: WindowFrame` + `RightPanel` (collapsed). **WindowFrame** (DECISION 2026-06-22: **native** decorations — keep the OS titlebar; do NOT use
`with_decorations(false)`): render the design's Adwaita-style inner top bar (project·conversation breadcrumb +
status) beneath the native titlebar, over a **DesktopBackdrop** (radial accent wash). The frameless variant
is deferred. **Right-panel collapsed rail** (the `ResizableContainer`
rail at 60px, static): a column of **RailButton**s (icon + status ring, `Tooltip` on hover) + the collapse
toggle. Static/placeholder content (no backing) — completes the 3-panel silhouette.

## Data flow / truthfulness / error handling

Unchanged from 2a: transport → `AppState` signals → regions. 2b is **additive styling** — the thread still
renders only transport-delivered content (Rule 1); the connection banner + reducer are untouched. No new
transport.

## Testing

- `freya-testing` render tests for each new/restyled `oxide-ui` component (renders, key style/state variants,
  built-in theming applies).
- Per-phase **headless `render_to_file` snapshot** (extend the `snapshot_shell` harness) at 1200×800 with a
  seeded `MockTransport`; visually compare to the design. The existing 2a suite (29 tests) stays green.
- Live run against agentd after each phase (rebuild + relaunch) for visual confirmation.

## Phase 4 — Responsive desktop (added 2026-06-22)

The design project gained a **`freya2-responsive.jsx`** (responsive desktop) — breakpoint reflow of the
3-panel shell at narrow window widths (auto-collapse the side rails, reduce to fewer columns, etc.). This is
a **shell-level** concern (it does not change P1 thread internals or P2 sidebar internals). P4: retrofit
`freya2-responsive.jsx` via the authoring contract first (emit its `freya.json` of breakpoint rules), then
translate the reflow behavior via the skill onto the P3 shell (window-size → which panels are full/rail).
Built after P1–P3.

## Out of scope (later)

The deferred surfaces above; markdown rich rendering in bubbles (use `MarkdownViewer` when added); the
drag-to-resize handle physics (basic collapse only); **mobile (`freya2-mobile.jsx`) → sub-project 2c
(Android)**, deferred per the desktop-first decision.
