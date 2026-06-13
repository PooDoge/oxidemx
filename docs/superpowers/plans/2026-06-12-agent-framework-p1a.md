# Agent Framework P1a: Provider Fallback + AI Settings Tab

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A second, fallback LLM backend (Gemini's classic generateContent API via AutoAgents' built-in `google` feature) selectable by config, and a dedicated **AI** tab in settings-rs to configure backend, model, API key, and the command allowlist.

**Architecture:** `oxidemx_agent::factory::provider_from_config(...)` returns `Arc<dyn LLMProvider>` for either backend so every consumer (CLI now; overlay/agentd later) selects by `AiConfig`. The API key UI moves from the Settings page's "AI Assistant" section into the new tab (no duplication). Config gains `overlay.ai.backend` + `overlay.ai.model` with serde defaults so existing configs load unchanged.

**Tech Stack:** autoagents `google` feature (full `LLMProvider` impl, `LLMBuilder<Google>`), iced settings patterns already in tree (`pick_list`, `state.touch()` autosave, write-only key field).

**Spec:** `docs/plans/agent-framework-integration-brainstorm.md` §12 P1; prior plan `2026-06-12-agent-framework-p0.md`.

---

### Task 1: Config schema (`oxidemx-shared`) + provider factory (`oxidemx-agent`) — MILESTONE M5

**Files:**
- Modify: `oxidemx-shared/src/config.rs` (AiConfig)
- Modify: `oxidemx-agent/Cargo.toml` (enable `google` feature; dep on oxidemx-shared)
- Create: `oxidemx-agent/src/factory.rs`
- Modify: `oxidemx-agent/src/bin/cli.rs` (`--backend` flag, config-driven defaults)

- [x] **Step 1:** Add to `AiConfig`:

```rust
/// Which Gemini transport the agent runtime uses. Interactions
/// (server-side sessions, agentic API) is the default; GenerateContent
/// is the classic stateless API kept as a fallback.
#[serde(default)]
pub backend: AiBackend,
/// Model id used by the agent runtime (overlay chat picks its own
/// per-thread model until P1b unifies on this).
#[serde(default = "default_ai_model")]
pub model: String,
```

with `#[derive(..., PartialEq, Eq)] pub enum AiBackend { #[default] Interactions, GenerateContent }` (serde `rename_all = "snake_case"`), `default_ai_model() -> "gemini-2.5-flash"`. Update `Default for AiConfig`.

- [x] **Step 2:** `oxidemx-agent/Cargo.toml`: `autoagents = { version = "=0.3.7", default-features = false, features = ["google"] }`, add `oxidemx-shared = { path = "../oxidemx-shared" }`.

- [x] **Step 3:** `factory.rs`:

```rust
pub fn provider_from_config(backend: AiBackend, model: &str, api_key: &str)
    -> Result<Arc<dyn LLMProvider>, LLMError>
{
    match backend {
        AiBackend::Interactions => Ok(GeminiInteractionsProvider::new(api_key, model)),
        AiBackend::GenerateContent => LLMBuilder::<Google>::new()
            .api_key(api_key).model(model).build(),
    }
}
```

(adjust to the actual `LLMBuilder<Google>` API verified in `AutoAgents/crates/autoagents-llm/src/backends/google.rs:725,924`). Unit test: both arms return a provider; unknown-model string passes through untouched.

- [x] **Step 4:** CLI: `--backend interactions|generate_content` (default: read `AppConfig` via oxidemx-shared, fall back to Interactions), `--model` default from config. Live smoke test BOTH backends with the same prompt; record behavior differences in Learnings.

- [x] **Step 5:** `cargo test -p oxidemx-agent` green, clippy clean, commit `feat(agent): config-selectable backend — Interactions default, generateContent fallback`.

### Task 2: Settings **AI** tab — MILESTONE M6

**Files:**
- Modify: `settings-rs/src/main.rs` (Tab enum: label/glyph/icon_name/tag/from_tag/ALL/router; new Messages + update arms)
- Create: `settings-rs/src/tabs/ai.rs`
- Modify: `settings-rs/src/tabs/settings_page.rs` (drop the "AI Assistant" section; point users at the new tab)

- [x] **Step 1:** `Tab::Ai` metadata: label "AI", glyph "A", icon `applications-science-symbolic`, tag `"ai"`, insert into `ALL` before `Settings`; router arm `Tab::Ai => tabs::ai::view(state)`.

- [x] **Step 2:** New messages + update arms (all mutate `state.config.overlay.ai.*` then `state.touch()` for the 250 ms-debounced autosave):
  - `AiBackendChanged(AiBackend)`, `AiModelChanged(String)`
  - `AiAllowlistDraftChanged(String)`, `AiAllowlistAdd`, `AiAllowlistRemove(usize)` (Add: trim, reject empty + bare `*`, dedupe)

- [x] **Step 3:** `tabs/ai.rs` view, four sections (reuse `section_block` pattern):
  1. **Backend** — pick_list Interactions (recommended) / GenerateContent (fallback) + one-line explanation of the difference.
  2. **Model** — pick_list of known ids (`gemini-2.5-flash`, `gemini-2.5-pro`) + free-text input for custom ids.
  3. **API key** — `ai_key_panel` MOVED here verbatim (same `AiKey*` messages, write-only field, 0600 file).
  4. **Command allowlist** — rows with remove buttons + add form; intro text explains whole-token prefix matching and the trailing `*` convention.

- [x] **Step 4:** Remove the "AI Assistant" `section_block` from settings_page.rs (leave a one-line pointer "AI settings moved to the AI tab").

- [x] **Step 5:** `cargo build -p settings-rs` clean; run the settings app, screenshot/verify the tab renders and edits persist to config.json (inotify reload). Commit `feat(settings): dedicated AI tab — backend, model, API key, allowlist`.

### Task 3: Close out — MILESTONE M7

- [x] Plan checkboxes + Learnings here; brainstorm doc §12 P1 status note (both copies); memory update. Commit `docs(agent): P1a results`.

---

## Learnings (filled during execution)

Executed 2026-06-12/13, commits 1617cc8 (M5) + 9bded16 (M6). 33 lib
tests green, clippy clean, settings app builds + launches clean.

1. **Both backends live-verified with the same prompt** ("check
   oxidemx-daemon.service"): Interactions and GenerateContent each
   issued one allowlisted tool call and answered correctly. The
   fallback is real, not theoretical. AutoAgents' `Google` backend
   builder is sync (`build() -> Result<Arc<Google>, LLMError>`),
   unlike our async-free constructor — the factory hides the
   difference behind `provider_from_config`.
2. The fallback provider costs ~10 lines + one feature flag because
   the factory is the only construction seam. Keep it that way: the
   overlay/agentd must construct providers ONLY through
   `oxidemx_agent::factory`.
3. Trade-offs the GenerateContent fallback accepts (documented in the
   AI tab's intro text): no server-side sessions (full history would
   need to ship — currently each ReAct turn carries only the newest
   message, so multi-turn context beyond tool results is weaker), no
   CancellationToken, no SSE delta sink. Fallback semantics only.
4. Settings package is `oxidemx-settings`, not `settings-rs` —
   `cargo build -p settings-rs` fails (dir name ≠ package name).
5. The `ai_key_panel` moved verbatim (same `AiKey*` messages) — no
   message-enum churn for the move itself; only the five new
   `AiBackend/AiModel/AiAllowlist*` variants were added. Conflict
   surface with Jim's concurrent settings work is therefore small
   and append-shaped.
6. GNOME Shell denies unprivileged `org.gnome.Shell.Screenshot` —
   no automated visual verification for settings; the AI tab needs a
   human walk-through (same status as the widget-plugins settings UI).
7. Config compatibility verified implicitly: the CLI loaded Jim's
   real config.json (which predates `backend`/`model`) through serde
   defaults during the smoke test. New fields only get written when
   the user first edits something in settings.
