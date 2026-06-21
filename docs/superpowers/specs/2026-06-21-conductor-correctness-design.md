# Conductor Correctness (W1) — Design Spec

**Date:** 2026-06-21
**Status:** Approved (design), pending spec review
**Part of:** the agent-execution architecture program (W1 of W1→W2→W3→W5→W4; see
`2026-06-21-agent-execution-architecture-analysis.md` for the layout — to be written in W3).
**Source analysis:** flow-DAG audit (2026-06-21) — see §"Defect inventory".

---

## Goal

Eliminate the flow-authoring foot-gun that made `research-digest` appear to run out of order,
and make the class of bug **un-shippable** going forward: fix the two defective flows, harden
`validate` to reject the foot-gun, expose the real concurrency contract (`stages`), and lock it
in with per-flow regression tests. No engine/scheduler changes — the scheduler is correct.

## Background (verified, not assumed)

The conductor scheduler (`oxidemx-conductor/src/supervisor.rs`) is correct: a step is "ready"
when every id in its `needs` is `done`; concurrent-ready steps run in parallel via a `JoinSet`.
A step with `needs=[]` is an **entry node** that runs at t=0. The foot-gun: a `reflect` step
typically declares `needs=[]` and relies only on `target=<step>` — but `target` is a *runtime*
gate (`effective_target`), invisible to Kahn's topological sort, so the step is treated as an
entry node and the printed `order:` string is misleading. `validate` does not catch this.

## Defect inventory (from the audit)

| Flow | File | Step | Defect | Sev | Fix |
|---|---|---|---|---|---|
| research-digest | `oxidemx-conductor/flows/research-digest/flow.md` | `stress-test` (reflect) | `needs=[]`, `target="digest"` → entry node | **HIGH** | `needs=["digest"]` |
| system-doctor | `oxidemx-conductor/flows/system-doctor/flow.md` | `review` (reflect) | `needs=[]`, `target="diagnose"` → entry node (works only by file-order luck) | **HIGH** | `needs=["diagnose"]` |
| system-doctor (user copy) | `~/.config/oxidemx/flows/system-doctor/flow.md` | `review` | same as above | **HIGH** | `needs=["diagnose"]` |
| rust-agent-self-diagnose (user only) | `~/.config/oxidemx/flows/rust-agent-self-diagnose/flow.md` | input `codebase_path` | stale default `.../juhradial-mx` (pre-rebrand) | LOW | default → the `oxidemx-phase1` path |
| log-analyzer (user only) | `~/.config/oxidemx/flows/log-analyzer/flow.md` | input `log_directory` | `required=true` AND `default="/var/log"` (contradictory) | LOW | drop `required=true` (it has a default) |

`research-digest` **user copy** is already correct (it lists redundant transitive ancestors —
cosmetic, leave it). `doc-digest`, `self-test-hello-world` have no defects.

## Design

### Part A — Fix the two defective repo flows
Edit the `[[step]]` for `stress-test` (research-digest) and `review` (system-doctor) in the
**repo** `oxidemx-conductor/flows/*/flow.md` to add the correct `needs`. These are the canonical
seeds; the stage tests (Part C) assert them, so they can't regress.

### Part B — Harden `validate` (Option B: honest ground truth, no auto-derive)
In `oxidemx-conductor/src/plan.rs`:
- **Reflect:** a `reflect` step's `target` MUST appear in its `needs` (directly or transitively
  via the `needs` graph) → **hard error**: `"reflect target \`<X>\` must be listed in \`needs\`
  (directly or transitively)"`. `validate_reflect_step` must receive the `ancestors` map (same
  threading pattern already used for context-token validation — pass it in).
- **Route:** a `route` step with `needs=[]` → **warning** (collected + printed, non-fatal):
  `"route step \`<id>\` has no \`needs\`; it will run with no upstream context"`. (No current
  flow hits this; it's an unguarded foot-gun the audit surfaced.)
- We do **not** auto-derive `needs` from `target`: the file must be the honest ground truth so a
  human (or the Tauri editor in W4) can trust what they read. Over-specified `needs` is safe;
  under-specified is a silent bug.

### Part C — Expose + lock the concurrency contract
- Add a pure helper `FlowPlan::stages(&self) -> Vec<Vec<String>>` that groups steps into the
  parallel execution stages the scheduler produces (stage 0 = entry nodes; stage N = steps whose
  `needs` all resolved in stages `0..N`). Mirrors the supervisor's readiness rule.
- Add **per-flow stage snapshot tests** (in `plan.rs` `#[cfg(test)]` or `tests/flow_stages.rs`)
  asserting the intended stage structure for the shipped flows:
  - `research-digest` → `[[ingest],[digest,claims],[stress-test],[answer]]`
  - `doc-digest` → `[[parse],[digest,claims],[answer]]`
  - `system-doctor` → `[[diagnose],[review],[report]]`
  These would have caught the bug (stage 0 would've been `[ingest, stress-test]`). Use a stable
  ordering inside a stage (sort by step id) so the snapshot is deterministic.
- Add a `validate` hardening test: a fixture flow with a `reflect` step whose `target` is absent
  from `needs` must fail validation with the new error.

### Part D — Operator visibility (`validate --stages`)
Add a `--stages` flag to the `oxidemx-conductor validate <id>` CLI (`src/main.rs` / the bin) that
prints the stage plan instead of (or in addition to) the linear `order:` string, e.g.:
```
Stage 0 (entry):    ingest
Stage 1 (parallel): digest, claims
Stage 2:            stress-test
Stage 3:            answer
```
This is the "detailed logging" requested — operators see the real concurrency, and the
reflect-gate problem is visible even before the hardening rejects it.

### Part E — Sync the user-disk flows
The shipped flows live in the repo; user-runnable copies live in `~/.config/oxidemx/flows/`.
The implementer must determine the sync mechanism (check `install.sh` — does it copy repo flows
to `~/.config`?) and then:
- If install syncs from repo: the Part A fixes propagate on reinstall; verify `system-doctor`
  user copy ends up correct.
- The **user-only** flows (`rust-agent-self-diagnose`, `log-analyzer`) are not in the repo. Apply
  the LOW fixes to them on disk AND seed corrected copies into `oxidemx-conductor/flows/` so they
  become canonical + test-covered (or, if out of scope, log them — but they're trivial, prefer
  fixing). The `system-doctor` user copy gets the `needs` fix directly if install doesn't re-sync.

## Out of scope (logged as follow-ups, NOT in W1)
- Single-step failure aborts the whole flow (no partial tolerance / skip-on-error).
- Step dataflow is raw-text token substitution (`@step:<id>@`), not typed.
- `reflect` runs inline and blocks the scheduler loop.
- Fixed (non-exponential) retry backoff.
These are real hardening items but not foot-guns; they belong to a later conductor-robustness
slice, not this correctness pass.

## Testing
- **Unit (host-side, `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor`):**
  `stages()` correctness; the three per-flow stage snapshots; the validate reflect-target-in-needs
  rejection; the route-needs-empty warning is emitted.
- **CLI smoke:** `cargo run -p oxidemx-conductor -- validate --stages research-digest` prints the
  4-stage plan; `validate <each shipped flow>` passes after the Part A fixes.
- oxidemx-conductor builds **host-side** (no GTK -devel; Rule 3).

## Files
- Modify: `oxidemx-conductor/flows/research-digest/flow.md`, `oxidemx-conductor/flows/system-doctor/flow.md`
- Modify: `oxidemx-conductor/src/plan.rs` (validate hardening + `stages()` + tests)
- Modify: `oxidemx-conductor/src/main.rs` (or the conductor bin — the `validate` CLI arg)
- Create (maybe): `oxidemx-conductor/tests/flow_stages.rs`
- Modify/seed: user-disk flows + possibly `oxidemx-conductor/flows/{rust-agent-self-diagnose,log-analyzer}/` (Part E)
- Verify: `install.sh` flow-sync behavior

## Global constraints (CLAUDE.md)
No hardcoded hex (n/a here); `cargo clippy` clean; no gold-plating; hand-formatted (no repo-wide
`cargo fmt`); host-side build with the dedicated target dir; commit each logical step; `git add`
only changed files (Jim edits concurrently); isolate implementation in a git worktree.
