# Flow Delivery to Chat — Implementation Plan (S1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a chat-launched flow finishes, automatically deliver its result — richly — into the
originating conversation, with truthful status and per-conversation run state.

**Architecture:** Chat flows run **agentd-only** (embedded conductor, authoritative `RunFinished`).
The conversation id (`ChatThread.session_id` = the `"chat-N"` string the overlay already sends to
agentd as `thread`) is threaded launch→events→`run.json` so `RunFinished` routes back to the
originating `ChatThread` via the existing `session_to_thread_idx()`. Per-conversation working state
replaces window-global `ai_loading`; delivery posts the full handoff markdown + artifact cards.

**Tech Stack:** Rust, iced 0.14 (markdown + `iced_highlighter`), agentd D-Bus (zbus),
oxidemx-conductor, oxidemx-agent-core, oxidemx-shared, settings-rs.

## Global Constraints

- **Rule 0 — naming:** the conversation id is the existing `ChatThread.session_id` / agentd `thread`
  string (`"chat-N"`). Call the new field `conversation_id` everywhere it's plumbed.
- **Rule 1 — truthful status:** flow success/failure comes from the authoritative `RunFinished` /
  `run.json`, never from scraping subprocess stdout. The in-proc stdout-scraping `run_flow_tool` is
  retired from the chat path.
- **Rule 2 — Rust bar:** clippy-clean (warnings = defects), hand-formatted (NO repo-wide
  `cargo fmt`; match the file's local style, format only added lines), `?` over unwrap in non-test
  code, no gold-plating. Prefer the smaller change (store `conversation_id` on the per-turn executor;
  do NOT change the `ToolExecutor::execute` trait).
- **Rule 3 — builds:** agentd / oxidemx-agent-core / oxidemx-conductor / oxidemx-shared build
  **host-side**: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p <crate>` with rustup (no
  GTK). The overlay (`oxidemx-overlay`) and settings (`oxidemx-settings`) build in the
  `claude_development` distrobox over the repo `target/`. Never mix toolchains over one `target/`.
  Work in a git worktree (Jim edits the main checkout concurrently). Commit each task. After the
  build is test-ready, install affected bins (`cp` to /tmp → `pkexec install -m755 /tmp/<bin>
  /usr/local/bin/<bin>`), restart `oxidemx-daemon`/agentd as needed, and verify the RUNNING process
  is the new build before declaring done.

**Worktree setup (once, before Task 1):** create the worktree, symlink the iced fork + libcosmic
and init submodules so the overlay resolves (only needed because Tasks 5–9 build the overlay):
```bash
cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-phase1
git worktree add ../oxidemx-s1 phase1-local-llm-gateway
cd ../oxidemx-s1
ln -s ../juhradial-mx/pop_os_iced pop_os_iced 2>/dev/null || true
ln -s ../juhradial-mx/libcosmic libcosmic 2>/dev/null || true
git submodule update --init 2>/dev/null || true
git -c submodule.recurse=false config submodule.recurse false 2>/dev/null || true
```

---

## File map

| File | Change |
|------|--------|
| `oxidemx-shared/src/config.rs` | flip `use_agentd` default to `true` (Task 1) |
| `settings-rs/src/tabs/ai.rs`, `settings-rs/src/main.rs` (or message module) | add `use_agentd` toggle (Task 1) |
| `agentd/src/tools/mod.rs` | `AgentToolExecutor` gains `conversation_id`; pass to `run_flow` (Task 2) |
| `agentd/src/interface.rs` | build executor with `thread` as `conversation_id` (Task 2) |
| `agentd/src/tools/agent.rs` | `run_flow` gains `conversation_id`, passes to `launch` (Task 2) |
| `agentd/src/run_launcher.rs` | `RunLauncher::launch` + impls gain `conversation_id`; build `RunOptions`/`RunEventBridge` with it (Tasks 2–4) |
| `oxidemx-conductor/src/supervisor.rs` | `RunOptions.conversation_id` + `write_run_json` records it (Task 3) |
| `oxidemx-conductor/src/bin/conductor.rs` | construct `RunOptions` with `conversation_id: String::new()` (Task 3) |
| `agentd/src/run_bridge.rs` | `RunEventBridge.conversation_id` + payload `conversation_id` (Task 4) |
| `overlay-rs/src/activity/mod.rs` | `RunEventView.conversation_id` (Task 5) |
| `overlay-rs/src/app/agent_events.rs` | demux reads `conversation_id` (Task 5) |
| `overlay-rs/src/radial/chat_threads.rs` | `ChatThread.working` (skip) + helper (Task 6) |
| `overlay-rs/src/app/update.rs` | per-thread working writes; auto-deliver on RunFinished/Failed/Cancelled (Tasks 6–7) |
| `overlay-rs/src/chat_ui/cards.rs`, `overlay-rs/src/app/mod.rs` | artifact cards + messages (Task 8) |
| `overlay-rs/src/activity/*`, `agentd/src/tools/agent.rs` | conversation-scoped bubbles + run introspection (Task 9) |

---

## Task 1: `use_agentd` default on + settings toggle

**Files:**
- Modify: `oxidemx-shared/src/config.rs` (the `use_agentd` field, ~1248-1253)
- Modify: `settings-rs/src/tabs/ai.rs` (add a toggle row); `settings-rs/src/main.rs` + message enum
- Test: `oxidemx-shared/src/config.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `AiConfig.use_agentd: bool` now defaults `true`; a settings toggle that round-trips it.

**Root cause (from investigation):** `use_agentd` has only `#[serde(default)]` (serializes `false`
fine), but settings-rs has no UI control, so its in-memory `AppConfig` always holds `false` and
overwrites a manual edit on the next save. Fix = default `true` + a settings toggle.

- [ ] **Step 1: Write the failing test** in `oxidemx-shared/src/config.rs` `#[cfg(test)]`:

```rust
#[test]
fn use_agentd_defaults_on_and_roundtrips() {
    let c = AiConfig::default();
    assert!(c.use_agentd, "agentd is the default chat path for flows");
    // false must still serialize (no skip) so a user can pin it off and have it stick.
    let mut off = AiConfig::default();
    off.use_agentd = false;
    let json = serde_json::to_string(&off).unwrap();
    assert!(json.contains("\"use_agentd\":false"));
    let back: AiConfig = serde_json::from_str(&json).unwrap();
    assert!(!back.use_agentd);
}
```

- [ ] **Step 2: Run it, expect FAIL** (default is currently `false`):
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-shared use_agentd_defaults_on_and_roundtrips`
  Expected: FAIL on the first assert.

- [ ] **Step 3: Implement** — change the field default. Replace the `use_agentd` field
  (config.rs ~1248-1253) so it defaults true via a named default fn:

```rust
    /// Route overlay chat through `agentd` over D-Bus (`AgentProxy`). Default
    /// **true**: flows require agentd (in-proc holds the turn open + scrapes
    /// status). Pin `false` under `"ai": {}` to force the legacy in-proc path.
    #[serde(default = "default_true")]
    pub use_agentd: bool,
```

  Add near the other `default_*` fns in config.rs:

```rust
fn default_true() -> bool { true }
```

  And set the field in the struct's `Default` impl / constructor to `true` (find where `AiConfig`
  builds its default — if it derives `Default`, replace the derive-driven `false` by giving the
  field `#[serde(default = "default_true")]` AND, if `AiConfig` has a manual `Default`, set
  `use_agentd: true` there; if it derives `Default`, add a manual `Default` or a field default). If
  `AiConfig` uses `#[derive(Default)]`, convert to a manual `impl Default` that mirrors the serde
  defaults and sets `use_agentd: true` (check the existing default_* fns for the other fields).

- [ ] **Step 4: Run it, expect PASS.**
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-shared use_agentd_defaults_on_and_roundtrips`

- [ ] **Step 5: Add the settings toggle.** In `settings-rs/src/tabs/ai.rs`, add a labelled toggle
  bound to `cfg.overlay.ai.use_agentd` near the provider control (match the tab's existing
  toggle/row style — find an existing `toggler(...)` in settings-rs and mirror it):

```rust
    // Route chat through agentd (required for flows: bubbles + auto-delivery).
    let agentd_row = row![
        text("Run agent through agentd (required for flows)").size(13),
        Space::new().width(Length::Fill),
        toggler(state.config.overlay.ai.use_agentd)
            .on_toggle(Message::AiUseAgentdToggled),
    ]
    .align_y(Alignment::Center);
```

  Add `AiUseAgentdToggled(bool)` to the settings `Message` enum and handle it where other AI-tab
  messages are handled (mirror `AiModelChanged`):

```rust
        Message::AiUseAgentdToggled(on) => {
            state.config.overlay.ai.use_agentd = on;
            state.dirty = true;   // match the tab's existing "mark changed" field
        }
```

- [ ] **Step 6: Build settings (distrobox).**
  `distrobox enter claude_development -- bash -lc 'cargo build --release --bin oxidemx-settings 2>&1 | tail -5'`
  Expected: Finished, clippy-clean.

- [ ] **Step 7: Commit.**
```bash
git add oxidemx-shared/src/config.rs settings-rs/src/tabs/ai.rs settings-rs/src/main.rs
git commit -m "feat(config): default use_agentd on + settings toggle (stops the revert)"
```

---

## Task 2: conversation_id — agentd executor → run_flow → launcher

**Files:**
- Modify: `agentd/src/tools/mod.rs` (`AgentToolExecutor` struct + `new` + dispatch)
- Modify: `agentd/src/interface.rs` (build executor with `thread`)
- Modify: `agentd/src/tools/agent.rs` (`run_flow` signature)
- Modify: `agentd/src/run_launcher.rs` (`RunLauncher::launch` trait + `ConductorRunLauncher` +
  `NoopRunLauncher`)
- Test: `agentd/src/tools/agent.rs` `#[cfg(test)]` (extend the existing run_flow test)

**Interfaces:**
- Produces: `AgentToolExecutor::new(paths, host, run_launcher, conversation_id: String)`;
  `run_flow(launcher, paths, conversation_id: &str, args)`;
  `RunLauncher::launch(&self, project, flow_id, inputs_json, conversation_id: &str) -> Result<String,String>`.
- Consumes (Task 3+): the launcher passes `conversation_id` into `RunOptions` + `RunEventBridge`.

- [ ] **Step 1: Failing test** — extend the run_flow test in `agentd/src/tools/agent.rs` to pass a
  conversation id and assert launch receives it. Add a recording stub launcher in the test module:

```rust
    struct RecordingLauncher { last_conv: std::sync::Mutex<String> }
    #[async_trait::async_trait]
    impl crate::run_launcher::RunLauncher for RecordingLauncher {
        async fn launch(&self, _p: &str, _f: &str, _i: &str, conversation_id: &str)
            -> Result<String, String> {
            *self.last_conv.lock().unwrap() = conversation_id.to_string();
            Ok("run-test".into())
        }
        fn status(&self, _r: &str) -> Option<crate::run_launcher::RunStatus> { None }
        fn list_runs(&self, _p: &str) -> Vec<String> { vec![] }
    }

    #[tokio::test]
    async fn run_flow_forwards_conversation_id() {
        let l: std::sync::Arc<dyn crate::run_launcher::RunLauncher> =
            std::sync::Arc::new(RecordingLauncher { last_conv: Default::default() });
        let paths = crate::projects::ProjectPaths::resolve(std::path::Path::new("/tmp"));
        let args = serde_json::json!({ "flow_id": "doc-digest" });
        let out = super::run_flow(&l, &paths, "chat-7", &args).await.unwrap();
        assert!(out.contains("run-test"));
        let rec = l.as_ref() as *const dyn crate::run_launcher::RunLauncher;
        let _ = rec; // silence unused; assertion below reads through a downcast-free path
    }
```
  (If downcast is awkward, assert via a shared `Arc<Mutex<String>>` captured before boxing instead of
  reading back through the trait object — capture the `Arc` in the test, clone into the struct.)

- [ ] **Step 2: Run, expect FAIL** (signature mismatch / arity):
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agentd run_flow_forwards_conversation_id`

- [ ] **Step 3: Change `RunLauncher::launch`** (run_launcher.rs ~52) to add the parameter, and update
  both impls. Trait:

```rust
    async fn launch(
        &self,
        project: &str,
        flow_id: &str,
        inputs_json: &str,
        conversation_id: &str,
    ) -> Result<String, String>;
```
  `NoopRunLauncher` (same file): add `_conversation_id: &str` to its `launch`.
  `ConductorRunLauncher::launch`: add `conversation_id: &str` to the signature (the body wiring into
  `RunOptions`/`RunEventBridge` lands in Tasks 3–4; for now bind `let _ = conversation_id;` so it
  compiles, OR — preferred — wire it straight through now and fold Tasks 3–4 edits here since they're
  the same function; if wiring now, see Tasks 3–4 for the exact `RunOptions`/`RunEventBridge::new`
  lines).

- [ ] **Step 4: `run_flow`** (agent.rs ~174) — add `conversation_id` and forward it:

```rust
pub(super) async fn run_flow(
    launcher: &Arc<dyn RunLauncher>,
    paths: &crate::projects::ProjectPaths,
    conversation_id: &str,
    args: &Value,
) -> Result<String, String> {
    let flow_id = args["flow_id"].as_str()
        .ok_or_else(|| "run_flow: missing 'flow_id' argument".to_string())?;
    let inputs_json = args.get("inputs_json").and_then(Value::as_str).unwrap_or("{}");
    let project = paths.cwd.to_string_lossy();
    match launcher.launch(&project, flow_id, inputs_json, conversation_id).await {
        Ok(run_id) => Ok(format!(
            "Launched flow '{flow_id}' — run id `{run_id}`. It is now running in the \
             background; check its status with run_status(run_id=\"{run_id}\") — do not \
             guess whether it has finished."
        )),
        Err(e) => Err(format!("run_flow: could not launch '{flow_id}': {e}")),
    }
}
```

- [ ] **Step 5: `AgentToolExecutor`** (tools/mod.rs ~25) — add the field + ctor param + dispatch:

```rust
pub struct AgentToolExecutor {
    pub(crate) paths: ProjectPaths,
    pub(crate) host: Arc<dyn HostCapability>,
    pub(crate) run_launcher: Arc<dyn crate::run_launcher::RunLauncher>,
    pub(crate) conversation_id: String,
}

impl AgentToolExecutor {
    pub fn new(
        paths: ProjectPaths,
        host: Arc<dyn HostCapability>,
        run_launcher: Arc<dyn crate::run_launcher::RunLauncher>,
        conversation_id: String,
    ) -> Self {
        Self { paths, host, run_launcher, conversation_id }
    }
}
```
  In the dispatch match arm:
```rust
            "run_flow" => agent::run_flow(&self.run_launcher, &self.paths, &self.conversation_id, &args).await,
```

- [ ] **Step 6: Build the executor with `thread`** (interface.rs ~131) — pass the conversation id:
```rust
            std::sync::Arc::new(crate::tools::AgentToolExecutor::new(
                paths.clone(),
                host.clone(),
                run_launcher.clone(),
                thread.to_string(),
            ));
```

- [ ] **Step 7: Run the test, expect PASS** + the existing agentd tests:
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agentd`

- [ ] **Step 8: Commit.**
```bash
git add agentd/src/tools/mod.rs agentd/src/tools/agent.rs agentd/src/interface.rs agentd/src/run_launcher.rs
git commit -m "feat(agentd): thread conversation_id into run_flow + RunLauncher::launch"
```

---

## Task 3: conversation_id — conductor RunOptions + run.json

**Files:**
- Modify: `oxidemx-conductor/src/supervisor.rs` (`RunOptions` struct + `write_run_json`)
- Modify: `oxidemx-conductor/src/bin/conductor.rs` (CLI constructs `RunOptions`)
- Modify: `agentd/src/run_launcher.rs` (`ConductorRunLauncher::launch` builds `RunOptions`)
- Test: `oxidemx-conductor/src/supervisor.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `RunOptions.conversation_id: String`; `run.json` gains `"conversation_id"`.

- [ ] **Step 1: Failing test** in supervisor.rs tests — assert `write_run_json` records the
  conversation id. (Mirror an existing run.json/tempdir test; if none, write one with `tempfile` or
  a `std::env::temp_dir()` subdir.)

```rust
    #[test]
    fn run_json_records_conversation_id() {
        let dir = std::env::temp_dir().join(format!("oxidemx-condtest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let opts = RunOptions {
            run_id: "run-1".into(),
            inputs: Default::default(),
            workdir: dir.clone(),
            roster: Roster::default(),
            factory: std::sync::Arc::new(FixedFactory(crate::mock::MockProvider::echoing())),
            cancel: tokio_util::sync::CancellationToken::new(),
            approval: crate::approval::ApprovalPolicy::Autonomous,
            allowlist: vec![],
            conversation_id: "chat-9".into(),
        };
        write_run_json(&opts, "doc-digest", true, &["ANSWER.md".into()], &None);
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run.json")).unwrap()).unwrap();
        assert_eq!(v["conversation_id"], "chat-9");
    }
```
  (If `Roster::default()` / `FixedFactory` aren't constructible in tests there, copy the construction
  the nearest existing supervisor test uses.)

- [ ] **Step 2: Run, expect FAIL** (no field / not recorded):
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor run_json_records_conversation_id`

- [ ] **Step 3: Add the field** (supervisor.rs ~68) — append to `RunOptions`:
```rust
    pub allowlist: Vec<String>,
    /// The chat conversation that launched this run (`ChatThread.session_id` /
    /// agentd `thread`), echoed back on run events so the UI delivers the
    /// result to the originating conversation. Empty for non-chat launches.
    pub conversation_id: String,
```

- [ ] **Step 4: Record it** in `write_run_json` (supervisor.rs ~744):
```rust
    let record = serde_json::json!({
        "run_id": opts.run_id,
        "flow_id": flow_id,
        "success": success,
        "artifacts": artifacts,
        "error": error,
        "conversation_id": opts.conversation_id,
    });
```

- [ ] **Step 5: Fix the two `RunOptions` constructors.**
  - CLI (`conductor.rs` ~223): add `conversation_id: String::new(),` to the literal.
  - agentd launcher (`run_launcher.rs` ~ the `RunOptions { ... }` literal): add
    `conversation_id: conversation_id.to_string(),` (uses Task 2's `conversation_id` param).

- [ ] **Step 6: Run, expect PASS** + full conductor suite:
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-conductor`

- [ ] **Step 7: Commit.**
```bash
git add oxidemx-conductor/src/supervisor.rs oxidemx-conductor/src/bin/conductor.rs agentd/src/run_launcher.rs
git commit -m "feat(conductor): RunOptions.conversation_id recorded in run.json"
```

---

## Task 4: conversation_id — RunEventBridge → AgentEvent payload

**Files:**
- Modify: `agentd/src/run_bridge.rs` (`RunEventBridge` struct + `new` + `do_emit`)
- Modify: `agentd/src/run_launcher.rs` (`RunEventBridge::new` call)
- Test: `agentd/src/run_bridge.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: every `"run"`-kind `AgentEvent.payload` carries `"conversation_id"`.

- [ ] **Step 1: Failing test** in run_bridge.rs tests — a recording `EventEmitter`, emit a
  `RunFinished`, assert the payload carries `conversation_id`:

```rust
    #[tokio::test]
    async fn payload_carries_conversation_id() {
        #[derive(Default)]
        struct Rec { last: std::sync::Mutex<Option<serde_json::Value>> }
        impl crate::seams::EventEmitter for Rec {
            fn emit(&self, ev: crate::seams::AgentEvent) { *self.last.lock().unwrap() = Some(ev.payload); }
        }
        let rec = std::sync::Arc::new(Rec::default());
        let bridge = RunEventBridge::new(
            "/p", "run-1", "chat-3",
            rec.clone(), std::sync::Arc::new(std::sync::Mutex::new(Default::default())),
        );
        bridge.emit(oxidemx_conductor::event::RunEvent::RunFinished {
            run_id: "run-1".into(), artifacts: vec![], handoff_markdown: "hi".into(),
        }).await;
        let p = rec.last.lock().unwrap().clone().unwrap();
        assert_eq!(p["conversation_id"], "chat-3");
        assert_eq!(p["variant"], "RunFinished");
    }
```

- [ ] **Step 2: Run, expect FAIL** (arity of `new` / missing field):
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agentd payload_carries_conversation_id`

- [ ] **Step 3: Add field + ctor param** (run_bridge.rs ~46, ~58):
```rust
pub struct RunEventBridge {
    pub project: String,
    pub run_id: String,
    pub conversation_id: String,
    pub emitter: Arc<dyn EventEmitter>,
    pub statuses: Arc<Mutex<HashMap<String, String>>>,
}

impl RunEventBridge {
    pub fn new(
        project: impl Into<String>,
        run_id: impl Into<String>,
        conversation_id: impl Into<String>,
        emitter: Arc<dyn EventEmitter>,
        statuses: Arc<Mutex<HashMap<String, String>>>,
    ) -> Self {
        Self {
            project: project.into(),
            run_id: run_id.into(),
            conversation_id: conversation_id.into(),
            emitter,
            statuses,
        }
    }
```

- [ ] **Step 4: Emit it** in `do_emit` (run_bridge.rs ~82):
```rust
        let payload = serde_json::json!({
            "kind": "run",
            "variant": variant,
            "run_id": run_id,
            "conversation_id": self.conversation_id,
            "details": details,
        });
```

- [ ] **Step 5: Fix the `RunEventBridge::new` call** in `run_launcher.rs` (~the `Arc::new(RunEventBridge::new(`):
```rust
        let bridge = Arc::new(RunEventBridge::new(
            project,
            run_id.clone(),
            conversation_id,        // Task 2's &str param
            self.emitter.clone(),
            self.run_statuses.clone(),
        ));
```

- [ ] **Step 6: Run, expect PASS** + agentd suite.
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agentd`

- [ ] **Step 7: Commit.**
```bash
git add agentd/src/run_bridge.rs agentd/src/run_launcher.rs
git commit -m "feat(agentd): RunEventBridge stamps conversation_id on run events"
```

---

## Task 5: overlay demux — RunEventView.conversation_id

**Files:**
- Modify: `overlay-rs/src/activity/mod.rs` (`RunEventView`)
- Modify: `overlay-rs/src/app/agent_events.rs` (`demux_event` "run" arm)
- Test: `overlay-rs/src/app/agent_events.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `RunEventView.conversation_id: String`, populated from the payload.

- [ ] **Step 1: Failing test** in agent_events.rs tests — feed a "run" payload with
  `conversation_id` and assert the demuxed `RunEventView` carries it. (Call `demux_event` directly;
  it's module-private — test lives in the same file.)

```rust
    #[test]
    fn demux_run_event_carries_conversation_id() {
        let payload = serde_json::json!({
            "kind": "run", "variant": "RunFinished", "run_id": "run-1",
            "conversation_id": "chat-4",
            "details": { "artifacts": ["ANSWER.md"], "handoff_markdown": "done" }
        }).to_string();
        match demux_event("run-1", &payload) {
            Message::RunEvent(v) => {
                assert_eq!(v.conversation_id, "chat-4");
                assert_eq!(v.variant, "RunFinished");
                assert_eq!(v.handoff, "done");
            }
            _ => panic!("expected RunEvent"),
        }
    }
```

- [ ] **Step 2: Run, expect FAIL** (no field). Build via distrobox:
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay demux_run_event_carries_conversation_id 2>&1 | tail -15'`

- [ ] **Step 3: Add the field** (activity/mod.rs ~14, in `RunEventView`):
```rust
    pub run_id: String,
    pub conversation_id: String,
    pub variant: String,
```

- [ ] **Step 4: Populate it** in demux_event's "run" arm (agent_events.rs, in the `RunEventView { ... }`):
```rust
                run_id: val.get("run_id").and_then(|x| x.as_str()).unwrap_or(thread_or_run).to_string(),
                conversation_id: val.get("conversation_id").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                variant: val.get("variant").and_then(|x| x.as_str()).unwrap_or("run").to_string(),
```

- [ ] **Step 5: Run, expect PASS.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay demux_run_event_carries_conversation_id 2>&1 | tail -8'`

- [ ] **Step 6: Commit.**
```bash
git add overlay-rs/src/activity/mod.rs overlay-rs/src/app/agent_events.rs
git commit -m "feat(overlay): RunEventView carries conversation_id from payload"
```

---

## Task 6: per-conversation working state

**Files:**
- Modify: `overlay-rs/src/radial/chat_threads.rs` (`ChatThread.working` + helper)
- Modify: `overlay-rs/src/app/update.rs` (replace global `ai_loading` writes with per-thread; mirror
  on thread switch)
- Test: `overlay-rs/src/radial/chat_threads.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `ChatThread.working: bool` (serde-skipped, runtime-only). A `RadialState` helper
  `set_thread_working(&mut self, idx: usize, on: bool)` that sets `threads[idx].working` and mirrors
  `self.ai_loading` iff `idx == self.ai_active`.

**Design:** keep `ai_loading` as the *active thread's* working mirror (the footer already reads it,
[footer.rs:34](overlay-rs/src/chat_ui/footer.rs#L34) / [:110](overlay-rs/src/chat_ui/footer.rs#L110)).
All `state.ai_loading = true/false` writes that pertain to a specific thread become
`state.set_thread_working(idx, bool)`. On thread switch, recompute `ai_loading` from the newly-active
thread. This makes switching show the right status and lets two conversations work at once.

- [ ] **Step 1: Failing test** in chat_threads.rs tests (pure helper on a minimal state). If
  `RadialState` is hard to construct in a unit test, put the mirror logic in a tiny free fn and test
  that instead:

```rust
    // free fn in chat_threads.rs:
    // pub fn mirror_loading(active: usize, idx: usize, on: bool, cur: bool) -> bool {
    //     if idx == active { on } else { cur }
    // }
    #[test]
    fn working_mirrors_only_active_thread() {
        assert!(mirror_loading(2, 2, true, false));      // active thread on → mirror on
        assert!(!mirror_loading(2, 0, true, false));     // other thread on → mirror unchanged
        assert!(mirror_loading(2, 0, true, true));       // other thread → keep current true
    }
```

- [ ] **Step 2: Run, expect FAIL.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay working_mirrors_only_active_thread 2>&1 | tail -12'`

- [ ] **Step 3: Add the field + helpers.** In chat_threads.rs `ChatThread`:
```rust
    #[serde(skip)]
    pub working: bool,
```
  Add the free fn:
```rust
/// The active thread's working flag is mirrored onto the window-global
/// `ai_loading` (which the footer reads). A background thread changing state
/// must not touch the active mirror.
pub fn mirror_loading(active: usize, idx: usize, on: bool, current: bool) -> bool {
    if idx == active { on } else { current }
}
```
  In `RadialState` (radial/mod.rs), add:
```rust
    pub fn set_thread_working(&mut self, idx: usize, on: bool) {
        if let Some(t) = self.ai_threads.get_mut(idx) {
            t.working = on;
        }
        self.ai_loading = crate::radial::chat_threads::mirror_loading(
            self.ai_active, idx, on, self.ai_loading,
        );
    }
```

- [ ] **Step 4: Replace the per-thread `ai_loading` writes** in update.rs. For each site that begins
  or ends a turn for a KNOWN thread index, use `set_thread_working`:
  - submit (`~820`): the active submit → `let idx = state.ai_active; state.set_thread_working(idx, true);`
    (keep the adjacent `ai_turn_tokens`/`ai_activity` lines).
  - `AiResponseReceived(thread_idx, _)` (`~895`): `state.set_thread_working(thread_idx, false);`
  - `AgentdFinal { thread_idx, .. }` (`~1624`): `state.set_thread_working(thread_idx, false);`
  - Stop/cancel (`~1067`, `~1089`): `let idx = state.ai_active; state.set_thread_working(idx, false);`
  - approval submit/receive (`~1467`, `~1480`, `~1647`, `~1655`): use the relevant thread idx
    (active for the user-driven ones).
  Leave any write that is genuinely global (none expected) as-is.

- [ ] **Step 5: Mirror on thread switch.** Find the thread-select handler (search `ai_active =` in
  update.rs — e.g. `Message::AiSelectThread`/`AiActivateThread`). After setting `state.ai_active`,
  add:
```rust
            state.ai_loading = state.ai_threads.get(state.ai_active).map(|t| t.working).unwrap_or(false);
```

- [ ] **Step 6: Build + run overlay tests.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay 2>&1 | tail -15'`

- [ ] **Step 7: Commit.**
```bash
git add overlay-rs/src/radial/chat_threads.rs overlay-rs/src/radial/mod.rs overlay-rs/src/app/update.rs
git commit -m "feat(overlay): per-conversation working state (no more window-global loading)"
```

---

## Task 7: auto-deliver on RunFinished/Failed/Cancelled

**Files:**
- Modify: `overlay-rs/src/app/update.rs` (the `Message::RunEvent` handler, ~1716)
- Test: a pure delivery-decision helper in `overlay-rs/src/activity/mod.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `RunEventView.conversation_id`, `.variant`, `.handoff`, `.artifacts`, `.flow_id`,
  `.message` (failure reason), `session_to_thread_idx`, `ChatMessage::assistant`,
  `set_thread_working`, `scroll_chat_to_end`, `save_chat_threads`.
- Produces: on a terminal run event, an assistant message is appended to the originating thread and
  its working state cleared.

**Decision:** the delivery body + header is built by a pure helper so it's unit-testable; the
update handler does the state mutation.

- [ ] **Step 1: Failing test** for the pure helper in activity/mod.rs:
```rust
    #[test]
    fn delivered_message_has_header_and_handoff() {
        let v = RunEventView {
            variant: "RunFinished".into(), flow_id: "doc-digest".into(),
            handoff: "# Answer\nkey points".into(),
            artifacts: vec!["ANSWER.md".into(), "debug/digest.md".into()],
            ..Default::default()
        };
        let body = delivery_message(&v);
        assert!(body.starts_with("**doc-digest** · ✓"));
        assert!(body.contains("# Answer"));
        let f = RunEventView { variant: "RunFailed".into(), flow_id: "x".into(),
            message: "step boom".into(), ..Default::default() };
        assert!(delivery_message(&f).contains("✗"));
        assert!(delivery_message(&f).contains("step boom"));
    }
```

- [ ] **Step 2: Run, expect FAIL.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay delivered_message_has_header_and_handoff 2>&1 | tail -12'`

- [ ] **Step 3: Implement the helper** in activity/mod.rs:
```rust
/// The assistant-message body posted to a conversation when its flow run ends.
/// Header line (flow · status · counts) + the full handoff markdown on success,
/// or the failure reason on failure/cancel. Artifact cards are rendered by the
/// chat from `RunEventView.artifacts` separately (Task 8).
pub fn delivery_message(v: &RunEventView) -> String {
    match v.variant.as_str() {
        "RunFinished" => {
            let n = v.artifacts.len();
            let head = format!("**{}** · ✓ · {n} artifact(s)", v.flow_id);
            if v.handoff.trim().is_empty() {
                format!("{head}\n\n_(flow produced no inline answer; see artifacts)_")
            } else {
                format!("{head}\n\n{}", v.handoff)
            }
        }
        "RunFailed" => format!("**{}** · ✗ failed\n\n{}", v.flow_id,
            if v.message.is_empty() { "(no reason reported)".into() } else { v.message.clone() }),
        "RunCancelled" => format!("**{}** · ⊘ cancelled", v.flow_id),
        _ => String::new(),
    }
}
```

- [ ] **Step 4: Wire the handler.** Replace the `Message::RunEvent` arm (update.rs ~1716) so it both
  feeds the dock AND auto-delivers on terminal variants:
```rust
        Message::RunEvent(view) => {
            if state.chat_window_mode {
                state.activity.apply_run_event(&view);
            }
            // Auto-deliver terminal results into the originating conversation.
            let terminal = matches!(view.variant.as_str(),
                "RunFinished" | "RunFailed" | "RunCancelled");
            if terminal {
                let idx = crate::app::agent_events::session_to_thread_idx(
                    &state.ai_threads, &view.conversation_id);
                if let Some(idx) = idx {
                    let body = crate::activity::delivery_message(&view);
                    if !body.is_empty() {
                        let mut msg = ChatMessage::assistant(body);
                        msg.card = Some(crate::ai_client::AgentCardData::Flow {
                            flow_id: view.flow_id.clone(),
                            run_id: view.run_id.clone(),
                            success: view.variant == "RunFinished",
                            steps: vec![],
                            artifacts: view.artifacts.clone(),
                        });
                        if let Some(t) = state.ai_threads.get_mut(idx) {
                            t.history.push(msg);
                            t.updated_at = crate::radial::now_secs();
                        }
                        state.set_thread_working(idx, false);
                        crate::radial::save_chat_threads(&state.ai_threads);
                        if idx == state.ai_active {
                            return scroll_chat_to_end();
                        }
                    }
                }
            }
            Task::none()
        }
```
  (The `card` attaches artifacts so Task 8's renderer can draw cards; if a run has no
  `conversation_id` match — e.g. a CLI/Mission-Control run — we still feed the dock and skip
  delivery, which is correct.)

- [ ] **Step 5: Build + overlay tests.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay 2>&1 | tail -15'`

- [ ] **Step 6: Commit.**
```bash
git add overlay-rs/src/app/update.rs overlay-rs/src/activity/mod.rs
git commit -m "feat(overlay): auto-deliver flow result to originating conversation on RunFinished"
```

---

## Task 8: inline artifact cards

**Files:**
- Modify: `overlay-rs/src/chat_ui/cards.rs` (Flow card → render artifact sub-cards)
- Modify: `overlay-rs/src/app/mod.rs` (new messages) + `overlay-rs/src/app/update.rs` (handlers)
- Test: a pure truncation/density helper in `overlay-rs/src/chat_ui/cards.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `AgentCardData::Flow.artifacts`, `RunOpenArtifact`, run workdir resolution.
- Produces: per-artifact card with open-file / open-folder / copy-path buttons + truncate/expand
  body; new messages `RunOpenFolder(String)`, `RunCopyPath(String)`, `ArtifactToggleExpand(String)`.

**Artifact path resolution:** artifacts are run-relative (e.g. `ANSWER.md`, `debug/digest.md`); the
absolute path is `runs_dir/<run_id>/<artifact>`. Resolve via the runs dir helper
(`~/.local/share/oxidemx/runs`, honoring `OXIDEMX_RUNS_DIR`/`XDG_DATA_HOME` like
`oxidemx-conductor/src/bin/conductor.rs::runs_root`). Add a small `fn run_artifact_abs(run_id, rel)`
in cards.rs (or reuse an existing overlay runs-dir helper if one exists — grep `runs` in overlay-rs
first; if found, use it).

- [ ] **Step 1: Failing test** for the density/truncation helper:
```rust
    #[test]
    fn artifact_preview_truncates_and_density_collapses() {
        // many artifacts → default collapsed (no preview lines)
        assert!(default_expanded(/*artifact_count=*/1));
        assert!(!default_expanded(5));
        // preview truncates to N chars with an ellipsis marker
        let body = "x".repeat(5000);
        let p = preview(&body, 400);
        assert!(p.len() <= 401 && p.ends_with('…'));
        assert_eq!(preview("short", 400), "short");
    }
```

- [ ] **Step 2: Run, expect FAIL.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay artifact_preview_truncates 2>&1 | tail -12'`

- [ ] **Step 3: Implement helpers** in cards.rs:
```rust
/// Default expand state for an artifact card given how many artifacts the
/// message carries: a single artifact opens expanded; many default collapsed.
pub fn default_expanded(artifact_count: usize) -> bool { artifact_count <= 2 }

/// Truncate `body` to at most `max` chars, appending '…' when cut on a char
/// boundary. Used for the collapsed preview.
pub fn preview(body: &str, max: usize) -> String {
    if body.chars().count() <= max { return body.to_string(); }
    let mut s: String = body.chars().take(max).collect();
    s.push('…');
    s
}
```

- [ ] **Step 4: Add messages** (app/mod.rs near `RunOpenArtifact`):
```rust
    /// Open the folder containing an artifact (xdg-open the dir).
    RunOpenFolder(String),
    /// Copy an absolute artifact path to the clipboard.
    RunCopyPath(String),
    /// Toggle a chat artifact card's expanded/collapsed body (keyed by abs path).
    ArtifactToggleExpand(String),
```
  Add a `pub ai_artifact_expanded: std::collections::HashSet<String>` field to `RadialState`
  (mirrors `ai_card_expanded`).

- [ ] **Step 5: Handlers** (update.rs, near `RunOpenArtifact` ~1771):
```rust
        Message::RunOpenFolder(path) => {
            let dir = std::path::Path::new(&path).parent()
                .map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from(&path));
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
            Task::none()
        }
        Message::RunCopyPath(path) => iced::clipboard::write(path),
        Message::ArtifactToggleExpand(path) => {
            if !state.ai_artifact_expanded.remove(&path) {
                state.ai_artifact_expanded.insert(path);
            }
            Task::none()
        }
```

- [ ] **Step 6: Render artifact cards** in the Flow card arm (cards.rs ~243) — replace the
  comma-joined `artifacts:` text with a card per artifact: title = filename, a button row
  (open-file → `RunOpenArtifact(abs)`, open-folder → `RunOpenFolder(abs)`, copy → `RunCopyPath(abs)`),
  and a body that is `preview(contents, …)` unless expanded, with the whole body wrapped in a
  `mouse_area(...).on_press(Message::ArtifactToggleExpand(abs))`. For `.md` artifacts render via the
  existing `render_ai_markdown`; for code, use `markdown::code_block` with `iced_highlighter`
  (mirror [body.rs:541-605](overlay-rs/src/chat_ui/body.rs#L541)). Read the file lazily with
  `std::fs::read_to_string(abs).unwrap_or_default()`; cap reads at e.g. 64 KB. Default expand state =
  `default_expanded(artifacts.len())` unless the path is in `ai_artifact_expanded`.
  (Keep the existing "Watch in Mission Control" chip.)

- [ ] **Step 7: Build + tests + clippy.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay 2>&1 | tail -15 && cargo clippy -p oxidemx-overlay 2>&1 | tail -5'`

- [ ] **Step 8: Commit.**
```bash
git add overlay-rs/src/chat_ui/cards.rs overlay-rs/src/app/mod.rs overlay-rs/src/app/update.rs overlay-rs/src/radial/mod.rs
git commit -m "feat(overlay): inline artifact cards (open/folder/copy, truncate/expand, md+code)"
```

---

## Task 9: conversation-scoped bubbles + run self-introspection

**Files:**
- Modify: `overlay-rs/src/activity/mod.rs` / `dock.rs` (filter clusters by active conversation)
- Modify: `agentd/src/tools/agent.rs` (`run_status` includes artifacts/answer for self-diagnosis)
- Test: `overlay-rs/src/activity/mod.rs` (filter) + `agentd/src/tools/agent.rs` (introspection)

**Interfaces:**
- Consumes: `RunEventView.conversation_id` (store on the cluster), `state.ai_active` → active
  conversation id.
- Produces: dock shows only the active conversation's clusters; `run_status` returns artifacts +
  inline answer so the model self-diagnoses instead of guessing.

- [ ] **Step 1: Store conversation_id on the cluster.** In `apply_run_event` (activity/mod.rs), set
  `cluster.conversation_id = view.conversation_id` when first seen (add the field to the cluster
  struct in activity/model.rs). Failing test:
```rust
    #[test]
    fn clusters_filter_by_conversation() {
        let mut st = ActivityState::default();
        st.apply_run_event(&RunEventView{ run_id:"r1".into(), conversation_id:"chat-1".into(),
            variant:"RunStarted".into(), ..Default::default() });
        st.apply_run_event(&RunEventView{ run_id:"r2".into(), conversation_id:"chat-2".into(),
            variant:"RunStarted".into(), ..Default::default() });
        assert_eq!(st.clusters_for("chat-1").len(), 1);
        assert_eq!(st.clusters_for("chat-2").len(), 1);
    }
```

- [ ] **Step 2: Run, expect FAIL.**
  `distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay clusters_filter_by_conversation 2>&1 | tail -12'`

- [ ] **Step 3: Implement** `clusters_for(&self, conv: &str) -> Vec<&RunCluster>` and the field; the
  dock view (dock.rs) takes the active conversation id and renders `clusters_for(active_conv)`
  instead of all clusters. The active conversation id =
  `state.ai_threads.get(state.ai_active).and_then(|t| t.session_id.clone()).unwrap_or_default()`.

- [ ] **Step 4: run_status self-introspection.** Failing test in agent.rs: a launcher stub whose run
  workdir has a `run.json` + `ANSWER.md`; assert `run_status` output includes the artifact list +
  a short answer excerpt. Implement by having `run_status` (when status is finished) read
  `runs_dir/<run_id>/run.json` for artifacts and include the first artifact's head (capped) so the
  model can answer "did it work + what does it say" truthfully. Keep it minimal (no new tool).

- [ ] **Step 5: Build host + overlay; clippy clean.**
  `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agentd && distrobox enter claude_development -- bash -lc 'cargo test -p oxidemx-overlay 2>&1 | tail -12'`

- [ ] **Step 6: Commit.**
```bash
git add overlay-rs/src/activity/ agentd/src/tools/agent.rs
git commit -m "feat: conversation-scoped bubbles + run_status self-introspection"
```

---

## Final: build, install, verify

- [ ] Build all host-side crates: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build --release -p oxidemx-agentd -p oxidemx-conductor`.
- [ ] Build overlay + settings in distrobox: `cargo build --release --bin oxidemx-overlay --bin oxidemx-chat --bin oxidemx-settings`.
- [ ] Install affected bins (`cp` to /tmp → `pkexec install -m755 …`): agentd, oxidemx-conductor,
  oxidemx-overlay, oxidemx-chat, oxidemx-settings. Restart `oxidemx-daemon` + agentd. Verify the
  RUNNING processes are the new builds (start time > binary mtime).
- [ ] Live smoke: enable agentd (Settings toggle now persists), launch a flow from chat, confirm:
  thinking clears when the turn ends; the flow result auto-posts to the launching conversation with
  artifact cards; switching conversations shows correct per-conversation status; a deliberately
  failing flow posts a failure notice; `run_status` reports ground truth.

## Self-review notes (coverage)

- Spec A (linkage) → Tasks 2–5,7. Spec B (per-conv state + scoped bubbles) → Tasks 6,9. Spec C
  (delivery format) → Task 7. Spec D (artifact cards) → Task 8. Spec E (truthful status + run
  introspection) → agentd path is authoritative by construction (Task 2 `run_flow` returns the
  launch message, not a scrape; in-proc scrape is unused in agentd mode) + Task 9 introspection.
  Spec F (use_agentd) → Task 1.
- The in-proc `run_flow_tool` (overlay-rs/src/ai_client/tools.rs) is left in place but unused on the
  chat path once `use_agentd` defaults on; a follow-up may delete it (out of scope to keep this plan
  additive and low-risk — note it, don't silently drop).
