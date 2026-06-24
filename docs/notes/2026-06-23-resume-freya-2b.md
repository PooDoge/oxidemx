# RESUME ANCHOR — Freya app 2b (Collapsible Panels visual implementation)

**Written 2026-06-23.** Post-compaction entry point. Everything below is committed/durable. Read this +
memories `project_ai_chat_rewrite`, `reference_design_system`, `reference_freya_dev`, `feedback_best_practices_rule`,
`feedback_worktree_for_subagent_work`, `feedback_commit_often`.

## Where we are
- Repo `juhradial-mx` holds `.git`. **2a is MERGED to `phase1-local-llm-gateway` (`ec3d1b9`).**
- **2b is IN PROGRESS on branch `2b-collapsible-panels`** (worktree
  **`/run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-2b`**, off phase1). **NOT merged** — we
  merge the whole of 2b to phase1 at the end. Branch tip after the composer fix ≈ `9eae6b9` (run
  `git -C oxidemx-2b log --oneline -5` to confirm).
- 2b = restyle the working `oxide-app` shell to the Claude Design "Collapsible Panels" mockup, built per the
  **`claude-design-to-freya`** skill consuming the validated **`oxide-app/design-pipeline/OxideMX Freya -
  Collapsible Panels.freya.json`** (92 components, validator-clean). Spec:
  `docs/superpowers/specs/2026-06-22-freya-app-2b-collapsible-panels-design.md`.

## Phasing + status
- **P1 — theme + thread: DONE** (final review READY-TO-MERGE). Full-palette flat-named `Theme` +
  `Theme::with_alpha(base,u8)`; `Accent{Cyan,Violet,Amber,Lime}`; `StatusPuck`/`Avatar`(gradient)/`WorktreeChip`;
  styled `Bubble` (accent-tint user `14,14,4,14` / avatar assistant `14,14,14,4`), `ThreadHeader`, `Composer`
  (themed built-in `Input` + accent send). Plan `docs/superpowers/plans/2026-06-22-freya-app-2b-p1-theme-thread.md`.
- **P2 — sidebar: DONE** (visually verified). Project-switcher pill + search `Input` + "+ New" `Button` +
  styled conversation rows (`ListItem` w/ state dot + `WorktreeChip`, selected accent-tint) + collapse rail.
  Dropped 2a's placeholder-page nav demo — the per-region-nav invariant is covered by `nav.rs::region_nav_is_independent`.
  Plan `docs/superpowers/plans/2026-06-22-freya-app-2b-p2-sidebar.md`.
- **Composer send-button overflow: FIXED** (`9eae6b9`) — see the Content::Flex lesson below.
- **P3 — NEXT: native window chrome + right-panel collapsed rail.** DECIDED **native** decorations (NOT
  frameless — do not `with_decorations(false)`); render the design's Adwaita-style inner top bar
  (project·conversation breadcrumb + status) under the OS titlebar, over a `DesktopBackdrop` (radial accent
  wash); + the right-panel collapsed status rail (the 3-panel silhouette). Spec P3 section has the decision.
- **P4 — responsive desktop.** Retrofit `freya2-responsive.jsx` via the authoring contract first (emit its
  `freya.json` of breakpoint rules), then translate the reflow onto the shell. Mobile (`freya2-mobile.jsx`) → 2c.

## Build / run / verify (don't relearn)
- Build/test from inside `oxidemx-2b/oxide-app/` with **`LIBRARY_PATH=/tmp/oxidemx-lib-links`** (ephemeral
  `.so` shim — recreate per `oxide-app/README.md` if gone). `oxide-freya` is a **binary** crate
  (`--bin oxide-freya`).
- **Headless visual check (no display needed):** the snapshot tests in `oxide-freya/src/app.rs`
  (`snapshot_shell` → `/tmp/oxide-shell-expanded.png` at 1200×800; `snapshot_thread_p1`, `snapshot_sidebar_p2`).
  Run e.g. `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_shell -- --ignored`,
  then Read the PNG. **Use this to self-verify EVERY UI change before asking Jim.**
- **Live app:** `cd oxidemx-2b/oxide-app && ./target/debug/oxide-freya` (connects to live agentd over the UDS).
- SDD process: superpowers brainstorm→spec→plan→subagent-driven (fresh implementer per task; per-task or
  batched review; commit often). Ledger `.superpowers/sdd/progress.md`. Each phase = a plan; build via
  subagent-driven-development in this worktree.

## THE recurring bug (now 4 instances) — Content::Flex
**`Size::flex(n)` is only honored when the PARENT rect has `.content(Content::Flex)`** — this applies to
HORIZONTAL rows too, not just scroll columns. A row with a flex-width child + a fixed-width trailing element
(send button / chip / chevron) NEEDS `.content(Content::Flex)` or the flex child eats the whole width and the
trailing element overflows off-screen. Bit: the composer, the sidebar rows, the switcher pill, and the
composer again. Hardened in `oxide-app/FREYA-PATTERNS.md`. **Before finishing any styled row, grep for
`Size::flex` and confirm its parent has `Content::Flex`.** Also: built-in `ScrollView` shows an accent
scrollbar by default → `.show_scrollbar(false)` for side rails. **Follow-up: make this a prominent checklist
item in the `claude-design-to-freya` skill (it's the skill's #1 failure mode).**

## Deferred (need data/reducer work, NOT styling — not in 2b's visual phases)
- Activity / Delivery / Artifact cards — render `activity`/`run`/`card` SSE events the 2a transcript reducer
  IGNORES; a functional addition (reducer + rendering).
- Markdown in assistant bubbles (plain text now) → wire `MarkdownViewer`.
- Right-panel EXPANDED internals (settings/MCP/editor/4 directions/popovers) → overlaps the 2d control plane.

## NEXT STEP after compaction
Resume P3 (native chrome + right rail): write the P3 plan (`writing-plans`) from the 2b spec P3 section + the
`freya.json` `WindowFrame`/`RightPanel(collapsed)` subtrees, then subagent-build in this worktree, headless-
snapshot-comparing each step. Then P4 (responsive). Then merge all of 2b → phase1.
