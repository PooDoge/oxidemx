# Agent Framework P3: Multi-provider + retire the bespoke transport

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Implementation guidance: `building-llm-agents-in-rust`. Steps use checkbox (`- [ ]`).

**Goal (user directive):** Reduce complexity — drop the bespoke Gemini **Interactions** transport, move to the **normal Gemini API** (`generateContent`, AutoAgents' built-in `Google` backend) as the default, and add **OpenAI, Ollama (local), Anthropic API, and Claude Code CLI** as selectable providers. One factory seam; per-provider API keys; the settings AI tab picks the provider.

**Decisions (from the user):** Claude = **Both** (Anthropic API backend *and* a Claude Code CLI provider). Providers: Gemini (default) + OpenAI + Ollama + Anthropic + ClaudeCode. **Retire** the Interactions provider (server-side sessions + custom SSE) — the complexity the directive targets.

**Key simplifications this buys:**
- No server-side sessions → every provider ships history via a `SlidingWindowMemory` seeded from the thread transcript (the standard pattern; we already do it for the old fallback). `session_id` plumbing goes away.
- No bespoke SSE folding / `arguments_delta` reconstruction / wire translation → delete `provider/{mod,sse,wire}.rs`.
- **Non-streaming** reply (arrives complete; "Thinking…" indicator covers the wait; tool cards still stream live as tools run via the sink). Removes the delta forwarder. (Token streaming for all providers via `run_stream` is a possible later add; explicitly deferred for simplicity.)

**What stays:** `embed_texts` (semantic memory; always Gemini, independent of chat provider — moves to `oxidemx-agent/src/embed.rs`). `allowlist`, `tools`, the whole agent_runtime tool/hook/card machinery. The AutoAgents ReAct loop.

**Architecture:**
- `AiConfig.provider: AiProvider { Gemini(default) | OpenAi | Anthropic | Ollama | ClaudeCode }` replacing `backend: AiBackend`. Serde aliases `interactions`/`generate_content` → `Gemini` so existing configs migrate. `model: String` (one field; the AI tab resets it to the provider default on provider change).
- Per-provider key resolution `oxidemx_agent::keys::provider_key(provider) -> Option<String>`: env (`GEMINI_API_KEY`/`OPENAI_API_KEY`/`ANTHROPIC_API_KEY`) then `~/.config/oxidemx/{gemini,openai,anthropic}.key`. Ollama + ClaudeCode need none.
- `factory::provider_from_config(provider, model, key) -> Result<Arc<dyn LLMProvider>>`: built-in `LLMBuilder::<Google|OpenAI|Anthropic|Ollama>::new().api_key().model().build()`, or the custom `ClaudeCodeProvider`.
- `claude_code.rs`: `ClaudeCodeProvider` impl `LLMProvider`; `chat_with_tools` shells `claude -p <prompt> --output-format json [--model M]` with tools DISABLED (text responder; our agent tools don't apply on this path — documented). Honors a `CancellationToken`.

---

### Task 1 (M16): Config + keys + factory (built-ins) + retire Interactions
**Files:** `oxidemx-shared/src/config.rs`, `oxidemx-agent/{Cargo.toml, src/lib.rs, src/factory.rs, src/keys.rs(new), src/embed.rs(moved)}`, delete `oxidemx-agent/src/provider/`, `oxidemx-agent/src/bin/cli.rs`

- [x] `config.rs`: replace `AiBackend` with `AiProvider` (variants above; `#[serde(rename_all="snake_case")]`, `OpenAi`→`rename="openai"`, `ClaudeCode`→`claude_code`, Gemini gets `#[serde(alias="interactions", alias="generate_content")]`). `AiConfig.backend` → `provider`. Add `AiProvider::{label, ALL, default_model, needs_key}` helpers. Keep `model`. Update `Default`.
- [x] `oxidemx-agent/Cargo.toml`: `features = ["google","openai","anthropic","ollama"]`.
- [x] Move `provider/embed.rs` → `src/embed.rs` (drop the `provider::` path); delete `src/provider/`. lib.rs: `pub mod embed; pub mod factory; pub mod keys; pub mod claude_code;` + `pub use embed::{embed_texts, EMBED_DIM, EMBED_MODEL};` + model-default consts.
- [x] `keys.rs`: `provider_key(AiProvider) -> Option<String>` (env → key file). `key_path(provider) -> Option<PathBuf>`.
- [x] `factory.rs`: rewrite `provider_from_config(AiProvider, model, key)` over the 4 built-ins + ClaudeCode. Unit test: each non-CLI arm constructs.
- [x] `cli.rs`: `--provider <gemini|openai|anthropic|ollama|claude_code>`, key via `keys::provider_key`, default model via `AiProvider::default_model`.
- [x] `cargo test -p oxidemx-agent` + clippy clean. Commit `feat(agent): multi-provider factory (Gemini default, +OpenAI/Anthropic/Ollama); retire Interactions transport`.

### Task 2 (M17): Claude Code CLI provider
**Files:** `oxidemx-agent/src/claude_code.rs`

- [x] `ClaudeCodeProvider { model: Option<String>, cancel: CancellationToken }` impl `ChatProvider`/`CompletionProvider`/`EmbeddingProvider`/`ModelsProvider`/`LLMProvider`. `chat_with_tools`: build a single prompt from messages (system + newest user / tool results flattened), `tokio::process::Command::new("claude").args(["-p", &prompt, "--output-format","json"])` (+ `--model` if set), `select!` on cancel, parse the JSON `result` field → `ChatResponse` text, `tool_calls()=None`. Tools arg ignored (documented: Claude Code path is chat-only).
- [x] Live check (claude CLI present): a `#[tokio::test] #[ignore]` that runs a trivial prompt and asserts non-empty text.
- [x] Commit `feat(agent): Claude Code CLI provider (subprocess, chat-only)`.

### Task 3 (M18): Simplify the overlay runtime
**Files:** `overlay-rs/{Cargo.toml, src/agent_runtime.rs, src/ai_client.rs, src/app/update.rs, src/agent/memory_semantic.rs}`

- [x] `agent_runtime::run`: collapse to ONE path — `keys::provider_key` + `factory::provider_from_config`, seed `SlidingWindowMemory` from `history` (all providers ship history now), `run()` (non-streaming), return `(reply, None)`. Delete the Interactions branch, the delta forwarder, `session_id` use. Keep tools/hooks/`system_instruction_async`. `on_turn_start` still emits "Thinking…".
- [x] `ai_client.rs`: `ask_ai` drops `session_id` param (or keeps it ignored for call-site stability — prefer dropping; update call site). Model constants stay.
- [x] `update.rs`: drop `session_id` threading for the request (history already passed); `AiResponseReceived` stores `None` session (or remove the field usage). Keep flash/pro toggle but it only matters for Gemini — leave as-is.
- [x] `memory_semantic.rs`: embeddings still call `oxidemx_agent::embed_texts` with the **Gemini** key (`keys::provider_key(Gemini)`), regardless of chat provider; if absent → lexical fallback (unchanged).
- [x] Build + clippy clean. `--agent-selftest` works on Gemini. Commit `feat(overlay): single-path multi-provider runtime (history via memory, non-streaming)`.

### Task 4 (M19): Settings AI tab — provider picker + per-provider keys
**Files:** `settings-rs/src/tabs/ai.rs`, `settings-rs/src/main.rs`

- [x] Provider pick_list (`AiProvider::ALL`); on change, set `config.overlay.ai.provider` AND reset `model` to `provider.default_model()`; `touch()`.
- [x] Model picker keyed to the selected provider's suggestions + free-text.
- [x] API-key section becomes per-provider: show a write-only key field for the selected provider when `needs_key()` (writes `~/.config/oxidemx/<provider>.key`, 0600); Ollama/ClaudeCode show "no key needed". Generalize the existing `AiKey*` messages to carry the target provider/path.
- [x] Allowlist section unchanged.
- [x] Build + clippy clean. Commit `feat(settings): provider picker + per-provider API keys`.

### Task 5 (M20): Live-verify + install + close-out
- [x] `--agent-selftest` per available provider (Gemini always; OpenAI/Anthropic only if a key exists; Ollama if running; ClaudeCode via CLI). Record which passed.
- [x] Rebuild release overlay + settings, pkexec-install, restart, single-instance check.
- [x] Plan checkboxes + Learnings; spec §12 status; memory. Commit `docs(agent): P3 multi-provider results`.

---

## Learnings (filled during execution)

Executed 2026-06-13, commits through M19 + install. Workspace builds
clean, clippy clean, installed to /usr/local/bin.

**Live-verified:** Gemini (`generateContent`) with full tool-calling
(execute_command exit 0 end-to-end through the overlay path); Claude
Code CLI (chat-only, keyless, coherent answer). **Wired + unit-tested
but not live-callable here** (no key / service): OpenAI, Anthropic API,
Ollama — the factory constructs each; an actual call needs the key or a
running Ollama.

1. **Tool-calling works on the built-in Google backend** — the earlier
   "model hallucinated running a tool" scare was two red herrings: the
   `--agent-selftest` path didn't print events, and the test command
   (`ls /tmp`) wasn't allowlisted so it hit the approval channel, which
   is absent headless ("Question channel not initialized"). With an
   allowlisted command the overlay agent calls the tool correctly. The
   CLI harness (which prints `[event]`) was the key diagnostic; I added
   `OXIDEMX_AGENT_DEBUG=1` event printing to the overlay drain too.
2. **Retiring the bespoke transport was a net simplification**, exactly
   as the `building-llm-agents-in-rust` skill frames it: deleted
   `provider/{mod,sse,wire}.rs` (~600 lines of session threading + SSE
   `arguments_delta` folding), collapsed agent_runtime's two paths into
   one (factory + memory-shipped history + non-streaming run). The only
   thing kept bespoke is the Claude Code subprocess (~150 lines) and
   `embed.rs` (semantic memory, Gemini-only).
3. **Config migration via serde alias** worked: old `backend:
   "interactions"` / `"generate_content"` deserialize to `provider:
   Gemini` (field `#[serde(alias="backend")]` + variant aliases). No
   migration code, existing configs load unchanged.
4. **Claude Code CLI as a chat provider**: `claude -p <prompt>
   --output-format json --disallowed-tools <set> [--system-prompt]
   [--model]`. `--output-format json` returns a JSON ARRAY of
   transcript messages; the `type=="result"` entry holds the reply in
   `.result`. Disabling the heavy tools keeps it a fast text responder.
   Chat-only: our agent tools (execute_command/memory) don't apply on
   this path — that's the documented tradeoff (use the Anthropic API
   provider for tool-calling Claude).
5. **Per-provider keys** via a shared contract: `AiProvider::{key_env,
   key_file_stem}` in oxidemx-shared so the settings writer and
   `oxidemx_agent::keys` reader agree on env vars + `~/.config/oxidemx/
   <stem>.key` filenames. Embeddings stay Gemini-only regardless of
   chat provider (no key → lexical-only memory recall).
6. **Tradeoff accepted for simplicity: non-streaming** — the reply
   arrives complete (with a "Thinking…" indicator; tool cards still
   stream live as tools run). `StreamEvent::Delta` + the chat UI's
   partial-text scaffolding are kept (allow-dead) so token streaming
   for all providers via `run_stream` can be re-added later.
7. **Install gotcha:** `pgrep -x`/`ps -C oxidemx-overlay` can list a
   `<defunct>` zombie first; killing it is a no-op and leaves the real
   bus-owning instance (the old build) running. Kill the non-`Z` PID
   (or the busctl bus owner, parsing the PID column correctly).
