# SP2b — Local-model Planner + schema-gating primitives — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Turn a goal/spec into a validated, typed `StepGraph` using the local model under a JSON-schema **generate-time constraint** + **validate-time** check, with a violation→retry→escalate recovery loop — the schema-gated plan boundary of the harness.

**Architecture:** Three pieces: (1) a typed `StepGraph` + `validate()` (cycles/orphans/reachability) in the leaf `oxidemx-ledger` crate; (2) grammar/JSON-schema constrained decoding added to `oxidemx-agent-local` (mistralrs 0.8.1 `Constraint`); (3) a new leaf-ish `oxidemx-planner` crate with a `PlannerModel` seam (mock-testable), `schemars`-derived schema, `jsonschema` validate-time gate, and the recovery loop. The actual *execution* of a StepGraph is SP2c.

**Tech Stack:** Rust, serde/serde_json, `schemars` (Rust type → JSON Schema), `jsonschema` (validate-time), mistralrs 0.8.1 `Constraint` (generate-time, behind `mistral` feature), thiserror.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-19-sp2-autonomous-coding-harness-design.md` §4.2, §4.7. Decision: **keep `oxidemx-agent-local`, add `Constraint` directly** (`docs/research/mistralrs-integration-comparison.md`) — do NOT adopt/fork `autoagents-mistral-rs`.
- `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]` on new crate; no `unwrap`/`expect` outside tests; `thiserror` (`#[non_exhaustive]`); `clippy -D warnings`; pristine build; per-crate `cargo test` green each task.
- `oxidemx-agent-local` default build MUST still exclude `mistralrs` (grammar code behind the `mistral` feature); `cargo tree -p oxidemx-agent-local | grep -i mistralrs` empty on default. The constraint plumbing through the engine SEAM (`EngineRequest`) is mock-testable on the default path; the real `set_constraint` call is `#[cfg(feature="mistral")]`.
- No clock calls in pure logic (resume-determinism); caller-supplied `ts` continues from SP2a.
- Worktree `../oxidemx-phase1`, branch `phase1-local-llm-gateway`. Commit per task.

## File structure

```
oxidemx-ledger/src/graph.rs        # T1: StepGraph newtype + validate() (cycles/orphans/reachability)
oxidemx-agent-local/src/engine.rs  # T2: EngineRequest gains `constraint`
oxidemx-agent-local/src/mistral.rs # T2: MistralEngine applies set_constraint (cfg mistral)
oxidemx-agent-local/src/provider.rs# T2: LocalChatProvider passes json_schema as a constraint
oxidemx-planner/Cargo.toml          # T3 new crate
oxidemx-planner/src/lib.rs
oxidemx-planner/src/model.rs        # PlannerModel trait (seam) + PlanRequest/PlanReply
oxidemx-planner/src/planner.rs      # Planner: schema-gated spec->StepGraph + recovery loop
oxidemx-planner/src/error.rs
```

---

### Task 1: `StepGraph` + `validate()` (cycles / orphans / reachability)

**Files:** Create `oxidemx-ledger/src/graph.rs`; Modify `oxidemx-ledger/src/lib.rs`.

**Interfaces:**
- Consumes: `Step`, `StepStatus`, `LedgerError` (SP2a).
- Produces: `pub struct StepGraph { steps: Vec<Step> }` with `pub fn new(steps: Vec<Step>) -> Self`, `pub fn steps(&self) -> &[Step]`, `pub fn into_manifest(self, task_id, goal) -> TaskManifest`, and `pub fn validate(&self) -> Result<(), GraphError>` checking: (a) **unique step ids** (dup → `GraphError::DuplicateId`); (b) **no orphan needs** — every `needs` id refers to an existing step (`MissingDep`); (c) **acyclic** — Kahn's topo-sort; a remaining-nonzero-indegree set → `Cycle(Vec<String>)`; (d) **reachability** is informational (a `warnings()` for unreachable nodes — not an error). Add `pub enum GraphError { DuplicateId(String), MissingDep{step:String,needs:String}, Cycle(Vec<String>) }` (thiserror) OR extend `LedgerError` — pick `GraphError` in graph.rs re-exported, mapping into `LedgerError::Corrupt` where a ledger op needs it.

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn validate_accepts_a_dag() {
    let g = StepGraph::new(vec![
        Step::new("a","first"),
        { let mut s = Step::new("b","second"); s.needs = vec!["a".into()]; s },
    ]);
    assert!(g.validate().is_ok());
}
#[test]
fn validate_rejects_cycle() {
    let g = StepGraph::new(vec![
        { let mut a = Step::new("a","A"); a.needs = vec!["b".into()]; a },
        { let mut b = Step::new("b","B"); b.needs = vec!["a".into()]; b },
    ]);
    assert!(matches!(g.validate(), Err(GraphError::Cycle(_))));
}
#[test]
fn validate_rejects_missing_dep_and_dup() {
    let g1 = StepGraph::new(vec![{ let mut a = Step::new("a","A"); a.needs = vec!["ghost".into()]; a }]);
    assert!(matches!(g1.validate(), Err(GraphError::MissingDep{..})));
    let g2 = StepGraph::new(vec![Step::new("a","A"), Step::new("a","dup")]);
    assert!(matches!(g2.validate(), Err(GraphError::DuplicateId(_))));
}
```
- [ ] **Step 2: Run → FAIL** (`cargo test -p oxidemx-ledger graph`).
- [ ] **Step 3: Implement** `graph.rs` (Kahn's algorithm for the cycle check; HashSet for dup/dep). Declare `pub mod graph;` + re-export `StepGraph`/`GraphError`.
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** — `feat(ledger): StepGraph + validate (cycles/orphans/dups)`

---

### Task 2: Grammar / JSON-schema constrained decoding in `oxidemx-agent-local`

**Files:** Modify `oxidemx-agent-local/src/engine.rs`, `src/mistral.rs`, `src/provider.rs` (inspect their real shapes first).

**Interfaces:**
- `EngineRequest` (engine.rs) gains a field `pub constraint: Option<SchemaConstraint>` where `SchemaConstraint` is a small feature-independent enum in agent-local: `pub enum SchemaConstraint { JsonSchema(serde_json::Value), Regex(String) }` (so the default/non-mistral build carries it as data without depending on mistralrs types).
- `MockEngine` records the `constraint` it received (a `#[cfg(test)]` accessor) so the plumbing is unit-tested without the native engine.
- `#[cfg(feature="mistral")] MistralEngine::generate` maps `SchemaConstraint::JsonSchema(v)` → `mistralrs::Constraint::JsonSchema(v)` (and `Regex`→`Constraint::Regex`) and calls `RequestBuilder::set_constraint(...)` before sending. (Verify the exact 0.8.1 path: `mistralrs` re-exports `Constraint`; `RequestBuilder::set_constraint` per `messages.rs`.)
- `LocalChatProvider`/`LocalModelService` (provider.rs): the `_json_schema` parameter that is currently ignored is threaded into `EngineRequest.constraint = Some(SchemaConstraint::JsonSchema(schema))` so a caller asking for structured output actually gets a constrained decode.

- [ ] **Step 1: Failing test (mock path — no mistralrs)**
```rust
#[tokio::test]
async fn engine_request_carries_constraint_to_engine() {
    let eng = MockEngine::new(/* canned reply */);
    // a request built with a JsonSchema constraint reaches the engine
    let req = EngineRequest::new(/* msgs */).with_constraint(
        SchemaConstraint::JsonSchema(serde_json::json!({"type":"object"})));
    let _ = eng.generate(req).await.unwrap();
    assert!(matches!(eng.last_constraint(), Some(SchemaConstraint::JsonSchema(_))));
}
```
(Adapt to the real `EngineRequest`/`MockEngine` constructors — inspect engine.rs. Add `with_constraint` builder + `MockEngine::last_constraint()` test accessor.)
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the `SchemaConstraint` enum + `EngineRequest.constraint` + `MockEngine` recording (default path); `MistralEngine` `set_constraint` mapping behind `#[cfg(feature="mistral")]`; provider threads `json_schema`→constraint.
- [ ] **Step 4: Run → PASS** (`cargo test -p oxidemx-agent-local`); confirm `cargo build -p oxidemx-agent-local --features mistral` builds + default `cargo tree | grep mistralrs` empty.
- [ ] **Step 5: Commit** — `feat(agent-local): JSON-schema/regex constrained decoding via mistralrs Constraint`

---

### Task 3: `oxidemx-planner` — schema-gated spec→StepGraph + recovery loop

**Files:** Create `oxidemx-planner/{Cargo.toml,src/lib.rs,src/model.rs,src/planner.rs,src/error.rs}`; Modify root `Cargo.toml` (members).

**Interfaces:**
- Deps: `oxidemx-ledger` (StepGraph/Step), `serde`/`serde_json`, `schemars` (derive a JSON Schema for the plan output type), `jsonschema` (validate-time), `thiserror`, `async-trait`. NOT a direct dep on `oxidemx-agent-local` — the model is a seam.
- `model.rs`: `#[async_trait] pub trait PlannerModel: Send+Sync { async fn complete(&self, req: PlanRequest) -> Result<String, PlannerError>; }` where `PlanRequest { pub system: String, pub goal: String, pub json_schema: serde_json::Value, pub escalate: bool }` (the `json_schema` is passed to the model as a generate-time constraint by the real impl; `escalate=true` asks the impl to use the cloud model). A `#[cfg(test)] MockPlannerModel` returns canned strings (valid JSON, invalid JSON, schema-violating JSON) per a script to exercise the gate + recovery.
- `planner.rs`: `pub struct Planner<M: PlannerModel> { model: M, max_local_retries: u32 }`; `#[derive(Serialize,Deserialize,JsonSchema)] struct PlanOutput { steps: Vec<PlanStep> }` + `PlanStep { id, title, needs: Vec<String> }` (the wire shape the model emits; mapped to ledger `Step`s). `pub async fn plan(&self, goal: &str) -> Result<StepGraph, PlannerError>`:
  1. build the JSON schema from `PlanOutput` via `schemars`;
  2. call `model.complete` with that schema (generate-time constraint);
  3. **validate-time:** parse the reply as JSON; validate against the schema with `jsonschema` → on failure, feed the validator error back as a corrective system note and retry (≤ `max_local_retries`); then deserialize to `PlanOutput`;
  4. map to `StepGraph` and call `StepGraph::validate()` (cycles/orphans) → on failure, retry with the graph error as the corrective note;
  5. if local retries exhausted → one `escalate=true` call → if that still fails, `Err(PlannerError::Unresolved{reasons})`.
- `error.rs`: `#[non_exhaustive] pub enum PlannerError { Model(String), SchemaInvalid(String), GraphInvalid(String), Unresolved{reasons:Vec<String>} }`.

- [ ] **Step 1: Failing tests (MockPlannerModel scripted)**
```rust
#[tokio::test]
async fn plan_returns_valid_stepgraph() {
    let model = MockPlannerModel::scripted(vec![
        r#"{"steps":[{"id":"a","title":"first","needs":[]},
                     {"id":"b","title":"second","needs":["a"]}]}"#.into(),
    ]);
    let p = Planner::new(model, 2);
    let g = p.plan("do the thing").await.unwrap();
    assert_eq!(g.steps().len(), 2);
    assert!(g.validate().is_ok());
}
#[tokio::test]
async fn plan_recovers_from_invalid_then_valid() {
    let model = MockPlannerModel::scripted(vec![
        "not json at all".into(),                              // attempt 1: schema-invalid
        r#"{"steps":[{"id":"a","title":"x","needs":[]}]}"#.into(), // attempt 2: valid
    ]);
    let p = Planner::new(model, 2);
    assert!(p.plan("g").await.is_ok());                        // recovered on retry
}
#[tokio::test]
async fn plan_rejects_cyclic_plan() {
    let model = MockPlannerModel::scripted(vec![
        r#"{"steps":[{"id":"a","title":"A","needs":["b"]},
                     {"id":"b","title":"B","needs":["a"]}]}"#.into(),  // valid JSON, cyclic graph
        r#"{"steps":[{"id":"a","title":"A","needs":["b"]},
                     {"id":"b","title":"B","needs":["a"]}]}"#.into(),  // escalate also cyclic
    ]);
    let p = Planner::new(model, 1);
    assert!(matches!(p.plan("g").await, Err(PlannerError::Unresolved{..})));  // graph gate caught it
}
```
- [ ] **Step 2: Run → FAIL.**
- [ ] **Step 3: Implement** the crate per the Interfaces block (schemars-derived schema, jsonschema validate-time, the retry/escalate loop, map `PlanOutput`→`StepGraph`). `MockPlannerModel::scripted` pops a canned reply per call + records the requests (to assert the schema + escalate flag were passed). Add to workspace members.
- [ ] **Step 4: Run → PASS** (`cargo test -p oxidemx-planner`).
- [ ] **Step 5: Commit** — `feat(planner): oxidemx-planner — schema-gated spec->StepGraph + recovery loop`

---

## Self-Review
- **Spec coverage** (§4.2, §4.7): typed `StepGraph` + `validate()` (boundary 2, cycles/orphans) → T1; generate-time constrained decoding (boundary 1, generate-time layer) → T2; the Planner = schema-gated spec→DAG (boundary 1, plan boundary) + validate-time `jsonschema` + violation→retry→escalate recovery → T3. Edge boundary (3) + completion (4) are SP2c/SP2a respectively (out of this plan).
- **Placeholders:** none; test code concrete. The mistralrs `set_constraint` exact call is the one symbol the T2 implementer verifies against 0.8.1 (the research doc + `messages.rs:749` give the path).
- **Type consistency:** `StepGraph`/`GraphError` (T1) consumed by T3; `SchemaConstraint`/`EngineRequest` (T2) consumed by agentd's real `PlannerModel` impl (SP2c wiring, not here); `PlannerModel`/`PlanOutput`/`PlannerError` consistent within T3. The default builds never compile mistralrs.
