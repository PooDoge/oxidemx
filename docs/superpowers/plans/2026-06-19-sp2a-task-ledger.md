# SP2a — TaskLedger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** A standalone, resumable, atomic on-disk task ledger — the persistence foundation for the autonomous coding harness (SP2).

**Architecture:** New `oxidemx-ledger` crate (UI-free, no agentd/conductor deps) holding the task/step model + an atomic-write store + an append-only event log + resume-on-startup. Pure persistence + state-machine logic, fully headless-testable with temp dirs. Conductor/agentd integration and the planner/executor are SEPARATE later sub-projects (SP2b–c) — this crate exposes the types + store they will consume.

**Tech Stack:** Rust, serde/serde_json, thiserror; `tempfile` (dev). No async needed (sync file I/O); no LLM, no D-Bus.

## Global Constraints

- New crate `oxidemx-ledger` (workspace member); deps: `serde`, `serde_json`, `thiserror` only (+ dev `tempfile`). NO agentd/conductor/autoagents/mistralrs deps — it's a leaf persistence crate.
- `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`; no `unwrap`/`expect` outside tests; `thiserror` errors (`#[non_exhaustive]`); `clippy -D warnings`; pristine build; `cargo test -p oxidemx-ledger` green each task.
- **Atomicity:** every manifest write is write-temp-then-rename (a crash mid-write must never corrupt the manifest). **Resumability:** state is rebuilt from `manifest.json` + `events.jsonl` on construction.
- **Ground-truth rule:** a step reaches `Done` ONLY via a recorded `CompletionPromise` (a verifier token), never by a bare status set. Enforce in the API.
- Spec: `docs/superpowers/specs/2026-06-19-sp2-autonomous-coding-harness-design.md` §4.1. Worktree `../oxidemx-phase1`, branch `phase1-local-llm-gateway`. Commit per task.
- On-disk layout (per task): `<base>/tasks/<task_id>/manifest.json`, `events.jsonl`, `artifacts/`. `<base>` is supplied by the caller (agentd will pass `projects/<key>`); this crate is base-agnostic.

## File structure

```
oxidemx-ledger/Cargo.toml
oxidemx-ledger/src/lib.rs        # crate attrs + re-exports
oxidemx-ledger/src/model.rs      # TaskId, Step, StepStatus, TaskManifest, CompletionPromise, StepGraph
oxidemx-ledger/src/event.rs      # LedgerEvent + append-only log
oxidemx-ledger/src/store.rs      # TaskLedger: atomic write/load, step transitions, resume scan
oxidemx-ledger/src/error.rs      # LedgerError (thiserror)
```

---

### Task 1: Crate scaffold + core model

**Files:** Create `oxidemx-ledger/Cargo.toml`, `src/lib.rs`, `src/model.rs`, `src/error.rs`; Modify root `Cargo.toml` (members).

**Interfaces:**
- Produces: `pub struct TaskId(String)` (`TaskId::new(slug: &str)` → `"<slug>-<8hex of a caller-supplied seed>"`; keep it simple — `TaskId::from_raw(String)` + a `TaskId::generate(slug, seed: u64)` using FNV-1a like `agentd::projects` does, NO `Date::now`/random). `pub enum StepStatus { Pending, Running, Blocked, Done, Failed, Skipped }` (serde, `Default = Pending`). `pub struct Step { pub id: String, pub title: String, pub status: StepStatus, pub needs: Vec<String>, pub artifacts: Vec<String>, pub tool_calls: u32, pub verifier_token: Option<String> }`. `pub struct CompletionPromise { pub step_id: String, pub verifier: String, pub token: String, pub ts: u64 }`. `pub struct TaskManifest { pub task_id: TaskId, pub goal: String, pub steps: Vec<Step>, pub created_ts: u64, pub updated_ts: u64 }` with `ready_steps(&self) -> Vec<&Step>` (Pending steps whose `needs` are all `Done`) and `step(&self, id) -> Option<&Step>`. All serde Serialize+Deserialize.
- `error.rs`: `#[non_exhaustive] pub enum LedgerError { Io(String), NotFound(String), BadTransition{from:StepStatus,to:StepStatus}, MissingPromise(String), Corrupt(String) }` (thiserror; `From<std::io::Error>`, `From<serde_json::Error>`).

- [ ] **Step 1: Add `"oxidemx-ledger"` to root `Cargo.toml` members + write `oxidemx-ledger/Cargo.toml`**
```toml
[package]
name = "oxidemx-ledger"
version = "0.0.1"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write the failing test** (in `model.rs`)
```rust
#[test]
fn ready_steps_respects_needs() {
    let mut m = TaskManifest::new(TaskId::from_raw("t-abc".into()), "goal".into());
    m.steps = vec![
        Step::new("a", "first"),
        { let mut s = Step::new("b", "second"); s.needs = vec!["a".into()]; s },
    ];
    // only "a" is ready (b needs a)
    let ready: Vec<_> = m.ready_steps().iter().map(|s| s.id.clone()).collect();
    assert_eq!(ready, vec!["a".to_string()]);
    // mark a Done → b becomes ready
    m.steps[0].status = StepStatus::Done;
    let ready2: Vec<_> = m.ready_steps().iter().map(|s| s.id.clone()).collect();
    assert_eq!(ready2, vec!["b".to_string()]);
}

#[test]
fn manifest_serde_round_trips() {
    let m = TaskManifest::new(TaskId::generate("build", 42), "goal".into());
    let json = serde_json::to_string(&m).unwrap();
    let back: TaskManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.goal, "goal");
    assert!(back.task_id.as_str().contains('-'));
}
```

- [ ] **Step 3: Run → FAIL** (`cargo test -p oxidemx-ledger`).
- [ ] **Step 4: Implement** `error.rs` + `model.rs` (+ `lib.rs` with `#![forbid(unsafe_code)] #![warn(missing_docs)]`, `pub mod` decls + re-exports). `TaskId::generate` uses FNV-1a over `format!("{slug}{seed}")`, format `"{slug}-{:08x}"`. `TaskManifest::new` stamps `created_ts/updated_ts` from a `ts` param OR 0 (caller stamps; do NOT call Date::now — accept `now: u64`? simpler: `new(task_id, goal)` sets ts=0 and a `with_ts` setter; document that the store stamps ts). Keep it minimal.
- [ ] **Step 5: Run → PASS.**
- [ ] **Step 6: Commit** — `feat(ledger): scaffold oxidemx-ledger + task/step model`

---

### Task 2: Atomic store — write/load round-trip

**Files:** Create `oxidemx-ledger/src/store.rs`; Modify `src/lib.rs`.

**Interfaces:**
- Consumes: `TaskManifest`, `TaskId`, `LedgerError` (Task 1).
- Produces: `pub struct TaskLedger { base: PathBuf }` with `pub fn new(base: impl Into<PathBuf>) -> Self`; `pub fn create(&self, manifest: &TaskManifest) -> Result<(), LedgerError>` (writes `<base>/tasks/<task_id>/manifest.json` atomically + creates `artifacts/`); `pub fn load(&self, task_id: &TaskId) -> Result<TaskManifest, LedgerError>`; `pub fn save(&self, manifest: &TaskManifest) -> Result<(), LedgerError>` (atomic overwrite, bumps `updated_ts` via a caller-passed `now`? — keep `save` pure: write as-is). `task_dir(&self, &TaskId) -> PathBuf`. **Atomic write helper:** write to `manifest.json.tmp` then `std::fs::rename` over `manifest.json`.

- [ ] **Step 1: Failing test**
```rust
#[test]
fn create_load_save_round_trips_atomically() {
    let d = tempfile::tempdir().unwrap();
    let led = TaskLedger::new(d.path());
    let mut m = TaskManifest::new(TaskId::from_raw("t-1".into()), "g".into());
    m.steps.push(Step::new("a", "first"));
    led.create(&m).unwrap();
    assert!(d.path().join("tasks/t-1/manifest.json").exists());
    assert!(d.path().join("tasks/t-1/artifacts").is_dir());
    let mut loaded = led.load(&TaskId::from_raw("t-1".into())).unwrap();
    assert_eq!(loaded.steps.len(), 1);
    loaded.steps[0].status = StepStatus::Running;
    led.save(&loaded).unwrap();
    // no .tmp left behind
    assert!(!d.path().join("tasks/t-1/manifest.json.tmp").exists());
    assert_eq!(led.load(&TaskId::from_raw("t-1".into())).unwrap().steps[0].status, StepStatus::Running);
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `store.rs` (atomic write-temp+rename, dir creation, load via serde). Declare `pub mod store;`.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** — `feat(ledger): atomic TaskLedger store (write-temp+rename, load/save)`

---

### Task 3: Event log + guarded step transitions

**Files:** Create `oxidemx-ledger/src/event.rs`; Modify `src/store.rs`, `src/lib.rs`.

**Interfaces:**
- Produces: `event.rs`: `#[serde(tag="kind")] pub enum LedgerEvent { TaskCreated{task_id,goal,ts}, StepStarted{step,ts}, StepDone{step,token,ts}, StepFailed{step,error,ts}, StepBlocked{step,reason,ts}, StepSkipped{step,reason,ts}, ToolCall{step,name,ok,ts}, Note{step:Option<String>,text,ts} }`.
- `store.rs` gains: `pub fn append_event(&self, task_id, &LedgerEvent) -> Result<(),LedgerError>` (append a JSON line to `events.jsonl`); `pub fn read_events(&self, task_id) -> Result<Vec<LedgerEvent>,LedgerError>`; and **guarded transition methods** that update the manifest step status AND append the matching event atomically: `start_step(&self, &mut TaskManifest, step_id, now) `, `complete_step(&self, &mut TaskManifest, step_id, promise: CompletionPromise, now)` (sets `Done` + records `verifier_token` — **errors `MissingPromise` if the promise's token is empty**), `fail_step`, `block_step`, `skip_step`. Illegal transitions (e.g. `Done`→`Running`, or completing a non-`Running` step) return `BadTransition`. Each method appends the event + `save`s the manifest.

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn complete_requires_promise_and_records_token() {
    let d = tempfile::tempdir().unwrap();
    let led = TaskLedger::new(d.path());
    let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "g".into());
    m.steps.push(Step::new("a","first"));
    led.create(&m).unwrap();
    led.start_step(&mut m, "a", 1).unwrap();
    // empty token rejected
    assert!(matches!(led.complete_step(&mut m, "a",
        CompletionPromise{step_id:"a".into(),verifier:"cargo".into(),token:"".into(),ts:2}, 2),
        Err(LedgerError::MissingPromise(_))));
    // valid token → Done + token recorded + event logged
    led.complete_step(&mut m, "a",
        CompletionPromise{step_id:"a".into(),verifier:"cargo".into(),token:"PASS".into(),ts:2}, 2).unwrap();
    assert_eq!(m.step("a").unwrap().status, StepStatus::Done);
    assert_eq!(m.step("a").unwrap().verifier_token.as_deref(), Some("PASS"));
    assert!(led.read_events(&m.task_id).unwrap().iter().any(|e| matches!(e, LedgerEvent::StepDone{..})));
}

#[test]
fn illegal_transition_rejected() {
    let d = tempfile::tempdir().unwrap();
    let led = TaskLedger::new(d.path());
    let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "g".into());
    m.steps.push(Step::new("a","first"));
    led.create(&m).unwrap();
    // completing a Pending (not Running) step is illegal
    assert!(matches!(led.complete_step(&mut m, "a",
        CompletionPromise{step_id:"a".into(),verifier:"v".into(),token:"PASS".into(),ts:1}, 1),
        Err(LedgerError::BadTransition{..})));
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `event.rs` + the store transition methods (append event + save manifest; guard transitions; `complete_step` requires non-empty token). Declare `pub mod event;`.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** — `feat(ledger): event log + ground-truth-guarded step transitions`

---

### Task 4: Resume-on-startup

**Files:** Modify `oxidemx-ledger/src/store.rs`.

**Interfaces:**
- `TaskLedger` gains: `pub fn list_tasks(&self) -> Result<Vec<TaskId>, LedgerError>` (scan `<base>/tasks/*/manifest.json` stems); `pub fn in_flight(&self) -> Result<Vec<TaskManifest>, LedgerError>` (load all; return those with any `Running`/`Pending`/`Blocked` step — i.e. not fully `Done`/`Failed`/`Skipped`); `pub fn resume(&self, task_id) -> Result<TaskManifest, LedgerError>` (load + reconcile: any step left `Running` from a crash is reset to `Pending` and a `Note` event is appended noting the reset).

- [ ] **Step 1: Failing test**
```rust
#[test]
fn resume_resets_crashed_running_steps() {
    let d = tempfile::tempdir().unwrap();
    let led = TaskLedger::new(d.path());
    let mut m = TaskManifest::new(TaskId::from_raw("t".into()), "g".into());
    m.steps.push(Step::new("a","first"));
    led.create(&m).unwrap();
    led.start_step(&mut m, "a", 1).unwrap();           // now Running, then "crash"
    // a fresh ledger over the same dir (simulating restart)
    let led2 = TaskLedger::new(d.path());
    assert_eq!(led2.in_flight().unwrap().len(), 1);     // detected as in-flight
    let resumed = led2.resume(&TaskId::from_raw("t".into())).unwrap();
    assert_eq!(resumed.step("a").unwrap().status, StepStatus::Pending);  // Running reset to Pending
    assert!(led2.read_events(&resumed.task_id).unwrap().iter().any(|e| matches!(e, LedgerEvent::Note{..})));
}

#[test]
fn list_tasks_finds_created_tasks() {
    let d = tempfile::tempdir().unwrap();
    let led = TaskLedger::new(d.path());
    led.create(&TaskManifest::new(TaskId::from_raw("t1".into()), "g".into())).unwrap();
    led.create(&TaskManifest::new(TaskId::from_raw("t2".into()), "g".into())).unwrap();
    let mut ids: Vec<_> = led.list_tasks().unwrap().iter().map(|t| t.as_str().to_string()).collect();
    ids.sort();
    assert_eq!(ids, vec!["t1".to_string(), "t2".to_string()]);
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** `list_tasks`/`in_flight`/`resume` (resume resets `Running`→`Pending`, persists, appends a `Note`). 
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** — `feat(ledger): resume-on-startup (list/in_flight/resume + crash-reset)`

---

## Self-Review
- **Spec coverage** (§4.1): atomic manifest + per-task dir → T2; status model incl. `Done`-requires-`CompletionPromise` → T1+T3; event log unifying step/tool/decision → T3; resume-on-startup + crash-reset → T4; `ready_steps` (needs-satisfied) for the future executor → T1. The conductor/`FlowPlan` sync + agentd hosting + planner are deliberately SP2b–c (out of this plan).
- **Placeholders:** none; all test code + signatures concrete. `ts` is caller-supplied (no `Date::now`, matching the project rule that breaks resume otherwise).
- **Type consistency:** `TaskId`/`Step`/`StepStatus`/`TaskManifest`/`CompletionPromise`/`LedgerEvent`/`TaskLedger`/`LedgerError` consistent across tasks; T2 consumes T1, T3 consumes T1+T2, T4 consumes T3.
