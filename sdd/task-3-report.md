# Task 3 Report — Remaining native tools + host-delegated dispatch (SP1c)

**Status:** DONE — 59/59 tests green (all existing + 12 new), clippy -D warnings clean, mistralrs absent.

## TDD sequence

| Step | Result |
|------|--------|
| Wrote `agent.rs` with all tests first (tests reference `execute` on `AgentToolExecutor`); wired dispatch in `mod.rs` | Compile errors — RED confirmed |
| Fixed `use oxidemx_agent_core::tool::ToolExecutor` in test module; removed unused imports | 59/59 green — GREEN |

## New tests (12) — all pass

```
tools::agent::tests::ask_multiple_choice_delegates_to_host   ← verbatim brief test
tools::agent::tests::ask_multiple_choice_missing_question_errors
tools::agent::tests::memory_save_then_list_round_trip
tools::agent::tests::memory_unknown_action_errors
tools::agent::tests::use_skill_reads_from_project_skills_dir
tools::agent::tests::use_skill_missing_name_errors
tools::agent::tests::schedule_task_list_returns_json_array
tools::agent::tests::schedule_task_unknown_action_errors
tools::agent::tests::persona_read_returns_both_files
tools::agent::tests::persona_unknown_action_errors
tools::agent::tests::compose_flow_empty_id_is_ok_not_panic
tools::agent::tests::run_flow_stub_returns_delegation_message
tools::tests::ask_multiple_choice_delegates_to_host   ← same verbatim test at mod level
```

## Per-tool: arg keys + source

| Tool | Declared arg keys (mode.rs) | Source |
|------|-----------------------------|--------|
| `use_skill` | `name` (+ tolerant `skill_name` fallback) | mode.rs L190–199 |
| `compose_flow` | `flow_id`, `flow_md` | mode.rs L122–138 |
| `run_flow` | `flow_id`, `inputs_json` (opt), `mock` (opt) | mode.rs L202–222 |
| `schedule_task` | `action`, `name`, `on_calendar`, `command`, `unit` | mode.rs L240–269 |
| `memory` | `action`, `text`, `scope`, `id`; `query`/`text` fallback for search | mode.rs L270–297 + oracle ~L708–713 |
| `persona` | `action`, `content` | mode.rs L298–317 |
| `ask_multiple_choice_question` | `question`, `options` | mode.rs L517–536 |
| `get_menu_config` / `set_menu_config` | passthrough | oracle ~L80; host-delegated |

## Core helpers reused vs ported

All six tools reuse `oxidemx-agent-core` public functions directly — zero oracle code ported:

- `memory` → `core::memory::{save_entry, load_all, search, delete, set_pinned, consolidation_input}`
- `persona` → `core::persona::{soul_path, user_path, write_file}`
- `schedule_task` → `core::tasks::{create, set_enabled, run_now, delete, list}`
- `use_skill` → `core::skills::{enabled_set, read_body, global_skill_roots}` + `ProjectPaths::merged_skill_roots()`
- `compose_flow` / `run_flow` → no core helpers (flows are a conductor concern); thin oracle port

## Project-scoping status

| Tool | Scoping |
|------|---------|
| `use_skill` | FULLY project-scoped — walks `paths.merged_skill_roots()` (global + `.claude/skills` + `.oxidemx/skills`) |
| `memory` | GLOBAL (followup) — core targets `~/.local/share/oxidemx/memories.json` |
| `persona` | GLOBAL (followup) — core targets `~/.config/oxidemx/{soul,user}.md` |
| `schedule_task` | GLOBAL by nature — systemd user timers are machine-wide |
| `compose_flow` / `run_flow` | GLOBAL — flows in `~/.config/oxidemx/flows/` |

Per-project memory/persona scoping is documented as a followup in followups.md.

## `run_flow` / `compose_flow` handling

- **`compose_flow`** — writes `flow.md` then spawns `oxidemx-conductor validate <id>`. Returns validation result. Thin, identical to oracle.
- **`run_flow`** — **thin stub**. Returns a typed "delegated to AgentService.run_flow (Task 4 pending)" message. Task 4 replaces the stub body. No supervisor logic duplicated. Marked clearly with `// Task 4 TODO:` comment.

## Host-delegation shape

```rust
// HostCapability::invoke dispatches capability name + args, returns JSON or AgentdError.
async fn invoke(&self, cap: &str, args: Value) -> Result<Value, AgentdError>;
```

`ask_multiple_choice_question`, `get_menu_config`, `set_menu_config` all call `host.invoke(cap, args)`. Clean "host unavailable" `Err(String)` if host returns error.

## `RecordingHost` (`#[cfg(test)]`)

In `agent::test_support`. Records capability names in a `Mutex<Vec<String>>`; returns a `canned_reply: Value`. Exposed `pub(super)` so `mod::tests` can access it. Implements `HostCapability` with the async_trait macro.

## Files changed

| File | Action |
|------|--------|
| `agentd/src/tools/agent.rs` | New — 6 native tools + host-delegated dispatch + `RecordingHost` test support |
| `agentd/src/tools/mod.rs` | Extended dispatch table; `pub(crate)` fields; added `agent` module; verbatim brief test |

## mistralrs
`cargo tree -p agentd | grep -i mistralrs` → empty.

## Concerns / followups
1. Per-project memory/persona — use global core fn for now; follow-up tracked.
2. `run_flow` stub — Task 4 must replace the stub body; comment is clear.
3. `memory consolidate` — degrades gracefully ("requires LLM call") since the Gemini consolidation call from the oracle can't run in agentd without the AI client wired.
4. `get_menu_config` / `set_menu_config` added to host-delegated dispatch (not in brief but logical: they're host-provided capabilities).
