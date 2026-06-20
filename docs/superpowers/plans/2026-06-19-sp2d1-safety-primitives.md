# SP2d-1 — Safety primitives (GatedToolExecutor + RealCommandRunner) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** The preventive tool-gate (`GatedToolExecutor`) + the real `tokio::process` command runner — the two safety primitives the real worker (SP2d-2) needs.

**Architecture:** `GatedToolExecutor` wraps SP1c's `AgentToolExecutor` at the `oxidemx-agent-core::tool::ToolExecutor::execute` chokepoint, running `ApprovalClassifier::classify` BEFORE delegating so a denied/unapproved tool never reaches the inner executor. `RealCommandRunner` implements the `oxidemx-harness::CommandRunner` seam via `tokio::process`, behind a `process` feature. Both headless-testable; no live models, no D-Bus.

**Tech Stack:** Rust, tokio, async-trait; `oxidemx-approval` (classifier), `oxidemx-agent-core` (ToolExecutor), `oxidemx-harness` (CommandRunner).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-19-sp2d1-safety-primitives-design.md`. Option A tool-gate (no OS sandbox this slice).
- **Preventive-gate invariant:** a non-allowed (Deny/Ask) tool's `inner.execute` is NEVER called. Tests must assert the inner executor records ZERO delegations for Deny/Ask.
- `#![forbid(unsafe_code)]` (existing crate attrs); no `unwrap`/`expect` outside tests; **no lock/guard held across `.await`** (the gate awaits the approval prompt + the inner executor); `thiserror`; `clippy -D warnings`; per-crate `cargo test` green each task.
- `oxidemx-harness` DEFAULT build stays pure-logic: the `RealCommandRunner` (and `tokio::process`) live behind a `process` cargo feature; `cargo test -p oxidemx-harness` (default) must still pass without it.
- Worktree `../oxidemx-phase1`, branch `phase1-local-llm-gateway`. Commit per task.

## File structure

```
agentd/src/tools/gated.rs   # T1: GatedToolExecutor + GateMode + ApprovalPrompt trait (+ test mocks)
agentd/Cargo.toml           # T1: add oxidemx-approval dep
oxidemx-harness/src/run.rs  # T2: RealCommandRunner (cfg feature "process")
oxidemx-harness/Cargo.toml  # T2: add "process" feature
```

---

### Task 1: `GatedToolExecutor` (the preventive tool-gate)

**Files:** Create `agentd/src/tools/gated.rs`; Modify `agentd/src/tools/mod.rs` (declare `pub mod gated;`), `agentd/Cargo.toml` (add `oxidemx-approval = { path = "../oxidemx-approval" }`).

**Interfaces:**
- Consumes: `oxidemx_agent_core::tool::ToolExecutor` (`async fn execute(&self, name: &str, args: serde_json::Value, sink: &Option<StreamSink>) -> Result<String, String>`), `oxidemx_agent_core::events::StreamSink`; `oxidemx_approval::{ApprovalClassifier, Tier}` (`classify(&self, tool: &str, args: &Value, cwd: &Path) -> Decision { tier, reason }`).
- Produces:
  - `pub enum GateMode { Attended, Autonomous }`.
  - `#[async_trait] pub trait ApprovalPrompt: Send + Sync { async fn confirm(&self, tool: &str, reason: &str) -> bool; }` (the real impl wrapping the SP1b `Approver` is a thin adapter wired in SP2d-2; this task ships the trait + a test mock).
  - `pub struct GatedToolExecutor { inner: Arc<dyn ToolExecutor>, classifier: ApprovalClassifier, prompt: Option<Arc<dyn ApprovalPrompt>>, mode: GateMode, cwd: PathBuf }` + `GatedToolExecutor::new(inner, classifier, prompt, mode, cwd)`, implementing `ToolExecutor`.
- Behavior of `execute(name, args, sink)`:
  1. `let d = self.classifier.classify(name, &args, &self.cwd);`
  2. `Tier::AutoAllow | Tier::AutoAllowIfReversible` → `self.inner.execute(name, args, sink).await`.
  3. `Tier::AutoDeny` → `Err(format!("tool denied: {}", d.reason))`.
  4. `Tier::Ask` →
     - `Attended` + `Some(prompt)` → `if self.prompt.as_ref().unwrap()… confirm(name, &d.reason).await { inner.execute(...).await } else { Err("denied by user: …") }` (do NOT hold any lock across the `confirm`/`execute` awaits — the struct holds no lock anyway).
     - else (`Autonomous`, or no prompt) → `Err(format!("NEEDS_APPROVAL: {name}: {}", d.reason))`.

- [ ] **Step 1: Failing tests** (`#[cfg(test)]` mocks: `RecordingExecutor` impl `ToolExecutor` recording each `name`; `OkPrompt`/`DenyPrompt` impl `ApprovalPrompt`)
```rust
#[tokio::test]
async fn auto_allow_delegates() {
    let rec = Arc::new(RecordingExecutor::default());
    let g = GatedToolExecutor::new(rec.clone(), ApprovalClassifier::default(), None,
        GateMode::Autonomous, tempfile::tempdir().unwrap().path().into());
    let r = g.execute("read_file", serde_json::json!({"file_path":"x"}), &None).await;
    assert!(r.is_ok());
    assert_eq!(rec.calls(), vec!["read_file".to_string()]);   // delegated
}
#[tokio::test]
async fn auto_deny_never_delegates() {
    let rec = Arc::new(RecordingExecutor::default());
    let g = GatedToolExecutor::new(rec.clone(), ApprovalClassifier::default(), None,
        GateMode::Autonomous, tempfile::tempdir().unwrap().path().into());
    let r = g.execute("execute_command", serde_json::json!({"command":"git push --force"}), &None).await;
    assert!(r.unwrap_err().contains("denied"));
    assert!(rec.calls().is_empty());                          // NEVER ran
}
#[tokio::test]
async fn ask_autonomous_returns_needs_approval_marker_no_delegate() {
    let rec = Arc::new(RecordingExecutor::default());
    let g = GatedToolExecutor::new(rec.clone(), ApprovalClassifier::default(), None,
        GateMode::Autonomous, tempfile::tempdir().unwrap().path().into());
    // a commit is Ask tier
    let r = g.execute("execute_command", serde_json::json!({"command":"git commit -m x"}), &None).await;
    assert!(r.unwrap_err().contains("NEEDS_APPROVAL"));
    assert!(rec.calls().is_empty());
}
#[tokio::test]
async fn ask_attended_approves_then_delegates() {
    let rec = Arc::new(RecordingExecutor::default());
    let g = GatedToolExecutor::new(rec.clone(), ApprovalClassifier::default(),
        Some(Arc::new(OkPrompt)), GateMode::Attended, tempfile::tempdir().unwrap().path().into());
    let r = g.execute("execute_command", serde_json::json!({"command":"git commit -m x"}), &None).await;
    assert!(r.is_ok());
    assert_eq!(rec.calls(), vec!["execute_command".to_string()]);
}
#[tokio::test]
async fn ask_attended_denies_no_delegate() {
    let rec = Arc::new(RecordingExecutor::default());
    let g = GatedToolExecutor::new(rec.clone(), ApprovalClassifier::default(),
        Some(Arc::new(DenyPrompt)), GateMode::Attended, tempfile::tempdir().unwrap().path().into());
    let r = g.execute("execute_command", serde_json::json!({"command":"git commit -m x"}), &None).await;
    assert!(r.is_err());
    assert!(rec.calls().is_empty());
}
```
- [ ] **Step 2: Run → FAIL** (`cargo test -p agentd gated`).
- [ ] **Step 3: Implement** `gated.rs` per the Interfaces block. `RecordingExecutor` uses a poison-safe `Mutex<Vec<String>>`; the `ToolExecutor::execute` impl pushes the name then returns `Ok("ok")`. No lock held across an `.await` (the gate holds none; `RecordingExecutor` locks only to push, no await inside). Declare the module + add the dep.
- [ ] **Step 4: Run → PASS** (`cargo test -p agentd`); confirm the existing agentd tests still pass + `cargo tree -p agentd | grep -i mistralrs` empty (default).
- [ ] **Step 5: Commit** — `feat(agentd): GatedToolExecutor — preventive tool-gate (classify before delegate)`

---

### Task 2: `RealCommandRunner` (tokio::process)

**Files:** Create `oxidemx-harness/src/run.rs`; Modify `oxidemx-harness/src/lib.rs` (`#[cfg(feature="process")] pub mod run;`), `oxidemx-harness/Cargo.toml` (add `[features] process = []` and ensure tokio has the `process` feature under that — `tokio = { …, features = [..., "process"] }` is fine to always enable since tokio is already a dep; gate only the module + impl).

**Interfaces:**
- Consumes: `oxidemx_harness::verify::{CommandRunner, CommandResult}` (`#[async_trait] async fn run(&self, program: &str, args: &[String], cwd: &Path) -> CommandResult`; `CommandResult { ok: bool, stdout: String, stderr: String }`).
- Produces: `#[cfg(feature="process")] pub struct RealCommandRunner;` implementing `CommandRunner`: `tokio::process::Command::new(program).args(args).current_dir(cwd).output().await` → on `Ok(out)` `CommandResult { ok: out.status.success(), stdout: String::from_utf8_lossy(&out.stdout).into_owned(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }`; on `Err(e)` (spawn failure) `CommandResult { ok: false, stdout: String::new(), stderr: format!("spawn failed: {e}") }` (never panics).

- [ ] **Step 1: Failing tests** (`#[cfg(all(test, feature="process"))]`)
```rust
#[tokio::test]
async fn true_succeeds_false_fails() {
    let r = RealCommandRunner;
    let cwd = std::env::temp_dir();
    assert!(r.run("true", &[], &cwd).await.ok);
    assert!(!r.run("false", &[], &cwd).await.ok);
}
#[tokio::test]
async fn echo_captures_stdout() {
    let r = RealCommandRunner;
    let out = r.run("echo", &["hello".into()], &std::env::temp_dir()).await;
    assert!(out.ok && out.stdout.contains("hello"));
}
#[tokio::test]
async fn missing_program_is_failure_not_panic() {
    let r = RealCommandRunner;
    let out = r.run("definitely-not-a-real-program-xyz", &[], &std::env::temp_dir()).await;
    assert!(!out.ok && out.stderr.contains("spawn failed"));
}
```
- [ ] **Step 2: Run → FAIL** (`cargo test -p oxidemx-harness --features process run`).
- [ ] **Step 3: Implement** `run.rs` + the feature gate. Ensure `cargo test -p oxidemx-harness` (DEFAULT, no `process`) still compiles + passes (the module is cfg'd out).
- [ ] **Step 4: Run → PASS** both: `cargo test -p oxidemx-harness --features process` (incl. the 3 new) AND `cargo test -p oxidemx-harness` (default, the existing 14). `cargo tree -p oxidemx-harness | grep -iE "reqwest|rustls"` still none.
- [ ] **Step 5: Commit** — `feat(harness): RealCommandRunner (tokio::process) behind 'process' feature`

---

## Self-Review
- **Spec coverage:** §3 GatedToolExecutor (classify-before-delegate, 4 tiers, two modes, the no-delegate invariant) → T1; §4 RealCommandRunner (tokio::process, spawn-error→failure-not-panic, `process` feature) → T2; §6 testing (headless mocks for the gate; trivial commands for the runner) → both. The Attended-mode approver is via the `ApprovalPrompt` trait (plan refinement of the spec's `Option<Arc<Approver>>` — decouples + makes it testable; the real `Approver` adapter is SP2d-2 wiring).
- **Placeholders:** none; all test + impl code concrete. The one spec→plan refinement (`ApprovalPrompt` trait) is stated explicitly.
- **Type consistency:** `GatedToolExecutor`/`GateMode`/`ApprovalPrompt` (T1) and `RealCommandRunner` (T2) match the spec; `ToolExecutor::execute`/`CommandRunner::run`/`CommandResult` signatures match the real traits in `oxidemx-agent-core`/`oxidemx-harness`. The preventive-gate invariant is asserted by `rec.calls().is_empty()` in the Deny/Ask tests.
