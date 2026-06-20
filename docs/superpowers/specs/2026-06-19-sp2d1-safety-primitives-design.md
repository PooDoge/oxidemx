# SP2d-1 — Safety primitives (GatedToolExecutor + RealCommandRunner) — design

Date: 2026-06-19
Status: design (brainstormed + approved in-chat; pending spec review → writing-plans)
First slice of SP2d (real-worker + autonomous run) of the autonomous coding harness.
Implements the **preventive tool-gate** contract pinned in
`docs/superpowers/specs/2026-06-19-sp2-autonomous-coding-harness-design.md` §4.3 (⚠️),
chosen Option A (`GatedToolExecutor`, no OS sandbox yet — `A now, sandbox later`).
Read `docs/AI-ARCHITECTURE-STATUS.md` first.

## 1. Goal & scope

Two small, headless-testable, design-stable primitives the real worker (SP2d-2) needs:
1. **`GatedToolExecutor`** — the preventive tool-gate: classifies every tool call
   BEFORE it executes, so a denied/unapproved tool never runs.
2. **`RealCommandRunner`** — the `tokio::process` impl of the harness `CommandRunner`
   seam, so the `Verifier` can actually run `cargo`.

**Out of scope (later SP2d slices):** the real `Worker` (route_turn-based) — SP2d-2;
the real `PlannerModel` — SP2d-2; agentd autonomous-run D-Bus surface + SDD-flow
template — SP2d-3; an OS sandbox (bwrap/seccomp) — a later defense-in-depth slice.

## 2. The chokepoint (why Option A is thin)

Every model tool-call in `oxidemx-agent-core::runtime` flows through ONE seam:
`CoreTool` (AutoAgents `ToolRuntime`) → `ToolExecutor::execute(name, args, sink)`
(`runtime.rs:80`). There is no bypass. So the gate is a wrapper at that seam — not a
redesign. (The SP2c "post-hoc" concern was an artifact of the *mock* worker reporting
tool calls after the fact; the real worker gates inline here.)

## 3. `GatedToolExecutor`

- Location: `agentd/src/tools/gated.rs` (next to SP1c's `AgentToolExecutor`, where the
  `oxidemx-approval` dep + the real tools already live).
- Implements `oxidemx_agent_core::tool::ToolExecutor`. Fields:
  `inner: Arc<dyn ToolExecutor>` (the real `AgentToolExecutor`), `classifier: ApprovalClassifier`,
  `approver: Option<Arc<Approver>>` (SP1b), `mode: GateMode {Attended, Autonomous}`, `cwd: PathBuf`.
- `async fn execute(&self, name, args, sink)`:
  1. `decision = classifier.classify(name, &args, &self.cwd)`.
  2. **AutoAllow | AutoAllowIfReversible** → `inner.execute(name, args, sink).await` (tool runs).
  3. **AutoDeny** → `Err(format!("tool denied: {}", decision.reason))` — agent sees it
     inline, the tool never reaches `inner`.
  4. **Ask** →
     - `Attended` + `approver` present → `approver.request(...)` (await the human
       decision); approved → delegate to `inner`; denied → `Err("denied by user: …")`.
     - `Autonomous` (or no approver) → `Err("NEEDS_APPROVAL: {tool}: {reason}")` — a
       recognizable marker the harness/worker maps to `Blocked{needs-approval}` (the
       step parks; the run continues — non-blocking, per §5.1).
- **Invariant (the whole point):** a non-allowed tool's `inner.execute` is NEVER
  called. The unit tests assert the inner executor records ZERO calls for Deny/Ask.

## 4. `RealCommandRunner`

- Location: `oxidemx-harness/src/run.rs` behind a `process` cargo feature (so the
  crate's pure executor logic stays runnable without spawning processes; the default
  build keeps `CommandRunner` as the trait only — the real impl is opt-in).
- Implements `oxidemx_harness::CommandRunner`: `async fn run(&self, program, args, cwd)`
  → `tokio::process::Command::new(program).args(args).current_dir(cwd).output().await`
  → `CommandResult { ok: status.success(), stdout: <utf8-lossy>, stderr: <utf8-lossy> }`.
  On spawn error (program not found) → `CommandResult{ ok:false, stderr: <error>, stdout:"" }`
  (never panics; a missing `cargo` is a verify failure, not a crash).

## 5. Data flow

```
agent (route_turn, SP2d-2) ──tool call──▶ GatedToolExecutor::execute
                                            ├ classify(name,args,cwd)
                                            ├ Allow      → AgentToolExecutor (runs)
                                            ├ Deny       → Err (never runs)
                                            └ Ask        → approver (attended) | Err NEEDS_APPROVAL (autonomous)
Verifier (SP2c) ──run(cargo,…)──▶ RealCommandRunner ──tokio::process──▶ CommandResult
```

## 6. Testing (headless)

- **GatedToolExecutor:** a `#[cfg(test)] RecordingExecutor` (impl `ToolExecutor`,
  records names it was asked to run). Assert: AutoAllow tool → recorded (delegated);
  AutoDeny tool (`git push --force`) → `Err`, recorder empty; Ask tool (autonomous, no
  approver) → `Err` containing `NEEDS_APPROVAL`, recorder empty; Ask tool (attended +
  a mock `Approver` that approves) → delegated; (denies) → `Err`. The classifier is
  the real `ApprovalClassifier` (already hardened in SP2c). No live models.
- **RealCommandRunner** (behind `--features process`): run `true` → `ok=true`; `false`
  → `ok=false`; `echo hi` → stdout contains `hi`; a nonexistent program → `ok=false`,
  no panic. Uses real `tokio::process` but trivial commands (no cargo).

## 7. Risks

- **Approver wiring in Attended mode** — `Approver::request` blocks on a oneshot; the
  GatedToolExecutor must not hold any lock across that await (project rule). The
  Approver already handles this (SP1b); the gate just awaits it.
- **`process` feature hygiene** — the real runner pulls only `tokio` process feature
  (already a tokio user); confirm no new heavy deps. Default build (no `process`) keeps
  the harness pure-logic.
- **classify cwd** — the gate is constructed per-project with the project `cwd`; SP2d-2
  passes the turn's `ProjectPaths.cwd`.

## 8. Out of scope / sequencing

SP2d-2 (real Worker over `route_turn` using `GatedToolExecutor`; real `PlannerModel`),
SP2d-3 (autonomous-run D-Bus + SDD-flow), then the OS sandbox (defense-in-depth), then
SP2e (AutoAgents adoptions). The SP2c followups (real per-call `ok` into
`record_tool_call`; code-step verify gate) are addressed when the real worker lands
(SP2d-2/3).
