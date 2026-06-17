# Claude Code prompt — Modernize the OxideMX AI chat UI (iced)

> Paste the contents of this file as the **opening message** to Claude Code in the OxideMX repository. It is a directive role + scoped plan request + acceptance criteria, not a code dump. **Do not delete sections** — each is load-bearing.

---

## ROLE

You are a senior platform engineer pair-programming with the project owner on the **OxideMX radial-menu overlay**. You write idiomatic, `clippy`-clean **Rust** against the **iced 0.14** GUI library (wgpu renderer). You favor small composable view functions, theme-resolved colors over hardcoded hex, and the `Task`/`Subscription` model over thread spawning.

You **never start writing code before producing a written plan the owner approves.** You ask focused clarifying questions when a design implication is ambiguous, but you do not re-litigate decisions already settled in the design artifacts.

> **How to read this prompt:** design tokens, the icon set, the IA decisions, and the per-component contracts are **contracts** — follow them exactly. Code-shape hints (widget names, message variants) are **sketches of intent** — if the pinned iced version suggests a cleaner idiom achieving the same contract, prefer it and note the deviation in your plan.

---

## CONTEXT — what this change is

The AI chat is an agentic assistant embedded in the radial overlay (`overlay-rs/`). It already works functionally: streaming replies, markdown, threads, agent cards, an approval prompt, attachments (transport only). This change is a **visual-system + component pass** — it does **not** add features, it makes the existing surface coherent, scannable, and a daily-driver.

Two design documents define the target. **Open both in a browser before planning:**

| File | What it specifies |
|---|---|
| `designs/ai-chat-visual-system-p0.html` | **P0 — Foundation.** Color roles (mapped to the iced `Kit`), type ramp, spacing scale, radii, elevation tiers, the monochrome icon set (replacing today's Unicode glyphs), and the toolbar/IA consolidation (before → after). |
| `designs/ai-chat-visual-system-p1.html` | **P1 — Components.** Attachments (staged chips w/ thumbnails, multi-attach, doc chip, oversize/unsupported guardrails, sent-bubble thumbnails), images in AI replies (loading/error/zoom + lightbox), message bubbles (timestamp + token/cost meta, links/quote/table, code-block chrome), the collapsible tool/flow card system, and the rich approval card. |

These are **design references built in HTML** — prototypes of look + behavior, **not** code to transpile. Recreate them in iced using the existing `chat_ui` patterns. The HTML uses striped placeholders where the user's real images/screenshots go.

---

## WHERE THE CODE LIVES — file map & per-file contracts

All chat UI is under `overlay-rs/src/chat_ui/`. Colors resolve through the shared `Kit` (`mod.rs`) at view time — **never hardcode hex; add a `Kit` key if a needed role is missing.**

| File | Region | Change |
|---|---|---|
| `chat_ui/mod.rs` | `Kit` palette + `view()` assembly | Add the missing semantic key (see **Token gap** below). Optionally expose token consts (spacing/radii) here. |
| `chat_ui/header.rs` | title + status + action buttons | **IA:** replace the `✱ ◔ ＋` glyph buttons with a **segmented view switcher** (Chat / Skills / Memory / Tasks). Keep `✕` Close. Swap glyphs for the SVG icon set. |
| `chat_ui/threads.rs` | thread chip strip | **IA:** remove the duplicate `＋`; keep a single **New**. Move `▦ ⚙ 🔌` (Command Center / Agents / MCP) into the Command Center destination. Turn the `✦ Flash` text toggle into a **labeled model pill** (picker is P2). |
| `chat_ui/footer.rs` | input + activity line | **Attachments:** staged-attachment row above the `text_editor` — thumbnail chips, doc chips, remove, uploading/oversize/unsupported states. Add the attach icon button. |
| `chat_ui/body.rs` | bubbles, stream, pending-question | **Bubbles:** assistant avatar + per-bubble timestamp/cost meta line; markdown chrome (links/quote/table/code header). **Reply images:** intercept markdown image nodes + model images, render with `image` widget (loading/error/zoom → lightbox). |
| `chat_ui/cards.rs` | agent cards | **Card system:** make tool-call cards **collapsible** (one-line summary → Input/Output), status-colored (running/done/failed); flow-run card with progress bar + sub-agent rows. Keep the 2.5px left rule + 92% width. |
| *(new)* `chat_ui/approval.rs` or extend `body.rs` | approval prompt | **Rich approval card** replacing the plain option-buttons: command preview, guardrail line, Deny+reason / Edit&approve / Always-allow(pattern) / Run it. |

---

## DESIGN TOKENS — contract (values are Catppuccin Mocha; resolve from active theme)

**Color roles** map to existing `Kit` keys: `crust` `mantle` `base` `surface0/1/2` (recessed→hover/border), `overlay0` (disabled/hairline), `text` `subtext1` `subtext0` (3 ink weights), `accent` (active/links/focus), `green` (success), `yellow` (caution), `red` (danger), `mauve` (memory/undo).

> **Token gap — decide in your plan.** The approval/pending state needs an "amber". The `Kit` has **no `peach`**. Either **(a)** map approvals to `yellow` (recommended — already the caution role; what the P1 doc assumes), or **(b)** add a `peach` key to `Kit::from_state` + the theme palette. Pick one and apply consistently.

**Type ramp** (`text().size()`): display 18/600 · heading 16/600 · title 14.5/600 · body 13 · body-sm 12 · label 11.5/500 · meta 11 · caption 10.5 · micro-mono 10. Inter for prose, `Font::MONOSPACE` for code/numbers/IDs/timestamps.

**Spacing** (4px base, +2/6 half-steps): 2 · 4 · 6 · 8 · 10 · 12 · 16 · 20 · 24. Matches existing paddings (footer 10, body 16, gaps 6/8).

**Radii** (by role): control/code 6 · icon-button 9 · card 12 · panel 16 · pill 999.

**Elevation** (`iced::Shadow{color, offset, blur_radius}`, no spread/inset): e0 none · e1 card `0·2·8 / rgba(0,0,0,.35)` · e2 overlay `0·12·32 / .5` · e3 modal `0·24·60 / .6`. Focus = 1px accent border + faint outer shadow (iced can't do spread rings).

---

## ICON SET — contract

Replace all ad-hoc Unicode glyphs (`✱ ◔ ＋ ✕ ▦ ⚙ 🔌 ✦ 🕓 ⛓ ⧉ ✎ $ 📎 📌`) with **one monochrome line set: 24×24 viewBox, 1.7px stroke, round caps/joins, no fill, `currentColor`.** The P0 doc renders all 20 (skills/memories/tasks/new/close/command-center/agents/mcp/model/copy/select/attach/export/retry/send/stop/search/pin/trash/chat) with the glyph each replaces. Ship as SVG via iced's `svg` widget; cache handles the way device/OS glyphs already are (see existing icon cache pattern). The P0 doc's inline `<svg>` paths are usable as-is.

---

## RENDERING CONSTRAINTS (non-negotiable)

- **iced 0.14 only.** Map everything to `container / column / row / button / text / text_editor / text_input / scrollable / stack / mouse_area / markdown / image / svg / canvas`.
- **No native context menu / dropdown / tooltip / popover** — build overlays from `mouse_area` + `stack`. The lightbox, the model picker, and card expanders all follow this.
- **Animation:** per-frame only — reuse the shared `Kit::pulse` sine. No spring/keyframe engine. Spinners = a rotating `canvas` arc or stepped dasharray; keep cheap.
- **markdown images:** `iced::widget::markdown::view` shows only alt text — you must detect image nodes and render them with the `image` widget yourself (P1 §08).
- Window is **resizable**; bubbles already clamp width 360–760 (`bubble_max_width`) — keep that and have attachment grids / cards reflow.

---

## IMPLEMENTATION ORDER (suggested)

1. **Tokens + icon set** (P0) — land the `Kit` token consts + the SVG icon module first; everything else consumes them.
2. **IA consolidation** (P0) — header segmented switcher + strip cleanup + model pill. Pure refactor of `header.rs`/`threads.rs`.
3. **Bubbles + cards** (P1 §09–10) — meta line, markdown chrome, collapsible status cards.
4. **Approval card** (P1 §11).
5. **Attachments + reply images + lightbox** (P1 §07–08) — most new view code; do last.

---

## ACCEPTANCE CRITERIA

- `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check` clean on `overlay-rs`.
- No hardcoded hex in `chat_ui/` — every color via a `Kit` key (grep for `Color::from_rgb` / `#` in the module should only hit `Kit::from_state`).
- All Unicode glyphs in `chat_ui/` replaced by the SVG icon set.
- Each P1 component renders all its states (default / hover / loading / error / empty where applicable).
- Theme switch (oxidemx / mocha / nord / dracula) re-tints the whole surface — verify with a vision-harness screenshot per theme.
- Resizing the window reflows bubbles, attachment grids, and cards without clipping.

## NON-GOALS (out of scope for this pass)

P2 items — model/provider **picker** (only the labeled pill lands now), unified hover message-actions, empty/onboarding states, the shared list/row/toggle panel component, toasts/motion language, and the formal a11y pass. Don't build these yet; leave clean seams.

---

## FIDELITY

**High-fidelity.** Match spacing, radii, type sizes, and color roles in the design docs exactly (redline lines are in each section). Where a value isn't specified, derive it from the token scales above rather than inventing one.
