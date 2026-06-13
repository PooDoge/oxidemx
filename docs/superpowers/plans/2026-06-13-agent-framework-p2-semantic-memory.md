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

- [ ] `embed.rs`: `pub async fn embed_texts(client, api_key, texts: &[String], cancel) -> Result<Vec<Vec<f32>>, LLMError>` — per-text POST to `…/text-embedding-004:embedContent` with `x-goog-api-key` header, `tokio::select!` on `cancel`. Request/response structs per the verified shape. Empty input ⇒ `Ok(vec![])`.
- [ ] `GeminiInteractionsProvider::embed()` delegates to `embed_texts` (uses its own client/key/cancel). Re-export `embed_texts` from the crate root.
- [ ] Unit test: request body shape; empty-input short-circuit. (Live dim/similarity check in Step 4.)
- [ ] Live check: a tiny `--embed-selftest` is overkill; instead add a `#[tokio::test] #[ignore]` that, when run with `GEMINI_API_KEY`, embeds two related + one unrelated phrase and asserts cosine(related) > cosine(unrelated). Run it once with the key.
- [ ] `cargo test -p oxidemx-agent` green; clippy clean. Commit `feat(agent): real text-embedding-004 embeddings (unstub provider embed)`.

### Task 2 (M14): Hybrid semantic recall in overlay memory
**Files:** `overlay-rs/src/agent/memory_semantic.rs` (new), `agent/memory.rs`, `agent/mod.rs`, `agent_runtime.rs`

- [ ] `memory_semantic.rs`:
  - `cosine(a, b) -> f32` (+ unit test).
  - embedding cache load/save (`memory-embeddings.json`, `HashMap<String, Vec<f32>>`), path-parameterised for tests.
  - `async fn ensure_embeddings(ids_texts: &[(String,String)]) -> HashMap<String, Vec<f32>>` — load cache, embed missing (via `oxidemx_agent::embed_texts` + `load_api_key`), persist, return id→vec for the requested set. Best-effort: embed failures leave that id uncached (lexical-only for it).
  - `async fn semantic_scores(query, candidates) -> Option<HashMap<id, f32>>` — embed query + ensure candidate embeddings, return cosine per id; `None` on query-embed failure.
- [ ] `memory.rs`: extract a `candidates_for(query)` helper (pinned + top-K lexical unpinned) reused by sync and async paths; add `pub async fn injection_block_for_async(query) -> Option<String>` that blends semantic into `score()` with a 3s overall timeout, falling back to `injection_block_for`. Keep `injection_block_for` (sync) intact.
- [ ] `agent_runtime.rs`: build the system prompt via `mode.system_instruction_async(query).await` (new async sibling of `system_instruction` that calls `injection_block_for_async`); keep sync `system_instruction` for heartbeat.
- [ ] Tests: cosine; cache round-trip; blended ranking with a stub embedder surfaces a zero-lexical-overlap-but-semantically-close entry above a lexical-only match.
- [ ] `cargo build -p oxidemx-overlay` + clippy clean; `--agent-selftest "what theme should I use?"` after saving a "prefers dark mode" memory recalls it. Commit `feat(overlay): hybrid lexical+semantic memory recall`.

### Task 3 (M15): Close-out
- [ ] Plan checkboxes + Learnings; brainstorm §2.1/§12 status; memory. Reinstall overlay for live test if results warrant. Commit `docs(agent): P2 semantic memory results`.

---

## Learnings (filled during execution)

- (none yet)
