# OxideMX AI Chat — UI Spec & Redesign Brief

> Living document. Purpose: hand to a design pass (e.g. Claude design) to
> redesign the AI chat surface. Captures **what exists today**, the
> **constraints** of the rendering stack, and the **UI needs** we want
> addressed. Keep it updated as features land.
>
> Last updated: 2026-06-17.

---

## 1. What this is

OxideMX is a Logitech MX-Master radial-menu tool for Linux/Wayland. The
**AI chat** is a panel inside the overlay window — an agentic assistant
(multi-provider: Gemini default, OpenAI/Anthropic/Ollama/Claude-Code)
that can run tools, multi-agent flows, manage device settings, and now
discover & use skills.

- **Rendering:** Rust + **iced 0.14** (wgpu), Wayland (xdg-shell via
  winit) + X11 fallback. Not GTK, not web — so design must map to iced
  primitives (see §6 constraints).
- **Theme:** Catppuccin-Mocha-derived dark palette (tokens in §3),
  applied via a per-frame `Kit` struct (`overlay-rs/src/chat_ui/mod.rs`).
- **Source of truth:** `overlay-rs/src/chat_ui/` (view) +
  `overlay-rs/src/app/` (state/update) + `overlay-rs/src/radial/` (state).

---

## 2. Layout (top → bottom)

The chat is one vertical column inside the overlay window:

```
┌───────────────────────────────────────────────┐
│ HEADER     puck · title ········ ❖ ✱ ◔ ＋ ✕    │  ai_chat_ui/header.rs
├───────────────────────────────────────────────┤
│ THREAD STRIP  [thread chips] ··· ▦ ⚙ 🔌  ✦Flash│  chat_ui/threads.rs
├───────────────────────────────────────────────┤
│                                                 │
│ MIDDLE  (one of:)                               │  chat_ui/mod.rs selector
│   • conversation (bubbles, cards, stream)       │  chat_ui/body.rs
│   • skills panel / memories / tasks / threads   │  chat_ui/{skills,memories,tasks}.rs
│                                                 │
│   [floating: ↓ Latest pill] [✓ Copied toast]    │  body.rs (Stack overlays)
├───────────────────────────────────────────────┤
│ FOOTER  activity line · [input] [➤/■]           │  chat_ui/footer.rs
│   [floating: / command palette]                 │  chat_ui/palette.rs
└───────────────────────────────────────────────┘
```

Key dimensions (current): header `EDGE_PAD + HEADER_H`; footer
`EDGE_PAD + FOOTER_H`; bubble max width `clamp(win_w*0.72, 360, 760)`;
input height 44px; icon buttons 32px; bubble radius 14px (4px on the
"tail" corner).

---

## 3. Design tokens (current palette)

From `Kit` (`chat_ui/mod.rs`). Catppuccin-Mocha-ish:

| Token | Role |
|---|---|
| `crust` / `mantle` | deepest bg / panel bg |
| `surface0/1/2` | card bg / borders / dividers |
| `overlay0` | muted icon |
| `text` / `subtext0` / `subtext1` | primary / secondary / tertiary text |
| `accent` (mauve/blue) | primary action, links, highlights |
| `green` | success (copy toast, skill-on) |
| `yellow` | pinned memory |
| `mauve` | accent variant |
| `red` | stop / destructive |
| `pulse` (0‥1 sine) | breathing animation for "working" dot |
| `alpha` | global fade for the rise-in animation |

Type sizes in use: 10–13.5px (dense). `fade(color, a)` applies global
alpha. No spacing scale / elevation system formalized yet — **a designer
should define one.**

---

## 4. Component inventory (current state)

### Header (`header.rs`)
- Canvas-drawn 32px puck (left), title block, right-aligned icon buttons:
  `❖` Skills, `✱` Memories, `◔` Tasks, `＋` New chat, `✕` close.
- **Issue:** glyph icons are ad-hoc Unicode; inconsistent visual weight;
  no labels/tooltips; active-state styling is minimal.

### Thread strip (`threads.rs`)
- Horizontal thread chips + action chips `▦` Command Center, `⚙` Agents,
  `🔌` MCP, and a `✦ Flash/Pro` model toggle. Clips at the edge.
- **Issue:** two icon clusters (header + strip) with overlapping concepts;
  discoverability low; the model toggle is a tiny text button.

### Conversation body (`body.rs`)
- **Bubbles:** user (right, plain text) and AI (left, **rendered
  markdown**, links clickable). Max-width clamped. Rounded with a tail.
- **Live streaming:** AI replies render markdown **as they stream**, with
  a `▌` cursor (parsed each delta).
- **Hover actions:** `⧉` copy + `⌶` select-toggle appear on row hover
  (whole-row hover region).
- **Right-click context menu:** Copy message / Select text / Paste into
  input — a small card attached under the bubble.
- **Selectable text:** a bubble can swap to a read-only editor for mouse
  selection + Ctrl+C (markdown isn't selectable in iced).
- **Agent cards:** structured cards for command-run / task-scheduled /
  memory-saved / flow results (`cards.rs`).
- **Pending question:** inline approval buttons (permission gating) when
  the agent asks to run a non-allowlisted command.
- **Floating overlays:** `↓ Latest` pill (when scrolled up; suppresses
  auto-scroll), `✓ Copied` toast (top-center, ~1.6s).
- **Empty state:** a plain text list of sample prompts.
- **Issues:** no per-bubble timestamps; no code-block syntax highlighting
  or per-block copy; empty state is unstyled; agent/flow cards styling is
  basic; no avatars / role affordance beyond side+color.

### Footer (`footer.rs`)
- Activity line (breathing dot + status text + "Esc to stop") while a
  turn runs; multi-line input (`text_editor`, Enter sends, Shift+Enter
  newline); accent send button that flips to a red stop.
- **Slash palette:** typing `/` opens a floating fuzzy command list
  (actions, flows, enabled skills). Up/Down/Enter/Esc.
- **Issues:** palette styling is minimal; no icons-with-meaning; input has
  no attach affordance (file/image); (token/cost now shown in the thread strip).

### Panels (mutually exclusive with conversation)
- **Skills** (`skills.rs`, `❖`): search + per-skill enable toggle, source
  badge (Claude/Antigravity/Project), description, count.
- **Memories** (`memories.rs`, `✱`): search, pin, scope tag, retention,
  delete.
- **Tasks** (`tasks.rs`, `◔`): scheduled flows, enable/run/delete.
- **Threads list:** rename / delete / open.
- **Issue:** four panels with similar-but-inconsistent row layouts; no
  shared list/row component; toggles styled differently per panel.

---

## 5. Recent changes (2026-06 session — all shipped)

1. **Live streaming markdown** — replies format as they stream (no
   end-of-message reflow).
2. **Copy button fix** — whole-row hover region (was unclickable).
3. **Right-click context menu** — copy / select / paste.
4. **Selectable bubbles** — read-only editor swap.
5. **Scroll-to-bottom** — `↓ Latest` pill + auto-scroll gating.
6. **`✓ Copied` toast.**
7. **Skills system** — native SKILL.md discovery (Claude + Antigravity),
   Skills panel, slash palette (`/`).
8. **Agent uses skills** — enabled skills' name+description go in the
   system prompt; the agent loads a skill's full body on demand via a
   `use_skill` tool (progressive disclosure). Slash palette also runs
   `.claude/commands/*.md` prompt-templates (`$ARGUMENTS`/`$1`..`$9`).
9. **Backend reliability** — transient errors (429/5xx/timeout/conn)
   auto-retry with backoff (a "retrying" activity shows); failed turns
   render a red **error bubble with a Retry button**; per-thread
   **token usage + rough cost** readout in the thread strip
   (`↑in ↓out · ~$cost`).
10. **Richer I/O** — **thread search** (filter the chat list by title +
    content); **export** a thread to Markdown (📤 → `~/.local/share/
    oxidemx/exports/`); **file attachment** via drag-drop or a 📎 picker
    (staged chip; on submit the agent reads it with read_file /
    parse_document). Code-block **syntax highlighting** is already active
    (iced `highlighter`). Deferred: per-code-block copy button,
    image→vision attach (needs Task/Image plumbing to the provider).

---

## 6. Rendering constraints (iced 0.14) — design within these

- **No HTML/CSS.** Everything is iced widgets: `container`, `column`,
  `row`, `button`, `text`, `text_editor`, `scrollable`, `stack`, `float`,
  `mouse_area`, `markdown::view`, `canvas`.
- **No native context menu / dropdown / tooltip-on-hover** out of the box;
  built from `mouse_area` + `stack`/`float` overlays.
- **Markdown is not selectable** (`markdown::view` renders rich text but
  can't be highlighted) — selection requires a `text_editor` swap.
- **Glyphs/emoji:** the default font renders monochrome symbols (▦ ⚙ ❖ ➤
  ✦ ◔ ✱ ⌶) reliably; color emoji are unreliable. Prefer an **icon font or
  SVG set** for a redesign (iced supports `svg`).
- **Animation:** per-frame via a `Tick`/`pulse`; no spring/keyframe system
  (there's a custom track-animation system available — see repo).
- **Available but unused:** `Float` (cursor/anchored popovers),
  `Stack` (z-layering — already used for overlays), `highlighter` feature
  (syntax highlighting — compiled in, not wired).
- **Window:** transparent regions must be managed (full-size surfaces can
  go opaque-black on Wayland); the chat rises-in via an alpha fade.

---

## 7. UI needs / redesign goals (the brief)

Priorities for a design pass, roughly high→low:

1. **Visual system:** define a spacing scale, elevation/shadow system,
   type ramp, and a consistent **icon set** (replace ad-hoc Unicode
   glyphs; add labels/tooltips). Consolidate the two icon clusters
   (header + thread strip) into one coherent toolbar/IA.
2. **Message bubbles:** clearer role distinction (avatar/label?), better
   density, per-bubble timestamp + token/cost line, **code blocks** with
   syntax highlighting + per-block copy, nicer link/quote/table styling.
3. **Agent & flow cards:** a proper card system — tool-call cards with
   collapsible input/output, flow-run progress, status colors. This is
   how the agent shows its work; today it's basic.
4. **Panels:** one shared list/row component + consistent toggles, badges,
   search, empty states across Skills / Memories / Tasks / Threads.
5. **Slash palette:** elevate to a real command-palette aesthetic
   (sections, icons, keyboard hints, recent/frequent, fuzzy match
   highlighting).
6. **Empty & onboarding states:** styled first-run / empty-thread with
   real, grounded sample actions (run a flow, enable a skill).
7. **Input area:** attach affordance (file/image), model/provider
   switcher, clearer send/stop, optional voice.
8. **Feedback & motion:** toasts (copy/paste/errors), inline error bubbles
   with Retry, subtle enter/exit motion for messages and overlays.
9. **Accessibility/contrast:** verify contrast ratios on the dark palette;
   focus rings; larger hit targets.
10. **Responsiveness:** the window resizes; bubbles/panels should adapt
    (current bubble width is a clamped % — extend the approach).

Deliverables we'd love from a design pass: a token set (colors, spacing,
type, radii, elevation), an icon set, redlined component specs for the
inventory in §4, and mockups for the bubble, card, palette, and panel
patterns.

---

## 8. Roadmap context (so the design anticipates it)

Four themes are in flight (theme 1 shipped):

1. ✅ **Discoverability / Skills** — palette + skills system + agent
   `use_skill` (progressive disclosure) + commands-as-templates. Done.
2. ✅ **Backend reliability** — retry/backoff, token/cost telemetry
   (thread-strip readout), error bubbles + Retry. Done.
3. ✅ **Richer I/O** — thread search, export, file attach (drag-drop + picker); code highlighting already on. Image→vision attach deferred. Done.
4. **Memory & long context** — chat→memory auto-capture, thread
   summarization.

Design should leave room for: a cost/usage indicator, attachment chips,
collapsible tool-call cards, and a skills-active indicator.
