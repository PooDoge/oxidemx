# Claude Design — brief for the OxideMX AI chat

> Paste this whole file to Claude design (or an equivalent UI design pass).
> It names what to design, the hard rendering constraints, and the exact
> deliverables. The companion file **`ai-chat-ui-spec.md`** (same folder)
> is the source-of-truth inventory of the *current* UI — read it first.

---

## Role & goal

You are designing the UI for the **OxideMX AI chat** — an agentic
assistant embedded in a Linux/Wayland desktop overlay (a Logitech
MX-Master radial-menu tool). The chat already works functionally; we
need a **cohesive visual system + component designs** for a set of new
and under-designed features. Deliver designs an engineer can implement
directly in **iced 0.14 (Rust, wgpu)** — see constraints below.

Read `ai-chat-ui-spec.md` for the current layout, palette tokens, full
component inventory, and what already shipped. Don't redesign what works
unless it's listed below; focus on the gaps.

## Hard rendering constraints (non-negotiable — design within these)

- **iced 0.14 only. No HTML/CSS, no web.** Everything maps to iced
  widgets: `container`, `column`, `row`, `button`, `text`, `text_editor`,
  `text_input`, `scrollable`, `stack`, `float`, `mouse_area`,
  `markdown::view`, `image`, `svg`, `canvas`.
- **No native context menu / dropdown / tooltip** — build from
  `mouse_area` + `stack`/`float` overlays.
- **Icons:** prefer a monochrome **icon font or SVG set** (color emoji
  render unreliably). Today we use ad-hoc Unicode glyphs — replace them.
- **Dark theme**, Catppuccin-Mocha-derived tokens (see spec §3). There is
  **no formal spacing/elevation/type scale yet** — define one.
- **Animation:** per-frame only (a shared `pulse` sine); no spring/
  keyframe engine. Keep motion cheap.
- Window is **resizable**; layouts must adapt (bubbles already use a
  clamped % width).

## Design tasks (priority order)

### P0 — Visual system foundation
1. **Design tokens:** spacing scale, elevation/shadow tiers, a type ramp
   (today 10–13.5px ad hoc), border-radii, and a refined color role map
   on top of the existing palette. Verify contrast ratios (dark bg).
2. **Icon set:** one coherent set replacing the Unicode glyphs
   (skills/memories/tasks/new/close/command-center/agents/mcp/model/
   copy/select/attach/export/retry). Provide SVGs or an icon-font spec.
3. **Toolbar/IA consolidation:** today there are *two* icon clusters
   (header `❖ ✱ ◔ ＋ ✕` + thread strip `▦ ⚙ 🔌 ✦Flash`) with overlapping
   concepts. Design one coherent information architecture.

### P1 — Images & attachments (functionally present, no real UI)
4. **Attachment chips with thumbnails:** an attached image should show a
   **thumbnail preview** in the input area and in the sent user bubble
   (today it's a text chip `📎 name`). Design the staged-attachment state,
   multi-attachment row, remove affordance, and a doc (non-image) chip.
5. **Images in AI replies:** design how the chat **displays images the
   model returns** — both markdown `![](url)` images and
   model-generated inline images — with loading, error, and click-to-
   zoom/lightbox states. (Engine note: iced's default markdown viewer
   shows only alt text; we'll render images ourselves with `image`.)
6. **Multi-attachment + guardrails:** layout for several attachments;
   visual treatment for over-size / unsupported files.

### P1 — Conversation surface
7. **Message bubbles:** clearer role distinction (avatar/label?),
   density, **per-bubble timestamp + token/cost line**, link/quote/table
   styling. Code blocks already syntax-highlight + have a copy button —
   refine their chrome.
8. **Agent & flow cards:** a proper card system — tool-call cards with
   **collapsible input/output**, flow-run progress, status colors. This
   is how the agent shows its work; today it's basic.
9. **Rich approval card:** the permission prompt (agent wants to run a
   non-allowlisted command) is plain text buttons today — design a card
   with the command preview, an "always allow" affordance, and clear
   allow/deny.

### P2 — Controls & states
10. **Model/provider switcher:** today only a Gemini Flash↔Pro text
    toggle — design an in-chat picker across providers (Gemini / OpenAI /
    Anthropic / Ollama / Claude-Code) + model.
11. **Message actions:** unify hover actions (copy / select / **regenerate**
    / edit-and-resend) into one consistent affordance set.
12. **Empty & onboarding states:** styled first-run / empty-thread with
    real, grounded sample actions (run a flow, enable a skill, attach an
    image).
13. **Shared panel component:** Skills / Memories / Tasks / Threads each
    hand-roll rows today — design one list/row/toggle/badge/search
    pattern they all use.

### P2 — Cross-cutting
14. **Feedback & motion:** toasts (copy/paste/error), the streaming
    cursor, the "↓ Latest" pill, inline error bubble + Retry — give them
    a consistent language.
15. **Accessibility:** focus rings, hit-target sizes, contrast on the
    dark palette.

## Deliverables

For each task above, provide:
- **Mockups** (annotated; ASCII/box layout is fine if it's precise about
  spacing, hierarchy, and states — we implement in iced, not Figma).
- **All states** (default / hover / active / loading / error / empty).
- **Redlines**: spacing, sizes, radii, colors referencing the token set.
- The **token set + icon set** as concrete, reusable specs.

Optimize for: clarity, scannability, low visual noise, and a "daily
driver" feel. Assume a power user who lives in the keyboard. When a
choice trades fidelity for iced-implementability, pick implementable.
