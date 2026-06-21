# Conductor Correctness (W1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the reflect-`needs` foot-gun in the shipped flows and make it un-shippable: add `FlowPlan::stages()` + per-flow stage tests, harden `validate`, and add `validate --stages`.

**Architecture:** Pure additions to `oxidemx-conductor` (the DAG validator + CLI). The scheduler is correct and is NOT touched. `stages()` derives the parallel execution stages from the existing `topo` + `needs`; the stage tests lock in each shipped flow's concurrency contract; `validate` gains a hard error for reflect steps whose `target` isn't in `needs`.

**Tech Stack:** Rust, `oxidemx-conductor` crate (pure — no GTK), serde/TOML.

## Global Constraints

- **Builds HOST-SIDE** (no GTK `-devel`, no distrobox): `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor` / `cargo clippy -p oxidemx-conductor -- -D warnings`. `$REPO` = the worktree root (absolute path, expanded).
- `cargo clippy` clean (warnings = defects). Hand-formatted — do NOT run repo-wide `cargo fmt`; match surrounding style.
- No gold-plating — exactly the spec's W1 scope. Out of scope (do NOT add): failure-aborts-flow changes, typed dataflow, reflect-blocks-scheduler. Those are the later Schema-v2 program.
- Commit after every task. `git add` only the files you changed (never `git add -A` — the user edits concurrently).
- **The agent roster** (real flows reference these): `web-researcher`, `summarizer`, `extractor`, `writer`, `skeptic` (+ any in `oxidemx-conductor/src/roster.rs` / the agents dir).
- Spec: `docs/superpowers/specs/2026-06-21-conductor-correctness-design.md` is authoritative.

---

## File Structure

| File | Change |
|---|---|
| `oxidemx-conductor/src/plan.rs` | Add `FlowPlan::stages()` + unit tests; thread `ancestors` into `validate_reflect_step` + the target-in-needs hard error. |
| `oxidemx-conductor/flows/research-digest/flow.md` | `stress-test` → add `needs = ["digest"]`. |
| `oxidemx-conductor/flows/system-doctor/flow.md` | `review` → add `needs = ["diagnose"]`. |
| `oxidemx-conductor/tests/flow_stages.rs` (create) | Load each shipped flow file, validate, assert `stages()`. |
| `oxidemx-conductor/src/bin/conductor.rs` | `--stages` flag on `validate`; route-needs-empty warning print. |
| `oxidemx-conductor/flows/{rust-agent-self-diagnose,log-analyzer}/flow.md` (create) | Seed the user-only flows into the repo (corrected). |

---

## Task 1: `FlowPlan::stages()` + unit tests

**Files:**
- Modify: `oxidemx-conductor/src/plan.rs` (add method to the `impl FlowPlan` block ~line 60; add tests to the `#[cfg(test)] mod tests` ~line 380)

**Interfaces:**
- Produces: `pub fn stages(&self) -> Vec<Vec<String>>` on `FlowPlan` — groups steps into the parallel execution stages the scheduler produces (stage 0 = entry nodes; stage N = steps whose `needs` all resolved in stages `0..N`). Order within a stage follows `topo` order (deterministic). Consumed by Task 2 (tests), Task 4 (CLI).

- [ ] **Step 1: Write the failing tests** — add to `#[cfg(test)] mod tests` in `plan.rs`. These use the existing inline-TOML `doc(...)` + `roster()` helpers:

```rust
    const DIAMOND: &str = r#"---
[flow]
id = "diamond"
description = "x"
[[step]]
id = "a"
task = "t"
agent = "web-researcher"
[[step]]
id = "b"
needs = ["a"]
task = "t"
agent = "web-researcher"
[[step]]
id = "c"
needs = ["a"]
task = "t"
agent = "web-researcher"
[[step]]
id = "d"
needs = ["b", "c"]
task = "t"
agent = "web-researcher"
---
body"#;

    #[test]
    fn stages_groups_parallel_steps() {
        let plan = validate(&doc(DIAMOND), &roster(), &["execute_command"]).expect("valid");
        assert_eq!(
            plan.stages(),
            vec![
                vec!["a".to_string()],
                vec!["b".to_string(), "c".to_string()],
                vec!["d".to_string()],
            ]
        );
    }

    #[test]
    fn stages_of_linear_chain() {
        // GOOD is the existing linear fixture (ingest → digest → answer).
        let plan = validate(&doc(GOOD), &roster(), &["execute_command"]).expect("valid");
        assert_eq!(
            plan.stages(),
            vec![
                vec!["ingest".to_string()],
                vec!["digest".to_string()],
                vec!["answer".to_string()],
            ]
        );
    }
```

(If `roster()` lacks `web-researcher`, extend it to include the full roster listed in Global Constraints — those agents are referenced by the real flows in Task 2 too. Check the `roster()` helper at ~line 385.)

- [ ] **Step 2: Run — verify fail** (`stages` method doesn't exist).

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor stages_`
Expected: FAIL — no method `stages` on `FlowPlan`.

- [ ] **Step 3: Implement `stages()`** — add to the `impl FlowPlan` block (near `step()`/`ancestors_of()`, ~line 60). It walks `topo` (already file-order-deterministic), promoting steps whose `needs` are all already placed:

```rust
    /// Steps grouped into the parallel execution stages the scheduler
    /// produces: stage 0 = entry nodes (no `needs`); stage N = steps whose
    /// `needs` all resolved in stages `0..N`. Order within a stage follows
    /// `topo` (deterministic). This mirrors the supervisor's readiness rule
    /// and is the concurrency contract the flow_stages tests assert.
    pub fn stages(&self) -> Vec<Vec<String>> {
        let mut placed: BTreeSet<String> = BTreeSet::new();
        let mut remaining: Vec<&str> = self.topo.iter().map(String::as_str).collect();
        let mut out: Vec<Vec<String>> = Vec::new();
        while !remaining.is_empty() {
            let ready: Vec<String> = remaining
                .iter()
                .filter(|id| {
                    self.step(id)
                        .map(|s| s.needs.iter().all(|n| placed.contains(n)))
                        .unwrap_or(false)
                })
                .map(|s| s.to_string())
                .collect();
            if ready.is_empty() {
                break; // defensive: a cycle can't reach here (validate rejects cycles)
            }
            for id in &ready {
                placed.insert(id.clone());
            }
            remaining.retain(|id| !placed.contains(*id));
            out.push(ready);
        }
        out
    }
```

- [ ] **Step 4: Run — verify pass.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor stages_`
Expected: PASS.

- [ ] **Step 5: Clippy + commit.**

```bash
cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p oxidemx-conductor -- -D warnings   # clean
git add oxidemx-conductor/src/plan.rs
git commit -m "feat(conductor): FlowPlan::stages() — parallel execution stage grouping"
```

---

## Task 2: Fix the 2 defective flows + shipped-flow stage regression test

**Files:**
- Create: `oxidemx-conductor/tests/flow_stages.rs`
- Modify: `oxidemx-conductor/flows/research-digest/flow.md` (stress-test step)
- Modify: `oxidemx-conductor/flows/system-doctor/flow.md` (review step)

**Interfaces:**
- Consumes: `FlowPlan::stages()` (Task 1), `validate`, `FlowDoc::parse`, `Roster`.

- [ ] **Step 1: Write the failing integration test** — `oxidemx-conductor/tests/flow_stages.rs`. It reads each shipped flow FILE (regression guard on the real files), validates against an in-memory roster, asserts the intended stages. Adapt the roster construction to the real `Roster` API (grep `oxidemx-conductor/src/roster.rs` for a constructor / `Roster::from_iter` / a test builder; mirror the `roster()` helper in plan.rs):

```rust
//! Regression guard: each shipped flow's parallel-stage structure is locked.
//! Reads the real flow.md files under oxidemx-conductor/flows/.
use oxidemx_conductor::{flowdoc::FlowDoc, plan::validate, roster::Roster};
use std::path::PathBuf;

fn flows_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("flows")
}

/// In-memory roster covering every agent the shipped flows reference.
fn full_roster() -> Roster {
    // Build a roster with: web-researcher, summarizer, extractor, writer, skeptic.
    // Use whatever the real Roster API offers (see src/roster.rs); the plan.rs
    // `roster()` test helper is the pattern to copy.
    todo_build_roster()
}

fn stages_of(flow_id: &str) -> Vec<Vec<String>> {
    let src = std::fs::read_to_string(flows_dir().join(flow_id).join("flow.md")).unwrap();
    let doc = FlowDoc::parse(&src).expect("flow.md parses");
    let plan = validate(&doc, &full_roster(), &["execute_command"])
        .unwrap_or_else(|e| panic!("{flow_id} invalid: {e:?}"));
    plan.stages()
}

#[test]
fn research_digest_stages() {
    assert_eq!(stages_of("research-digest"), vec![
        vec!["ingest".to_string()],
        vec!["digest".to_string(), "claims".to_string()],
        vec!["stress-test".to_string()],
        vec!["answer".to_string()],
    ]);
}

#[test]
fn doc_digest_stages() {
    assert_eq!(stages_of("doc-digest"), vec![
        vec!["parse".to_string()],
        vec!["digest".to_string(), "claims".to_string()],
        vec!["answer".to_string()],
    ]);
}

#[test]
fn system_doctor_stages() {
    assert_eq!(stages_of("system-doctor"), vec![
        vec!["diagnose".to_string()],
        vec!["review".to_string()],
        vec!["report".to_string()],
    ]);
}
```

> Replace `todo_build_roster()` with the real construction. If `flowdoc`/`plan`/`roster` aren't re-exported at the crate root, use the real module paths (check `src/lib.rs` exports — `validate` is exported per the explorer; confirm `FlowDoc`, `Roster`).

- [ ] **Step 2: Run — verify research-digest + system-doctor FAIL** (they're broken: stress-test/review land in stage 0).

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor --test flow_stages`
Expected: `research_digest_stages` FAILS (got `[["ingest","stress-test"], ...]`); `system_doctor_stages` FAILS (got `[["diagnose","review"], ...]`); `doc_digest_stages` PASSES.

- [ ] **Step 3: Fix research-digest** — in `oxidemx-conductor/flows/research-digest/flow.md`, the `stress-test` step (the `[[step]]` with `id = "stress-test"`). Add the `needs` line:

```toml
[[step]]
id = "stress-test"
kind = "reflect"
needs = ["digest"]
target = "digest"
critic = "skeptic"
max_rounds = 2
accept_when = "no_blocking_findings"
output = "debug/digest-reviewed.md"
```

- [ ] **Step 4: Fix system-doctor** — in `oxidemx-conductor/flows/system-doctor/flow.md`, the `review` step:

```toml
[[step]]
id = "review"
kind = "reflect"
needs = ["diagnose"]
target = "diagnose"
critic = "skeptic"
max_rounds = 2
accept_when = "no_blocking_findings"
output = "debug/diagnostics-reviewed.md"
```

- [ ] **Step 5: Run — verify all pass.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor --test flow_stages`
Expected: PASS (all 3).

- [ ] **Step 6: Commit.**

```bash
git add oxidemx-conductor/tests/flow_stages.rs oxidemx-conductor/flows/research-digest/flow.md oxidemx-conductor/flows/system-doctor/flow.md
git commit -m "fix(conductor): reflect steps declare needs; lock flow stages via regression test"
```

---

## Task 3: Harden `validate` — reflect `target` must be in `needs`

**Files:**
- Modify: `oxidemx-conductor/src/plan.rs` (`validate` call site ~line 154; `validate_reflect_step` ~line 260; tests ~line 380)

**Interfaces:**
- Consumes: the `ancestors: BTreeMap<String, BTreeSet<String>>` already built in `validate` at ~line 149.
- Produces: `validate` rejects a `reflect` step whose `target` is not in `needs` (directly or transitively).

- [ ] **Step 1: Write the failing test** — a reflect step with `target` set but `needs` empty must fail validation:

```rust
    const REFLECT_NO_NEEDS: &str = r#"---
[flow]
id = "rn"
description = "x"
[[step]]
id = "make"
task = "t"
agent = "web-researcher"
[[step]]
id = "check"
kind = "reflect"
target = "make"
critic = "skeptic"
---
body"#;

    #[test]
    fn reflect_target_must_be_in_needs() {
        let errs = validate(&doc(REFLECT_NO_NEEDS), &roster(), &["execute_command"]).unwrap_err();
        assert!(
            errs.iter().any(|e| e.step.as_deref() == Some("check")
                && e.message.contains("needs")),
            "expected a reflect-target-in-needs error, got {errs:?}"
        );
    }
```

- [ ] **Step 2: Run — verify fail** (no such error today — the flow currently validates).

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor reflect_target_must_be_in_needs`
Expected: FAIL — `unwrap_err` panics because validation currently SUCCEEDS.

- [ ] **Step 3: Thread `ancestors` into `validate_reflect_step` + add the check.** At the kind-match in `validate` (~line 154), change the reflect arm to pass `&ancestors`:

```rust
            "reflect" => validate_reflect_step(s, &ids, roster, &ancestors, &mut errors),
```

Update `validate_reflect_step` (~line 260) — add the param + the check after the existing `target` resolution:

```rust
fn validate_reflect_step(
    s: &Step,
    ids: &BTreeSet<&str>,
    roster: &Roster,
    ancestors: &BTreeMap<String, BTreeSet<String>>,
    errors: &mut Vec<ValidationError>,
) {
    match &s.target {
        None => errors.push(ValidationError::step(&s.id, "reflect step has no `target`")),
        Some(t) if !ids.contains(t.as_str()) => {
            errors.push(ValidationError::step(
                &s.id,
                format!("reflect target `{t}` is not a step"),
            ));
        }
        Some(t) => {
            // The target must be a real dependency, not just a runtime gate —
            // otherwise Kahn's sort treats this reflect step as an entry node
            // and it goes "ready" too early (the foot-gun).
            let empty = BTreeSet::new();
            let anc = ancestors.get(&s.id).unwrap_or(&empty);
            if !s.needs.iter().any(|n| n == t) && !anc.contains(t.as_str()) {
                errors.push(ValidationError::step(
                    &s.id,
                    format!("reflect target `{t}` must be listed in `needs` (directly or transitively)"),
                ));
            }
        }
    }
    // (critic check unchanged below)
    match &s.critic {
        None => errors.push(ValidationError::step(&s.id, "reflect step has no `critic`")),
        Some(c) if roster.get(c).is_none() && !(c.starts_with("./") || c.ends_with(".md")) => {
            errors.push(ValidationError::step(&s.id, format!("critic `{c}` not in roster")));
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Run — verify pass** (the new test + the whole suite, confirming the fixed flows from Task 2 still validate).

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor`
Expected: PASS (incl. `reflect_target_must_be_in_needs` + `flow_stages` — the Task-2-fixed flows now satisfy the new rule).

- [ ] **Step 5: Clippy + commit.**

```bash
cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p oxidemx-conductor -- -D warnings
git add oxidemx-conductor/src/plan.rs
git commit -m "feat(conductor): validate rejects reflect step whose target isn't in needs"
```

---

## Task 4: `validate --stages` CLI flag (+ route-needs-empty warning)

**Files:**
- Modify: `oxidemx-conductor/src/bin/conductor.rs` (`Opts` ~line 482, `parse_opts` ~line 496, `cmd_validate` ~line 95)

**Interfaces:**
- Consumes: `FlowPlan::stages()` (Task 1).

- [ ] **Step 1: Add the `--stages` opt.** In `Opts` (~line 482) add `stages: bool,`. In `parse_opts` (~line 496) add an arm: `"--stages" => o.stages = true,`.

- [ ] **Step 2: Print stages + route warning in `cmd_validate`.** In the `Ok(plan)` arm (~line 113), after the existing `✓ ... order:` line, add:

```rust
            if opts.stages {
                println!("stages:");
                for (i, stage) in plan.stages().iter().enumerate() {
                    let label = if i == 0 { " (entry)" } else if stage.len() > 1 { " (parallel)" } else { "" };
                    println!("  stage {i}{label}: {}", stage.join(", "));
                }
            }
            // Non-fatal foot-gun warning: a route step with no needs runs with
            // no upstream context.
            for s in &doc.manifest.steps {
                if s.kind == "route" && s.needs.is_empty() {
                    eprintln!("  warning: route step `{}` has no `needs`; it will run with no upstream context", s.id);
                }
            }
```

(`opts` is already in scope in `cmd_validate`; `doc` too.)

- [ ] **Step 3: Build + manual smoke** (no unit test for the CLI print — verify by running it).

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo run -p oxidemx-conductor --bin oxidemx-conductor -- validate --stages research-digest --flows-dir oxidemx-conductor/flows --agents-dir oxidemx-conductor/agents`
Expected: prints `✓ ... order: ...` then a `stages:` block with `stage 0 (entry): ingest` / `stage 1 (parallel): digest, claims` / `stage 2: stress-test` / `stage 3: answer`. (If `--agents-dir` differs, use the real default; the goal is it loads + prints stages.)

- [ ] **Step 4: Clippy + commit.**

```bash
cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p oxidemx-conductor -- -D warnings
git add oxidemx-conductor/src/bin/conductor.rs
git commit -m "feat(conductor): validate --stages prints the parallel stage plan + route-needs warning"
```

---

## Task 5: Seed user-only flows into the repo + LOW cleanups + sync

**Files:**
- Create: `oxidemx-conductor/flows/rust-agent-self-diagnose/flow.md`, `oxidemx-conductor/flows/log-analyzer/flow.md` (corrected copies of the user-disk flows)
- Verify: `install.sh` flow-sync behavior

**Interfaces:** none (data + docs).

- [ ] **Step 1: Determine the flow-sync mechanism.** Grep `install.sh` for how flows reach `~/.config/oxidemx/flows/` (does it `cp -r oxidemx-conductor/flows/* ~/.config/oxidemx/flows/`?). Record the finding in the commit message. This decides whether seeding repo copies propagates on reinstall.

- [ ] **Step 2: Seed `rust-agent-self-diagnose` into the repo, corrected.** Copy `~/.config/oxidemx/flows/rust-agent-self-diagnose/flow.md` to `oxidemx-conductor/flows/rust-agent-self-diagnose/flow.md`, and fix the stale default: change the `codebase_path` input default from the `.../juhradial-mx` path to the current repo path (use a generic/home-relative default like `"."` or `"$HOME/src/oxidemx"` — pick the least-surprising; do NOT hardcode a machine-specific absolute path).

- [ ] **Step 3: Seed `log-analyzer` into the repo, corrected.** Copy `~/.config/oxidemx/flows/log-analyzer/flow.md` to `oxidemx-conductor/flows/log-analyzer/flow.md`, and fix the contradictory `log_directory` input: it has both `required = true` and `default = "/var/log"`. Drop `required = true` (a default implies optional).

- [ ] **Step 4: Validate the two seeded flows.**

Run: `cd $REPO && CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo run -p oxidemx-conductor --bin oxidemx-conductor -- validate rust-agent-self-diagnose --flows-dir oxidemx-conductor/flows --agents-dir oxidemx-conductor/agents` (and `log-analyzer`)
Expected: both print `✓ ... valid`. (Fix any validation errors surfaced — e.g. if they reference agents not in the repo roster, note it; these are user flows so they may need agent seeding too — if so, report DONE_WITH_CONCERNS rather than expanding scope.)

- [ ] **Step 5: Apply the same fixes to the user-disk copies** (so the running system is correct now, not only after a reinstall): edit `~/.config/oxidemx/flows/system-doctor/flow.md` `review` step → `needs = ["diagnose"]`; apply the rust-agent-self-diagnose path + log-analyzer schema fixes on disk too. (These are user-disk files, not committed — just correct them in place.)

- [ ] **Step 6: Commit (repo files only).**

```bash
git add oxidemx-conductor/flows/rust-agent-self-diagnose/flow.md oxidemx-conductor/flows/log-analyzer/flow.md
git commit -m "fix(conductor): seed rust-agent-self-diagnose + log-analyzer flows (stale path + input schema fixed)"
```

---

## Self-Review (filled in)

**1. Spec coverage:** §Part A (fix flows) → Task 2 ✅. §Part B (validate hardening: reflect-target-in-needs hard error → Task 3 ✅; route-needs-empty warning → Task 4 ✅; ancestors threaded → Task 3 ✅). §Part C (stages() + per-flow snapshot tests) → Tasks 1+2 ✅. §Part D (`validate --stages`) → Task 4 ✅. §Part E (user-disk sync + LOW cleanups + seed) → Task 5 ✅. Out-of-scope items explicitly excluded in Global Constraints ✅.

**2. Placeholder scan:** Task 2's `todo_build_roster()` and Task 5's loader/agent specifics are flagged "use the real API, grep src/roster.rs / src/lib.rs" — real seams in existing code the implementer resolves, not silent TODOs. Each names exactly what to find.

**3. Type consistency:** `stages() -> Vec<Vec<String>>` consistent across Tasks 1, 2, 4. `validate_reflect_step` new signature (adds `ancestors: &BTreeMap<String, BTreeSet<String>>`) matches the call-site change in Task 3. `ValidationError::step(id, msg)` / `.step`/`.message` fields match the explorer's real definitions. The route warning reads `s.kind`/`s.needs` (real `Step` fields).
