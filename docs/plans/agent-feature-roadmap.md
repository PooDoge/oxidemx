# Agent feature roadmap — lessons from the best autonomous agents

Distilled from a 2026-06-12 survey of Claude Code, Codex/ChatGPT
Agent, Cursor, Windsurf/Devin, Goose, Gemini CLI, Open Interpreter,
OpenHands, Warp, AnythingLLM, Khoj, Leon, and OpenClaw (the
single-user personal-agent reference). Full reports in session
89b684a1 transcripts. Prioritised impact × effort against what the
OxideMX agent already has (Interactions API, search/exec/schedule/
memory tools, two modes, approval cards, vision hook, threads).

## Field convergences (context for the choices)

1. **MCP won** as the plugin bus; "extension" now means a manifest
   bundling MCP servers + a context file + commands + permissions.
2. **Markdown is the persona substrate** — CLAUDE.md / GEMINI.md /
   AGENTS.md / OpenClaw's SOUL.md; hierarchical, user-editable,
   with the 2026 refinement of *trigger-conditional loading*
   (Cursor glob rules, OpenHands keyword microagents).
3. **Graduated autonomy ladder** (chat-only → approve-all →
   read-only-auto → allowlist → yolo+sandbox). Plain denylists were
   repeatedly bypassed (Cursor deprecated theirs); allowlist +
   classification is the pattern that survived.
4. **Proactivity differentiates personal assistants** from coding
   agents: scheduled outputs, heartbeats, agent-initiated contact.
5. **Undo as a feature**: checkpoint/restore independent of git.

## Tier 1 — high impact, low effort

1. **SOUL.md / USER.md persona files** (OpenClaw pattern).
   `~/.config/oxidemx/soul.md` (identity/tone/values — loaded into
   every system prompt, user-editable) + `user.md` (durable facts
   about the user, distinct from the rotating memory store). Editing
   personality becomes a text-file edit. Settings page: a simple
   editor pane. Per-mode sections (general vs settings-customizer).
2. **Heartbeat distinct from cron** (OpenClaw's two-tier design).
   One recurring timer wakes the agent with full context + a
   `heartbeat.md` checklist; replies with a sentinel ("HEARTBEAT_OK")
   → no notification; anything else → GNOME notification + thread
   message. OxideMX uniquely has a hardware daemon worth heartbeating
   about (battery low, device switched, build finished).
3. **Read-only auto-approval tier** (Goose smart-approve,
   static version). Classify tools: read-only (search, memory
   list/search, screenshot) auto-pass; mutating (exec, config write,
   timer create) keep approval cards. Add an **"always allow this
   command" button on the approval card itself** that appends to the
   allowlist (Claude Code's killer detail). Kills most confirmation
   fatigue.
4. **Prefix/glob allowlist patterns** for execute_command
   (`git status*`, `systemctl --user status*`) in a visible config
   file, replacing exact-match entries.

## Tier 2 — high impact, medium effort

5. **Recipes bound to radial slices** (Goose recipes, our form
   factor). A recipe file = name + icon + instructions + initial
   prompt + tool scope + parameters; bindable to a slice/submenu.
   "Summarize clipboard", "explain this screenshot", "device
   briefing" as one-flick agents. Generalises the two hardcoded
   modes into user-definable files.
6. **Scheduled-run delivery contract** (Khoj/AnythingLLM): every
   scheduled agent run lands as a GNOME notification + a message in
   a dedicated thread; silent when nothing to report.
7. **Per-request context chips** (ChatGPT Work-with-Apps + Open
   Interpreter clipboard): toggle chips in the chat input — "📋
   clipboard", "🪟 current app" (focused window title/app-id via our
   privileged GNOME extension — nearly free on our stack, hard for
   everyone else on Wayland). Opt-in per message, never always-on.
8. **Config checkpoint/undo for the settings customizer** (Gemini
   /restore): snapshot config dir before agent writes; "revert last
   agent change" button. Big trust win for a mode that edits live
   mouse behaviour.

## Tier 3 — worthwhile, higher effort

9. **Trigger-conditional instruction files** — markdown snippets
   with `triggers: [keyword|app-id]` frontmatter, loaded only when
   matched (e.g. gaming.md when Steam focused). Pairs with #7.
10. **Research subagent** — second Interactions session, own system
    prompt, read-only tools, returns a summary card. We already have
    session forking via previous_interaction_id.
11. **Audit log** — append-only JSONL of tool calls + approval
    decisions + result hashes; viewer in settings. Invaluable the
    first time a scheduled agent surprises.
12. **Live streaming tool-status cards** — spinner → collapsed
    result per tool call (Warp/ChatGPT "show the work").

## Deliberate anti-features

- **Marketplace/plugin distribution** — single-user local; OpenClaw's
  malicious-skill incidents are the cautionary tale. Local recipe
  files give ~90% of the value with zero supply-chain surface.
- **Docker sandboxing** — wrong weight for a mouse-driver companion
  on atomic Fedora; allowlist + confirm + audit log is right. If
  ever needed: bubblewrap (what Codex/Claude Code use on Linux).
- **LLM permission judge** — static read/write classification
  suffices at our tool count; the judge adds latency + cost.
- **Multi-agent fleets** — solves a parallel-coding problem we don't
  have. One research subagent is the ceiling.
- **Voice** — the radial gesture IS our summon mechanism; STT/TTS on
  Wayland is a huge surface for marginal gain.

## Addendum (terminal-agent deep dive)

- **Per-turn token/cost readout** (Aider `/tokens`): show tokens +
  est. cost per turn in the chat footer — cheap, builds trust in
  the heartbeat/scheduled features' running cost. → Tier 2.
- **"Approve once / always / reject with reason"** (OpenHands
  `reject_pending_actions(reason)`): letting the user attach a
  reason to a rejection feeds the agent corrective context instead
  of a silent no. Small addition to the approval card. → Tier 1,
  fold into item 3.
- Aider's git-as-undo (every agent edit auto-committed, `/undo`)
  reinforces Tier 2 #8 — for config edits, an auto-commit shadow
  repo is the strongest variant.
- AGENTS.md is now the cross-vendor rules filename (Warp default,
  OpenHands, Open Interpreter Rust rewrite) — name our per-recipe
  instruction files accordingly for familiarity.

## Addendum 2 (personal-assistant deep dive — OpenClaw details)

Refinements to Tier 1 items from OpenClaw's verified docs:

- **First-run ritual** (BOOTSTRAP.md → IDENTITY.md): on first chat
  the agent interviews the user and WRITES its own identity/user
  files, then the bootstrap file is deleted. Charming onboarding
  for our soul.md/user.md — the assistant names itself and fills
  the files in conversation rather than shipping blank templates.
- **Per-file injection budgets** (20k chars/file, 60k total) — cap
  soul.md/user.md/heartbeat.md the same way so a runaway file can't
  eat the context window.
- **Heartbeat cost knobs** worth copying: active-hours window,
  skip-when-busy (defer while a chat turn is in flight), and a
  light-context variant for cheap ticks.
- **Recipe requirements gating** (skills `requires: bins/env/os`):
  a recipe declaring `requires: { bins: ["wl-paste"] }` greys out
  in the picker when the binary is missing — cheap robustness.
- Anti-marketplace stance reinforced: ClawHub's "ClawHavoc"
  campaign planted 824 malicious skills; registry trust is a
  full-time job we should not take on.

## Addendum 3 (Claude Code / Gemini CLI primary-source pass)

- **Background monitors** (Claude Code plugin `monitors.json`: a
  watcher command whose stdout lines arrive as agent notifications)
  — a push-based complement to the heartbeat: our daemon could feed
  battery/device/Easy-Switch events straight into the chat thread
  as they happen instead of waiting for the next heartbeat tick.
- **Dynamic context injection in recipes** (both vendors support
  `!`cmd`` pre-execution in prompt files): recipe prompts that
  embed e.g. `!`wl-paste`` make the clipboard-summarizer recipe a
  one-liner. Gate behind the same confirm dialog Gemini uses.
- Everything else in the two catalogues either confirms existing
  tiers or is enterprise/IDE-scale machinery out of scope for a
  single-user desktop assistant.
