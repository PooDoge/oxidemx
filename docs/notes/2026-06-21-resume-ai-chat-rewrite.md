# RESUME ANCHOR — AI-chat rewrite program (post-compaction entry point)

**Written 2026-06-21, end of a long session.** Everything below is durable (committed/saved).
Read this + memory `project_ai_chat_rewrite` + the spec/plan files it names. Memories
`project_flow_delivery_s1`, `project_connector_architecture`, `reference_autoagents_source`,
`feedback_best_practices_rule`, `feedback_install_and_restart_after_updates`,
`feedback_host_vs_distrobox_builds` are all relevant.

## Where we are (branch state)
- Repo: `juhradial-mx` holds `.git`. Active dev branch = **`phase1-local-llm-gateway`**, checked out at
  worktree **`/run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1`** (Jim's main checkout;
  has 2 untracked files `oxidemx-conductor/agents/background-job.md` + `flows/background-task/` — leave them).
- `phase1-local-llm-gateway` HEAD = **`bc575fd`** (sub-project 1a merge). Earlier same day: S1 flow-delivery
  merged (`ca21ed7`), CLAUDE.md Rule-0 naming-refs (`c26fc23`).
- **Two MERGED feature efforts this session, both fully reviewed (per-task + whole-branch):**
  1. **S1 — flow delivery to chat** (agentd-only chat flows, `conversation_id` linkage, auto-deliver on
     RunFinished, artifact cards, run_status introspection, per-conversation working state). Plus fixes:
     chat X-close hang, `doc-digest` false-"failed" (was in-proc stdout-scrape; agentd path is authoritative),
     mission-control `RunOptions` E0063. See `project_flow_delivery_s1`.
  2. **Sub-project 1a — Projects/Conversations/Worktrees model** (agentd = single source of truth). See
     `project_ai_chat_rewrite` + `docs/superpowers/specs/2026-06-21-projects-conversations-worktrees-design.md`
     + `docs/superpowers/plans/2026-06-21-projects-conversations-worktrees.md`.

## The program (decided 2026-06-21 — full rewrite of the standalone AI chat off iced → Freya)
Decomposition (each: brainstorm→spec→plan→subagent-driven-build→whole-branch-review→merge):
1. **Projects/Conversations/Worktrees model — ✅ DONE+MERGED (bc575fd).**
2. **1b — HTTP/SSE + Tailscale transport — ⏭ NEXT.**
3. **Freya chat app (desktop + Android).**
4. **Overlay chat decommission** (keep radial menu + optional agentd status).
Locked decisions: Freya (v0.4.0-rc.23, cloned `/run/media/system/fastdrive/repos/freya`) does desktop+Android,
**no iOS (Jim doesn't want iOS)**; webview is X11-only → **deferred** (external browser). Cross-platform lives
in the **transport contract** (HTTP/SSE), not shared UI beyond Freya desktop+Android.

## NEXT STEP: 1b — HTTP/SSE + Tailscale transport
Research is DONE (summarized in `project_ai_chat_rewrite` + `docs/research/connector-architecture.md`). Approach:
- agentd grows an **`HttpConnector`** (axum) beside the existing D-Bus connector, sharing one `Arc<AppState>` in
  one tokio runtime (`zbus` already `features=["tokio"]`). agent-protocol shape: `POST /conversations`,
  `POST /conversations/{id}/messages` (+ `/stream` SSE with `Last-Event-ID` reconnect), `GET` history/projects,
  `GET /health`, + the project/conversation/worktree API from 1a.
- **Bind to the Tailscale interface IP, NEVER `0.0.0.0`** (defense-in-depth). **Bearer-token** app auth
  (PassKeys deferred — `.well-known` public-fetch blocker on a tailnet-only host). `tailscaled` sidecar
  (NOT the experimental `tailscale-rs` embed — DERP-only + unaudited). Same API serves the local desktop
  client too (uniform contract for the future Freya app).
- Start by invoking **superpowers:brainstorming** for 1b, then writing-plans, then subagent-driven-development.

## Build / test / process facts (don't relearn these)
- **Host-side** (rustup, no GTK): `agentd`, `oxidemx-conductor`, `oxidemx-agent-core`, `oxidemx-shared` build with
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`. **agentd's cargo package name is `agentd`** (the BINARY is
  `oxidemx-agentd`) — use `cargo test -p agentd`.
- **distrobox `claude_development`** (GTK): only the overlay (`oxidemx-overlay`/`oxidemx-chat`) + `oxidemx-settings`.
  Needs `pop_os_iced`/`libcosmic` symlinks to `../juhradial-mx/*` + do NOT `git submodule update` (it replaces the
  symlinks with empty mountpoints). Never mix host+distrobox over the same `target/`.
- Never repo-wide `cargo fmt`; clippy clean (Rule 2). Install bins: `cp` to /tmp → `pkexec install -m755`.
  Restart `systemctl --user restart oxidemx-agentd.service` (or oxidemx-daemon). Verify running pid start >
  binary mtime. `pkexec` may background on its auth prompt — re-check it landed.
- Process: superpowers brainstorming→writing-plans→subagent-driven-development (fresh implementer per task,
  per-task spec+quality review, ONE whole-branch review on **opus** at the end = the merge gate; it caught the
  real cross-task defects both times). Isolate each build in a git worktree off phase1.
- Live now: the **1a agentd is installed + running** (back-compat: legacy D-Bus `list_projects`→`Vec<String>` +
  `send_message` signature unchanged, so the current overlay still works). `config.json overlay.ai.use_agentd`
  defaults **true** (settings has a toggle now).

## Open items (none blocking)
- **Fast-follow (recommended):** PRE-EXISTING parallel-test flake — the FLOW tests (`OXIDEMX_FLOWS_DIR`/
  `AGENTS_DIR`/`RUNS_DIR`/`TEST_MOCK_FLOW` `set_var` without serialization) intermittently fail under parallel
  `cargo test -p agentd`; single-threaded reliably green. Fix = dep-free shared mutex serializing those tests.
- **1a deferred Minors:** `projects_root`/`list_project_keys` XDG dup + `default_home_config` copy
  (interface.rs vs projects.rs); `ProjectStore`/`ConversationIndex` `save` leave `*.json.tmp` on rename failure.
- **Jim-requested chore:** doc/spec cleanup — prune deprecated specs so context/memories aren't contaminated by
  stale docs.
- **Tidy:** worktrees `../oxidemx-s1` (S1, merged) + `../oxidemx-s1a` (1a, merged) are removable
  (`git worktree remove`); their SDD ledgers/briefs are scratch. Jim was viewing s1a briefs — confirm before removing.
- Old resume notes now SUPERSEDED (candidates for the doc cleanup): `docs/notes/2026-06-21-flow-output-ux-and-bugs.md`
  (the S1 batch — all shipped).
