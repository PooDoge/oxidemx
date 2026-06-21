# Projects · Conversations · Worktrees — Implementation Plan (Sub-project 1a)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to
> implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make agentd the single source of truth for an explicit Project + Conversation model with a
`.oxide` walk-up config resolver and opt-in worktrees, behind a connector-agnostic core (D-Bus now).

**Architecture:** New pure modules (project store, conversation index, `.oxide` resolver, worktree
seam) own the data; `AgentService` gains methods that use them; the D-Bus `AgentInterface` exposes
them; `send_message` upserts the conversation index (back-compat). Clients hold no authoritative
state.

**Tech Stack:** Rust, agentd (host-side), `zbus 5` (`tokio`,`blocking-api`), `serde`/`serde_json`,
`toml 0.8`, `git worktree`, `tokio` (fs). Dev: `tempfile 3`.

## Global Constraints

- **Naming:** new user-facing entity = **`conversation`** (not "thread"); the D-Bus `thread` param
  name stays on the wire but maps to `conversation_id`. In-repo config dir = **`.oxide`** (rename
  from `.oxidemx`, back-compat READ of `.oxidemx`). Newtypes `ProjectId`, `ConversationId`. Core is
  connector-agnostic; the existing `ProjectRegistry` (a cwd→`ProjectPaths` resolver, NOT a project
  list) stays as-is — the persisted named-project list is a NEW type `ProjectStore`.
- **Rule 2:** clippy-clean, hand-formatted (only added lines), `?`/`expect` over unwrap in non-test
  code, `thiserror`, trait seams for git, no gold-plating, no new crates (mint ids without
  uuid/ulid).
- **Rule 3:** build host-side `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd`
  (package name is **`agentd`**, not `oxidemx-agentd`); clippy `-p agentd`. Work in a git worktree
  off `phase1-local-llm-gateway`. Commit each task; `git add` only changed files; never touch
  Cargo.lock (revert if dirtied).

**Worktree setup (once):**
```bash
cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1
git worktree add -b s1a-projects-conversations ../oxidemx-s1a phase1-local-llm-gateway
```
(agentd builds host-side — no pop_os_iced/libcosmic symlinks needed; do NOT `git submodule update`.)

---

## File map

| File | Change |
|------|--------|
| `agentd/src/model.rs` (new) | `ProjectId`, `ConversationId`, `Project`, `Conversation`, `Worktree` (serde types) — Task 1 |
| `agentd/src/project_store.rs` (new) | persisted `projects.json` registry of named Projects + Personal default — Task 2 |
| `agentd/src/conversations_index.rs` (new) | per-project `conversations.json` metadata index — Task 3 |
| `agentd/src/oxide_config.rs` (new) | `.oxide` walk-up resolver → `ResolvedConfig` — Task 4 |
| `agentd/src/worktree.rs` (new) | git worktree seam + create/remove/.oxideinclude — Task 5 |
| `agentd/src/projects.rs` | `.oxidemx`→`.oxide` rename (back-compat read) — Task 4 |
| `agentd/src/interface.rs` | `AgentService` new methods + `send_message` index upsert + migration — Tasks 6,7,9 |
| `agentd/src/interface.rs` | D-Bus `AgentInterface` new methods — Task 8 |
| `agentd/src/lib.rs` / `main.rs` | module declarations + wiring — Tasks 1,6 |

---

## Task 1: Model types

**Files:** Create `agentd/src/model.rs`; modify `agentd/src/lib.rs` (add `pub mod model;`).
**Interfaces — Produces:** `ProjectId(String)`, `ConversationId(String)` (newtypes, `Display`,
`From<&str>`/`From<String>`, serde transparent); structs below.

- [ ] **Step 1: Failing test** in `model.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_and_conversation_roundtrip() {
        let p = Project { id: ProjectId::from("personal"), name: "Personal".into(),
            default_working_dir: std::path::PathBuf::new(), created_at: 1 };
        let j = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id.as_str(), "personal");
        let c = Conversation { id: ConversationId::from("chat-1"),
            project_id: ProjectId::from("personal"), title: "hi".into(),
            working_dir: std::path::PathBuf::from("/tmp"), model: "gemini-2.5-flash".into(),
            summary: String::new(), summary_upto: 0, tokens_prompt: 0, tokens_completion: 0,
            created_at: 1, updated_at: 2, worktree: None };
        let back: Conversation = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.id.as_str(), "chat-1");
        assert!(back.worktree.is_none());
    }
}
```
- [ ] **Step 2: Run, expect FAIL** (types undefined):
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd project_and_conversation_roundtrip`
- [ ] **Step 3: Implement** `model.rs`:
```rust
//! Project / Conversation / Worktree value types — the agentd-owned model
//! (single source of truth; clients are thin readers).
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name { pub fn as_str(&self) -> &str { &self.0 } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
        }
        impl From<&str> for $name { fn from(s: &str) -> Self { Self(s.to_string()) } }
        impl From<String> for $name { fn from(s: String) -> Self { Self(s) } }
    };
}
id_newtype!(ProjectId);
id_newtype!(ConversationId);

pub const PERSONAL_PROJECT_ID: &str = "personal";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    /// Empty for the Personal project (no specific repo).
    pub default_working_dir: PathBuf,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub project_id: ProjectId,
    pub title: String,
    pub working_dir: PathBuf,
    pub model: String,
    #[serde(default)] pub summary: String,
    #[serde(default)] pub summary_upto: usize,
    #[serde(default)] pub tokens_prompt: u64,
    #[serde(default)] pub tokens_completion: u64,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Worktree>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Worktree { pub path: PathBuf, pub branch: String, pub base_ref: String }
```
- [ ] **Step 4: Run, expect PASS.** Add `pub mod model;` to `agentd/src/lib.rs` (next to the other `pub mod`).
- [ ] **Step 5: Commit** `git add agentd/src/model.rs agentd/src/lib.rs` → `feat(agentd): Project/Conversation/Worktree model types`.

---

## Task 2: ProjectStore (projects.json registry)

**Files:** Create `agentd/src/project_store.rs`; `agentd/src/lib.rs` (`pub mod project_store;`).
**Interfaces — Consumes:** Task 1 types. **Produces:**
`ProjectStore::new(store_base: PathBuf)`; `list() -> Vec<Project>`; `ensure_personal() -> Project`;
`create(name: &str, default_working_dir: PathBuf) -> Project`; `get(&ProjectId) -> Option<Project>`;
`save(&[Project])`. Persists `<store_base>/projects.json`. Mint ids: Personal = `"personal"`; others
= `format!("proj-{}", now_ms())` (monotonic enough; collisions impossible within a process tick —
if `get` finds a collision, append a counter).

- [ ] **Step 1: Failing test**:
```rust
#[test]
fn ensure_personal_then_create_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(tmp.path().to_path_buf());
    let personal = store.ensure_personal();
    assert_eq!(personal.id.as_str(), "personal");
    let p = store.create("My Repo", std::path::PathBuf::from("/home/x/repo"));
    assert_ne!(p.id.as_str(), "personal");
    // reload from disk → both present
    let store2 = ProjectStore::new(tmp.path().to_path_buf());
    let ids: Vec<String> = store2.list().iter().map(|p| p.id.to_string()).collect();
    assert!(ids.contains(&"personal".to_string()));
    assert!(ids.iter().any(|i| i.starts_with("proj-")));
}
```
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** `project_store.rs` — load `projects.json` (`Vec<Project>`, empty if
  absent), `ensure_personal` inserts+saves the Personal project if missing, `create` appends+saves,
  `get`/`list` read. Use `now_ms()` (copy the helper from interface.rs or `std::time`). Atomic-ish
  write: serialize pretty → write to `projects.json.tmp` → rename. All fs errors logged + non-fatal
  except `create`/`ensure` which return the in-memory value regardless. `now_ms()` helper:
```rust
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64).unwrap_or(0)
}
```
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(agentd): persisted ProjectStore (projects.json + Personal default)`.

---

## Task 3: ConversationIndex (conversations.json per project)

**Files:** Create `agentd/src/conversations_index.rs`; `lib.rs` (`pub mod conversations_index;`).
**Interfaces — Produces:** `ConversationIndex::new(project_store_dir: PathBuf)` where
`project_store_dir = ~/.local/share/oxidemx/projects/<key>`; `list() -> Vec<Conversation>`;
`get(&ConversationId) -> Option<Conversation>`; `upsert(Conversation)`; `rename(&ConversationId,
&str)`; `delete(&ConversationId)`; `set_working_dir(&ConversationId, PathBuf)`. Persists
`<project_store_dir>/conversations.json`.

- [ ] **Step 1: Failing test**:
```rust
#[test]
fn upsert_list_rename_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let idx = ConversationIndex::new(tmp.path().to_path_buf());
    let c = mk("chat-1", "first");      // helper builds a Conversation
    idx.upsert(c.clone());
    assert_eq!(idx.list().len(), 1);
    idx.rename(&c.id, "renamed");
    assert_eq!(idx.get(&c.id).unwrap().title, "renamed");
    // reload from disk
    let idx2 = ConversationIndex::new(tmp.path().to_path_buf());
    assert_eq!(idx2.list().len(), 1);
    idx2.delete(&c.id);
    assert!(idx2.get(&c.id).is_none());
    assert!(ConversationIndex::new(tmp.path().to_path_buf()).list().is_empty());
}
```
(Add a `fn mk(id,title)->Conversation` test helper mirroring Task 1's literal.)
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** — same load/save-with-tmp-rename pattern as ProjectStore; `upsert`
  replaces by id or appends; `set_working_dir`/`rename` mutate-then-save; `delete` retains != id.
  Reads are from disk each call (small files; simplicity over caching for 1a — matches the
  append-only transcript philosophy). Interior mutability not needed (methods take `&self` and
  re-read/re-write the file each call; document this).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(agentd): per-project ConversationIndex (conversations.json)`.

---

## Task 4: `.oxide` walk-up config resolver + rename

**Files:** Create `agentd/src/oxide_config.rs`; `lib.rs` (`pub mod oxide_config;`); modify
`agentd/src/projects.rs` (`.oxidemx`→`.oxide` with back-compat).
**Interfaces — Produces:** `resolve_oxide_config(working_dir: &Path, user_global: &Path) ->
ResolvedConfig`; `struct ResolvedConfig { settings: toml::Value (merged), skill_roots: Vec<PathBuf>,
mcp_files: Vec<PathBuf>, instruction_files: Vec<PathBuf> }`. Pure (no global fs state beyond reading
the dirs passed in).

- [ ] **Step 1: Failing test** (build a temp dir tree with nested `.oxide/settings.toml`):
```rust
#[test]
fn walk_up_scalars_closest_win_lists_union_root_stops() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // user-global
    let ug = root.join("global"); std::fs::create_dir_all(ug.join("")).unwrap();
    write(&ug.join("settings.toml"), "model = \"flash\"\n[permissions]\nallow=[\"a\"]\n");
    // project root .oxide (root=true)
    let proj = root.join("proj"); let proj_ox = proj.join(".oxide");
    std::fs::create_dir_all(&proj_ox).unwrap();
    write(&proj_ox.join("settings.toml"), "root = true\nmodel=\"pro\"\n[permissions]\nallow=[\"b\"]\n");
    // nested dir .oxide
    let sub = proj.join("src"); let sub_ox = sub.join(".oxide");
    std::fs::create_dir_all(&sub_ox).unwrap();
    write(&sub_ox.join("settings.toml"), "[permissions]\nallow=[\"c\"]\n");
    let rc = resolve_oxide_config(&sub, &ug);
    // scalar: closest (proj has model=pro; sub doesn't set it) → "pro" (user-global "flash" shadowed by proj)
    assert_eq!(rc.settings.get("model").and_then(|v| v.as_str()), Some("pro"));
    // list union of permissions.allow across proj(.oxide root) + sub — but NOT user-global ("a"),
    // because root=true at proj stops the walk before user-global is layered.
    let allow = perm_allow(&rc.settings);
    assert!(allow.contains(&"b".to_string()) && allow.contains(&"c".to_string()));
    assert!(!allow.contains(&"a".to_string()), "root=true must stop the user-global underlay");
}
```
(Provide `write`, `perm_allow` test helpers.)
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** the resolver: from `working_dir` walk up collecting `<dir>/.oxide/`
  (prefer `.oxide`, else `.oxidemx` back-compat) parsing `settings.toml`; stop after a layer whose
  settings has `root = true`; if no `root=true` reached, also layer `user_global` (a
  `settings.toml`/legacy) at the bottom. Order farthest→closest. Merge: deep-merge `toml::Value`
  tables where **scalars** = closest-wins and **arrays under known list keys** (`permissions.allow`,
  `permissions.deny`, top-level `skills`, `mcp`, `flows`, `agents`) = **union (dedup, closest
  order first)**. Collect `skill_roots`/`mcp_files`/`instruction_files` (each layer's `.oxide/skills`,
  `.oxide/mcp.toml`, `.oxide/OXIDE.md`) closest-first. Write a small `merge_toml(base, over,
  list_keys)` helper (pure, separately unit-tested with one extra test for the scalar-vs-list rule).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: `.oxidemx`→`.oxide` rename in projects.rs:** change `local = cwd.join(".oxidemx")`
  (projects.rs:128) to resolve `.oxide` if it exists else `.oxidemx` (back-compat); update
  `merged_skill_roots`/`merged_mcp` to use the same. Keep the existing lib.rs:52 test green (it
  asserts `.oxidemx/skills` is last — update it to `.oxide/skills` and create `.oxide` in that test).
- [ ] **Step 6: Run full agentd tests, expect PASS.**
- [ ] **Step 7: Commit** `feat(agentd): .oxide walk-up config resolver (+ rename from .oxidemx)`.

---

## Task 5: Worktree module (git seam)

**Files:** Create `agentd/src/worktree.rs`; `lib.rs` (`pub mod worktree;`).
**Interfaces — Produces:** `trait Git { fn worktree_add(&self, repo:&Path, dst:&Path, branch:&str,
base_ref:&str) -> Result<(),String>; fn worktree_remove(&self, repo:&Path, dst:&Path) ->
Result<(),String>; fn is_clean(&self, worktree:&Path) -> Result<bool,String>; }`;
`struct RealGit;` (shells `git`); `fn create_conversation_worktree(git:&dyn Git, repo:&Path,
name:&str, base_ref:&str) -> Result<Worktree,String>` (dst = `repo/.oxide/worktrees/<name>`, branch
= `worktree-<name>`, copies `.oxideinclude` patterns); `fn remove_if_unchanged(git:&dyn Git,
wt:&Worktree, repo:&Path) -> Result<bool,String>`.

- [ ] **Step 1: Failing test** with a mock `Git` recording calls:
```rust
struct MockGit { clean: bool, calls: std::sync::Mutex<Vec<String>> }
impl Git for MockGit {
    fn worktree_add(&self, _r:&Path, dst:&Path, branch:&str, base:&str)->Result<(),String>{
        self.calls.lock().unwrap().push(format!("add {} {} {}", dst.display(), branch, base)); Ok(()) }
    fn worktree_remove(&self,_r:&Path,dst:&Path)->Result<(),String>{
        self.calls.lock().unwrap().push(format!("remove {}", dst.display())); Ok(()) }
    fn is_clean(&self,_w:&Path)->Result<bool,String>{ Ok(self.clean) }
}
#[test]
fn create_uses_oxide_worktrees_path_and_branch() {
    let g = MockGit{ clean:true, calls:Default::default() };
    let wt = create_conversation_worktree(&g, std::path::Path::new("/repo"), "feat-x", "head").unwrap();
    assert!(wt.path.ends_with(".oxide/worktrees/feat-x"));
    assert_eq!(wt.branch, "worktree-feat-x");
    assert_eq!(wt.base_ref, "head");
}
#[test]
fn remove_if_unchanged_only_removes_clean() {
    let dirty = MockGit{ clean:false, calls:Default::default() };
    let wt = Worktree{ path:"/repo/.oxide/worktrees/x".into(), branch:"worktree-x".into(), base_ref:"head".into() };
    assert_eq!(remove_if_unchanged(&dirty,&wt,std::path::Path::new("/repo")).unwrap(), false);
    assert!(dirty.calls.lock().unwrap().is_empty());
}
```
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** the trait, `RealGit` (shell `git -C <repo> worktree add --b
  worktree-<name> <dst> <base>` where base `head`→`HEAD`, `fresh`→`origin/HEAD`; `git -C <repo>
  worktree remove <dst>`; `is_clean` = `git -C <wt> status --porcelain` empty), and the two helpers.
  `create_conversation_worktree`: compute dst, call add, copy `.oxideinclude` (gitignore-syntax;
  read `repo/.oxideinclude`, copy matching files into dst — minimal glob match; if file absent, skip),
  return `Worktree`. `remove_if_unchanged`: `is_clean` → if clean, `worktree_remove` + return true;
  else false.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(agentd): worktree module (git seam, .oxide/worktrees, .oxideinclude)`.

---

## Task 6: AgentService — project & conversation methods

**Files:** Modify `agentd/src/interface.rs` (`AgentService` impl). 
**Interfaces — Consumes:** Tasks 2–5. **Produces** (on `impl AgentService`, all `async` returning
`Result<_, AgentdError>` for consistency with existing methods):
`list_projects() -> Vec<Project>`, `create_project(name,&Path) -> Project`,
`init_project_from_conversation(conversation_id,&str,&Path) -> Project`,
`list_conversations(project_id:&str) -> Vec<Conversation>`,
`get_conversation(conversation_id:&str) -> Option<Conversation>`,
`create_conversation(project_id:&str, working_dir:Option<&Path>) -> Conversation`,
`rename_conversation(&str,&str)`, `delete_conversation(&str)`,
`set_conversation_working_dir(&str,&Path)`,
`create_worktree_for_conversation(&str, Option<&str>, Option<&str>) -> Conversation`,
`remove_worktree(&str)`, `resolve_config(&Path) -> ResolvedConfig`.

**Key wiring decisions (controller):**
- `AgentService` gains a `store_base: PathBuf` (derive from the existing `projects:
  ProjectRegistry`'s `store_base_override` OR add a field set in `new`/`with_store_base`/`TestEnv`).
  Build a `ProjectStore::new(store_base)` per call (cheap; files are small) — do NOT add a cached
  field to keep it stateless + test-friendly.
- A conversation's per-project index dir = `data_dir()/oxidemx/projects/<key>` where `<key>` =
  `ProjectKey::from_cwd(&project.default_working_dir)` (Personal uses the existing
  `~/.config/oxidemx` cwd → its key, so Personal conversations keep landing in today's transcript
  dir — preserves migration). Provide a private helper `conversation_index_for(&self, project:
  &Project) -> ConversationIndex`.
- Look up a conversation by id: scan each project's index (small N) — provide
  `find_conversation(&self, id) -> Option<(Project, Conversation)>`.
- `init_project_from_conversation`: create the project, move the conversation's index entry to the
  new project's index, reassign `project_id`, set its `working_dir` to the project default.
- Mint conversation ids the existing way (`crate::...new_session_id()` if reachable, else
  `format!("conv-{}", now_ms())`).

- [ ] **Step 1: Failing test** in interface.rs tests (uses `TestEnv`; `TestEnv` already injects a
  temp `store_base` via `ProjectRegistry::with_store_base`):
```rust
#[tokio::test]
async fn project_and_conversation_crud_via_service() {
    let env = TestEnv::new();
    let p = env.svc.create_project("Repo", std::path::Path::new("/tmp/repo")).await.unwrap();
    assert!(env.svc.list_projects().await.unwrap().iter().any(|x| x.id == p.id));
    let c = env.svc.create_conversation(p.id.as_str(), None).await.unwrap();
    assert_eq!(c.project_id, p.id);
    env.svc.rename_conversation(c.id.as_str(), "renamed").await.unwrap();
    assert_eq!(env.svc.get_conversation(c.id.as_str()).await.unwrap().unwrap().title, "renamed");
    assert_eq!(env.svc.list_conversations(p.id.as_str()).await.unwrap().len(), 1);
}
```
  (If `TestEnv` doesn't expose `store_base`, extend `TestEnv::build` to stash the temp base + thread
  it into `AgentService`; the controller noted `AgentService` needs a `store_base` field — add it to
  the struct + `new`/`with_store_base`, default `data_dir().join("oxidemx").join("projects")` is
  wrong — use the base that `ProjectRegistry::with_store_base` already received; READ how `main.rs`
  builds `ProjectRegistry::with_store_base(store_base)` at main.rs:127 and mirror that single
  `store_base` into `AgentService`.)
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** the methods + helpers per the wiring decisions. Reuse `now_ms()`.
- [ ] **Step 4: Run agentd tests, expect PASS.**
- [ ] **Step 5: Commit** `feat(agentd): AgentService project + conversation methods`.

---

## Task 7: send_message → ConversationIndex upsert (back-compat shim)

**Files:** Modify `agentd/src/interface.rs` (`AgentService::send_message`).
**Interfaces — Consumes:** Task 3/6. Keeps the existing `send_message(project, thread, text,
model_hint)` signature.

- [ ] **Step 1: Failing test**:
```rust
#[tokio::test]
async fn send_message_upserts_conversation_index() {
    let env = TestEnv::new();
    // project = the overlay's global path string; thread = a session id
    let proj = env.cwd_str();           // TestEnv's temp cwd (acts as the project path)
    env.svc.send_message(proj, "chat-77", "hello world", None).await.unwrap();
    // a conversation row now exists with that id, title from first prompt
    let key_proj = env.svc.ensure_personal_or_for_cwd(proj).await.unwrap(); // helper: maps cwd→project
    let convs = env.svc.list_conversations(key_proj.id.as_str()).await.unwrap();
    assert!(convs.iter().any(|c| c.id.as_str() == "chat-77" && c.title.starts_with("hello")));
}
```
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement:** after the existing transcript append + turn run in `send_message`,
  upsert a `Conversation` into the index for the project derived from `project` (the cwd string).
  Mapping: if `project` == the overlay global (`~/.config/oxidemx` / `XDG_CONFIG_HOME/oxidemx`), use
  the **Personal** project; else find/create a project whose `default_working_dir` == that cwd
  (auto-register, name = dir basename). Set `title` from the first user turn if the conversation is
  new (empty title), bump `updated_at`, add `usage` tokens, set `working_dir = project cwd`,
  `model` from the hint/config. Provide the `ensure_personal_or_for_cwd(&self, cwd:&str) ->
  Result<Project>` helper used by the test + the body. Do NOT change the wire signature.
- [ ] **Step 4: Run, expect PASS** (+ existing `send_message_appends_transcript_and_emits` test still
  green).
- [ ] **Step 5: Commit** `feat(agentd): send_message upserts the conversation index (back-compat)`.

---

## Task 8: D-Bus AgentInterface — expose the new methods

**Files:** Modify `agentd/src/interface.rs` (the `#[interface(name="org.oxidemx.Agent")] impl
AgentInterface` block).
**Interfaces — Consumes:** Task 6. Each D-Bus method delegates to `self.svc.<method>` and maps via
`to_fdo`, returning JSON strings for structured data (mirror existing `get_transcript` which returns
a JSON string).

- [ ] **Step 1: Failing test** (call through `AgentInterface`? — these are thin wrappers; test the
  service in Task 6 and here assert the wrappers compile + delegate by a direct call):
```rust
#[tokio::test]
async fn dbus_list_projects_returns_json() {
    let env = TestEnv::new();
    let iface = AgentInterface::new(env.svc.clone());
    let json = iface.list_projects().await.unwrap();
    assert!(json.contains("personal"));   // Personal present after ensure
}
```
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** wrappers, each returning `fdo::Result<String>` (serde_json of the core
  return) or `fdo::Result<()>`:
  `list_projects`, `create_project(name, default_working_dir)`,
  `init_project_from_conversation(conversation_id, name, default_working_dir)`,
  `list_conversations(project_id)`, `get_conversation(conversation_id)`,
  `create_conversation(project_id, working_dir)`, `rename_conversation(conversation_id, title)`,
  `delete_conversation(conversation_id)`, `set_conversation_working_dir(conversation_id, dir)`,
  `create_worktree_for_conversation(conversation_id, name, base_ref)`,
  `remove_worktree(conversation_id)`, `resolve_config(working_dir)`. (Empty strings for optional
  args → `None`, mirroring the existing `model_hint` pattern.)
- [ ] **Step 4: Run, expect PASS** + `cargo clippy -p agentd` clean.
- [ ] **Step 5: Commit** `feat(agentd): expose project/conversation/worktree methods over D-Bus`.

---

## Task 9: Migration — Personal project + index existing transcripts

**Files:** Modify `agentd/src/interface.rs` (a `AgentService::migrate_on_start()` called from
construction or first use) + `agentd/src/main.rs` (call it after building the service).
**Interfaces — Consumes:** Tasks 2,3.

- [ ] **Step 1: Failing test**:
```rust
#[tokio::test]
async fn migration_creates_personal_and_indexes_existing_transcripts() {
    let env = TestEnv::new();
    // simulate a pre-existing transcript by sending a message first
    env.svc.send_message(env.cwd_str(), "chat-old", "old prompt", None).await.unwrap();
    // wipe the index (simulate upgrade from a build with transcripts but no index)
    env.svc.debug_delete_conversation_index_files();   // test helper: rm conversations.json
    env.svc.migrate_on_start().await.unwrap();
    let personal = env.svc.list_projects().await.unwrap().into_iter().find(|p| p.id.as_str()=="personal").unwrap();
    let convs = env.svc.list_conversations(personal.id.as_str()).await.unwrap();
    assert!(convs.iter().any(|c| c.id.as_str() == "chat-old"));
}
```
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** `migrate_on_start`: `project_store.ensure_personal()`; for the Personal
  project's transcript dir, list `*.jsonl` stems not already in the conversation index and upsert a
  `Conversation` (title from first turn via `TranscriptStore.read`, timestamps from file mtime /
  first+last turn ts). Idempotent (skip ids already indexed). Call it once at startup in `main.rs`
  after the service is built (spawn or await before serving). Add the small
  `debug_delete_conversation_index_files` test-only helper (`#[cfg(test)]`).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(agentd): startup migration — Personal project + index existing transcripts`.

---

## Task 10: Build, install, verify

- [ ] Build host-side: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build --release -p agentd`.
- [ ] Full agentd suite green + clippy clean: `cargo test -p agentd && cargo clippy -p agentd`.
- [ ] Install + restart: `cp` to /tmp → `pkexec install -m755 /tmp/oxidemx-agentd
  /usr/local/bin/oxidemx-agentd` → `systemctl --user restart oxidemx-agentd` → verify the running
  pid's start time > binary mtime.
- [ ] Live smoke (busctl): `busctl --user call org.oxidemx.Agent /org/oxidemx/Agent
  org.oxidemx.Agent ListProjects` returns JSON containing `personal`; `CreateProject` + `CreateConversation`
  + `ListConversations` round-trip; the existing overlay chat still sends/receives (back-compat).

## Self-review (coverage)

Spec §Entity model → T1,2,3,6. §`.oxide`+inheritance → T4. §Working-dir+worktrees → T5,6. §Core+D-Bus
contract → T6,7,8. §Migration → T9. §Source of truth (index authoritative) → T3,6,7. Back-compat shim
→ T7. Out-of-scope (HTTP/SSE 1b, Freya, decommission) → untouched. The D-Bus method param/return
shapes (JSON strings) match the existing `get_transcript` convention; `AgentService` method names are
reused verbatim by the D-Bus wrappers (T8) and the tests (T6).
