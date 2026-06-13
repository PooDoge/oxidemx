# Agent Framework Integration — AutoAgents vs kowalski analysis + UI brainstorm

Date: 2026-06-12 (Part I), amended same day with Part II (Conductor orchestrator design)
Status: brainstorm / pre-design → Part II is a working spec draft
Repos analyzed: `/run/media/system/fastdrive/Games/AutoAgents` (liquidos-ai, v0.3.7) and
`/run/media/system/fastdrive/Games/kowalski` (yarenty, v1.2.0)

Contents: Part I (§1–6) framework evaluation + integration architecture.
Part II (§7–14) the Conductor orchestration layer: flow document spec, three
authoring tiers (.md / wizard / natural-language), situational triggers, event
vocabulary, revised phasing, references appendix, learnings log.

## 1. Verdict

**AutoAgents as the base framework, kowalski as the idea quarry.** The lean toward
AutoAgents is justified by the code, not just the README:

| Dimension | AutoAgents | kowalski |
|---|---|---|
| Size / tests | ~60k LoC, **904 unit tests**, clippy `-D warnings` CI | ~16k LoC, **4 integration tests** |
| Agent abstraction | Trait-layered (`AgentDeriveT` / `AgentExecutor` / `AgentHooks`) + derive macros, swappable executors (ReAct, Basic, CodeAct) | One concrete `BaseAgent` + config-driven `TemplateAgent`, ReAct loop hardcoded (5 iters, JSON-repair) |
| LLM layer | 11 backends, unified `StreamChunk` enum, per-call sampling overrides, optimization pipeline (retry/cache/rate-limit), llama.cpp + mistral-rs local inference w/ KV-cache reuse + GBNF grammar | Ollama + OpenAI-compatible only |
| Memory | **Weak**: `SlidingWindowMemory` only; `TrimStrategy::Summarize` unimplemented. But the `MemoryProvider` trait is clean and built for plugging | **Strong**: 3-tier (working / episodic SQLite / semantic in-process-vector + triple graph) all actually wired; consolidation code exists |
| Multi-agent | Typed pub/sub `Topic<M>`, ractor actors, `Environment` + `Runtime`, serializable `Event` protocol | ACL envelopes + capability registry + orchestrator + markdown "horde" pipelines — more *product-shaped*, less *library-shaped* |
| Safety | Guardrails crate (prompt-injection, PII redaction, Block/Sanitize/Audit policies), **wasmtime-sandboxed tools** | None |
| UI hooks | `Event` enum (TaskStarted, ToolCallRequested/Completed/Failed, CodeExecution*) all serde-serializable, runtime `subscribe_events()` stream — purpose-built for a live status UI | Shipping Vue dashboard (good *inspiration*, wrong stack for us) |
| Risk | Pre-1.0 (0.3.7), API churn likely → **vendor & pin** | Solo-maintainer, test-poor |

The decisive synergies for OxideMX specifically:

1. **Event protocol → UI**: AutoAgents' serializable event stream is exactly the feed a
   live multi-agent status UI needs. We forward it over D-Bus and render.
2. **wasmtime tool runtime ↔ our widget plugin system**: we already ship a WASM plugin
   pipeline (oxidemx-widget-host). Third-party *agent tools* can ride the same
   distribution/sandboxing story.
3. **Pluggable `MemoryProvider`**: the hole in AutoAgents is the exact place we install
   kowalski's memory architecture *and* our existing memory v2 store.

## 2. What to steal from kowalski

Ranked by value-for-effort:

### 2.1 Three-tier memory (HIGH — this is the main prize)
`kowalski-core/src/memory/{working,episodic,semantic}.rs`. Implement as a single
`MemoryProvider` for AutoAgents:

- **Tier 1 working**: sliding window (AutoAgents already has this — reuse).
- **Tier 2 episodic**: SQLite `episodic_kv(id, payload JSON)` — every turn journaled.
  Kowalski deliberately dropped RocksDB/Qdrant for dependency-light SQLite; that
  philosophy is *perfect* for a desktop overlay. No daemons, no vector DB.
- **Tier 3 semantic**: in-process `Vec<f32>` embeddings + cosine via std, plus a
  `HashMap<subject, Vec<(predicate, object)>>` triple graph. Embeddings via Gemini
  embedding endpoint (or local model later).
- **Migration**: our memory v2 (`agent/memory.rs` — Jaccard dedupe, 90-day retention,
  pin/inject scoring) becomes the *seed content* of Tier 3 and its dedupe/scoring rules
  carry over. The injection-block contract (`injection_block_for(query)`) stays as the
  recall surface the system prompt consumes.

### 2.2 Memory Weaver consolidation on our heartbeat (HIGH, cheap)
Kowalski's `Consolidator` (LLM-summarize episodic → semantic, LLM triple extraction,
mark-processed) exists but has **no scheduler** — their roadmap gap. We already have one:
the agent heartbeat systemd timer (`agent/heartbeat.rs`). Add "run memory consolidation"
to the heartbeat checklist. We complete their design with 50 lines.

### 2.3 Markdown horde orchestration (HIGH, very OxideMX-flavored)
`horde.md` + `agents/*.md` with TOML frontmatter = declarative, git-diffable,
user-editable multi-agent pipelines. Map onto AutoAgents Topics/actors at load time.
Then expose as a radial action: `ActionKind::AgentFlow("research.md")` — a slice on the
radial menu launches a whole pipeline. This is the killer feature for the radial UI.

### 2.4 ACL envelope + capability registry (MEDIUM)
`federation/acl.rs`: message envelope with sender/topic/provenance + **delegation depth
tracking (default max 3, hard cap 32)**. AutoAgents' typed topics carry the messages;
kowalski's envelope shape gives us provenance + runaway-delegation protection. The
capability-ranked `AgentRegistry` (exact > substring match) is the model for "which
sub-agent gets this task".

### 2.5 MCP hub pattern (MEDIUM)
Both repos have MCP clients; kowalski's `McpHub` (multi-server merge, `server::tool`
collision-namespacing, `McpToolProxy` adapting MCP tools into the native Tool trait,
config-file driven `[[mcp.servers]]`) is the cleaner consumption pattern. Gives users
a config-file path to add tools without us writing any.

### 2.6 Operator-grade error messages (LOW effort, real polish)
Kowalski's provider errors embed troubleshooting ("Is Ollama running? Did you pull the
model?"). Adopt as a convention for every agent-runtime error surfaced in chat.

### 2.7 Tool-aware streaming toggle (LOW)
Kowalski's `tools_stream: true`: suppress token streaming until tool rounds complete.
Worth offering as a chat setting — stops the "model narrates then a tool card stomps it"
jank.

### 2.8 UI patterns from their Vue dashboard (inspiration only)
Federation run timeline (per-step progress + artifact links + "open output folder"),
agent registry panel, MCP ping panel, ~2-minute operator smoke checklist. Stack is Vue —
we re-do it in iced — but the information design is proven.

## 3. Integration architecture

### 3.1 Where the framework lives: a new `agentd` process

Embed AutoAgents in a **new persistent process** (`agent-rs` / `org.oxidemx.Agent`),
not in overlay-rs:

- The overlay is ephemeral UI; multi-agent flows are long-running. Flows must survive
  the overlay closing; reopening the chat reconnects and replays status.
- ractor/tokio runtime + llama.cpp (if we ever go local) don't belong in the
  compositor-adjacent UI process.
- Supervisor already manages daemon + overlay; one more unit is routine.

```
 daemon-rs (org.oxidemx.Daemon)      hardware, config, profiles
 agent-rs  (org.oxidemx.Agent)  ←NEW AutoAgents runtime: agents, memory, tools, MCP
 overlay-rs (org.oxidemx.overlay)    chat shell + radial = thin client
 settings-rs                         + new Agents tab
 popup-rs / indicator                + agent activity badge
```

D-Bus interface sketch for `org.oxidemx.Agent`:
- Methods: `SendMessage(thread, text)`, `SpawnFlow(flow_id)`, `CancelTask(sub_id)`,
  `RespondApproval(id, verdict)`, `ListAgents()`, `ListRuns()`, `MemoryQuery(...)`
- Signals: `AgentEvent(json)` — the AutoAgents `Event` enum forwarded verbatim
  (it's already serde-serializable), plus `StreamDelta(thread, chunk)`,
  `ApprovalRequested(id, card_json)`
- Big payloads (transcripts) over a side channel (unix socket or `GetTranscript` method)
  to keep the bus light.

Fallback/MVP variant: embed AutoAgents directly in overlay-rs behind a feature flag
first (Phase 1 spike), split into agentd once flows outlive the window.

### 3.2 Custom LLM provider: Gemini Interactions

Implement `ChatProvider`/`CompletionProvider`/`EmbeddingProvider`/`ModelsProvider` +
empty `LLMProvider` for our v1beta/interactions transport. Port `ai_client/sse.rs`'s
FnCallFold (`arguments_delta` accumulation) into the provider's
`chat_stream_with_tools`, emitting AutoAgents' `StreamChunk::ToolUseStart/InputDelta/
Complete` — the shapes line up almost 1:1.

**The one real design tension**: Interactions is *server-side stateful*
(`previous_interaction_id` replays context), while AutoAgents assumes the framework
ships history from `MemoryProvider` each call. Options:

- **(a) Keep server-side sessions** — provider holds `previous_interaction_id` per
  conversation; `MemoryProvider.recall()` output is injected as system-prompt context
  only (exactly how memory v2 injection works today). Cheapest, preserves Google-side
  caching. *Recommended for main chat agent.*
- (b) Stateless mode (`store:false` per call, full history shipped) — framework-pure,
  lets executors rewrite history, costs tokens. Use for sub-agents/one-shot flow steps,
  which are short-lived anyway (we already use `store:false` for nested search).

So: hybrid. Main thread = (a), horde workers = (b).

### 3.3 Tool bridge

Wrap existing executors as `#[tool]` impls in a shared `oxidemx-agent-tools` crate:
`execute_command` (keep allowlist + compound-split + wrapper-strip logic verbatim —
~280 LoC that must not regress), `schedule_task`, `memory`, `persona`, `google_search`
(nested interaction), `get/set_menu_config`, `list_system_apps`.

**Approval flow**: tool execution in agentd blocks on a D-Bus round-trip —
`ApprovalRequested` signal → overlay renders the existing chip ("Run it / Always allow
/ Don't run") → `RespondApproval`. If no overlay is up: desktop notification + indicator
badge + queue (see §4.5). The QUESTION_TX channel pattern survives, just crosses the bus.

**Guardrails**: wrap the provider with autoagents-guardrails — prompt-injection
detection in front of a shell-executing tool is exactly the right paranoia, and Audit
mode feeds the activity log UI.

**WASM tools**: AutoAgents' wasmtime `ToolRuntime` + our widget-host pipeline →
third-party tools installable like widgets, same sandbox story, same settings page
mental model.

### 3.4 What stays homegrown

Persona (soul.md/user.md + section filtering + first-run ritual), memory v2 scoring
rules (move into the Tier-3 provider), heartbeat, chat shell morph/threads, allowlist
matcher. Net: replace ~1,000–1,500 LoC of transport/loop plumbing, add ~300–400 LoC of
glue, gain executors, sub-agents, events, guardrails, MCP, local-model future.

## 4. UI brainstorm

### 4.1 Radial overlay — live agent presence
- **Agent ring**: while flows run, active agents render as nodes orbiting the disc rim,
  color-coded by state (thinking / tool-running / awaiting-approval / done / failed).
  Driven directly off the Event stream. Click a node → focus its transcript thread.
  The existing track-animation system + ai_fx über-shader give us the states' visual
  language nearly for free (VISION_FX/HOVER hooks already exist).
- **Footer activity stack**: today's single tool-activity label becomes a collapsible
  stack — one row per in-flight agent (`name · current tool · elapsed`).
- **Slice-launched flows**: `ActionKind::AgentFlow(<flow file>)` — radial slice fires a
  horde pipeline. Long-press → flow picker.
- **Sub-agent cards in chat**: when the main agent delegates, render a card
  (like Command/Task/Memory cards) showing the sub-agent, its task, live status, and a
  "watch" affordance that opens Mission Control.

### 4.2 Mission Control (new window — the multi-agent flow monitor)
The big new surface; kowalski's FederationRunPanel is the proof-of-concept to outdo:
- **Run timeline / DAG**: pipeline steps as a graph, per-agent swim-lanes, live
  progress from TaskStarted/ToolCall*/TaskComplete events.
- **Event console**: filterable raw event feed (the Audit guardrail policy logs here too).
- **Artifacts**: files produced by flow steps, with "open folder" (kowalski's
  `open-path` trick) and "copy" actions.
- **Token/cost meters**: AutoAgents emits `Usage` stream chunks — per-agent and per-run
  token tallies, cheap to aggregate.
- **Controls**: pause/cancel run, retry failed step, re-run with edits.
- Implementation: a third iced window in overlay-rs (the multi-window plumbing exists)
  or popup-rs-style sibling. Subscribes to `AgentEvent` like any other client.

### 4.3 Settings → new "Agents" tab (`Tab::Agents` + `tabs::agents`)
- **Roster**: define agents — name, persona file, model (flash/pro), executor type,
  tool grants, memory scope, max turns. (AutoAgents `AgentBuilder` params, serialized
  into config.json → agentd inotify-reloads like the daemon does.)
- **Memory browser**: three-tier visualization; search/pin/delete (the existing tool
  actions get a GUI); "consolidate now" button; episodic journal viewer with
  promote-to-semantic.
- **Flow editor**: list `~/.config/oxidemx/flows/*.md`, edit-in-place w/ frontmatter
  form, DAG preview, "run now" + bind-to-slice.
- **Tools & MCP**: builtin tool toggles, allowlist editor (finally a GUI for
  `command_allowlist`), MCP server list with add/ping/tool-count (kowalski's McpPanel),
  WASM tool plugin installer (shared with widget plugins page).
- **Safety**: guardrail toggles (injection detection, PII redaction), approval policy
  (always-ask / allowlist / yolo-per-agent), audit log viewer.

### 4.4 Indicator + popup
Badge on the tray indicator when agents are active or an approval is pending; popup
gains a compact "running flows" list (name, step k/n, spinner) → click opens Mission
Control.

### 4.5 Approval center + haptics
Approvals outlive the overlay: queued in agentd, surfaced via desktop notification +
indicator badge; the queue renders at the top of chat on next open. And the MX4 piezo
(haptic bridge!) buzzes on approval-needed and flow-complete — agent events as physical
sensation is a genuinely novel touch only we can do.

### 4.6 Widget slices
New `WidgetSource::AgentStatus` — a radial widget wedge showing active-run count /
current step, so agent state is visible from the plain radial menu without opening chat.

## 5. Phasing

- **P0 spike (1–2 days)**: vendor AutoAgents (pin 0.3.7), implement
  `GeminiInteractionsProvider`, ReAct agent + wrapped `execute_command` in a CLI
  harness. Validates §3.2's hybrid session model before anything else.
- **P1**: embed in overlay-rs behind `agent-framework` feature flag; existing chat
  path stays as fallback. Tool bridge + approval round-trip.
- **P2**: 3-tier `MemoryProvider` (kowalski design, memory-v2 rules + seed data);
  consolidation on heartbeat.
- **P3**: split out agentd + D-Bus event bridge; sub-agents via Topics; ACL envelope +
  depth caps; Mission Control v1; Agents settings tab (roster + memory browser).
- **P4**: markdown flows + slice binding; MCP hub; guardrails; WASM tools;
  widget/indicator/haptic touches.

## 6. Risks

- **AutoAgents pre-1.0 churn** — vendor & pin; our provider/memory/tools live in our
  crates, touching only trait surfaces.
- **Interactions statefulness vs framework memory** — resolved by hybrid (§3.2); P0
  spike proves it.
- **Event enum has no timestamps** — wrap events with arrival time in agentd before
  forwarding.
- **ractor not WASM** — irrelevant; agentd is native. WASM matters only for tools
  (wasmtime side), which works.
- **kowalski code reuse is conceptual, not copy-paste** — different trait shapes;
  we re-implement their *designs* (memory tiers, ACL, hordes) on AutoAgents traits.
  Their docs (`DESIGN_MEMORY_AND_DEPENDENCIES.md`, `memory_architecture.md`) are the
  spec to crib.

---

# Part II — The Conductor: orchestration layer design (added 2026-06-12)

Ground truth established by reading AutoAgents' `examples/design_patterns/` +
core APIs line-by-line, and kowalski's `markdown_pipeline.rs` + `horde.rs` +
`agent_app_ops.rs` parsers/lifecycle verbatim. Two findings reshape the plan:

1. **AutoAgents deliberately ships NO orchestration construct.** No workflow
   engine, no DAG runner, no supervisor type. It ships primitives — typed
   `Topic<M>` pub/sub, `AgentHooks` (with two abortable gates), an `Event`
   stream — and five worked patterns in `examples/design_patterns/`
   (chaining, parallel, routing, reflection, planning). The orchestrator is
   *ours to build*, which is good: it means the product-shaped layer (kowalski's
   strength) is greenfield, not a fight against framework opinions.
2. **AutoAgents has no mid-flight task cancellation.** Only
   `HookOutcome::Abort` from `on_run_start` / `on_tool_call`, plus
   `Runtime::stop()`. This forces a specific isolation decision (§7.3).

## 7. The Conductor (`oxidemx-conductor` crate, hosted in agentd)

One orchestrator, three authoring surfaces, one canonical artifact. Everything
— wizard output, NL-composed flows, hand-written files — normalizes to a
**flow document** (§8). The Conductor compiles flow documents onto AutoAgents
primitives at run time.

### 7.1 Compilation model: FlowDoc → AutoAgents graph

Per run:

```
FlowDoc ──parse/validate──▶ FlowPlan (DAG) ──instantiate──▶
  • one SingleThreadedRuntime per run            (isolation + cancellation)
  • one ActorAgent per step, subscribed to Topic::<Task>::new("flow.{run_id}.{step_id}")
  • one Supervisor task (plain tokio) owning the run:
      subscribes runtime.subscribe_events()
      – joins: collects TaskComplete for all `needs`, merges payloads, publishes join task
        (the exact handle_events pattern from examples/design_patterns/parallel.rs:207-259)
      – retry: republish on TaskError up to step.retry.max with backoff
      – timeout: tokio::time::timeout per step → cancel path
      – routing steps: DirectAgent one-shot (routing.rs pattern), supervisor publishes
        to the chosen branch topic
      – reflect steps: bounded generator↔critic loop (reflection.rs pattern), supervisor
        counts rounds and applies accept_when
      – terminal: emits RunFinished{artifacts, handoff_markdown} / RunFailed
```

Key mappings (verified APIs):

| Conductor concept | AutoAgents primitive | Source of pattern |
|---|---|---|
| Step worker | `AgentBuilder::<_, ActorAgent>::new(x).llm().runtime(rt).subscribe(topic).memory().build()` | `examples/design_patterns/src/chaining.rs:87-108` |
| Sequential edge | `AgentHooks::on_run_complete` → `ctx.publish(Topic::<Task>::new(next), Task::new(result))` | `chaining.rs:26-31` |
| Fan-out | publish same Task to N topics | `parallel.rs:180-184` |
| Join / fan-in | event-collector watching `Event::TaskComplete` keyed by `sub_id` + `actor_name` | `parallel.rs:207-259` |
| Router | `AgentBuilder::<_, DirectAgent>` + `.run(Task)` one-shot | `routing.rs:120-129` |
| Reflection loop | two agents republishing via hooks, bounded by supervisor | `reflection.rs`, `planning.rs` |
| Approval gate | `AgentHooks::on_tool_call → HookOutcome::Abort` | `crates/autoagents-core/src/agent/hooks.rs` |
| Run cancel | per-run `Runtime::stop()` (sends `InternalEvent::Shutdown`) | `runtime/single_threaded.rs` |
| Lifecycle host | `Environment::register_runtime` / `take_event_receiver` / `shutdown` | `crates/autoagents-core/src/environment.rs` |

**Per-run runtime is the load-bearing decision.** `Environment` supports
multiple registered runtimes addressed by `RuntimeID`; a run owns its
`SingleThreadedRuntime` so (a) `stop()` cancels exactly that run, (b) topic
names can't collide across concurrent runs, (c) the event receiver is already
run-scoped for the UI. This also fixes kowalski's documented "federation topic
is global per horde; no per-run topic isolation" limitation — our topics embed
`run_id`.

### 7.2 Approval gating via hooks (better than tool-wrapping)

The Conductor injects a shared `ApprovalGate` (holds the D-Bus question
channel + per-agent policy) into every step agent's `on_tool_call`:

```rust
async fn on_tool_call(&self, call: &ToolCall, _ctx: &Context) -> HookOutcome {
    match self.gate.check(&self.agent_id, call).await {   // blocks on D-Bus round-trip
        Verdict::Allow => HookOutcome::Continue,
        Verdict::Deny(reason) => { self.gate.record_denial(call, reason); HookOutcome::Abort }
    }
}
```

This keeps the existing allowlist matcher (`agent/commands.rs` compound-split +
wrapper-strip logic) as the `check()` fast path, generalized from
"shell commands" to "any tool call" with per-tool policies
(`always | allowlist | autonomous`). `on_tool_start/result/error` hooks feed
the audit log for free.

### 7.3 Cancellation & pause design (filling the framework gap)

- **Cancel run** = supervisor aborts + `runtime.stop()` on the run's runtime.
  In-flight LLM HTTP call: provider holds a per-run `CancellationToken`;
  our `GeminiInteractionsProvider` selects on it around the reqwest future
  (this is why the provider must be ours — third-party backends wouldn't
  honor it).
- **Cancel step / skip** = supervisor stops routing that step's outputs;
  `on_run_start` checks the token → `HookOutcome::Abort` for not-yet-started.
- **Pause** = ApprovalGate flips to "queue everything"; agents block at the
  next `on_tool_call`. Cheap, coarse, good enough for v1; true suspend needs
  upstream support (candidate for an AutoAgents PR — see §14).

### 7.4 Sub-agent guardrails (kowalski ACL learnings)

Every Conductor-spawned task carries a provenance envelope (kowalski
`federation/acl.rs` shape): `run_id`, `parent_step`, `delegation_depth`
(default max 3, hard cap 32), `originator` (user | flow | conductor-nl |
trigger). The NL conductor's `spawn_subagent` tool decrements depth; at 0 it
must return rather than delegate. Guardrails crate wraps providers for
injection/PII; `Audit` policy events land in the run's event log.

## 8. Flow document spec v0 (`~/.config/oxidemx/flows/<id>/flow.md`)

A deliberate **superset of kowalski's horde format** (TOML frontmatter in
`---` fences, markdown body) fixing each of their documented limitations:
sequential-only → DAG via `needs`; no retry → per-step `retry`; no
parallelism → free from DAG (ready steps run concurrently); no conditionals →
`kind = "route"`; body-is-dead-weight → body is the NL conductor's discovery
text (§9.3).

Directory layout (kowalski convention, kept):

```
flows/research-digest/
  flow.md            # manifest (below)
  agents/*.md        # optional flow-local agent defs (roster refs preferred)
  prompts/*.md       # per-step prompt templates
```

```toml
---
[flow]
id = "research-digest"                  # required, unique
name = "Research Digest"                # optional, defaults to id
description = "Ingest a URL, digest it, answer the user's question."
version = 1                             # spec version for forward-compat

[inputs]                                # typed run inputs, templated as {{input.*}}
url = { type = "string", required = true }
question = { type = "string", default = "Summarize the key changes." }

[defaults]                              # per-run defaults, overridable per step
model = "gemini-2.5-flash"              # or "gemini-2.5-pro"
executor = "react"                      # basic | react | codeact
approval = "allowlist"                  # always | allowlist | autonomous
memory = "run"                          # none | run (episodic-scoped to run) | shared
max_turns = 10

[[step]]
id = "ingest"
agent = "web-researcher"                # roster id (§9.1) or "./agents/ingest.md"
needs = []                              # DAG edges; [] = entry point
task = "Fetch and normalize {{input.url}} into markdown."
output = "debug/raw.md"                 # artifact path under run workdir
timeout_secs = 300
retry = { max = 2, backoff_secs = 10 }

[[step]]
id = "digest"
agent = "summarizer"
needs = ["ingest"]                      # receives ingest artifact as @artifact@
task = "Digest the source. Question to keep in mind: {{input.question}}"
context = ["@artifact@"]                # kowalski tokens kept: @artifact@, @step:<id>@
output = "debug/digest.md"
normalize = { title = "Source Digest", sections = ["Summary", "Claims", "Sources"] }

[[step]]
id = "stress-test"
kind = "reflect"                        # generator↔critic loop
target = "digest"                       # step whose output is critiqued+revised
critic = "skeptic"                      # roster agent
max_rounds = 2
accept_when = "no_blocking_findings"

[[step]]
id = "answer"
agent = "writer"
needs = ["digest", "stress-test"]       # 2+ needs ⇒ join (supervisor merges payloads)
context = ["@step:digest@", "@step:stress-test@"]
output = "ANSWER.md"

[[step]]                                # routing example (optional branch)
id = "disposition"
kind = "route"
needs = ["answer"]
choices = { archive = "archive-note", notify = "send-notification" }
prompt = "Decide: is this worth notifying the user immediately?"

[triggers]                              # situational orchestration (§10)
slice = true                            # may be bound to ActionKind::AgentFlow
schedule = []                           # e.g. ["daily 09:00"] → heartbeat-dispatched
on_dbus = []                            # e.g. ["GamingModeChanged:false"]

[delivery]                              # kowalski delivery block, kept verbatim
root = "ANSWER.md"
title = "Research digest"
note = "Final answer is ANSWER.md; intermediates under debug/."
---

# Research Digest

Use this flow when the user wants a source fetched, digested and interrogated.
Good for: release notes, changelogs, papers. Not for: multi-source synthesis
(use `research-sweep` instead).
```

**Body contract** (improvement over kowalski, where the body is unparsed):
the markdown body is the flow's *self-description for the NL conductor* —
embedded into Tier-3 semantic memory so `find_flow` retrieval works on it.

Normalization rules: adopt kowalski's *fixed* semantics
(`markdown_pipeline.rs:160-184` post-1.2.0): never substring-match headings
(emoji-heading false positives), never inject H1 above YAML frontmatter,
fallback text only into explicitly listed fallback sections.

Agent roster files (`~/.config/oxidemx/agents/<id>.md`) follow the same shape:
frontmatter = `id, name, persona_file | inline persona, model, executor,
tools = [...], approval, memory_scope, capabilities = [...]`; body = the
agent's system-prompt addendum. Capabilities feed a kowalski-style ranked
registry (exact > substring) used by `route`-steps and the NL conductor's
delegation tool.

Validation: `oxidemx-agent flow validate <id>` (and the same engine behind the
wizard + NL composer): DAG acyclicity, every `needs` resolves, every agent ref
resolves, every `@step:` token references an ancestor, input templating
closed, tool grants exist. Modeled on kowalski's `agent-app validate`
(`kowalski-cli/src/agent_app_ops.rs:127-167`) plus DAG checks they don't need.

## 9. Three authoring tiers (one artifact)

### 9.1 Tier A — Files first
Flow + roster .md files are the source of truth: git-diffable, shareable,
hand-editable. We ship a starter pack (`research-digest`, `system-doctor`,
`inbox-triage`) the way kowalski ships `knowledge-compiler` — each one is also
the living documentation of the format.

### 9.2 Tier B — Wizard (settings-rs `Tab::Agents → Flows`)
The wizard is **an editor of flow.md, not a separate store**. Round-trip
guarantee: open file → form/canvas → save file (preserve unknown fields +
body). Fine controls per step: agent picker (roster), model override,
executor, approval policy, retry/timeout, normalize sections editor, context
token picker (lists ancestor steps). DAG canvas renders the `needs` graph
live with validation errors inline; "dry run" button executes the plan with a
mock provider and renders the supervisor's scheduling order. Long-tail fields
stay editable in a raw-TOML drawer so the wizard never blocks hand-authoring.

### 9.3 Tier C — Natural-language + situational conductor
A persistent meta-agent ("Conductor") — itself just a ReAct `ActorAgent` with
a privileged toolset:

| Tool | Behavior |
|---|---|
| `list_agents()` / `list_flows()` / `list_tools()` | roster + flow registry with capability metadata |
| `find_flow(query)` | semantic search over flow *bodies* (Tier-3 memory embeddings) |
| `run_flow(id, inputs)` | validate → start run → returns run_id; chat card links Mission Control |
| `compose_flow(spec)` | emits a full flow.md draft → engine-validates → renders DAG preview as an **approval card** in chat; on accept, saved to `flows/drafts/<id>/` and runnable; "promote" moves it out of drafts |
| `spawn_subagent(agent, task)` | ad-hoc one-shot delegation (no flow file) — DirectAgent run with depth-capped envelope (§7.4) |
| `cancel_run(run_id)` / `run_status(run_id)` | supervisor controls |

Two operating modes:
- **Conversational**: "every morning grab the AutoAgents changelog, digest it,
  and ping me if the LLM trait changed" → Conductor calls `compose_flow` (a
  3-step flow with a `schedule` trigger) → user approves the DAG card → done.
  Composition is *teaching the system*, not just executing.
- **Situational**: the trigger engine (§10) wakes the Conductor with an event
  payload; the Conductor decides (cheap flash call): ignore / run matching
  flow / surface a suggestion chip on the radial ("Gaming session ended —
  run highlights-clipper?"). Suggestion chips, never silent actions, for
  anything not pre-approved via a trigger binding.

Safety asymmetry by tier: Tier A/B flows run with their declared approval
policies; Tier C *composition* always requires explicit user approval of the
DAG card before a draft can run; Tier C *ad-hoc* spawns inherit
`approval = "always"` unless the user has promoted the pattern.

## 10. Situational trigger engine (agentd)

Sources, all existing infrastructure:
- **D-Bus signals** from `org.oxidemx.Daemon`: GamingModeChanged,
  DeviceStateChanged, MacroPlaybackStarted/Stopped, ActionExecuted… —
  agentd already subscribes for its own needs; triggers pattern-match
  `signal:args`.
- **Heartbeat** (`agent/heartbeat.rs` systemd timer): `schedule` trigger
  cron-lite strings dispatch at tick; also runs memory consolidation (§2.2).
- **Radial slices**: `ActionKind::AgentFlow(flow_id)` → daemon `ExecuteAction`
  → agentd `SpawnFlow`.
- **Idle/charging context** (battery state via daemon): cheap conditions for
  deferring heavy flows ("run when charging").

Trigger bindings live in flow.md `[triggers]` (auditable, diffable). The
trigger engine only ever *starts flows or wakes the Conductor* — it contains
no LLM logic itself, keeping the hot path deterministic.

## 11. Event vocabulary & UI data contract

Two layers, both forwarded over `org.oxidemx.Agent` as `AgentEvent(json)`
with agentd-stamped `ts` and `run_id` (AutoAgents events carry no timestamps —
verified):

- **Agent layer** (AutoAgents verbatim): TaskStarted, ToolCallRequested/
  Completed/Failed, TaskComplete, TaskError, CodeExecution*, Usage stream
  chunks (token meters).
- **Run layer** (Conductor-emitted, kowalski vocabulary adopted —
  `kowalski/src/horde.rs:453-776` is the reference implementation):
  `run_started{pipeline}`, `task_assigned{step,to}`, `task_started`,
  `agent_message` (progress prose), `task_finished{success,artifact,summary}`,
  `run_finished{artifacts[], handoff_markdown}`, `run_failed{reason,step}`,
  plus ours: `approval_requested{card}`, `run_cancelled`, `step_retrying{n}`.

UI contract notes carried over from FederationRunPanel.vue: inline handoff
markdown in the terminal event **capped at 48KB** with truncation note (big
artifacts fetched by path); per-step rows keyed on `step` with
status/artifact/summary; delivery title/note sourced from flow.md
`[delivery]`. Mission Control (§4.2) renders exactly this contract; the
chat sub-agent card (§4.1) renders the run-layer events filtered to one run.

## 12. Revised phasing (supersedes §5)

- **P0 spike — DONE 2026-06-12** (branch `agent-framework`, commits
  8e9a8a8→d7aa85f; plan + learnings in `docs/superpowers/plans/
  2026-06-12-agent-framework-p0.md`): autoagents 0.3.7 pinned from
  crates.io, `GeminiInteractionsProvider` with CancellationToken on both
  paths, allowlist-bridged `execute_command`, ReAct CLI harness. Live
  smoke tests passed (happy + denial paths, session threading). Key
  discovery: blocking responses omit `status` on pending function_call
  steps — overlay's `== "waiting"` filter is a latent bug to fix.
  Deferred to P1: StreamChunk mapping (text-delta sink shipped instead),
  multi-call input-array live verification.
- **P1a — DONE 2026-06-13** (commits 1617cc8, 9bded16): config-selectable
  backend (`overlay.ai.backend`: Interactions default / GenerateContent
  fallback via AutoAgents' `google` feature; `factory::provider_from_config`
  is the single construction seam, both backends live-verified) + dedicated
  settings **AI tab** (backend/model pickers, API-key panel moved from the
  Settings page, command-allowlist GUI editor). Plan + learnings:
  `docs/superpowers/plans/2026-06-12-agent-framework-p1a.md`.
- **P1b — DONE 2026-06-13**: the overlay chat now runs on the AutoAgents
  runtime (`overlay-rs/src/agent_runtime.rs`), not the hand-rolled
  `ask_ai` loop — straight replacement, no feature flag. One `OverlayTool`
  delegates to the untouched `execute_local_tool` (all tools/cards/approval
  preserved); `description()` carries persona + memory injection; the
  provider streams SSE deltas while the executor stays non-streaming;
  session threaded via seed/read. Allowlist unified (overlay re-exports
  `oxidemx_agent::allowlist`). Headless `--agent-selftest` verified 4/4
  tool paths live (reply+session, google_search, execute_command, memory).
  Interactive UI walk-through human-gated (not installed over Jim's live
  binary). Plan: `docs/superpowers/plans/2026-06-13-agent-framework-p1b-overlay-replacement.md`.
- **P2 — DONE 2026-06-13** (semantic memory): unstubbed the provider's
  `embed()` with `gemini-embedding-001` (768-dim; classic text-embedding-004
  404s on this key); overlay memory recall is now hybrid lexical+semantic —
  `injection_block_for_async` embeds query + saved memories (cached,
  text-keyed), blends cosine as `relevance = max(lexical, rescaled_cosine)`
  at the dominant 0.6 weight, 3s timeout + lexical fallback. Live-verified
  zero-word-overlap recall (dark-mode preference ranks #1 for "what visual
  appearance…"). INSTALLED to /usr/local/bin for live testing. Plan:
  `docs/superpowers/plans/2026-06-13-agent-framework-p2-semantic-memory.md`.
- **P3 — DONE 2026-06-13** (multi-provider + reduce complexity, per user):
  **retired the bespoke Gemini Interactions transport** (deleted
  provider/{mod,sse,wire}.rs); normal Gemini `generateContent` is the new
  default. AiConfig `backend`→`provider` (serde-migrated): Gemini / OpenAI /
  Anthropic / Ollama / **Claude Code (CLI)**. One factory seam over
  AutoAgents built-ins + a custom subprocess `ClaudeCodeProvider`
  (`claude -p … --output-format json`, chat-only). Per-provider keys
  (`~/.config/oxidemx/<p>.key` + env). agent_runtime collapsed to one path
  (history via memory, **non-streaming**). Settings AI tab: provider picker +
  per-provider key fields. Live-verified Gemini (tools) + Claude Code (chat);
  OpenAI/Anthropic/Ollama wired+unit-tested (need keys/service). INSTALLED.
  Plan: `docs/superpowers/plans/2026-06-13-agent-framework-p3-multiprovider.md`.
- **P1 (overlay-embedded MVP)** — tool bridge + `on_tool_call` ApprovalGate;
  main chat on the framework behind `agent-framework` flag. *Exit: feature
  parity with today's chat.*
- **P2 (memory)** — 3-tier MemoryProvider; consolidation on heartbeat. *Exit:
  memory browser data API exists.*
- **P3 (Conductor core)** — FlowDoc parser/validator, supervisor (joins,
  retry, timeout, cancel via per-run runtimes), starter flows, CLI
  `flow validate|run|status`. **This is the new center of gravity.** *Exit:
  research-digest runs headless end-to-end with events on stdout.*
- **P4 (agentd split + UI)** — `org.oxidemx.Agent` D-Bus surface, event
  forwarding, Mission Control v1, Agents settings tab (roster + flows list +
  memory browser), chat sub-agent cards.
- **P5 (authoring surfaces)** — wizard/DAG canvas (Tier B), NL Conductor with
  compose_flow + approval cards (Tier C), trigger engine, slice binding.
- **P6 (ecosystem)** — MCP hub config, WASM tool plugins, guardrails wrap,
  indicator/haptics/widget touches.

## 13. References appendix

### AutoAgents (`/run/media/system/fastdrive/Games/AutoAgents`, pin v0.3.7)
- Repo: https://github.com/liquidos-ai/AutoAgents · API docs: https://docs.rs/autoagents
- Orchestration patterns (the Conductor's recipe book):
  `examples/design_patterns/src/{chaining,parallel,routing,reflection,planning}.rs`
- Builder: `crates/autoagents-core/src/agent/builder.rs` ·
  Hooks (incl. the two `HookOutcome::Abort` gates): `crates/autoagents-core/src/agent/hooks.rs`
- Environment: `crates/autoagents-core/src/environment.rs` ·
  Runtime: `crates/autoagents-core/src/runtime/{mod,single_threaded}.rs`
- Topics/messaging: `crates/autoagents-core/src/actor/{topic,messaging}.rs`
- Memory trait: `crates/autoagents-core/src/agent/memory/mod.rs` ·
  ReAct executor: `crates/autoagents-core/src/agent/prebuilt/executor/react.rs`
- LLM traits: `crates/autoagents-llm/src/chat/mod.rs` (ChatProvider :449),
  `crates/autoagents-llm/src/builder.rs` · Event enum: `crates/autoagents-protocol/src/protocol.rs`
- Guardrails: `crates/autoagents-guardrails/` · WASM tool runtime:
  `crates/autoagents-core/src/tool/runtime/wasm.rs` · MCP: `examples/mcp/src/main.rs`
- Docs site source: `docs/content/core-concepts/{architecture,actor_agents,advanced_patterns,executors,memory,tools}.md`

### kowalski (`/run/media/system/fastdrive/Games/kowalski`, v1.2.0)
- Repo: https://github.com/yarenty/kowalski
- Markdown pipeline parser (flow.md spec ancestor): `kowalski-core/src/markdown_pipeline.rs`
  (manifest meta :14-21, stage meta :25-47, frontmatter parse :81-112,
  context tokens :114-141, normalization :160-184)
- Horde run lifecycle + event vocabulary: `kowalski/src/horde.rs:453-776`
- Worker lifecycle: `kowalski-cli/src/agent_app_ops.rs:561-669` ·
  validate: :127-167
- Federation/ACL (envelope, depth caps, `handoff_markdown` serde alias):
  `kowalski-core/src/federation/{acl,broker,registry,orchestrator}.rs`
- Memory tiers: `kowalski-core/src/memory/{working,episodic,semantic,consolidation}.rs`
- Memory design docs: `docs/DESIGN_MEMORY_AND_DEPENDENCIES.md`, `memory_architecture.md`
- Reference UI: `ui/src/panels/FederationRunPanel.vue` (run timeline contract),
  `ui/src/api.ts` (endpoint map)
- Worked example (format reference): `examples/knowledge-compiler/{horde.md,agents/*.md}`

### OxideMX (this repo)
- Interactions transport + SSE fold: `overlay-rs/src/ai_client.rs`,
  `overlay-rs/src/ai_client/{sse,tools}.rs`
- Allowlist matcher (survives as ApprovalGate fast path): `overlay-rs/src/agent/commands.rs`
- Memory v2 / persona / heartbeat / tasks: `overlay-rs/src/agent/{memory,persona,heartbeat,tasks}.rs`
- Chat shell + threads: `overlay-rs/src/chat_shell.rs`, `overlay-rs/src/radial/chat_threads.rs`
- D-Bus surfaces: `daemon/src/dbus/interface.rs`, `overlay-rs/src/dbus.rs`
- Config: `oxidemx-shared/src/config.rs` (AiConfig) · Settings tabs: `settings-rs/src/main.rs`
- Prior plans: `docs/plans/agent-features-implementation.md`,
  `docs/plans/agent-feature-roadmap.md`,
  `docs/superpowers/specs/2026-06-10-ai-chat-arc-shell-morph-design.md`

## 14. Learnings log (accumulated during this analysis)

1. **AutoAgents = primitives only.** Verified: no workflow/DAG/supervisor
   construct anywhere in crates or docs. `design_patterns` examples are the
   sanctioned way. Consequence: Conductor is greenfield, not a wrapper fight.
2. **No mid-flight cancellation upstream.** Only `on_run_start`/`on_tool_call`
   can abort; `Runtime::stop()` is the hammer. Consequence: per-run runtimes
   (§7.1) + CancellationToken inside our own provider (§7.3). Candidate
   upstream PR: cooperative cancellation checked at turn boundaries.
3. **AutoAgents events carry no timestamps.** agentd must stamp on arrival.
4. **`on_tool_call → Abort` is the native approval seam** — gate at the hook,
   not by wrapping tools; audit comes free from `on_tool_start/result/error`.
5. **Joins are userland.** `parallel.rs`'s `handle_events` collector
   (`results: HashMap` keyed by `actor_name`, gate on `expected_keys`) is the
   canonical join; our supervisor generalizes it with `sub_id` scoping.
6. **Kowalski's pipeline is deliberately limited** (sequential, no retry, no
   conditionals, global topic per horde) — each limitation is an explicit
   line item our flow spec fixes (§8).
7. **Heading normalization must not substring-match** — kowalski 1.2.0 fixed
   false positives on emoji headings (`## 📝 TL;DR`); never inject H1 above
   YAML frontmatter (Obsidian compat). Adopt their fixed semantics.
8. **Schema evolution pattern**: rename wire fields with
   `#[serde(alias = "old_name")]` (their `paste_for_obsidian` →
   `handoff_markdown`). Adopt for all run-layer events from day one
   (`version` field in flow.md serves the same purpose).
9. **Long-lived SSE clients need request timeouts disabled** (kowalski 1.2.0
   federation worker fix) — applies to our Interactions SSE client too.
10. **Cap inline payloads in terminal events at 48KB** with truncation note;
    fetch big artifacts by path.
11. **Interactions API statefulness** (Part I §3.2): hybrid model — server-side
    sessions for the main thread, `store:false` stateless for flow workers,
    which are short-lived and need reproducible context anyway.
12. **Markdown body should earn its keep**: kowalski parses only frontmatter;
    we feed the body to semantic memory so NL flow-discovery works (§8, §9.3).

## 15. Federator deep-dive: mechanics to adopt, gaps to design around
(added 2026-06-12 after line-level read of `kowalski-core/src/federation/` +
`kowalski/src/http_api.rs` worker management + both federation Vue panels)

### 15.1 Mechanics worth adopting directly

1. **Envelope-ID dedup cache at the broker boundary**
   (`broker.rs:58-96`): every envelope carries a UUID; the broker keeps a
   2048-entry FIFO of recent IDs and silently drops repeats. This is what
   makes their local-mpsc ↔ Postgres-NOTIFY echo loop safe. We will have the
   same echo topology (agentd internal bus ↔ D-Bus signals ↔ multiple UI
   subscribers, possibly external workers republishing) — adopt verbatim:
   dedup by event UUID in the agentd event bridge, FIFO-capped.
2. **Depth enforcement at the publish seam, not in agents**
   (`acl.rs:145-170`, called from `orchestrator.rs:41-44`):
   `check_delegate_depth()` validates every envelope at publish time —
   soft cap (3) overridable per message, hard cap (32) non-negotiable.
   Agents *cannot* bypass it because they don't own the publish path. Our
   Conductor supervisor owns all task publishing (§7.1), so the same
   centralized check drops in naturally.
3. **Capability ranking algorithm** (`registry.rs:71-95`): exact
   case-insensitive match scores 10 000; substring match scores by capability
   length (longer = more specific wins); ties broken lexicographically by
   agent id. Deterministic, explainable, 20 lines. Adopt for `route` steps,
   `spawn_subagent`, and the NL Conductor's agent picker — determinism
   matters because the wizard can *show* why an agent was chosen.
4. **Compound readiness predicate + pre-flight loop**
   (FederationManagementPanel.vue): READY = process running ∧ registered with
   exact capability ∧ no stale registration; hordes show an `X/Y READY`
   badge; `ensureHordeReady()` retries 8× with 650 ms backoff before a run.
   Translate to our in-process world as **flow pre-flight**: before
   `run_flow`, verify per step — agent roster entry resolves, granted tools
   exist, MCP servers ping, provider key valid, (external worker alive if
   applicable). Mission Control renders the same X/Y READY gate; the run
   button stays disabled with per-step reasons until green. This is the
   single best UX idea in their federation layer.
5. **Stale-registration sweep on (re)start** (`http_api.rs:1115-1126`):
   before spawning a replacement worker, deregister its stale registry entry.
   Generalizes to: any registration surface needs a crash-recovery sweep at
   the *next* lifecycle event, not just a periodic GC.
6. **`last_exit` bookkeeping** (`http_api.rs:61-62`): managed process map
   (`HashMap<id, Child>`) plus a parallel map of human-readable exit reasons,
   surfaced in the UI card. Trivial and exactly what a "why is this step NOT
   READY" tooltip needs. Also adopt for run post-mortems.
7. **Heartbeat touch + stale marking** (`persist.rs:175-196, 239-256`):
   activity touches `agent_state.updated_at`; a cleanup endpoint marks
   agents inactive past a threshold (UI default 300 s). Needed only for
   *external* workers in our model (in-process agents can't go silently
   stale) — but required there.

### 15.2 The big idea: external worker registration → agentd as a local agent hub

Kowalski's worker loop proves a minimal viable contract for agents that
live *outside* the orchestrator process: `register(id, capabilities)` →
subscribe to a task stream → receive `TaskDelegate` filtered by `to_agent` →
do work → publish `task_finished{success, artifact}`. Their workers are
`kowalski-cli` subprocesses, but nothing in the contract requires that.

For OxideMX this unlocks a new capability tier: **flow steps satisfiable by
external processes**. A Claude Code session, a browser-harness script, a
containerized tool, or another app entirely registers with agentd as a
capability provider; the Conductor's scheduler treats it as just another
step backend (`agent = "external:video-transcoder"`). Concretely:

- Registration + task delivery over the **session D-Bus bus** (methods
  `RegisterWorker(id, capabilities[])` / signal-per-worker or a returned
  unix-socket pair for task delivery) — peer credentials from the bus give
  us the auth kowalski lacks (§15.3.4) for free.
- The managed-worker pattern (§15.1.5/6) covers agentd-spawned helpers;
  self-registered externals use heartbeat + stale sweep (§15.1.7).
- Flow spec addition: step `agent` accepts `external:<capability>`, and
  pre-flight (§15.1.4) treats "no live worker for capability" as NOT READY.

Defer to P6, but reserve the spec surface now (one enum variant, one
frontmatter convention) so flow files don't churn later.

### 15.3 Anti-patterns to design around (their gaps, our requirements)

1. **Fire-and-forget delegation, watchdog in the wrong layer**: orchestrator
   never times out a delegated task — the *Vue UI* implements a 30 s
   watchdog (FederationPanel.vue:213-222). Tasks hang forever server-side if
   a worker dies mid-task. Our supervisor already owns per-step
   `timeout_secs` + retry (§7.1) — this confirms it belongs there, never in
   the UI.
2. **No reconnect on the bridge** (`pg_broker.rs:42-82`): PgListener failure
   logs and exits the task permanently; operator restarts the server. Any
   bridge we run (D-Bus reconnect after bus restart, external worker
   sockets) must reconnect with backoff as a base requirement.
3. **No backpressure strategy**: broker fanout `send().await`s into each
   subscriber's bounded channel — one slow subscriber stalls fanout for all.
   For UI event fanout use `try_send` + drop-oldest (UIs want freshest
   state); only the *supervisor's* own event consumption may block, since
   it's correctness-bearing.
4. **No auth, permissive CORS** on every federation endpoint; world-open if
   bound to 0.0.0.0 (their ROADMAP defers "auth on register/deregister").
   Our rule: no TCP listener at all — session D-Bus + unix sockets only,
   identity from peer credentials. Cross-machine federation is explicitly
   out of scope until there's a real auth story.
5. **Registration is silent overwrite** (`registry.rs:30-37` HashMap insert):
   re-registering an id replaces capabilities with no conflict signal.
   We should detect id-collision-with-different-capabilities and surface a
   warning event (likely a crashed-and-restarted worker — fine — but the UI
   should say so).
6. **Hard payload ceilings need a by-reference fallback**: NOTIFY caps at
   ~8 KB so they clamp to 7 500 bytes; combined with the 48 KB handoff cap
   (§14.10) the rule generalizes: bus messages carry summaries + paths,
   never artifacts. D-Bus has the same character (signals should stay small).
7. **First-match-only selection**: ranked list, then always `candidates
   .first()` — no load awareness. Fine for v1 (we adopt it), but our
   registry record should carry `busy: bool` from day one so the scheduler
   can skip occupied externals without a schema change later.
