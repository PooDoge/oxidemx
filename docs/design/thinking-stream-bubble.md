# Idea — Stream reasoning ("Thinking…") then collapse into an expandable bubble — user request 2026-06-20

The user wants the agent's reasoning/thinking to **stream live** (like Claude Code and other
agents), then, once the final response is returned, **collapse into an expandable "Thinking…"
bubble** above/around the answer (click to re-expand the reasoning).

## Feasibility — good; we already have most of the pipe
Our turn already streams **token deltas** end-to-end (Gemini live streaming WITH tools via the
vendored AutoAgents patch → `StreamBridge` → D-Bus `event` signal → overlay renders). Reasoning
streaming is the **same pipe with a second channel**: most providers expose reasoning/thinking
separately from answer tokens —
- **Anthropic:** `thinking` content blocks (extended thinking) — `thinking_delta` SSE events.
- **Gemini:** "thought" parts / thinking tokens (thinking models).
- **OpenAI / reasoning models:** reasoning summary deltas.
- **mistral.rs (local):** depends on model; may need a `<think>…</think>` parser.

## Rough shape (its own brainstorm → spec → plan slice later)
1. **Provider seam:** the streaming provider emits a discriminated delta —
   `StreamDelta::{ Reasoning(String), Answer(String), Tool(..), ... }` — so reasoning and answer
   are separable, not concatenated. (Field-standard: keep "thinking" a distinct channel.)
2. **Transport:** `StreamBridge` / the `event` signal carries a `kind: "thinking"` delta
   alongside the existing answer deltas (and the run/tool kinds). Connector-agnostic
   (`OutboundMessage` gains a reasoning variant) so HTTP/Telegram can drop or fold it per
   `ConnectorCaps`.
3. **Overlay UI:** while streaming, show a live "Thinking…" area accumulating reasoning tokens;
   on final-answer completion, **collapse it into an expandable "Thinking…" bubble** (chevron to
   re-expand). Reuse the chat bubble + expand/collapse patterns from the activity-UI work.
4. **Truthfulness tie-in:** reasoning is the model's *private* chain — clearly delimited from the
   answer, and NOT treated as a source of verified state (Rule 1). It's shown for transparency,
   not as ground truth.

## Status
Idea captured; **its own slice**, after the truthful-runs slice (it shares the streaming +
expandable-bubble UI infra, so it's natural to do alongside/after the activity UI). Needs a
provider-capability check per backend (which of our providers expose reasoning deltas today).
