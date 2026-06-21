# OxideMX — engineering standards (binding)

OxideMX is a coding-first AI agent framework (Rust): `agentd` (the Gateway) + an iced/GTK4
overlay, on a vendored AutoAgents core. These rules bind **every system we build**. Keep
this file high-signal — add a rule only when it changes what code gets written.

## Rule 0 — Follow current best practices + field naming conventions, everywhere

In every system we build, follow **current Rust best practices and idioms** AND **current
AI-agentic-framework best practices and naming conventions**. When you introduce a
type/trait/module/concept, name it the way the field names it — don't invent a local term
when a field-standard one exists. The convention sources we've researched live in
`docs/research/` and are authoritative for naming:

- **Agent architecture naming** (`docs/research/{hermes-agent-architecture,connector-architecture}.md`):
  `agentd` = the **Gateway/host**; per-channel front-ends are **`Connector`s**
  (`OverlayConnector`/`TelegramConnector`/`HttpConnector`); normalized `InboundEvent` /
  `OutboundMessage` / `ConnectorCaps`; HTTP surface follows the agent-protocol
  (conversations + messages + SSE) shape. Approvals: decision in core
  (`ApprovalClassifier`/`GatedToolExecutor`), rendering per-connector (`ApproverPrompt`).
- **Agent behavior / prompts** (`docs/research/claude-code-system-prompt-patterns.md`):
  enforce truthfulness **structurally, not by exhortation**.

**Anything touching the AI/agent framework** (agents, tools, flows, schemas, events, the
provider seam, prompts) must use names + shapes that match how the field's well-built
projects name them — so the code reads natively to LLMs and future contributors. When a
term isn't already fixed in `docs/research/`, compare against quality references before
naming, and prefer their field-standard terms over a local invention:
- **Microsoft AgentSchema** — https://github.com/microsoft/AgentSchema (declarative agent /
  tool / action schema shapes).
- **Nous Research Hermes-Agent** — https://github.com/nousresearch/hermes-agent (agent loop +
  skills naming).
- **Claude Code source** — https://github.com/chauncygu/collection-claude-code-source-code/tree/main/claude-code-source-code
  (tool, message, and harness naming + prompt patterns).

If a new area has no researched convention, find the field-standard term before coding;
if none exists, pick the clearest and note why.

## Rule 1 — Truthfulness is structural (grounded narration)

The model is never the authority on system-known state. Assert verifiable state
(run/task/file/test/command status or result) **only from a tool result in the current
turn**; otherwise call the tool or abstain. Narrate **actions**, never **unobserved
outcomes** ("looking is not acting"). Surface ground truth in the UI where a connector
has one; back it with a guard. (Full design: `docs/superpowers/specs/2026-06-20-truthful-visible-runs-design.md`.)

## Rule 2 — Rust quality bar

- **Formatting:** the workspace has NO `rustfmt.toml` and is hand-formatted; do NOT run
  repo-wide `cargo fmt` (it reformats unrelated files into a huge noise diff). Match the
  surrounding style of the file you edit; only format the lines you add.
- `cargo clippy` clean (treat warnings as defects). Prefer idiomatic Rust: borrow over
  clone, `?` over unwrap in non-test code, newtypes over primitive obsession, `thiserror`
  for error enums, trait **seams** (`Arc<dyn Trait>`) for anything that needs a mock in tests.
- **No gold-plating** (Claude Code rule): don't add features, abstractions, or refactors
  beyond the task. Bug fixes stand alone. Three similar lines beat a premature abstraction.
- TDD where it pays: mock-test pure/seam logic; compile-wire + live-test the
  config-built live paths (provider factories, D-Bus handlers) that can't be injected.

## Rule 3 — Process

- Brainstorm → spec → plan → subagent-driven build for any non-trivial slice (superpowers).
- Atomic-Fedora host; **never** `rpm-ostree install`. Builds: **agentd + oxidemx-agent-core
  + oxidemx-conductor build host-side** with the rustup toolchain (no `-devel` libs needed)
  — use a dedicated `CARGO_TARGET_DIR=/tmp/oxidemx-host-target` so host builds never clobber
  the distrobox-built `target/`. **Only the overlay** (GTK4/glib/libadwaita `-devel`) needs
  the `claude_development` distrobox. Never mix host + distrobox cargo over the *same*
  `target/` (toolchain mismatch → full recompile churn). Isolate parallel implementation in
  git worktrees (the user edits the main checkout concurrently).
- Keep slices focused + connector-agnostic in the core; only the connector layer is
  channel-specific.
