# Multi-session / worktree coordination

Several AI sessions (Claude Code, Antigravity/Gemini subagents) work
on this repo concurrently. This note records the topology, the one
real incident, and the rules that keep parallel work safe.

## Topology (as of 2026-06-12)

- **Main checkout** `…/mx-master-4-linux/juhradial-mx` — branch
  `rust-gtk4-overlay`. Shared by interactive sessions. The
  widget-plugins feature branch was developed here and merged back
  (`53ada73`, "first-class WASM widget system").
- **Linked worktree** (Antigravity-managed, under
  `~/.gemini/antigravity/brain/…/worktrees/`) — branch
  `subagent-Menu-Porter-WidgetPorter-1885dcce`. Its diff vs
  merge-base touches `overlay-rs/src/render` (27 files),
  `settings-rs/src` (18), `daemon/src` (18) and accidentally
  committed `docs/iced-guide-examples/target/` build artifacts
  (~6k files) — that branch needs a cleanup pass (drop target/,
  rebase onto current head) BEFORE merging.

## What's safe and what isn't

**Separate worktrees are mechanically safe.** Git refuses to check
the same branch out twice; each worktree has its own index and HEAD.
Commits in one cannot corrupt the other. Conflicts only materialise
at merge time.

**Two sessions sharing ONE checkout is the hazard.** The incident:
a parallel session switched the main tree from `rust-gtk4-overlay`
to `widget-plugins` mid-flight; another session's `git commit`
landed on the unexpected branch and `git push origin
rust-gtk4-overlay` silently pushed nothing ("Everything
up-to-date"). Commits were never lost — but they were not where
their author believed.

## Rules

1. **Check `git status -sb` immediately before every commit and
   push.** If HEAD isn't the branch you think it is, stop and look
   (`git log --oneline -3`, `git worktree list`).
2. **Feature work that will span hours gets its own linked worktree
   + branch** (`git worktree add ../oxidemx-<feature> <branch>`),
   exactly like the Antigravity subagent does. Merge back when the
   tree is quiet.
3. **Keep changes additive where surfaces are shared**
   (`oxidemx-shared/src/config.rs`: append serde-defaulted fields,
   never reorder/rename existing ones — both sides then merge
   cleanly).
4. **Never commit build artifacts** — check `git status` for
   `target/` before `git add`; the porter branch shows the cost
   (and we already paid a `git filter-repo` once for >100 MB
   binaries).
5. **Pushes of a shared branch belong to whoever owns the current
   merge state.** After a local merge by another session, don't
   push their 37-commit stack reflexively — confirm or let them.
6. **Installed binaries ≠ HEAD.** With multiple sessions building,
   note which commit a `pkexec install` came from in the commit/
   summary (the deploy memory has the full checklist).

## Merge-time checklist for the porter branch (whoever does it)

1. `git rm -r docs/iced-guide-examples/target` on the branch first.
2. Rebase (or merge) onto current `rust-gtk4-overlay`; expect
   conflicts concentrated in `render/slices/*` (CustomWidgets
   threading), `settings-rs/src/tabs/*`, `daemon/src`.
3. Run the full gate: `cargo fmt --check`, clippy `-D warnings` on
   the five first-party crates, `cargo test`, plus the WGSL naga
   test (`cargo test -p oxidemx-overlay all_wgsl`).
4. Re-run the vision harness for radial + chat + weather popup
   before installing.
