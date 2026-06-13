# Agent Framework P2: Semantic memory (embeddings + meaning-aware recall)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Implementation guidance: the `building-llm-agents-in-rust` skill (provider/memory seams). Steps use checkbox (`- [ ]`).

**Goal:** Upgrade the overlay's memory recall from purely lexical (Jaccard token-overlap) to **hybrid lexical + semantic** — so "what theme should I use?" recalls "user prefers dark mode" even with zero shared words. Real embeddings via the Gemini `text-embedding-004` endpoint (unstubbing the provider's `embed()`), a persisted embedding cache, and a blended re-rank inside the injection-block builder. Graceful fallback to lexical when embeddings are unavailable (no key / offline / timeout).

**Why this, why now:** the `building-llm-agents-in-rust` skill's memory seam is exactly the `MemoryProvider`/embedding boundary. Our Interactions provider stubs `embed()`; semantic recall needs it for real. This is the kowalski "Tier 3 semantic" idea (spec §2.1) applied to the surface that actually feeds the model — `injection_block_for`, which builds the system-prompt memory block. (The AutoAgents `MemoryProvider` slot is NOT the injection path — our Interactions provider ignores shipped history; memory reaches the model via `description()` → `system_instruction`. So semantic recall augments `injection_block_for`, not the framework memory slot.)

**Architecture:**
- **Provider seam (oxidemx-agent):** `pub async fn embed_texts(api_key, model, texts, cancel) -> Result<Vec<Vec<f32>>>` hitting `v1beta/models/text-embedding-004:embedContent` (shape verified against AutoAgents' Google backend: body `{model:"models/text-embedding-004", content:{parts:[{text}]}}`, resp `{embedding:{values:[f32;768]}}`). `GeminiInteractionsProvider::embed()` delegates to it (unstubs the trait).
- **Cache:** sidecar `~/.local/share/oxidemx/memory-embeddings.json` (`id -> Vec<f32>`) so each memory is embedded once. 768 floats/entry is too big for the main `memories.json`; keep it separate.
- **Hybrid recall (overlay agent/memory):** new `pub async fn injection_block_for_async(query)`. Lexical-prefilter to the top ~12 unpinned candidates + all pinned (bounds embedding calls), ensure their embeddings are cached (embed misses), embed the query, blend `cosine` into the existing `score()` (new weight split: relevance 0.4 lexical + 0.3 semantic + recency 0.2 + usage 0.1), build the block. Any embedding error / 3s timeout ⇒ return the existing sync `injection_block_for` (lexical) unchanged.
- agent_runtime builds the system prompt with the async path; heartbeat keeps the sync lexical path (background, no latency budget for network embeds).

**Scope boundary:** do NOT rewrite memory.rs's store, pinning, retention, or consolidation. Add a semantic submodule + one async recall entry point. Episodic-SQLite + triple-graph tiers are deferred (the flat store + semantic recall covers the recall win; journaling is a separate phase).

---

### Task 1 (M13): Real embeddings in the provider seam
**Files:** `oxidemx-agent/src/provider/embed.rs` (new), `provider/mod.rs`, `oxidemx-agent/src/lib.rs`

- [x] `embed.rs`: `pub async fn embed_texts(client, api_key, texts: &[String], cancel) -> Result<Vec<Vec<f32>>, LLMError>` — per-text POST to `…/text-embedding-004:embedContent` with `x-goog-api-key` header, `tokio::select!` on `cancel`. Request/response structs per the verified shape. Empty input ⇒ `Ok(vec![])`.
- [x] `GeminiInteractionsProvider::embed()` delegates to `embed_texts` (uses its own client/key/cancel). Re-export `embed_texts` from the crate root.
- [x] Unit test: request body shape; empty-input short-circuit. (Live dim/similarity check in Step 4.)
- [x] Live check: a tiny `--embed-selftest` is overkill; instead add a `#[tokio::test] #[ignore]` that, when run with `GEMINI_API_KEY`, embeds two related + one unrelated phrase and asserts cosine(related) > cosine(unrelated). Run it once with the key.
- [x] `cargo test -p oxidemx-agent` green; clippy clean. Commit `feat(agent): real text-embedding-004 embeddings (unstub provider embed)`.

### Task 2 (M14): Hybrid semantic recall in overlay memory
**Files:** `overlay-rs/src/agent/memory_semantic.rs` (new), `agent/memory.rs`, `agent/mod.rs`, `agent_runtime.rs`

- [x] `memory_semantic.rs`:
  - `cosine(a, b) -> f32` (+ unit test).
  - embedding cache load/save (`memory-embeddings.json`, `HashMap<String, Vec<f32>>`), path-parameterised for tests.
  - `async fn ensure_embeddings(ids_texts: &[(String,String)]) -> HashMap<String, Vec<f32>>` — load cache, embed missing (via `oxidemx_agent::embed_texts` + `load_api_key`), persist, return id→vec for the requested set. Best-effort: embed failures leave that id uncached (lexical-only for it).
  - `async fn semantic_scores(query, candidates) -> Option<HashMap<id, f32>>` — embed query + ensure candidate embeddings, return cosine per id; `None` on query-embed failure.
- [x] `memory.rs`: extract a `candidates_for(query)` helper (pinned + top-K lexical unpinned) reused by sync and async paths; add `pub async fn injection_block_for_async(query) -> Option<String>` that blends semantic into `score()` with a 3s overall timeout, falling back to `injection_block_for`. Keep `injection_block_for` (sync) intact.
- [x] `agent_runtime.rs`: build the system prompt via `mode.system_instruction_async(query).await` (new async sibling of `system_instruction` that calls `injection_block_for_async`); keep sync `system_instruction` for heartbeat.
- [x] Tests: cosine; cache round-trip; blended ranking with a stub embedder surfaces a zero-lexical-overlap-but-semantically-close entry above a lexical-only match.
- [x] `cargo build -p oxidemx-overlay` + clippy clean; `--agent-selftest "what theme should I use?"` after saving a "prefers dark mode" memory recalls it. Commit `feat(overlay): hybrid lexical+semantic memory recall`.

### Task 3 (M15): Close-out
- [x] Plan checkboxes + Learnings; brainstorm §2.1/§12 status; memory. Reinstall overlay for live test if results warrant. Commit `docs(agent): P2 semantic memory results`.

---

## Learnings (filled during execution)

Executed 2026-06-13, commits 9f… (M13) + close-out (M14). 70 overlay +
35 agent tests green, clippy clean, semantic recall live-verified.

1. **Model-name discovery (live):** `text-embedding-004` and
   `embedding-001` both 404 on this key — the available embedding
   models are `gemini-embedding-001` (stable) + `gemini-embedding-2*`,
   same post-cutoff API surface as the Interactions chat API. ListModels
   (`GET /v1beta/models`, filter `supportedGenerationMethods` for
   `embedContent`) is the way to find what a key actually exposes;
   don't trust a framework's hardcoded model name. Used
   `gemini-embedding-001` with `outputDimensionality: 768` to keep the
   cache compact (it defaults to 3072).
2. **Gemini cosines have a high, compressed baseline** (~0.45 unrelated,
   ~0.72 related at 768-dim). Blending the RAW cosine at a 0.3 weight
   was too weak — the `usage` tiebreaker on an older, more-injected
   entry overcame it, and the semantically-best memory ranked LAST.
   Two fixes together: (a) rescale `[0.40,0.85]→[0,1]` to recover
   dynamic range, (b) unify lexical+semantic as ONE relevance term via
   `max(lexical, rescaled_cos)` kept at the original dominant 0.6
   weight, rather than a separate weaker semantic weight. After this,
   the dark-mode preference ranks #1 for "what visual appearance…"
   (zero word overlap) and the codebase entry ranks #1 for "where is
   the source…" — query-dependent, as intended.
3. **The injection path, not the MemoryProvider slot, is where memory
   reaches the model** (skill-confirmed): our Interactions provider
   ignores AutoAgents' shipped history (server-side session), so memory
   enters via `description()` → `system_instruction`. Semantic recall
   therefore augments `injection_block_for`, not the framework memory
   slot. (The GenerateContent fallback DOES use the MemoryProvider slot
   — both paths now exist.)
4. **Bounded cost + hard fallback:** embed the whole store (not a
   lexical pre-filter — that would exclude the zero-overlap matches
   semantic recall is FOR), but cache by text so each fact embeds once;
   cap at 256/recall; wrap the whole thing in a 3s timeout that falls
   back to lexical. Steady state = 1 query embedding per turn.
5. **Cache keyed by memory TEXT** (not id): identical texts dedup, a
   changed text auto-invalidates, and the cache self-prunes to live
   texts on every recall. No manual invalidation on edit/consolidate.
6. **Debug entrypoints earn their keep:** `--memory-recall "<q>"`
   printed the ranked block directly, which is how the weighting bug
   was caught (the model-facing answer alone would have hidden it).
