# OxideMX Codebase Split — Execution Handoff

> **Purpose:** self-contained context to split the OxideMX monorepo into three focused projects, for
> AI-coding focus + disk reclaim, **without losing the Freya UI work**. Written so a FRESH context window
> (after `/clear`) can execute it. Pairs with memory `project_codebase_split` and the prior plan doc
> `~/.claude/plans/foamy-plotting-pascal.md` (session 055ed4a1). Status: **planned + validated, not started.**

## How to resume after `/clear`
1. Read this doc fully + the `project_codebase_split` and `project_multi_session_state` memories.
2. Start at **Phase 0** (§7). Do work on a branch in a worktree off `main` (never in the `oxidemx-2b`
   main checkout, which Jim edits concurrently). Tag `pre-reorg` before any code phase.
3. Pause for human review after **Phase 1** (the approach-validating spike) before any repo carving.

---

## 1. Goal & why
OxideMX began as a "MX Master 4 radial-menu overlay + mouse-settings utility", absorbed an embedded AI
chat, and pivoted into an autonomous AI coding agent (agentd backend + Freya frontend). It is now **one
Cargo workspace of ~25–31 crates (~120k LOC)** + a *separate* nested `oxide-app` Freya workspace (~11k),
across worktrees whose `target/` dirs total ~250 GB. Every Claude session searches/indexes/builds the whole
tangle; disk is exploding. **Split into three projects with hard boundaries** so each AI session has a small,
coherent search + build surface; reclaim disk.

## 2. Current repo state (as of 2026-06-28)
- **Trunk = `main`** (renamed from `2b-collapsible-panels`), pushed to `origin` (PooDoge/oxidemx), set as
  default. It is the integration tip: all Freya UI work + agent plumbing. The `oxidemx-2b` worktree is on `main`.
- **The prior reorg plan was never executed** — no PR on origin, no remote reorg branch. Clean slate.
- Worktrees kept: `oxidemx-2b`(main), `oxidemx-phase1`, `oxidemx-s1` (both have untracked `oxidemx-conductor`
  WIP; s1's `background-task` flow backed up to `.backups/s1-background-task-wip/`), `juhradial-mx`
  (rust-gtk4-overlay = the GTK project, separate active line on origin), Antigravity subagent (external).
- **Build modes (CLAUDE.md Rule 3):** agentd + agent-core + conductor build **host-side** (rustup,
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`); the iced overlay builds in the **`claude_development`
  distrobox**; `oxide-app` (Freya) builds in distrobox with `LIBRARY_PATH=/tmp/oxidemx-lib-links` against
  Freya at `/run/media/system/fastdrive/repos/freya`. Never share a `target/` across toolchains.
- **Submodule/symlink gotchas:** `pop_os_iced` + `libcosmic` are SYMLINKS to `juhradial-mx/` (workspace-
  excluded, `update=none`); a fresh worktree's `pop_os_iced` is empty → symlink it before building. Git
  fetch/push fail on these symlinks → use `git -c {fetch,push}.recurseSubmodules=no`. `git worktree remove`
  fails on submodule worktrees → `--force` or `rm -rf` + `git worktree prune`.
- **Disk:** 316 GB free (83% used, 1.9 TB). Reclaimable `target/` now: `juhradial-mx` 100G + `phase1` 98G +
  `s1` 12G ≈ **210 GB** (reproducible build cache).

## 3. Verified findings — the cut is CLEAN (~80% done at the process boundary)
- **Agent backend is GUI-free:** no agent/conductor crate depends on any GUI/iced/freya crate. All
  cross-domain edges point UI→AI, never AI→UI (except mission-control→oxidemx-widgets, see below).
- **`oxide-app` (Freya) is already its own workspace with ZERO `oxidemx-*` deps.** It reaches the backend
  ONLY over the agentd UDS socket (HTTP/1 + SSE) via `oxide-client`'s `Transport`/`UdsTransport`. Wire DTOs
  in `oxide-client/src/dto.rs` are a *tolerant mirror* of agentd's JSON (unknown fields allowed), not a
  shared crate. This is the proven seam the whole split copies; the AI frontend lifts out trivially.
- **`oxidemx-shared`** (~6.3k LOC, 15 files) is 92% radial/device config; the AI footprint is ~470 LOC
  isolated in ONE file `oxidemx-shared/src/config.rs` (`AiConfig`@1227, `AiFxConfig`/`AiStatusFx`@600/642,
  + `AiProvider`/`ModelSpec`/`Capabilities`/`SamplingConfig`/`LocalModelConfig`/`HttpConfig`), nested under
  `AppConfig→OverlayConfig→ai`. `ActionKind` has NO AI variant — the radial menu's only AI link is plain
  `Exec`/`Submenu`/`Settings` slices (runtime, untyped).
- **The coupling to sever is concentrated in TWO straddler crates** (everything else is clean):
  - **`overlay-rs`** (~16k LOC) — a single crate holding BOTH domains: radial (`radial/` 2781, `render/`
    4990, `editor/`, `sampler.rs`, `geometry.rs`, `input.rs`, `haptic_client.rs`, `widget_host.rs`) AND AI
    (`agent/` + `agent_runtime.rs`, `ai_client/`+`ai_client.rs` 851+418, `chat_shell.rs` 739, `chat_ui/`
    3060, `chat_window/`, `handoff.rs` 330, `activity/` 1156). Cargo deps pull BOTH stacks: links
    `oxidemx-agent`, `oxidemx-agent-core`, `oxidemx-agent-proxy` (→ autoagents, conductor). The biggest
    straddle + #1 context-bloat source.
  - **`settings-rs`** — radial settings GUI whose "Agents tab" (`settings-rs/src/tabs/agents.rs`, one file)
    links `oxidemx-conductor` + `oxidemx-agent`.
- **`oxidemx-mission-control`** (AI flow monitor, iced GUI) is the only AI→UI edge: it depends on the
  radial-leaning `oxidemx-widgets`. Resolve before full separation (inline the few styles it uses).
- Embedded overlay-chat **decommission is already the documented direction**
  (`docs/notes/2026-06-21-resume-ai-chat-rewrite.md`: "overlay chat decommission, keep radial menu +
  optional agentd status"). So `oxide-app` IS "AI-frontend v2" replacing overlay's chat.

## 4. Per-crate map (top-level workspace = `oxidemx-2b/Cargo.toml`)
| Crate (pkg) | Purpose | Domain | Workspace deps |
|---|---|---|---|
| `daemon` (oxidemxd) | HID++/D-Bus/KWin radial-menu daemon | RADIAL | oxidemx-shared |
| `overlay-rs` (bins overlay+chat) | iced radial overlay **+ chat** | **STRADDLE** | oxidemx-shared/widgets/icons/window/widget-host/widget-proto/widget-cli/scene-render **+ oxidemx-agent-core/-proxy/-agent** |
| `settings-rs` | iced settings GUI (+ Agents tab) | **STRADDLE** | oxidemx-shared/icons/widgets/widget-host/widget-proto/widget-cli/scene-render **+ oxidemx-conductor/-agent** |
| `popup-rs` | indicator-popup window | RADIAL | oxidemx-shared/widgets/window |
| `oxidemx-window` | xdg-shell window helpers | RADIAL | (none) |
| `oxidemx-icons` | XDG icon resolver/rasteriser | SHARED/infra (radial-only use) | (none) |
| `oxidemx-scene-render` | widget Scene → iced canvas | RADIAL | oxidemx-shared, oxidemx-widget-proto |
| `oxidemx-widgets` | shared iced widgets/palette | SHARED (radial + mission-control) | oxidemx-shared |
| `oxidemx-widget-api/-host/-proto`, `tools/oxidemx-widget-cli` | wasm widget PDK/host/wire/cli | RADIAL | (proto/host edges) |
| `agentd` | production D-Bus Gateway hosting agent core | AI | shared/planner/agent/agent-core/harness/conductor/agent-local/agent-proxy/approval |
| `oxidemx-agent` | AutoAgents runtime (Gemini provider, tools) | AI | oxidemx-shared |
| `oxidemx-agent-core` | UI-free agent brain | AI | oxidemx-shared, oxidemx-agent, oxidemx-conductor |
| `oxidemx-agent-local` | embedded local-LLM (mistral.rs) | AI | oxidemx-shared |
| `oxidemx-agent-proxy` | zbus `#[proxy]` D-Bus client (the clean seam) | AI (seam) | (none) |
| `oxidemx-conductor` (bin) | FlowDoc→DAG→supervised run | AI | oxidemx-shared, oxidemx-agent |
| `oxidemx-planner` | goal→validated StepGraph | AI | oxidemx-ledger |
| `oxidemx-approval` | risk-tier approval classifier (no UI) | AI | (none) |
| `oxidemx-ledger` | task/step persistence | AI (used by harness+planner) | (none) |
| `oxidemx-harness` | cargo check/test verification harness | AI | oxidemx-ledger |
| `oxidemx-mission-control` (bin) | iced flow monitor embedding conductor | AI (→widgets edge) | oxidemx-shared, oxidemx-conductor, oxidemx-agent, **oxidemx-widgets** |
| `spike-iced` | throwaway spike | **DROP** | oxidemx-shared |
| `iced_gtk_themer` | GTK-theme→iced gallery (uses excluded pop_os_iced) | EXPERIMENTAL | (none) |
| `vendor/AutoAgents` (excluded) | vendored agent framework + Gemini-streaming patch | AI | — |

**Freya workspace = `oxidemx-2b/oxide-app/Cargo.toml`** (own `[workspace]`, isolated):
`oxide-client` (Transport/UdsTransport + dto.rs tolerant mirror + mock; deps: hyper/tokio/serde, NO oxidemx-*) ·
`oxide-ui` (Freya widget lib; deps: freya, NO oxide-client/oxidemx-*) · `oxide-freya` (app binary; path-deps
`../oxide-client` + `../oxide-ui` only). agentd seam endpoints: `GET /v1/health|projects|.../conversations|
.../messages(history)`, `POST /v1/conversations|.../messages`, `DELETE /v1/conversations/{id}`,
`GET .../events`(SSE). One connection per request (agentd closes per response); SSE reconnects with Last-Event-ID.

## 5. Target topology — THREE git repos (history-preserving)
```
oxidemx-input/    ~70k LOC  radial/mouse: daemon, overlay-rs(radial-only after sever), settings-rs(after sever),
                            popup-rs, oxidemx-window, oxidemx-icons, oxidemx-scene-render, oxidemx-widget-*,
                            tools/oxidemx-widget-cli, oxidemx-shared(device only), iced_gtk_themer
oxidemx-agentd/   ~30k LOC  AI backend: agentd, oxidemx-agent*, conductor, planner, approval, ledger, harness,
                            mission-control, vendor/AutoAgents, + NEW oxidemx-protocol crate
oxide-app/        ~11k LOC  Freya frontend: oxide-client, oxide-ui, oxide-freya (already a standalone workspace)
```
**`oxidemx-protocol` (NEW) = the ONLY cross-repo code.** Holds: wire DTOs (`Project`/`Worktree`/`Conversation`/
`Turn`/`AgentEvent` + id newtypes + request/response — currently duplicated in agentd + oxide-client/dto.rs)
**plus** the AI-config types lifted out of `oxidemx-shared/config.rs`. Lives in the agentd repo (agentd defines
the API). Consumed by `oxidemx-input` via a pinned `{ git = "<agentd-repo>", package = "oxidemx-protocol",
rev = "<sha>" }`; `oxide-app` KEEPS its tolerant `oxide-client/dto.rs` mirror (preserves unknown-field
tolerance + the future tailnet-TCP swap). Everything else talks over the agentd UDS/HTTP/SSE wire.

**Why 3 repos (not one workspace / not workspace-groups):** only polyrepo hard-bounds the per-worktree
`target/` footprint (the 210 GB problem) and makes the host/distrobox build split *structural* instead of
tribal CLAUDE.md convention. Runner-up = one repo / three cargo workspaces (lower friction, but doesn't
bound disk or focus). Fallback to runner-up only if cross-repo `oxidemx-protocol` rev churn proves painful.

## 6. Decisions locked
- **`AiConfig`:** lift OUT of `OverlayConfig` into `oxidemx-protocol` (not a new `*-shared-ai` crate). The
  AI side reads AI config via the agentd API, not a shared file; the radial side keeps device config.
- **`oxide-app` dto:** keep its tolerant `oxide-client/dto.rs` mirror; do NOT hard-pin it to `oxidemx-protocol`.
  `oxidemx-protocol` is the source-of-truth agentd serializes from; agentd + input depend on it.
- **settings-rs Agents tab:** drop it now (re-add an API-backed version later). One file detaches settings-rs.
- **`oxidemx-mission-control`→`oxidemx-widgets`:** inline the few palette/style closures it uses (don't make
  widgets cross-repo shared).
- **`spike-iced`:** drop (throwaway, `publish=false`). **`iced_gtk_themer`:** candidate to archive/leave.
- **overlay seam:** overlay + settings become **pure transport/proxy clients** (route through
  `oxidemx-agent-proxy` D-Bus and/or a transport client; drop direct `oxidemx-agent*`/`oxidemx-conductor`
  links); embedded overlay-chat decommissioned (its replacement is `oxide-app`).

## 7. Phase plan (DO NOT REORDER — carving repos first = three repos that don't build)
**Each phase ends green + committed (a revert point). All on a branch off `main` in a worktree.**

**P0 — Disk reclaim + sccache (FIRST; biggest value/risk, zero code risk).**
- `git status` + `git stash list` each worktree to confirm clean, then `rm -rf` (or `cargo clean`) the
  reproducible `target/` in `juhradial-mx`(100G) + `oxidemx-phase1`(98G) + `oxidemx-s1`(12G) ≈ 210 GB.
  (KEEP `oxidemx-respsidebar-target` 24G = warm main build target; `/tmp/oxidemx-host-target` 9G = agentd.)
- Install **sccache** via rustup/cargo into `$HOME` (NEVER `rpm-ostree` — atomic Fedora); set as
  `RUSTC_WRAPPER` so re-clones don't recompile from scratch.
- *Cost:* those worktrees recompile on next build (source untouched). Exit: ~525 GB free.

**P1 — Extract `oxidemx-protocol` in-place + un-nest `AiConfig` (one workspace, stays green). The
approach-validating spike — STOP + review after.**
- Create `oxidemx-protocol` as a new workspace member. Move wire DTOs out of
  `agentd/src/{model.rs,seams.rs,conversations_index.rs}`; lift the AI-config block out of
  `oxidemx-shared/src/config.rs` AND lift `AiConfig` out of `OverlayConfig` (its own commit; grep every
  `.overlay.ai` / `config…ai` access first). Repoint `agentd` to `oxidemx-protocol`.
- `cargo check` whole workspace green (host-side for agentd) + a runtime smoke (agentd loads AI config via
  API; overlay still loads device config — compiles either way, so test at runtime). Commit.

**P2 — Sever the two straddlers (riskiest CODE phase).**
- `overlay-rs`: rewrite `src/ai_client.rs`(418) + `src/ai_client/tools.rs`(851) + `src/agent/` shims +
  `src/app/agent_events.rs`(343) + the 3 agent call sites in `src/app/update.rs`(2333) to go through a
  transport/proxy client; remove `oxidemx-agent*` from `overlay-rs/Cargo.toml`; decommission embedded chat
  (`src/chat_ui/`, `chat_window/`, `chat_shell.rs`, `handoff.rs`, `activity/`). Radial/render/editor untouched.
  **Order guard:** build+smoke `oxide-app` against agentd UDS FIRST (confirm it replaces the chat), tag
  `pre-sever-overlay`, THEN delete the overlay chat.
- `settings-rs`: drop `src/tabs/agents.rs` (detaches `oxidemx-conductor`/`oxidemx-agent`).
- Exit: no INPUT crate links any AI crate; `cargo check` green per binary; commit each sever separately.

**P3 — Carve 3 repos (history-preserving, into COPIES).**
- `git filter-repo --path <crate> …` (or `git subtree split`) per cluster into fresh repos; add per-repo
  `Cargo.toml` (lift `[workspace.dependencies]` rows), `.gitignore`(`/target`), focused `CLAUDE.md`, wire the
  `oxidemx-protocol` git dep in input. `oxide-app` = a directory move; convert its absolute Freya path-deps
  (`/run/.../repos/freya/...`) to a pinned git/submodule for portability. Keep the monorepo branch as
  fallback; verify each repo `cargo check`s in its build mode BEFORE deleting source. Push each origin.

**P4 — Dev-env reset.** Per-repo Serena `activate_project`; per-repo `CARGO_TARGET_DIR`/build mode; confirm
sccache shared; update memories (`project_multi_session_state`, `feedback_host_vs_distrobox_builds`).

## 8. Freya-UI safety (the #1 user concern) — STRUCTURAL guard
The Freya work is on `origin/main`, pushed; `oxide-app` is a standalone workspace with **zero `oxidemx-*`
deps**. **No phase modifies `oxide-app`'s contents — P3 only MOVES the directory.** Guards: tag `pre-reorg`
before any code phase; do everything in an isolated worktree off `main` (never `git restore`/`reset` shared
files in the `oxidemx-2b` main checkout — Jim edits it concurrently); before deleting overlay's embedded
chat in P2, confirm `oxide-app` builds + talks to agentd. `main` stays untouched until P3's carve copies from it.

## 9. Risks & mitigations
| Risk | Guard |
|---|---|
| Losing Freya UI | §8 — it's on main, isolated, only dir-moved at P3; tag `pre-reorg`; isolated worktree |
| Delete overlay chat before oxide-app replaces it | P2 ordering: build+smoke oxide-app first; tag `pre-sever-overlay` |
| `AiConfig` un-nest compiles but reads wrong path | runtime smoke (don't trust `cargo check` alone) |
| `git filter-repo` corrupts history/drops crates | carve into COPIES; keep monorepo fallback; verify each builds first |
| `rm -rf target` with uncommitted source | `git status`/`stash list` each worktree first; target/ is reproducible |
| Mixing host/distrobox toolchains over one target/ | agentd stays on `/tmp/oxidemx-host-target` throughout |
| Reordering phases (carve first) | the de-tangle-in-place → green → carve order is non-negotiable |

## 10. Immediate next action
Start **P0** (reclaim ~210 GB + sccache), then the **P1 `oxidemx-protocol` spike** on a branch in a worktree
off `main`, ending green + committed; **pause for human review before P2/P3**. This validates the load-bearing
assumption (the `oxidemx-shared` AI/device config un-nests cleanly + the workspace stays green) cheaply,
before any irreversible repo carving.

## 11. References
- Prior plan: `~/.claude/plans/foamy-plotting-pascal.md` (session 055ed4a1).
- Memories: `project_codebase_split`, `project_multi_session_state`, `feedback_host_vs_distrobox_builds`,
  `feedback_no_rpm_ostree`, `feedback_worktree_for_subagent_work`, `project_connector_architecture`.
- Decommission direction: `docs/notes/2026-06-21-resume-ai-chat-rewrite.md`.
- s1 WIP backup: `.backups/s1-background-task-wip/`.
- Key files: `Cargo.toml`, `overlay-rs/src/{ai_client.rs,ai_client/tools.rs,agent/,app/agent_events.rs,
  app/update.rs,chat_ui/,chat_window/,chat_shell.rs,handoff.rs,activity/}`, `settings-rs/src/tabs/agents.rs`,
  `oxidemx-shared/src/config.rs`, `agentd/src/{model.rs,seams.rs,conversations_index.rs}`,
  `oxide-app/Cargo.toml`, `oxide-client/src/{transport.rs,uds.rs,dto.rs,sse.rs}`.
