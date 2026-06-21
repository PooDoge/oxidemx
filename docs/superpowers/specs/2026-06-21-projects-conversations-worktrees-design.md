# Projects · Conversations · Worktrees — Design (Sub-project 1a)

**Status:** Approved for planning (2026-06-21).
**Branch:** new branch off `phase1-local-llm-gateway` (S1 merged at `ca21ed7`).
**Program context:** First sub-project of the **AI-chat rewrite program**, decomposed as:
1. **Projects · Conversations · Worktrees model** (this doc = phase **1a**; phase **1b** = HTTP/SSE+Tailscale transport, own spec).
2. **Freya chat application** (desktop + Android, own spec).
3. **Overlay chat decommission** (own spec).
Related memory: [[project_connector_architecture]], [[project_flow_delivery_s1]], [[feedback_best_practices_rule]].

**Goal:** Make agentd the single source of truth for an explicit **Project** + **Conversation** model with a `.oxide` per-directory config system (walk-up inheritance) and opt-in **worktrees**, exposed through a connector-agnostic core — so the dual-store thread desync disappears and the model is ready for a cross-platform (desktop + Android) client over a remote transport (1b).

**Architecture:** A connector-agnostic core service (`AgentService`) owns a Project registry, a per-project Conversation index (metadata) + transcripts, a `.oxide` walk-up config resolver, and worktree mechanics. Connectors call it: the existing D-Bus connector now (1a); the HTTP/SSE connector next (1b). Clients hold no authoritative conversation state.

**Tech Stack:** Rust, agentd (host-side build), `zbus` (`features=["tokio"]`), `serde`/JSON on disk, `git worktree`, `tokio`. (axum + Tailscale land in 1b.)

---

## Global Constraints (bind every task)

- **Rule 0 — field-standard naming.** Use **`conversation`** (not "thread") for the user-facing entity and new types/methods; the existing D-Bus `thread` parameter name stays on the wire for back-compat but maps to `conversation_id`. Use **`.oxide`** for the in-repo config dir (rename from `.oxidemx`, back-compat read). Core is **connector-agnostic** ([[project_connector_architecture]]: Gateway/Connector, agent-protocol shape).
- **Rule 1 — truthfulness is structural.** Conversation/run/project state asserted to a client comes from the authoritative store (registry/index/transcript/run.json), never a client cache.
- **Rule 2 — Rust quality bar.** clippy-clean, hand-formatted (no repo-wide `cargo fmt`), `?` over unwrap in non-test code, newtypes over primitive obsession (`ProjectId`, `ConversationId`), `thiserror` errors, trait seams for mocks, no gold-plating.
- **Rule 3 — builds + process.** agentd builds **host-side** (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target`, rustup). Isolate work in a git worktree off `phase1-local-llm-gateway` (Jim edits the main checkout concurrently). Commit each task. Brainstorm→spec→plan→subagent-driven build.

---

## Background — current state (from the codebase map)

- **Dual store (the bug):** the overlay keeps its own `~/.config/oxidemx/ai-chats.json` (conversation metadata: title/model/summary/tokens, pruned to 30) **separate** from agentd's per-conversation transcripts (`~/.local/share/oxidemx/projects/<key>/transcripts/<id>.jsonl`). They drift → thread-list desync. agentd stores **only transcripts**, not conversation metadata.
- **agentd already has the bones:** `ProjectKey`/`ProjectPaths` ([projects.rs]), `TranscriptStore` + unused `list_threads()` ([sessions.rs]), `merged_skill_roots`/`merged_mcp`/`project_config` (a partial `.oxidemx` resolver), `AgentService` ([interface.rs]) behind the D-Bus connector.
- **Single global project today:** the overlay always sends project = `~/.config/oxidemx` ([ai_client.rs] `agentd_project()`); no per-working-dir projects, no per-conversation working dir.

This sub-project **evolves** that — it does not start over.

---

## Entity model

Newtypes: `ProjectId(String)`, `ConversationId(String)` (Rule 2).

### Project
```
Project {
  id: ProjectId,                 // ulid/uuid; "personal" reserved for the default
  name: String,
  default_working_dir: PathBuf,  // "" for Personal (no specific repo)
  key: ProjectKey,               // existing path-hash, derived from default_working_dir
  created_at: u64,
}
```
- A built-in **"Personal"** project (`id = "personal"`, no working dir) holds project-less/assistant chats (today's behavior).
- **Registry:** `~/.local/share/oxidemx/projects.json` — `Vec<Project>` (authoritative list).
- **Init-from-conversation:** promote a Personal conversation into a new Project (name + default_working_dir); the conversation's `project_id` is reassigned.

### Conversation
```
Conversation {
  id: ConversationId,
  project_id: ProjectId,
  title: String,                 // first prompt, truncated
  working_dir: PathBuf,          // defaults to project.default_working_dir
  model: String,
  summary: String, summary_upto: usize,
  tokens_prompt: u64, tokens_completion: u64,
  created_at: u64, updated_at: u64,
  worktree: Option<Worktree>,    // Some when this conversation runs in a dedicated worktree
}
Worktree { path: PathBuf, branch: String, base_ref: String }
```
- **Per-project conversation index:** `~/.local/share/oxidemx/projects/<key>/conversations.json` — `Vec<Conversation>` (the `sessions-index.json` equivalent; the metadata agentd does NOT store today).
- **Transcript:** unchanged append-only `~/.local/share/oxidemx/projects/<key>/transcripts/<id>.jsonl` (`TranscriptTurn`).

### Source of truth
agentd owns the registry + per-project index + transcripts. The core service reads/writes them; clients re-query. The overlay's `ai-chats.json` is **not** used by the new path (the overlay stays on it as a throwaway cache until decommissioned in sub-project 3 — it remains functional via the back-compat shim below).

---

## Disk layout

```
~/.local/share/oxidemx/
├── projects.json                         # Project registry (authoritative)
└── projects/<project-key>/
    ├── conversations.json                # Conversation index (metadata)
    ├── transcripts/<conversation-id>.jsonl
    ├── runs/<run-id>/…                    # (existing, conductor runs)
    ├── journal.jsonl  learned/  meta.json # (existing)
~/.config/oxidemx/                         # user-global config (lowest config layer)
<working-dir>/.oxide/                       # per-directory project config (committed)
```

---

## `.oxide` folder + config inheritance

`.oxide/` (committed to the repo, like `.claude/`):
```
.oxide/
├── settings.toml      # model, permissions {allow,deny}, defaults, optional `root = true`
├── agents/  skills/  commands/  flows/     # project-scoped definitions
├── mcp.toml                                # MCP servers
├── hooks/             # hook scripts (referenced from settings; convention)
├── OXIDE.md           # instruction file (the CLAUDE.md equivalent)
└── worktrees/         # gitignored; created worktrees live here
```

**Resolver (walk-up + merge):**
1. From the conversation's `working_dir`, walk **up** to filesystem root, collecting every `.oxide/` found; **stop** if a `.oxide/settings.toml` sets `root = true` (EditorConfig sentinel).
2. Layer **user-global** `~/.config/oxidemx` underneath all of them.
3. Apply **farthest-first → closest-wins**. Merge semantics:
   - **scalars** (model, defaults): closest wins.
   - **list/set-like** (permissions allow/deny, skills, agents, commands, flows, mcp servers): **union** across layers; on key/id collision the closest wins.
4. `OXIDE.md` instruction files: concatenate root-first (closest last), like Claude Code's CLAUDE.md walk-up.

Evolves agentd's `merged_skill_roots`/`merged_mcp`/`project_config` into one `ResolvedConfig` produced by a pure resolver (mock-testable). Precedence + merge rules documented in `OXIDE.md` docs (sub-project W-docs later).

---

## Working-dir binding + worktrees

- The agent's **cwd per turn = conversation.working_dir**; `.oxide` resolves from it. Setting/changing the working dir re-resolves config.
- **Opt-in conversation worktree** ("branch this conversation"): `git worktree add .oxide/worktrees/<name>` on branch `worktree-<name>`, `base_ref` ∈ {`head`,`fresh`} (config default `head`); set `conversation.working_dir` to the worktree path; record `conversation.worktree`. `.oxideinclude` (gitignore-syntax) copies selected gitignored files into the worktree (like `.worktreeinclude`).
- **Subagent auto-isolation:** the multi-agent framework dispatches coding subagents each in their own worktree (existing pattern), auto-removed if unchanged.
- **Cleanup:** an unchanged worktree (no diff, no untracked, no new commits) is auto-removed; a changed one prompts/keeps. `.oxide/worktrees/` is gitignored.

(Worktree mechanics live in a small `worktree` module with a trait seam so the git calls are mockable.)

---

## Connector-agnostic core + D-Bus contract (1a scope)

The core is `AgentService` (existing), extended with the Project/Conversation/Worktree/Config API — **transport-agnostic**. The **D-Bus connector** (existing `AgentInterface`) delegates to it (1a). The **HTTP/SSE connector** (1b) will delegate to the *same* methods.

New/extended core methods (each exposed on the D-Bus connector in 1a):
- Projects: `list_projects()`, `create_project(name, default_working_dir)`, `init_project_from_conversation(conversation_id, name, default_working_dir)`.
- Conversations: `list_conversations(project_id)`, `get_conversation(conversation_id)`, `create_conversation(project_id, working_dir?)`, `rename_conversation`, `delete_conversation`, `set_conversation_working_dir(conversation_id, dir)`.
- Worktrees: `create_worktree_for_conversation(conversation_id, name?, base_ref?)`, `remove_worktree(conversation_id)`.
- Config: `resolve_config(working_dir) -> ResolvedConfig` (debug/introspection).
- Messaging: `send_message` stays, now keyed by `conversation_id` (the existing `thread` param), recording user+assistant turns to the transcript AND updating the conversation index (title/updated_at/tokens).

**Back-compat shim:** the existing `send_message(project="~/.config/oxidemx", thread=<session_id>)` call maps to the **Personal** project + a conversation whose `id = <session_id>`; first contact auto-creates the conversation index entry. The overlay therefore keeps working unchanged on 1a.

---

## Migration

- On first run of the new agentd: build `projects.json` with the **Personal** project; for each existing transcript under the overlay's project key, create a `conversations.json` entry (title from first turn, timestamps from file/mtime).
- Read `.oxidemx/` if `.oxide/` is absent (back-compat); new writes use `.oxide/`.
- No destructive migration of `ai-chats.json` (the overlay keeps reading it until decommissioned).

## Out of scope (other specs)

- **1b:** the `HttpConnector` (axum HTTP/SSE agent-protocol), Tailscale-bind, bearer auth, SSE `Last-Event-ID` streaming. (Prerequisite for the Freya app.)
- **Sub-project 2:** the Freya desktop+Android UI.
- **Sub-project 3:** overlay chat decommission.
- PassKeys (deferred — `.well-known` public-fetch blocker), `tailscale-rs` embedding (experimental).

## Testing

- **Pure/mock:** the `.oxide` walk-up resolver (scalars closest-wins; lists union; `root=true` stop; user-global underlay); ProjectRegistry + ConversationIndex CRUD + JSON round-trip; back-compat shim mapping (`~/.config/oxidemx`+session_id → Personal+conversation); worktree module against a mocked git seam (path/branch/base_ref, `.oxideinclude` copy, unchanged-cleanup).
- **Live-wire:** D-Bus methods end-to-end (create project → create conversation → send_message records transcript + index → list shows it); migration builds Personal + indexes existing transcripts; `send_message` back-compat still works for the unchanged overlay.
- conductor/flows untouched; existing agentd tests stay green.
