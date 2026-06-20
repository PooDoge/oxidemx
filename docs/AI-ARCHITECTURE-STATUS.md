# AI architecture — status index (READ FIRST)

This project is in **fast, experimental trial-and-error** development. Whole
systems get ripped out and replaced; we do NOT phase things out behind feature
flags like a production app. This file is the single source of truth for **what
is current vs. superseded** so no one implements from a stale doc. Update it
whenever a system is replaced.

_Last updated: 2026-06-18 (branch `phase1-local-llm-gateway`)._

## Current canonical docs — implement from these

| Doc | Scope | Status |
|---|---|---|
| `docs/superpowers/specs/2026-06-18-agent-framework-rearchitecture-design.md` | Umbrella: coding-first, **agentd** (single backend, overlay = thin D-Bus client), orchestration/multi-agent/federation, SP1–SP7 roadmap | CURRENT (one amendment — see below) |
| `docs/superpowers/specs/2026-06-18-local-model-manager-design.md` | **Embedded** local LLM: `oxidemx-agent-local` crate (mistral.rs 0.8.1 in-process), capability scoping, `ResponseGuard` failsafe | CURRENT — canonical local-LLM approach (BUILT) |
| `docs/superpowers/specs/2026-06-18-agentd-design.md` + plan `docs/superpowers/plans/2026-06-19-agentd-sp1b.md` | **agentd** (SP1b): persistent project-aware process hosting core+conductor+local-model, `org.oxidemx.Agent` D-Bus; per-project skills/mcp/config/transcripts/journal (Claude-Code-style); reserves SP-Learn/SP-Research seams | **BUILT** (commits 98eee01..658942e; 25 tests+ignored live-bus; final review READY-WITH-MINORS). |
| `docs/superpowers/specs/2026-06-19-sp1c-overlay-cutover-design.md` | **SP1c**: complete agentd's REAL turn path + overlay → thin `org.oxidemx.Agent` client. | **T1–T8a BUILT + headless-tested** (live-bus e2e green); **T8b (flip+delete in-proc) DEFERRED** until the user's GUI walkthrough. |
| `docs/superpowers/specs/2026-06-19-sp2-autonomous-coding-harness-design.md` + `docs/research/*` + plans `…sp2a-task-ledger.md`, `…sp2b-planner.md` | **SP2**: autonomous coding harness — TaskLedger + local-model schema-gated Planner + orchestrator-worker execution + SDD-loop flow + AutoAgents adoptions (keep conductor). Schema-gated DAG (§4.7) + risk-tiered approval (§5.1) settled with user. | **SP2a (oxidemx-ledger) + SP2b (StepGraph/validate, constrained decoding, oxidemx-planner) BUILT + final-reviewed** (commits b74e744..5af04cc). **SP2c next** (orchestrator-worker exec + ApprovalClassifier + edge validation + agentd wiring). SP2d/e after. |
| `docs/superpowers/plans/2026-06-18-sp1a-agent-core-extraction.md` | Extract `oxidemx-agent-core` from the overlay | **DONE** (commits b62924c..0baa856) |
| `docs/superpowers/plans/2026-06-18-oxidemx-agent-local.md` | Build the embedded local-model crate | **NEXT / in progress** |
| `docs/plans/phase2-session-manager.md` | Per-conversation `SessionManager` substrate | CURRENT (shipped; backend-swap doesn't affect it) |
| `docs/plans/phase4-hybrid-router.md` | Hybrid local/cloud router | CURRENT logic (its local backend is moving to the embedded provider) |

## Superseded — do NOT implement from these

| Doc | Why superseded | Replaced by |
|---|---|---|
| `docs/plans/phase1-local-llm-gateway.md` | Local LLM ran mistral.rs as an **OpenAI-compatible HTTP server** (`AiProvider::MistralRs` via the OpenAI backend + `base_url`). We are switching to **embedded** mistral.rs. | the embedded local-model-manager spec (`oxidemx-agent-local`) |
| `docs/plans/agent-framework-integration-brainstorm_OLD.md`, `agent-feature-roadmap_OLD.md`, `agent-features-implementation_OLD.md` | Interactions-era brainstorms; assumed the bespoke Gemini Interactions transport (retired) | the 2026-06-18 rearchitecture spec (research quarry only) |

**Key superseding decision:** local inference moved from *mistral.rs-as-HTTP-server*
→ *embedded mistral.rs 0.8.1 in agentd* (`oxidemx-agent-local`). The
`AiProvider::MistralRs` provider variant survives in name but will be **re-pointed**
from the HTTP/OpenAI path to the in-process `LocalChatProvider` (no HTTP). When that
lands, delete the HTTP path rather than flag-gate it.

## Historical execution records (accurate for their time; not forward guidance)

`docs/superpowers/plans/2026-06-12-agent-framework-p0.md`,
`2026-06-12-agent-framework-p1a.md`,
`2026-06-13-agent-framework-p1b-overlay-replacement.md`,
`2026-06-13-agent-framework-p2-semantic-memory.md`,
`2026-06-13-agent-framework-p3-multiprovider.md` — completed P0–P3 of the original
AutoAgents integration. The Interactions transport they reference was retired in P3.
Keep for history; don't treat as current design.

## Crate map (current)

- `oxidemx-agent-core` — agent brain (providers/turn loop/tools/memory/persona), UI-free, hosted by agentd later. **Exists** (SP1a).
- `oxidemx-agent-local` — embedded local-LLM service (mistral.rs 0.8.1). **Being built.**
- `oxidemx-conductor` — flow orchestration engine. Exists.
- `agentd` — persistent **project-aware** process hosting core + conductor + local-model manager, `org.oxidemx.Agent` D-Bus; per-project (cwd) skills/mcp/config/transcripts/journal. **BUILT (SP1b, 658942e)** — headless-tested; default build excludes mistralrs (real engine behind `mistral` feature). `oxidemx-agent-proxy` = light (zbus+serde) `#[proxy]` client for SP1c. **SP1c (overlay→thin client) NOT started.** SP1c carry-overs: unify EventEmitter↔conductor EventSink, full `run_flow`/`cancel_*` wiring, real tool-executor+token-usage at the turn seam, consume project skills/mcp/config merge in the turn path, swap the stub SessionManager. Later: SP-Learn (self-improving loop off the per-project journal) + SP-Research (local web-grounded research offload) — seams reserved.
- overlay-rs / settings-rs / oxidemx-mission-control — clients (overlay shims to core today; become D-Bus clients in SP1c).
