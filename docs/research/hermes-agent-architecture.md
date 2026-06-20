# Hermes Agent (Nous Research) — Architecture & Naming Research

> Researched 2026-06-20. Sources: official docs at hermes-agent.nousresearch.com, GitHub NousResearch/hermes-agent, DeepWiki, dplooy.com write-up.
> Purpose: architecture + naming inspiration for OxideMX's Rust agent framework (oxidemx-agent-core, oxidemx-conductor, agentd).

---

## 1. High-Level Architecture

Hermes is a **Python** (82 %) + TypeScript (14 %) long-lived agent runtime, not a library. The design principle is explicitly stated: *"One AIAgent class serves CLI, gateway, ACP, batch, and API server. Platform differences live in the entry point, not the agent."*

```
Entry Points (CLI / Gateway / ACP / Web)
         │
         ▼
   AIAgent  (run_agent.py)          ← orchestration engine / conversation loop
   ├─ PromptBuilder                 (agent/prompt_builder.py)
   ├─ ContextEngine / Compressor    (context_compressor.py)
   ├─ model_tools  ← tool registry + dispatch
   │    └─ tools/registry.py        (70+ tools, 28 toolsets; self-register at import)
   ├─ MemoryManager                 (agent/memory_manager.py)
   └─ hermes_state.py               (SQLite + FTS5 session persistence)
         │
   ProviderResolver (hermes_cli/runtime_provider.py)
   └─ maps (provider, model) → (api_mode, api_key, base_url)
         │
   LLM API   (chat_completions / codex_responses / anthropic modes)
```

**Entry point modules:** `cli.py` · `gateway/run.py` · `acp_adapter/` · `tui_gateway/` · `web/`

---

## 2. Connectors / Channels / Platform Adapters

### What Hermes calls this abstraction

Hermes uses the word **"Gateway"** for the long-running multi-platform bridge, and **"platform adapter"** (or just "adapter") for each individual integration. The key seam is called `gateway/` in the module tree. The entry point is `gateway/run.py`.

There are **20 platform adapters** bundled: Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Mattermost, Email, SMS, DingTalk, Feishu, WeCom, BlueBubbles, Home Assistant, and others.

### Seam shape

- The **Gateway** is a long-running process that handles routing, session continuity, user authorization (allowlists + DM pairing), slash command dispatch, a hook system, and cron scheduling.
- Each **platform adapter** translates platform-specific wire format → unified message + session context, then hands it to `AIAgent`.
- `AIAgent` has no awareness of which platform it's on — all platform differences stay in the entry point/adapter layer.
- **ACP adapter** (`acp_adapter/`) is a separate integration for programmatic/API access via the Agent Communication Protocol.
- Conversation state **survives channel switches** — a session is cross-platform continuous.

### Other connector-related terms

| Hermes term | What it means |
|---|---|
| `gateway` | The multi-platform routing process |
| `platform adapter` | Per-platform integration (Telegram, Discord, etc.) |
| `acp_adapter` | ACP protocol integration for programmatic access |
| `optional-mcps/` | MCP server plugins (tool capability connectors) |
| `tui_gateway` | Terminal-UI gateway entry point |
| `Tool Gateway` (v0.10.0) | A per-tool opt-in proxy for external APIs (Firecrawl, FAL, Browser Use, TTS) — config field `use_gateway` |

---

## 3. Background / Async Runs + Status Truthfulness

### Scheduling / background tasks

Hermes handles background execution via **cron** (`cron/` module) within the Gateway process — scheduled automations trigger `AIAgent` just like a user message would. There is no separate "Run" or "Job" entity tracked as a named abstraction at the public API level.

### Status and progress

Hermes does not appear to have a distinct "RunStatus" / "job status" query API. Status is surfaced through:
- **Context compression checkpoints** — `/compress` command, plus automatic lossy summarization when context exceeds threshold.
- **Usage and insight commands** — `/usage`, `/insights` expose token/context state.
- **Streaming output** — tool output streams back through the same channel the message arrived on.
- **Platform-native approval prompts** — "Approval buttons in Slack and Telegram mean sensitive command execution can require a tap on your phone before anything touches your server."

### Truthfulness / grounding mechanism

Hermes grounds status in **SQLite session state** (`hermes_state.py`) with atomic writes. The agent does not fabricate tool results — actual tool output is injected as tool-result messages into the conversation history. The **iteration budget** in `AIAgent` is an explicit cap: the loop terminates rather than running indefinitely. No separate truthfulness oracle is documented; the architecture relies on actual tool execution returning real results.

---

## 4. Tool System

### Definition and registration

- All tools live under `tools/` and self-register at import time via `tools/registry.py`.
- The registry provides **schema collection, dispatch, availability checking, and error wrapping** (all in `model_tools.py`).
- 70+ tools across ~28 toolsets (terminal, file ops, web browsing, image gen, voice, memory, etc.).
- **Terminal tools** support six execution backends: Local, Docker, SSH, Daytona, Modal, Singularity — the backend is a config choice, not a per-tool concern.

### Approval / permission gating

- Platform-level: **approval buttons** in Slack/Telegram gate sensitive tool calls behind a user tap. This is enforced at the Gateway layer, not inside `AIAgent`.
- Per-tool opt-in to the **Tool Gateway** proxy (`use_gateway: true` in config) for externally-hosted tool APIs with OAuth 2.1 + PKCE.
- DM pairing: messaging-app user IDs are bound to specific Hermes instances to prevent unauthorized access.

### Structured I/O

- Tools use standard OpenAI-compatible JSON schema for input.
- Three API modes (`chat_completions`, `codex_responses`, `anthropic`) allow tool schemas to be sent in whichever format the provider expects — the resolver picks the mode.

---

## 5. Naming Conventions Glossary

| Hermes term | Our current term | Notes |
|---|---|---|
| `AIAgent` | `agent_runtime` / `AutoAgent` | The orchestration engine class; single class serves all entry points |
| `handle_function_call` | `ToolExecutor` / `route_turn` | The per-tool dispatch method inside `AIAgent` |
| `tool` / `toolset` | `tool` | Tools self-register; grouped into toolsets by domain |
| `skill` | — | Hermes-specific: Markdown procedural memory docs auto-generated after complex tasks (agentskills.io standard); distinct from tools |
| `gateway` | — | The multi-platform connector/routing process; what Hermes calls the connector abstraction |
| `platform adapter` | `Connector` | Per-platform integration living in the gateway layer |
| `session` | `session` / `thread` | SQLite-persisted, cross-platform continuous; `hermes_state.py` |
| `provider` | `PlannerModel` / `AiProvider` | `(provider, model)` tuple resolved to `(api_mode, api_key, base_url)` |
| `api_mode` | — | One of `chat_completions` / `codex_responses` / `anthropic`; the wire format variant |
| `context engine` | — | Pluggable abstraction for context compression; default = lossy summarization |
| `iteration budget` | — | Explicit cap on the agent loop's tool-call cycles |
| `memory` | memory | File-based: `MEMORY.md` + `USER.md` managed by `memory_manager.py` |
| `plugin` | — | Discovery from `~/.hermes/plugins/`, `.hermes/plugins/`, or pip entry points |
| `ACP adapter` | — | Agent Communication Protocol integration for programmatic/API access |
| `cron` | — | The scheduled background task system within the gateway process |
| `hook system` | `EventEmitter` / `RunEvent` | Event hooks inside the gateway for platform event handling |

---

## 6. Ideas Worth Adopting

### 1. Gateway as a named, first-class abstraction (not just "connectors")

Hermes names the entire multi-transport layer `gateway` — a long-running process with its own entry point (`gateway/run.py`). This is a stronger abstraction than calling each adapter a "connector": the **gateway** owns routing, session continuity, authorization, slash commands, cron, and hooks. For OxideMX, this maps well to `agentd` — we should be explicit that the D-Bus service IS the gateway, and name the front-end adapters as `PlatformAdapter` or `Channel`.

**Concrete adoption:** rename the OxideMX overlay/chat-shell integration from `EventEmitter` to a typed `OverlayChannel` or `OverlayAdapter`; the D-Bus service is the `Gateway` layer. This makes the seam explicit.

### 2. Approval gating at the Gateway layer, not inside the agent loop

Hermes puts approval buttons (Telegram/Slack taps for sensitive tool calls) in the **gateway adapter**, not inside `AIAgent`. This is the right seam: the agent loop should not know whether a tool is gated — it issues the call and waits for a result; the gateway intercepts and holds for human approval before forwarding. Our `GatedToolExecutor` currently lives inside the agent runtime — consider moving gating logic to the conductor/gateway boundary, so the agent core stays approval-agnostic and gating policy is a gateway concern.

### 3. Provider resolver as a separate module with a clear tuple contract

`hermes_cli/runtime_provider.py` maps `(provider, model) → (api_mode, api_key, base_url)`. The `api_mode` enum (`chat_completions` / `codex_responses` / `anthropic`) cleanly separates **which provider** from **which wire format**. Our multi-provider factory (`AiProvider` enum + factory fn) should similarly make the wire-format mode explicit — especially since we support OpenAI-compat, native Gemini, Anthropic, and Ollama. A `ProviderBinding { provider, model, wire_format, base_url }` struct would be cleaner than smearing that logic across `ai_client.rs`.

### 4. Skill store as a distinct memory layer (separate from episodic session memory)

Hermes has **four distinct memory layers**: session context, persistent SQLite (episodic), skill store (procedural — Markdown docs at agentskills.io standard), and the optional Honcho user-model layer. Our memory system conflates episodic and procedural. A separate `SkillStore` (searchable Markdown/structured docs describing how to do tasks) would let the agent load only skill stubs into the system prompt and pull full text on demand — the same pattern Hermes uses ("only skill names and brief descriptions load by default"). This is directly applicable to the OxideMX skills palette.

### 5. Three-tier prompt ordering (stable → context → volatile)

`agent/prompt_builder.py` assembles system-prompt tiers: `stable` (identity + tool guidance + skills) → `context` (context files + memory results) → `volatile` (per-request: profile + timestamp + memory). This ordered-tier model is a clean, testable alternative to ad-hoc string concatenation and makes cache-prefix stability explicit (stable tier never changes → prefix cache hits). Our `oxidemx-agent-core` prompt assembly could adopt this structure to maximize Anthropic prefix caching.

---

## Summary Table for Quick Reference

| Concern | Hermes term | OxideMX current term | Recommendation |
|---|---|---|---|
| Core orchestration class | `AIAgent` | `AgentRuntime` / `AutoAgent` | Keep `AgentRuntime`; matches Hermes intent |
| Tool dispatch method | `handle_function_call` | `ToolExecutor::execute` | OK as-is |
| Multi-transport layer | `Gateway` | `agentd` / EventEmitter | Adopt "Gateway" as explicit name for agentd's role |
| Per-platform integration | `platform adapter` | (unnamed) | Name these `OverlayAdapter`, `TelegramAdapter`, etc. |
| Provider wire-format | `api_mode` | implicit in match arm | Add explicit `WireFormat` enum |
| Approval gating | gateway-layer approval buttons | `GatedToolExecutor` | Move gating to gateway boundary |
| Procedural memory | `skill` (Markdown) | skills palette SKILL.md | Separate `SkillStore` from episodic memory |
| Prompt assembly | three-tier builder | ad-hoc string concat | Adopt stable/context/volatile tier model |
| Session persistence | `hermes_state.py` (SQLite) | in-memory + embeddings | Aligned; continue with SQLite backing |
| Background scheduling | `cron` (inside gateway) | conductor oneshot/run | Conductor handles this; aligned |

---

## Sources

- Architecture docs: https://hermes-agent.nousresearch.com/docs/developer-guide/architecture
- GitHub repo: https://github.com/NousResearch/hermes-agent
- DeepWiki analysis: https://deepwiki.com/NousResearch/hermes-agent
- dplooy.com write-up: https://www.dplooy.com/blog/hermes-agent-nous-researchs-self-learning-ai-runtime
- i-scoop overview: https://www.i-scoop.eu/hermes-agent-from-nous-research/
