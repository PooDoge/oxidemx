# Phase 4 — Hybrid router (cost-efficient local/cloud routing)

**Branch:** `phase1-local-llm-gateway` (worktree `../oxidemx-phase1`)
**Status:** implemented + verified (compile, unit tests, live local route) 2026-06-18.

## Goal

A fast LOCAL model classifies each chat turn and answers the SIMPLE ones itself
(no cloud call — private, fast, free); COMPLEX or tool-using turns escalate to
the SMART cloud provider. Realizes the original "hybrid routing & cost
efficiency" requirement.

## Decisions (confirmed with Jim)

- **Classifier = hybrid.** A cheap heuristic settles obvious cases (tool-intent
  → cloud; trivial/short → local); only ambiguous turns cost one local-SLM
  classify call.
- **Local scope = chat-only.** The local model answers from knowledge with no
  tools (dodges the 4B model's weakness with large tool schemas / the OOM seen
  in Phase 2). Any tool need → escalate. Plus a runtime escape: the local system
  prompt tells the model to emit `NEEDS_TOOLS` if it realizes it needs tools,
  and we escalate — a safety net beyond the upfront classifier.
- **Off by default**, fail-safe to cloud on any local/classify error, no
  auto-prompt-rewrite (explicit `/optimize` from the prior step covers that).

## Flow (`agent_runtime::route_turn`, called by `ask_ai`)

```
ask_ai → route_turn
  routing off OR image attached → run()  [smart cloud agent, unchanged]
  else classify_route(prompt):
      heuristic_route: tool-intent → Cloud · trivial(<40 chars) → Local · else None
      None → fast SLM classify (SIMPLE/COMPLEX); error → Cloud (fail-safe)
  Cloud → run()  (full agentic tools, smart provider)
  Local → fast_chat(LOCAL_SYS, history, prompt) on the fast model
            answer == NEEDS_TOOLS / empty / error → escalate to run()
            else → return the local answer
```

Routing decisions are surfaced to the chat as `StreamEvent::Activity`
("↳ simple — answering locally" / "↳ complex — escalating to cloud" /
"↳ needs tools — escalating to cloud") for transparency + cost visibility.

## What changed

| File | Change |
|---|---|
| `oxidemx-shared/src/config.rs` | `AiConfig`: `routing_enabled`, `fast_provider` (default MistralRs), `fast_model` (default "default"); fast endpoint reuses `local_endpoint`. `AiProvider` now derives `Hash`. |
| `oxidemx-agent/src/session.rs` | `Session` caches providers in a `HashMap<ProviderFingerprint, _>` (was a single slot) so the FAST + SMART instances both stay warm across turns. `ProviderFingerprint: Hash`. |
| `overlay-rs/src/agent_runtime.rs` | `Route`, `TOOL_HINTS`, `heuristic_route`, `resolve_fast`, `fast_chat`, `classify_route`, `route_turn`. Unit tests for the heuristic. |
| `overlay-rs/src/ai_client.rs` | `ask_ai` calls `route_turn` (was `run`). |
| `settings-rs/src/{main.rs,tabs/ai.rs}` | "Hybrid routing" section: enable toggle + fast-provider picker (MistralRs/Ollama) + fast-model picker/field. Messages `AiRoutingToggled` / `AiFastProviderChanged` / `AiFastModelChanged`. |

## Verification

- `cargo check --workspace` — clean.
- Unit: `heuristic_route` tests (tool-intent→Cloud, trivial→Local, substantive→
  defer-to-SLM) + the session multi-provider-cache test (both fingerprints stay
  warm). 32 agent + 2 overlay tests pass.
- Live (mistral.rs @ :1234, routing on, throwaway config without a cloud key):
  `"what is a hash map?"` → heuristic Local → **answered by mistral.rs** with a
  correct explanation (cloud never touched). The earlier curl battery confirmed
  the SLM classifier returns `SIMPLE`/`COMPLEX` correctly.

## Notes / follow-ups

- The cloud path is the existing, already-verified `run()` — routing only chooses
  *whether* to call it.
- Per-turn token counts for the local path aren't summed into the thread usage
  readout yet (the local path returns a blocking reply, no Usage event). Minor.
- A future refinement: let the local model also do prompt *sanitization* before
  escalation (currently it only classifies + answers). Deferred — auto-rewrite
  changes intent; `/optimize` stays explicit.
- mistral.rs streaming still off (Phase 2 vendor-SSE gap) — the local path is
  blocking, which is fine for short simple answers.
