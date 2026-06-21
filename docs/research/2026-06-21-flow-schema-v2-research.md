# Flow Schema v2 — Research Synthesis (2026-06-21)

Reference for designing OxideMX's deterministic multi-agent workflow schema + the Tauri flow-builder.
Sources: MS declarative-agent schema v1.7, Copilot Studio (schema + UI + the
`new-copilot-studio-tech-guide` repo), and a control-flow survey of n8n, Temporal, LangGraph,
GitHub Actions, Prefect, Airflow.

## The 8 universal control-flow/IO primitives (present in EVERY mature engine)

| # | Primitive | OxideMX today | Minimal addition |
|---|---|---|---|
| P1 | Conditional branch | **Partial** — `kind="route"` (LLM-classified only) | `kind="branch"` + `condition` expr (deterministic) |
| P2 | Fan-out / map over array | **Missing** (DAG-structural parallelism only) | `kind="foreach"` (`over` array ref + `body` step) |
| P3 | Fan-in / join | **Have** (`needs`) | — |
| P4 | Retry w/ backoff | **Have** (`retry{max,backoff_secs}`, fixed) | exponential + jitter + `non_retryable` types |
| P5 | Typed structured I/O + field ref | **Missing** (raw-text `@step:id@` blobs) | `output_schema` + `done_json` map + `@step:id.field@` |
| P6 | Soft-fail / error boundary | **Missing** (any failure aborts whole flow) | `on_failure: "abort"\|"continue"` per step |
| P7 | Human-in-loop / approval gate | **Partial** — exists but doesn't bubble up (= W2) | surface flow-step approvals to chat |
| P8 | Subflow composition | **Missing** | `kind="subflow"` (later) |
| + | Pure non-LLM step | **Missing** (all 3 kinds call an LLM) | `kind="tool"`: `http`/`shell`/`expr` (no LLM) |
| + | Repeat-until loop | **Partial** (`reflect`, fixed predicate) | `kind="loop"` (`until` expr + `max_iterations`) |

Already solid: P3 (join), P4 (retry skeleton), timeout (`timeout_secs`), DAG dependency.

## Proposed Step-model additions (extend `flowdoc.rs::Step`; keep the flat `[[step]]` TOML)
- `output_schema: Option<String>` — `"json"` or inline JSON Schema; parsed into a parallel
  `done_json: BTreeMap<String, serde_json::Value>` after the step runs (opportunistic, additive —
  steps without it keep working as opaque strings).
- `@step:id.field.nested@` context tokens (dot-path into `done_json`).
- `kind="branch"`: `condition` (expr), `then_step`, `else_step`.
- `kind="tool"`: `tool` (`http`/`shell`/`expr`) + `tool_config` (url/method/body/command/expression).
  **Bypasses the ProviderFactory entirely** — the first non-LLM step kind.
- `kind="foreach"`: `over` (`@step:id.items@` → JSON array) + `body` (step id); supervisor
  instantiates one ephemeral sub-step per item with `{{loop.item}}`/`{{loop.index}}`.
- `kind="loop"`: `body` + `until` (expr on the body's structured output) + `max_iterations`.
- `on_failure: "abort"(default)|"continue"` — soft vs critical fail.
- `mcp_servers: Vec<String>` + `skills: Vec<String>` per step (extend `resolve_tools`).
- `actions` (MS-style, later): two-layer binding — step refs an action id; the action manifest
  declares runtime (`http`/`mcp`/`local`) + OpenAPI/MCP spec URL + auth (vault `reference_id`,
  never inline secrets). Steal MS's `data_handling` attestation enum to drive approval risk level.

## Expression language: **Rhai** (expression-only mode)
For `branch.condition`, `loop.until`, `tool.expr`. Rust-native, readable (`step_check.total * qty
<= budget`), LLM-composable, compile-once/eval-many. Inject each step's parsed output into the
Rhai scope as `step_<id>` object maps (via `serde_json::Value → rhai::Dynamic`). Runners-up: CEL
(younger Rust crate), JSONLogic (rejected — JSON encoding is hostile to hand/LLM authoring).
Note: `rhai::Engine` is not `Send` — build per-eval or `Arc<Mutex<>>` the compiled AST.

## KEY ARCHITECTURAL INSIGHT — loops/foreach are RUNTIME constructs, not DAG nodes
Every code-first engine (Temporal, Prefect, LangGraph cycles) treats loops as host-language
control flow, NOT graph topology. So `foreach`/`loop` must be **supervisor-level constructs that
generate ephemeral sub-steps at runtime** — they do NOT add nodes to the validated DAG (which
would break the Kahn topo-sort + the stages contract). This keeps W1's `stages()` + validation
intact and is the proven pattern. Branching (`route`/`branch`) DOES stay in the DAG (pre-declared
edges, skip the untaken branch) — matches Copilot Studio's `ConditionGroup`.

## The seat-tier example, expressed in v2
`foreach` over `["lower","middle","upper","general"]` → body step checks price (a `kind="tool"`
http/expr step producing structured `{ticketAmount, ticketPrice}`) → `branch`/`until`:
`step_check.ticketAmount * step_check.ticketPrice <= input.maxTicketPrice` → on exhaustion,
`on_failure` decides critical (abort) vs soft (pass partial downstream).

## Format decision (from the Copilot Studio repo)
**Keep TOML-frontmatter + markdown-body `flow.md`.** Copilot Studio converged on the exact same
pattern (`.mcs.yml` = YAML frontmatter + markdown body for skills; `kind:` discriminator on every
polymorphic node; XML only for CRM packaging; verbose `$kind` JSON only for machine-gen runtime
blobs — avoid that for hand-authored files). Adopt: a `kind:` discriminator on every step (have it)
+ typed I/O inline in the TOML block. Conditions/loops belong in the conductor DAG/runtime, NOT in
the prose body (Studio delegates prose-logic to the LLM; we want deterministic).

## Builder-UI patterns to adopt (W4 — Tauri/Vue, deferred)
From Copilot Studio's designers: **vertical-linear backbone with horizontal branch fan-out** for
the flow (topics), free-form spatial canvas + **Tidy Up auto-layout** for DAGs; **right-hand side
config panel** (non-obscuring); **type badges** on every input/output field; **variable picker**
grouped by scope + a formula tab; **node-level testing** with type-aware mock inputs + load-from-
previous-run; **Health Center** inline error highlighting (live validation, not publish-time);
**Ctrl+F canvas search**; color-coded **annotation notes**. These map directly onto the
`validate --stages` + typed-IO foundation.

## Proposed phasing (Schema v2 is large — build incrementally on the W1 foundation)
- **Phase 2a — Typed structured I/O** (`output_schema` + `done_json` + `@step:id.field@`). The
  foundation; conditions/loops can't evaluate fields without it.
- **Phase 2b — Deterministic steps + branching + fail modes** (`kind="tool"` http/shell/expr,
  `kind="branch"` + Rhai, `on_failure`). The "lean deterministic" win.
- **Phase 2c — Iteration** (`kind="foreach"`, `kind="loop"` + `until`). The seat-tier retry.
- **Phase 2d — Capability/action binding** (per-step REST/MCP/skill via MS-style action manifests;
  retry exponential+jitter+non-retryable). 
- **Cross-cutting:** W2 (approval bubble-up = P7) should land early — it's a safety gate the
  deterministic `tool` steps (curl/shell) make urgent.

W1 (reflect-`needs` fix + `stages()` + validate hardening) stays the FOUNDATION — additive, no
rework. Recommend building W1 now, then 2a.
