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

- [ ] `config.rs`: replace `AiBackend` with `AiProvider` (variants above; `#[serde(rename_all="snake_case")]`, `OpenAi`→`rename="openai"`, `ClaudeCode`→`claude_code`, Gemini gets `#[serde(alias="interactions", alias="generate_content")]`). `AiConfig.backend` → `provider`. Add `AiProvider::{label, ALL, default_model, needs_key}` helpers. Keep `model`. Update `Default`.
- [ ] `oxidemx-agent/Cargo.toml`: `features = ["google","openai","anthropic","ollama"]`.
- [ ] Move `provider/embed.rs` → `src/embed.rs` (drop the `provider::` path); delete `src/provider/`. lib.rs: `pub mod embed; pub mod factory; pub mod keys; pub mod claude_code;` + `pub use embed::{embed_texts, EMBED_DIM, EMBED_MODEL};` + model-default consts.
- [ ] `keys.rs`: `provider_key(AiProvider) -> Option<String>` (env → key file). `key_path(provider) -> Option<PathBuf>`.
- [ ] `factory.rs`: rewrite `provider_from_config(AiProvider, model, key)` over the 4 built-ins + ClaudeCode. Unit test: each non-CLI arm constructs.
- [ ] `cli.rs`: `--provider <gemini|openai|anthropic|ollama|claude_code>`, key via `keys::provider_key`, default model via `AiProvider::default_model`.
- [ ] `cargo test -p oxidemx-agent` + clippy clean. Commit `feat(agent): multi-provider factory (Gemini default, +OpenAI/Anthropic/Ollama); retire Interactions transport`.

### Task 2 (M17): Claude Code CLI provider
**Files:** `oxidemx-agent/src/claude_code.rs`

- [ ] `ClaudeCodeProvider { model: Option<String>, cancel: CancellationToken }` impl `ChatProvider`/`CompletionProvider`/`EmbeddingProvider`/`ModelsProvider`/`LLMProvider`. `chat_with_tools`: build a single prompt from messages (system + newest user / tool results flattened), `tokio::process::Command::new("claude").args(["-p", &prompt, "--output-format","json"])` (+ `--model` if set), `select!` on cancel, parse the JSON `result` field → `ChatResponse` text, `tool_calls()=None`. Tools arg ignored (documented: Claude Code path is chat-only).
- [ ] Live check (claude CLI present): a `#[tokio::test] #[ignore]` that runs a trivial prompt and asserts non-empty text.
- [ ] Commit `feat(agent): Claude Code CLI provider (subprocess, chat-only)`.

### Task 3 (M18): Simplify the overlay runtime
**Files:** `overlay-rs/{Cargo.toml, src/agent_runtime.rs, src/ai_client.rs, src/app/update.rs, src/agent/memory_semantic.rs}`

- [ ] `agent_runtime::run`: collapse to ONE path — `keys::provider_key` + `factory::provider_from_config`, seed `SlidingWindowMemory` from `history` (all providers ship history now), `run()` (non-streaming), return `(reply, None)`. Delete the Interactions branch, the delta forwarder, `session_id` use. Keep tools/hooks/`system_instruction_async`. `on_turn_start` still emits "Thinking…".
- [ ] `ai_client.rs`: `ask_ai` drops `session_id` param (or keeps it ignored for call-site stability — prefer dropping; update call site). Model constants stay.
- [ ] `update.rs`: drop `session_id` threading for the request (history already passed); `AiResponseReceived` stores `None` session (or remove the field usage). Keep flash/pro toggle but it only matters for Gemini — leave as-is.
- [ ] `memory_semantic.rs`: embeddings still call `oxidemx_agent::embed_texts` with the **Gemini** key (`keys::provider_key(Gemini)`), regardless of chat provider; if absent → lexical fallback (unchanged).
- [ ] Build + clippy clean. `--agent-selftest` works on Gemini. Commit `feat(overlay): single-path multi-provider runtime (history via memory, non-streaming)`.

### Task 4 (M19): Settings AI tab — provider picker + per-provider keys
**Files:** `settings-rs/src/tabs/ai.rs`, `settings-rs/src/main.rs`

- [ ] Provider pick_list (`AiProvider::ALL`); on change, set `config.overlay.ai.provider` AND reset `model` to `provider.default_model()`; `touch()`.
- [ ] Model picker keyed to the selected provider's suggestions + free-text.
- [ ] API-key section becomes per-provider: show a write-only key field for the selected provider when `needs_key()` (writes `~/.config/oxidemx/<provider>.key`, 0600); Ollama/ClaudeCode show "no key needed". Generalize the existing `AiKey*` messages to carry the target provider/path.
- [ ] Allowlist section unchanged.
- [ ] Build + clippy clean. Commit `feat(settings): provider picker + per-provider API keys`.

### Task 5 (M20): Live-verify + install + close-out
- [ ] `--agent-selftest` per available provider (Gemini always; OpenAI/Anthropic only if a key exists; Ollama if running; ClaudeCode via CLI). Record which passed.
- [ ] Rebuild release overlay + settings, pkexec-install, restart, single-instance check.
- [ ] Plan checkboxes + Learnings; spec §12 status; memory. Commit `docs(agent): P3 multi-provider results`.

---

## Learnings (filled during execution)

- (none yet)
