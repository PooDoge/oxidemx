# Phase 2 — Session manager substrate

**Branch:** `phase1-local-llm-gateway` (worktree `../oxidemx-phase1`)
**Status:** implemented + verified (compile, 32 unit tests, live path reached the
mistral.rs model) on 2026-06-18.

## Goal

Make a chat/project **session** a first-class object that owns its isolation:
one LLM provider instance per conversation (reused across turns) and a
per-session cancellation token. This is the substrate the later phases build on
(per-session routing state for the hybrid router; a Ractor actor-per-session
supervisor) — without adopting the actor runtime now.

### Why a thin manager, not Ractor (decision)

Jim leaned Ractor; the accepted recommendation is a thin manager because:

- The isolation guarantee we need — *one provider instance per conversation,
  never shared* (the `building-llm-agents-in-rust` hard rule; server-side
  sessions interleave otherwise) — is an **ownership** property, not an actor
  one. A `HashMap<SessionId, Arc<Session>>` gives it directly.
- AutoAgents' actor runtime is `SingleThreadedRuntime` — pub/sub message
  routing, not parallelism. It buys no concurrency the overlay lacks (turns
  already run on iced's multi-thread Tokio runtime via `Task::perform`).
- Migrating the overlay off `DirectAgent` onto the actor runtime is a large
  rewrite for marginal single-user benefit.

The [`SessionStore`] trait is the seam: a Ractor-backed store can replace
`SessionManager` later without touching call sites, *if* concurrent background /
project sessions ever need supervision.

## What changed

| File | Change |
|---|---|
| `oxidemx-agent/src/session.rs` (new) | `Session` (cached provider + cancel token), `SessionManager` (`HashMap` map), `SessionStore` trait, `ProviderFingerprint`. 4 unit tests. |
| `oxidemx-agent/src/lib.rs` | `pub mod session;` |
| `overlay-rs/src/agent_runtime.rs` | Global `static SESSIONS` + `new_session_id()`. `run(...)` gained a `session_id` param: gets the session, builds the provider via `session.provider(fp)` (built once, reused), opens a per-turn token, and `select!`s the model round against it. Returns `(reply, Some(session_id))`. |
| `overlay-rs/src/ai_client.rs` | `ask_ai` threads `session_id`. |
| `overlay-rs/src/app/update.rs` | Submit mints/loads the thread's stable `session_id` (reusing the dead `ChatThread.session_id` field) and persists it via the existing `AiResponseReceived` path. STOP also fires `SESSIONS.cancel(id)`. Delete calls `SESSIONS.end(id)` before the index shifts. |
| `overlay-rs/src/main.rs`, in-file vision test | pass a fixed session id. |
| `oxidemx-shared/src/config.rs` | **`MistralRs.supports_streaming_tools()` → false** (see below). |

### Session key

The chat thread's `session_id: Option<String>` (a dead leftover from the retired
Gemini Interactions transport) is repurposed as the **stable** key — it survives
`ai_delete_thread`'s index-shifting, unlike the Vec index. Minted lazily
(`chat-{counter}`) on the first turn.

### Cancellation

`run` wraps the model round in `tokio::select! { biased; _ = cancel.cancelled()
=> Err("stopped"), res = turn => res }`. The overlay still drops the `Task` via
its abort handle (which alone cancels the in-flight request); the token is the
**canonical** path so non-UI consumers (agentd, conductor) cancel uniformly, and
it covers a turn parked on an approval `await` that dropping an HTTP request
would not unblock. `SessionManager::end` cancels before removing.

## Live finding: mistral.rs streaming → blocking

The headless selftest (`--agent-selftest` with an `XDG_CONFIG_HOME` throwaway
config pointing at mistral.rs) surfaced a real interop bug **unit/compile checks
could not**:

- **A/B (curl):** mistral.rs emits clean OpenAI SSE for streaming, with and
  without tools (note the extra `reasoning_content` field, Qwen3 reasoning).
- **Overlay streaming path:** `LLM error: JSON Parse Error: expected value at
  line 1 column 1` — the vendored OpenAI backend's shared SSE parser chokes on
  mistral.rs's framing.
- **C (curl) + blocking path:** clean JSON, correct reply.

**Decision:** flip `MistralRs.supports_streaming_tools()` to `false` so `run`
uses the blocking path (works), exactly like Ollama. This is the lever the
Phase 1 doc called out. Re-enabling streaming = a vendor-side SSE fix (like the
existing Google patch) — a separate follow-up, out of Phase 2 scope.

After the flip, the selftest reached the model with a well-formed blocking
request (`prompt_tokens: 4043`) and the server returned **500
CUDA_ERROR_OUT_OF_MEMORY** — i.e. the full session→provider→HTTP path works; the
failure was **environmental** (a second local LLM instance was running
concurrently and exhausted VRAM, per Jim — not our prompt size). Phase 1's CLI
turn returned `pong`.

### Auth: mistral.rs is unauthenticated here

The server accepted `Authorization: Bearer EMPTY` and returned completions, so it
requires no key. (The 40-hex value initially shared as an "API key" was the
Qwen3-4B HuggingFace model revision SHA from
`~/.cache/huggingface/hub/models--Qwen--Qwen3-4B/refs/main`, not an auth token —
nothing was persisted as a secret.) We nonetheless added **optional**-key support
for `MistralRs` (`key_env = MISTRALRS_API_KEY`, `key_file_stem = mistralrs`,
`needs_key = false`): an authed local/remote OpenAI-compatible server now works by
dropping a key in `~/.config/oxidemx/mistralrs.key` (or the env var) or via
Settings → AI (the panel shows it as optional). Empty ⇒ the factory's `EMPTY`
placeholder. Resolution is opportunistic in the overlay and CLI.

## Verification

- `cargo check --workspace` — clean.
- `cargo test -p oxidemx-agent --lib` — **32 pass** (incl. 4 session tests:
  session reuse by id, provider cache invalidation on fingerprint change,
  `end()` cancels + removes, `begin_turn`/`cancel`).
- Live: the `run()`→session→provider→blocking-HTTP path delivered a well-formed
  request to the live mistral.rs server (500 = server VRAM, not our code).

## Best confirmed next in a visual walk-through

- Provider reuse across turns within one thread (warm connection pool).
- STOP mid-turn (token + abort) and thread-delete dropping a live session.
- A turn against a cloud provider (Gemini) to confirm no regression in the
  common path. Suggest also testing mistral.rs with a lighter mode / smaller
  context, or more VRAM, to dodge the OOM.

## Follow-ups (logged, out of scope)

1. Vendor-side OpenAI SSE fix to re-enable mistral.rs streaming.
2. Optional: migrate conversation memory ownership into `Session` (today the UI
   still holds history and ships it each turn — deliberately unchanged to avoid
   destabilizing the working summary/history flow).
3. Ractor-backed `SessionStore` if/when concurrent background sessions land.
