# Phase 1 — Local LLM gateway (mistral.rs via the provider seam)

**Branch:** `phase1-local-llm-gateway` (worktree `../oxidemx-phase1`)
**Status:** implemented + verified against a live `mistralrs-server` (Qwen3-4B) on 2026-06-18.

## Goal

Let the assistant route to a **local** LLM without dropping GUI frames, and without
adding a heavy native inference dependency to the overlay binary. We run
[mistral.rs](https://github.com/ericlbuehler/mistral.rs) as a **separate local
server** exposing its OpenAI-compatible `/v1` API and point the existing OpenAI
backend at it. Heavy inference lives in its own process; the overlay just makes
HTTP calls.

### Why server, not embedded

`autoagents-mistral-rs` *does* exist (in-process `mistralrs = 0.7.0`), but embedding
it would:

- drag the CUDA/Metal/MKL native build into the GUI binary,
- run inference on iced's shared Tokio runtime (frame-drop risk — the very thing
  the non-blocking requirement targets), and
- pin us to a pre-1.0 `mistralrs` that moves fast.

The server path costs **one small code change** (a `base_url`) and isolates the
compute. Embedded remains available later behind the same factory seam if we ever
want single-binary deployment.

## What changed

All changes sit on the two seams the framework gives us — the **provider** and
**config** — exactly as the `building-llm-agents-in-rust` skill prescribes
("construct providers only at the factory seam").

| File | Change |
|---|---|
| `oxidemx-shared/src/config.rs` | New `AiProvider::MistralRs` variant (serde `mistral_rs`, alias `mistralrs`) + all `match` arms (`label`/`default_model`/`key_env`/`key_file_stem`/`model_suggestions`/`ALL`→6) and `supports_streaming_tools` (true — OpenAI-compatible SSE). New `AiConfig.local_endpoint` field (default `http://localhost:1234/v1/`). |
| `oxidemx-agent/src/factory.rs` | New `provider_from_config_with_endpoint(...)`; `provider_from_config` delegates to it with `None`. `MistralRs` arm rides `LLMBuilder::<OpenAI>` with `.base_url(normalized)`. `normalize_base_url` enforces the trailing slash. Keyless: sends the `EMPTY` placeholder (the OpenAI backend rejects an empty key with `AuthError`). |
| `overlay-rs/src/agent_runtime.rs` | `resolve_provider` returns the endpoint; the two factory call sites (`run`, `summarize`) use the endpoint-aware fn. |
| `oxidemx-agent/src/bin/cli.rs`, `oxidemx-conductor/src/bin/conductor.rs` | `mistral_rs`/`mistralrs`/`mistral-rs` added to the string→provider parsers. |
| `settings-rs/src/{main.rs,tabs/ai.rs}` | `AiLocalEndpointChanged` message + handler; a "Local server endpoint" card that appears **only** when the MistralRs provider is selected, with an inline trailing-slash hint. Provider intro updated. |

## Traps encountered (and handled)

1. **`Url::join` drops the last segment without a trailing slash.** The OpenAI
   backend does `base_url.join("chat/completions")`; `http://host/v1` →
   `http://host/chat/completions` (loses `v1`). Default and `normalize_base_url`
   both guarantee the trailing `/`. The settings field warns inline.
2. **Empty API key → `AuthError`.** `backends/openai.rs` rejects an empty key
   before any request. We pass the conventional `EMPTY` sentinel; mistral.rs
   ignores `Authorization` by default.
3. **Worktree submodules.** `git worktree add` leaves `pop_os_iced` / `libcosmic`
   (git submodules) empty. Symlinked them from the main checkout rather than
   re-cloning. (Build only reads them.)

## Verification

- `cargo check --workspace` — clean (only pre-existing `iced_gtk_themer` warnings).
- `cargo test -p oxidemx-agent --lib factory` — 4/4 pass (incl. keyless MistralRs
  construct + trailing-slash normalize).
- **Live end-to-end:** `oxidemx-agent-cli "…" --provider mistral_rs --model default`
  against `mistralrs-server` (Qwen3-4B, port 1234) returned a correct response
  through the ReAct executor.

## Config example

```jsonc
// ~/.config/oxidemx/config.json  → overlay.ai
{
  "provider": "mistral_rs",
  "model": "default",                       // or "Qwen/Qwen3-4B"
  "local_endpoint": "http://localhost:1234/v1/"
}
```

Or via Settings → AI: pick "mistral.rs (local server)", set the endpoint.

## Deliberately deferred (need later-phase backends)

These were requested but have **no backend to configure yet** — building UI for
them now would be configuring nothing:

- **Multiple simultaneous providers / primary+fallback routing** → the hybrid
  router (Phase 4). The settings model is one active provider until then.
- **Orchestrator settings** → the conductor isn't wired into overlay chat yet
  (Phase 2 session manager + later). Today it's CLI / mission-control only.
- **Streaming-with-tools robustness for small local models** → `MistralRs` is
  flagged `supports_streaming_tools = true` (the wire protocol supports it), but a
  model that can't tool-call will simply not emit tool calls. Revisit if a loaded
  model misbehaves; the flag is the single lever.

## Next (per the agreed sequencing)

Phase 2 — thin `SessionManager`/`SessionHandle` substrate (one provider instance
per conversation, per-session `CancellationToken`), behind a trait boundary so the
vendored Ractor actor runtime (`autoagents-core::runtime`, `ractor 0.15`) can slot
in as a supervision layer later without touching the iced boundary.
