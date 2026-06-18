# Agent features — implementation plan (Tier 1 + early Tier 2)

Executable companion to `agent-feature-roadmap.md` (the survey).
Scope here: persona files, approval-card overhaul, allowlist
patterns, heartbeat, and the groundwork recipes need. Written so a
fresh session (or worktree subagent) can implement without
re-deriving context.

## Worktree workflow (per multi-session-coordination.md)

```bash
git worktree add ../oxidemx-agent-features agent-features
# implement + commit there; merge back when mainline is quiet.
```

Collision posture vs the WidgetPorter branch: our footprint is
`overlay-rs/src/{ai_client*,agent/,chat_ui/}` + additive
`oxidemx-shared/src/config.rs` fields + a new settings panel —
near-zero overlap with the porter's `render/slices` + widget-store
territory. Safe to develop in parallel; merge AFTER the porter
lands if possible, else ours first (additive config merges clean).

## 1. Persona files — soul.md / user.md (Tier 1 #1)

**Files:** `~/.config/oxidemx/soul.md`, `~/.config/oxidemx/user.md`.

- Loader in `overlay-rs/src/agent/persona.rs`: read both files,
  cap each at 20k chars / 60k total (OpenClaw's budgets), strip
  HTML comments. Cache with mtime check per turn (cheap stat).
- Injection: `AgentMode::system_instruction()` order = base persona
  → soul.md → MEMORY RULES → user.md → memories block. Soul wins
  over base text by coming after it; per-mode `## general` /
  `## settings` H2 sections filtered by mode (absent sections =
  whole file applies).
- **First-run ritual** (OpenClaw BOOTSTRAP.md): when soul.md is
  missing AND the chat opens, inject a one-time bootstrap
  instruction: interview the user briefly (name/tone/boundaries),
  then write both files via a new `persona` tool (`action:
  write_soul|write_user`, full-content replace, capped). Delete
  nothing; the files simply exist afterwards. Show a card linking
  to the files.
- Settings: a "Persona" section (AI page category) with an inline
  text editor per file + "open in editor" button.
- **Gotchas:** files are user-owned — the agent may only rewrite
  them via the persona tool (which the approval card gates as
  mutating); never auto-rewrite from consolidation. Config exports
  must include them (unlike gemini.key).

## 2. Approval-card overhaul (Tier 1 #3 + addenda)

Today: `ask_multiple_choice_question` card + exec allowlist
(exact match) + confirm flow in `ai_client/tools.rs`.

- **Static tool classification** in `tools.rs`: `fn is_read_only
  (tool, args) -> bool`. Read-only: google_search, memory
  list/search, screenshot/vision, schedule_task list, persona read.
  Mutating: execute_command (unless allowlisted), memory
  save/delete/pin/consolidate (save can be auto: it's what MEMORY
  RULES governs — decide: auto), schedule create/enable/delete,
  set_menu_config, persona write.
- **Card actions** (extend the approval card UI in `chat_ui/`):
  - *Approve once* (existing)
  - *Always allow* — appends a PATTERN to the allowlist (for exec:
    the command's first token + ` *`; user can edit before saving)
  - *Edit & approve* — textarea pre-filled with the command/args
    (LM Studio pattern); edited value replaces tool args
  - *Reject with reason* — optional text field; the reason is
    returned to the model as the tool result ("user declined:
    <reason>") instead of a bare error (OpenHands pattern).
- **Allowlist patterns** (Tier 1 #4): upgrade matching to
  prefix-with-`*` (one `*` spans to end; match against each
  subcommand after splitting on `&&`/`;`/`|` — Claude Code's rule,
  prevents `git status && rm -rf` smuggling). Storage:
  `overlay.ai.exec_allowlist: Vec<String>` (already exists? verify
  field name in AiConfig before renaming anything — additive only).
- **Gotchas:** compound-command splitting MUST come before
  matching; strip wrappers (`timeout`, `nice`, `env`) like Claude
  Code does or the pattern is trivially bypassed. Keep the
  `rm -rf /` circuit-breaker unconditional.

## 3. Heartbeat (Tier 1 #2)

- `~/.config/oxidemx/heartbeat.md` — user checklist, same budget
  caps as persona files.
- Runner: a systemd user timer (`oxidemx-heartbeat.timer`, default
  every 30 min, managed via the existing `agent/tasks.rs`
  machinery) executing `oxidemx-overlay --heartbeat` (new flag:
  headless one-turn run, no window) OR a daemon-spawned headless
  call — prefer the overlay flag so it reuses ai_client wholesale.
- Turn contract (OpenClaw): system instruction = soul/user/memories
  + heartbeat.md + "if nothing needs attention reply exactly
  HEARTBEAT_OK". Response == HEARTBEAT_OK → exit silently.
  Anything else → `notify-send` (via gdbus, the extension, or
  `notify-rust`) + append to a dedicated "Heartbeat" thread (thread
  storage already exists in chat_threads).
- Cost knobs (config, serde-defaulted): `active_hours: (u8,u8)`,
  `skip_when_chat_open: bool` (check the overlay bus name's owner),
  `every_minutes: u32`, `enabled: bool` (default FALSE — opt-in).
- Read-only tool scope for heartbeat turns (no exec/no writes) in
  v1; revisit after the approval overhaul.
- **Gotchas:** the daemon's overlay single-instance guard — a
  headless heartbeat run must NOT claim `org.oxidemx.overlay` (use
  the OXIDEMX_VISION_SHOT-style exemption: a `--heartbeat` flag
  skips request_name). Never run while a turn is in flight.

## 4. Scheduled-run delivery (Tier 2 #6)

Extend `agent/tasks.rs`-created units: wrap the scheduled command
so stdout lands in a thread message + notification, with the same
HEARTBEAT_OK-style silence contract. Goose's `retry.checks`
pattern: optional `checks: [shell…]` per task; failed checks retry
once with the failure appended to the prompt, then report.

## 5. Recipes (Tier 2 #5) — groundwork only in this pass

Define the file format now so persona/heartbeat don't need rework:
`~/.config/oxidemx/recipes/<slug>.md` with TOML/YAML frontmatter
(`name`, `icon`, `description`, `tools = [...]`, `params`,
`prompt`); body = instructions. `AgentMode` becomes
`Mode::Recipe(slug)` alongside the two built-ins (which become
bundled recipes eventually). Slice binding via a new
`ActionKind::AgentRecipe` — coordinate with widget-plugins schema
(config schema_version bumps live there now — additive field, no
bump needed).
Dynamic `!`cmd`` injection in prompts: gate behind a per-recipe
`allow_shell_context: bool` + show the resolved command in the
approval card the first time (Gemini CLI's dialog pattern).

## Cross-cutting gotchas (hard-won, do not relearn)

- **Interactions API**: function-call arguments arrive ONLY via
  `step.delta` `arguments_delta` JSON-string fragments (NOT in
  step.start) — any new SSE consumer must reuse `FnCallFold`.
  Custom functions can't mix with built-in google_search (nested
  call pattern in tools.rs). `store: false` for one-shot nested
  calls.
- **WGSL uniforms**: never `vec3` padding (16-byte alignment makes
  host/shader sizes diverge — wgpu rejects at draw). naga test
  (`all_wgsl`) validates shader syntax but CANNOT catch size
  mismatches.
- **Memory store**: write-time Jaccard dedupe collapses
  near-identical texts — test fixtures need distinct token sets.
  Pinned entries are physically excluded from consolidation.
- **Process hygiene**: `pgrep/pkill -x` silently fails for names
  >15 chars (`oxidemx-settings`!); never `ps | grep <name> | kill`
  from a script whose own cmdline contains `<name>`; overlay
  duplicates exit via NameTaken — headless modes must skip the
  name claim.
- **Deploys**: pkexec can't stat /run/media — install via /tmp.
  Full checklist in the deploy memory note.
- **iced**: cache_epsilon() must stay until the upstream stale-layer
  fix; canvas meshes clip in offset containers (disc stays
  top-left-anchored); emoji don't render in the default font.

## Definition of done (per feature)

fmt + clippy `-D warnings` + `cargo test` green on touched crates;
vision-harness screenshot for any UI change; release build,
install, cycle, live verify; commit message documents behaviour +
rationale; lessons.md/memory updated when a new gotcha surfaced.
