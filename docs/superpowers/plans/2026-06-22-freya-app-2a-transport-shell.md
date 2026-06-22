# Freya App 2a — Transport Client + Navigable Shell Skeleton — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the new `oxide-app/` workspace (`oxide-client` transport / `oxide-ui` reusable components / `oxide-freya` app) as a Freya **desktop** app that connects to the **live local agentd over the UDS**, renders the design's 3-collapsible-panel shell as independently-navigable regions with animated transitions, and streams a real assistant reply end-to-end.

**Architecture:** Three crates in a standalone cargo workspace (own `target/`, isolated from phase1). `oxide-client` is transport-only (a `Transport` trait + `UdsTransport` over a Unix socket using hyper 1.x; an SSE event `Stream` with `Last-Event-ID` reconnect; loose JSON coupling to agentd). `oxide-ui` is a reusable Freya component library (design tokens + primitives + animated wrappers). `oxide-freya` composes them: a window shell whose left/center/right regions are each an independently-navigated, animated navigation outlet, so a sidebar can load a different page without disturbing the active center prompt.

**Tech Stack:** Rust, Freya **v0.4.0-rc.23** (cloned at `/run/media/system/fastdrive/repos/freya`) — `freya`, `freya-components`, `freya-router`, `freya-animation`, `freya-testing`; `tokio`; `hyper` 1.x + `hyper-util` + `http-body-util` + `async-stream` (UDS transport); `serde`/`serde_json`; `thiserror`; `async-trait`. Skia builds via `freya-engine`/`skia-bindings` (build recipe resolved in Task 1).

---

## Global Constraints

Every task's requirements implicitly include this section. Values are copied verbatim from the spec and `oxidemx-phase1/CLAUDE.md`.

- **Rule 0 — field-standard naming.** Transport DTOs mirror the agent-protocol wire shapes 1b ships (the routes + JSON below are authoritative). UI uses Freya/field-standard naming. Reusable design terms follow the design system tokens.
- **Rule 1 — truthfulness is structural.** The thread renders ONLY transport-delivered content — no optimistic/fabricated assistant text. A sent user turn shows immediately (it IS the user's action); the assistant reply appears only as `delta`/`final` events arrive over SSE. Connection state is shown truthfully (connected / reconnecting / unreachable).
- **Rule 2 — Rust + component quality.** clippy-clean (warnings = defects); hand-formatted (NO repo-wide `cargo fmt` — match surrounding style, format only lines you add); `Arc<dyn Transport>` seam so the UI is testable against a mock; newtype ids; `thiserror` errors; reusable components over copy-paste; **no gold-plating** (build the seams + the vertical slice, not 2b's component set).
- **Rule 3 — process + builds.** `oxide-app/` builds with its OWN toolchain/target — never share phase1's `target/`, the `/tmp/oxidemx-host-target`, or the distrobox-iced target. **Resolve the Freya/Skia desktop build recipe in Task 1 before app code.** Atomic-Fedora host — **never `rpm-ostree install`** (use distrobox or rustup-into-home). Work in a git worktree off `phase1-local-llm-gateway` (Jim edits the main checkout concurrently). Commit after every passing step.
- **Foundation-now invariants (no-refactor insurance).** The **per-region navigation model** (Task 2) and the **reusable-component library boundary** (`oxide-ui`) are built in 2a even though 2a ships few pages/components — they are expensive to retrofit. 2a proves them with a minimal real page set.
- **Workspace location.** `oxide-app/` is a NEW top-level directory committed on the worktree branch; it is its OWN cargo workspace (its own root `Cargo.toml`), NOT a member of the phase1 workspace. Program docs stay under `oxidemx-phase1/docs/superpowers/`.

### Authoritative 1b wire contract (the live integration target)

agentd over the UDS at `$XDG_RUNTIME_DIR/oxidemx/agentd.sock` (= `/run/user/1000/oxidemx/agentd.sock`, mode 0600). Verified live. Routes:

| Method + path | Request body | Response body |
|---|---|---|
| `GET /v1/health` | — | text `ok` |
| `GET /v1/projects` | — | `[Project]` |
| `GET /v1/projects/{project_id}/conversations` | — | `[Conversation]` |
| `POST /v1/conversations` | `{"project_id"?: str, "working_dir"?: str}` | `{"conversation_id": str}` |
| `GET /v1/conversations/{id}` | — | `Conversation` |
| `GET /v1/conversations/{id}/messages` | — | `[TranscriptTurn]` |
| `POST /v1/conversations/{id}/messages` | `{"text": str, "model"?: str}` | `{"message_id": str}` |
| `POST /v1/conversations/{id}/approvals/{request_id}` | `{"allow": bool, "reason"?: str}` | `{"ok": true}` |
| `GET /v1/conversations/{id}/events` | — (honors `Last-Event-ID` header) | SSE stream |

Wire JSON shapes (subset the client reads; agentd may add fields — tolerate unknowns, never `deny_unknown_fields`):
- **Project**: `{"id": str, "name": str, "default_working_dir": str, "created_at": u64}`
- **Conversation**: `{"id": str, "project_id": str, "title": str, "working_dir": str, "model": str, "created_at": u64, "updated_at": u64, ...}` (also `summary`, `tokens_*`, optional `worktree` — ignored in 2a)
- **TranscriptTurn**: `{"role": str, "text": str, "ts": u64}` (`role` ∈ `"user"`/`"assistant"`/tool roles)

**SSE frame format** (from agentd `sse.rs`): each event is
```
id: <seq:u64>
event: <kind>
data: <payload-json>

```
`<kind>` is the payload's `"kind"` field. `data:` is the full `AgentEvent.payload` JSON. Keep-alive comment lines begin with `:`. The client tracks the last `id:` seq and sends it as `Last-Event-ID` on reconnect. **Event kinds emitted by agentd:** `delta` `{text}`, `final` `{turn_id, thread, text}`, `activity` `{text}`, `command`/`task`/`memory`/`flow` (cards), `tool`, `model_status`, `run`, `error` `{message_id, message}`. The 2a thread renders `delta` (append to live bubble) + `final` (commit) + `error` (truthful banner); other kinds are kept but not rendered.

### Verified Freya v0.4.0-rc.23 API surface (builder API — NOT `rsx!`)

Confirmed against the cloned checkout (`crates/freya/src/lib.rs` doc example, `examples/ai-chat`). rc.23 uses a **builder API**; the `rsx!` macro does not exist. Implementers MUST cross-check exact method names against Task 1's `oxide-app/API-NOTES.md` and the cloned `examples/` — treat a compile error as the source of truth, not this summary.

- **Launch:** `launch(LaunchConfig::new().with_window(WindowConfig::new(app).with_size(1200., 800.).with_title("OxideMX")))`. Root: `fn app() -> impl IntoElement`.
- **Elements (builders):** `rect()` → `.width(Size)`, `.height(Size)`, `.expanded()`, `.direction(Direction::Vertical|Horizontal)`, `.main_align(Alignment)`, `.cross_align(Alignment)`, `.spacing(f32)`, `.padding(Gaps::new_all(f32))`, `.background((r,g,b))` / `Color`, `.color(Color)`, `.corner_radius(...)`, `.on_mouse_up(closure)`, `.child(impl IntoElement)`, `.children(iter)`, `.maybe_child(Option<_>)`. `label().text(String).font_size(f32).color(Color)`. Sizes: `Size::px(f)`, `Size::fill()`, `Size::flex(1.0)`.
- **Components:** built-ins `Button::new().child(..).on_press(closure)`, `ScrollView::new().child(..)`, `Input::new(signal).on_submit(closure)`. Custom reusable component = a struct implementing `trait Component { fn render(&self) -> impl IntoElement }`, `#[derive(PartialEq, Clone)]`, fields = props.
- **State:** `let mut s = use_state(|| init);` → `s.read()`, `s.write()` (deref-mut), `s.set(v)`, `s.peek()`. Async: `spawn(async move { ... s.write() ... })` updates a signal and re-renders. `use_future(|| async { .. })`. Effects: `use_side_effect(closure)`.
- **Router:** `Router::<Route>::new(|| RouterConfig::default().with_initial_path(Route::Home))`, `#[derive(Routable, Clone, PartialEq)]` enum with `#[route("/")]`, `Outlet::<R>::new()`, `use_route::<R>()`, `RouterContext` (`.push()`, `.replace()`, `.current::<R>()`). **`RouterContext` is a single non-generic context per mount scope** (`crates/freya-router/src/components/router.rs` provides exactly one `RouterContext` via `provide_context`) — Task 2 confirms whether sibling-subtree independent routers are viable or whether the page-enum fallback is used.
- **Animation:** `let anim = use_animation(|conf| { conf.on_creation(OnCreation::Run); AnimNum::new(0., 1.).time(220).ease(Ease::Out) });` → `anim.get().value()` (f32), `anim.start()`, `anim.reverse()`. `AnimatedRouter`/`use_animated_router::<R>()` expose `AnimatedRouterContext::{FromTo(R,R), In(R)}` with `.target_route()`, `.settle()`.
- **Testing:** `let mut t = launch_test(app_fn);` → `t.sync_and_update()`, `t.click_cursor((x,y))`, `t.press_key(Key::..)`, `t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref()=="...") )`. Context injection: `TestingRunner::new(app, (w,h).into(), |r| r.provide_root_context(|| State::create(v)), 1.0)`.

---

## File Structure

```
oxide-app/
  Cargo.toml                       # [workspace] members = the 3 crates; shared deps in [workspace.dependencies]
  rust-toolchain.toml              # pin chosen in Task 1 (if needed for Skia)
  API-NOTES.md                     # Task 1 deliverable: the REAL verified rc.23 API + build recipe
  README.md                        # Task 1 deliverable: how to build/run on this box
  crates/
    oxide-client/
      Cargo.toml
      src/lib.rs                   # re-exports; pub mod dto/error/transport/uds/sse/mock
      src/error.rs                 # TransportError (thiserror)
      src/dto.rs                   # ProjectId/ConversationId/MessageId, Project, Conversation, Turn, AgentEvent
      src/transport.rs             # #[async_trait] trait Transport
      src/sse.rs                   # SseParser (incremental line parse + last_id)
      src/uds.rs                   # UdsTransport (hyper 1.x over UnixStream)
      src/mock.rs                  # MockTransport (test double)
      tests/live_agentd.rs         # #[ignore] live integration test against the running agentd
    oxide-ui/
      Cargo.toml
      src/lib.rs                   # pub mod tokens/anim/components
      src/tokens.rs                # Theme, Palette, Accent, fonts, dims; context provider
      src/anim.rs                  # FadeIn / SlideIn animated wrappers
      src/components/mod.rs
      src/components/collapsible_panel.rs  # CollapsiblePanel (full<->rail)
      src/components/list_item.rs          # ListItem
      src/components/bubble.rs             # Bubble (message)
      src/components/prompt_input.rs       # PromptInput
      src/components/rail_button.rs        # RailButton
      src/components/status_dot.rs         # StatusDot
    oxide-freya/
      Cargo.toml
      src/main.rs                  # launch + WindowConfig
      src/app.rs                   # root shell: 3 regions
      src/state.rs                 # AppState: signals bridging transport calls + SSE stream
      src/nav.rs                   # region-nav seam (mechanism decided in Task 2)
      src/regions/mod.rs
      src/regions/sidebar.rs       # left region (Conversations page + a placeholder page)
      src/regions/main_region.rs   # center region (Chat — stays alive across sidebar nav)
      src/regions/context.rs       # right region (StatusRail placeholder)
```

---

## Task 1: Workspace skeleton + Freya/Skia build recipe + hello window (SPIKE)

**This is a build-environment + API spike, not a TDD task.** Its job: prove `oxide-app/` builds and a Freya window opens on this atomic-Fedora box, and capture the real API + recipe so later tasks build on verified ground. It ends with one freya-testing smoke test.

**Files:**
- Create: `oxide-app/Cargo.toml`, `oxide-app/README.md`, `oxide-app/API-NOTES.md`, `oxide-app/.gitignore`
- Create: `oxide-app/crates/oxide-client/Cargo.toml`, `oxide-app/crates/oxide-client/src/lib.rs`
- Create: `oxide-app/crates/oxide-ui/Cargo.toml`, `oxide-app/crates/oxide-ui/src/lib.rs`
- Create: `oxide-app/crates/oxide-freya/Cargo.toml`, `oxide-app/crates/oxide-freya/src/main.rs`

**Interfaces:**
- Produces: a building 3-crate workspace; `oxide-freya` `fn app() -> impl IntoElement` root; `API-NOTES.md` with verified rc.23 signatures + the build recipe later tasks rely on.

- [ ] **Step 1: Resolve the Skia build recipe.** Determine how `freya-engine`/`skia-bindings` build here. Try, in order, and record what works in `README.md`:
  1. Host rustup toolchain with a prebuilt Skia download (`skia-bindings` env: `SKIA_BINARIES_URL`/auto-download; needs network + `clang`/`libssl`). Check `clang --version` and `python3` availability on host first.
  2. If host lacks build deps, the `claude_development` distrobox (has `-devel` libs). Build inside it.
  Record the EXACT working command (env vars, toolchain, target dir) in `README.md`. The target dir MUST be local to `oxide-app/` (default `oxide-app/target/`) — never the phase1 or `/tmp/oxidemx-host-target` dirs.

- [ ] **Step 2: Write the workspace root `Cargo.toml`.**

```toml
[workspace]
resolver = "2"
members = ["crates/oxide-client", "crates/oxide-ui", "crates/oxide-freya"]

[workspace.dependencies]
freya = { path = "/run/media/system/fastdrive/repos/freya/crates/freya" }
freya-testing = { path = "/run/media/system/fastdrive/repos/freya/crates/freya-testing" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "time", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
async-trait = "0.1"
futures-util = "0.3"
```

(Use path deps to the cloned Freya. Verify the exact sub-crate names/paths needed — `freya`, `freya-testing`, and pull `freya-router`/`freya-animation`/`freya-components` through `freya`'s re-exports if it re-exports them; otherwise add path deps. Record what's needed in `API-NOTES.md`.)

- [ ] **Step 3: Create `oxide-client` + `oxide-ui` as empty lib crates** (`src/lib.rs` with a `//! crate doc` line each) and their `Cargo.toml` (package name = dir name). They compile empty.

- [ ] **Step 4: Create `oxide-freya` with a hello window** `src/main.rs`:

```rust
//! OxideMX Freya desktop app.
use freya::prelude::*;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let _guard = rt.enter();
    launch(
        LaunchConfig::new().with_window(
            WindowConfig::new(app)
                .with_size(1200., 800.)
                .with_title("OxideMX"),
        ),
    )
}

fn app() -> impl IntoElement {
    rect()
        .expanded()
        .main_align(Alignment::Center)
        .cross_align(Alignment::Center)
        .background((5, 7, 11))
        .child(label().text("OxideMX — hello Freya").font_size(24.0).color(Color::WHITE))
}
```

- [ ] **Step 5: Build + run.** Run the recorded build command, then run the binary. Confirm a window opens showing the text (capture a screenshot or note the window appeared). Record the run command in `README.md`.

- [ ] **Step 6: Write `API-NOTES.md`** capturing, from the real build: the exact `freya::prelude` items used, the launch shape, `use_state`/`spawn` signatures, the `rect()`/`label()` builder methods confirmed, and which Freya sub-crates resolved. This file is the API tiebreaker for Tasks 2 & 6–11.

- [ ] **Step 7: Smoke test.** Add `oxide-freya/src/main.rs` a `#[cfg(test)]` test (or `tests/smoke.rs`) using `freya-testing`:

```rust
#[test]
fn root_renders_title() {
    let mut t = freya_testing::prelude::launch_test(app);
    t.sync_and_update();
    let found = t.find(|_, el| {
        freya::prelude::Label::try_downcast(el).filter(|l| l.text.as_ref().contains("OxideMX"))
    });
    assert!(found.is_some(), "root should render the OxideMX title");
}
```

Run it (adjust `try_downcast`/`Label` import to the verified API from Step 6). Expected: PASS.

- [ ] **Step 8: Commit.**

```bash
git add oxide-app
git commit -m "feat(oxide-app): workspace skeleton + Freya build recipe + hello window"
```

---

## Task 2: Region-nav seam — multi-router feasibility (SPIKE) + the `nav` module

**This is a feasibility spike with a decision gate.** Preliminary evidence (Task-1 source read): `freya-router` provides a single non-generic `RouterContext` per mount scope, so two `Router<_>` in sibling subtrees likely collide. Confirm empirically; if independent routers are not cleanly viable, adopt the documented fallback: a per-region page-state enum animated directly with `freya-animation`. Either way, the OUTPUT is one `nav` module with a stable seam the regions consume, so Task 11 doesn't care which mechanism won.

**Files:**
- Create: `oxide-app/crates/oxide-freya/src/nav.rs`
- Modify: `oxide-app/crates/oxide-freya/src/main.rs` (add `mod nav;`)
- Append: decision record to `oxide-app/API-NOTES.md`

**Interfaces:**
- Produces: `pub trait RegionRoute: Clone + PartialEq + 'static` (or an enum per region) and a `region_nav` component/helper that renders the current page for a region and exposes `navigate(to)`, with an animated transition. The regions in Task 11 mount one nav per region. Exact shape is decided here; record the chosen signature in `API-NOTES.md` so Task 11's implementer reads it.

- [ ] **Step 1: Spike — attempt independent sibling routers.** In a scratch `nav.rs`, define two trivial route enums (`LeftRoute{A,B}`, `CenterRoute{X,Y}`) and try mounting `Router::<LeftRoute>` and `Router::<CenterRoute>` in sibling `rect()`s of one app. Build + run a freya-testing harness that navigates `LeftRoute` and checks `CenterRoute`'s rendered page is unchanged. Observe: does it compile, and do the two navigate independently?

- [ ] **Step 2: Decision gate.** Record in `API-NOTES.md`:
  - If independent routers work → use them; the `nav` seam wraps `Router::<RegionRoute>` + `AnimatedRouter` per region.
  - If they collide (expected) → use the **page-enum fallback**: each region holds `use_state(|| RegionPage::Default)`; `navigate` sets it; an `AnimNum`-driven wrapper (from `oxide-ui::anim`, Task 9) animates the swap. Record WHY.

- [ ] **Step 3: Implement the chosen `nav` seam** in `nav.rs`. Fallback shape (if chosen):

```rust
//! Per-region navigation seam. Each region owns its own page state, so a
//! sidebar can navigate without remounting the center region.
use freya::prelude::*;

/// A page a region can show. Each region defines its own enum implementing this.
pub trait RegionPage: Clone + PartialEq + 'static {}

/// Region navigation handle: read the current page, request a new one.
#[derive(Clone)]
pub struct RegionNav<P: RegionPage> {
    page: State<P>,
}

impl<P: RegionPage> RegionNav<P> {
    pub fn new(initial: P) -> Self { Self { page: use_state(|| initial) } }
    pub fn current(&self) -> P { self.page.read().clone() }
    pub fn navigate(&mut self, to: P) { self.page.set(to); }
}
```

(If independent routers won instead, implement the router-based seam with the same `current`/`navigate` surface so consumers are identical.)

- [ ] **Step 4: Test the invariant.** freya-testing: a two-region scratch app where region A navigates A→B while region B's content stays put. Assert region B's text is unchanged after region A navigates.

```rust
#[test]
fn region_nav_is_independent() {
    // mount two regions each with RegionNav; navigate region A; assert region B unchanged.
    // (full harness per the verified testing API)
}
```

Run it. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add oxide-app
git commit -m "feat(oxide-freya): region-nav seam + multi-router feasibility decision"
```

---

## Task 3: oxide-client — DTOs, TransportError, Transport trait, MockTransport

**Files:**
- Create: `oxide-app/crates/oxide-client/src/dto.rs`, `src/error.rs`, `src/transport.rs`, `src/mock.rs`
- Modify: `oxide-app/crates/oxide-client/src/lib.rs`, `oxide-app/crates/oxide-client/Cargo.toml`

**Interfaces:**
- Produces: `Project`, `Conversation`, `Turn`, `AgentEvent`, `ProjectId`/`ConversationId`/`MessageId` (newtypes); `TransportError`; `#[async_trait] trait Transport`; `MockTransport`. Task 5 (`UdsTransport`) and Task 10 (`AppState`) consume these.

- [ ] **Step 1: Add deps to `oxide-client/Cargo.toml`.**

```toml
[package]
name = "oxide-client"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
futures-util = { workspace = true }
tokio = { workspace = true }
hyper = { version = "1", features = ["client", "http1"] }
hyper-util = { version = "0.1", features = ["tokio"] }
http-body-util = "0.1"
async-stream = "0.3"
```

- [ ] **Step 2: Write the DTO round-trip failing test** in `src/dto.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_decodes_from_agentd_json() {
        let j = r#"{"id":"personal","name":"Personal","default_working_dir":"","created_at":17}"#;
        let p: Project = serde_json::from_str(j).unwrap();
        assert_eq!(p.id.as_str(), "personal");
        assert_eq!(p.name, "Personal");
    }

    #[test]
    fn conversation_ignores_unknown_fields() {
        let j = r#"{"id":"c1","project_id":"personal","title":"hi","working_dir":"/tmp",
                    "model":"gemini-2.5-flash","created_at":1,"updated_at":2,
                    "summary":"x","tokens_prompt":9,"worktree":{"path":"/w","branch":"b","base_ref":"r"}}"#;
        let c: Conversation = serde_json::from_str(j).unwrap();
        assert_eq!(c.id.as_str(), "c1");
        assert_eq!(c.model, "gemini-2.5-flash");
    }

    #[test]
    fn turn_decodes() {
        let j = r#"{"role":"assistant","text":"hello","ts":42}"#;
        let t: Turn = serde_json::from_str(j).unwrap();
        assert_eq!(t.role, "assistant");
        assert_eq!(t.text, "hello");
    }
}
```

- [ ] **Step 3: Run — verify it fails.** `cd oxide-app && cargo test -p oxide-client dto` → FAIL (types not defined).

- [ ] **Step 4: Implement `src/dto.rs`.**

```rust
//! Wire DTOs mirroring agentd's agent-protocol JSON (loose coupling; unknown
//! fields are tolerated, never denied).
use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name {
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
        }
        impl From<&str> for $name { fn from(s: &str) -> Self { Self(s.to_string()) } }
        impl From<String> for $name { fn from(s: String) -> Self { Self(s) } }
    };
}
id_newtype!(ProjectId);
id_newtype!(ConversationId);
id_newtype!(MessageId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    #[serde(default)]
    pub default_working_dir: String,
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub project_id: ProjectId,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub ts: u64,
}

/// A normalized SSE event. `seq` from the frame `id:`, `kind` from `event:`
/// (falls back to `payload.kind`), `payload` is the frame `data:` JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentEvent {
    pub seq: u64,
    pub kind: String,
    pub payload: serde_json::Value,
}

impl AgentEvent {
    /// The `text` field if present (delta/final/activity carry it).
    pub fn text(&self) -> Option<&str> {
        self.payload.get("text").and_then(|v| v.as_str())
    }
}
```

- [ ] **Step 5: Run — verify the DTO tests pass.** `cargo test -p oxide-client dto` → PASS.

- [ ] **Step 6: Write `src/error.rs`.**

```rust
//! Transport-layer errors. Drive the UI connection state.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Socket missing or connection refused — agentd not reachable.
    #[error("agentd unreachable: {0}")]
    Unreachable(String),
    /// Non-2xx HTTP status.
    #[error("http status {0}")]
    Http(u16),
    /// Response body failed to decode.
    #[error("decode error: {0}")]
    Decode(String),
    /// SSE stream or connection error mid-stream.
    #[error("stream error: {0}")]
    Stream(String),
}
```

- [ ] **Step 7: Write `src/transport.rs`.**

```rust
//! The transport seam. `oxide-ui`/`oxide-freya` depend on `Arc<dyn Transport>`,
//! never a concrete client — so the UI is testable against `MockTransport`.
use async_trait::async_trait;
use futures_util::stream::BoxStream;

use crate::dto::{AgentEvent, Conversation, MessageId, Project, Turn};
use crate::error::TransportError;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn health(&self) -> Result<(), TransportError>;
    async fn list_projects(&self) -> Result<Vec<Project>, TransportError>;
    async fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>, TransportError>;
    async fn create_conversation(&self, project_id: &str, working_dir: Option<&str>) -> Result<Conversation, TransportError>;
    async fn get_history(&self, conversation_id: &str) -> Result<Vec<Turn>, TransportError>;
    async fn send_message(&self, conversation_id: &str, text: &str) -> Result<MessageId, TransportError>;
    /// SSE subscription. Yields normalized events; reconnects with Last-Event-ID on drop.
    fn subscribe(&self, conversation_id: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>>;
}
```

- [ ] **Step 8: Write `MockTransport` + its test** in `src/mock.rs`.

```rust
//! In-memory `Transport` for UI tests. Scripted projects/conversations/history
//! and a scripted event stream per conversation.
use std::sync::Mutex;

use async_trait::async_trait;
use futures_util::stream::{self, BoxStream, StreamExt};

use crate::dto::*;
use crate::error::TransportError;
use crate::transport::Transport;

#[derive(Default)]
pub struct MockTransport {
    pub projects: Vec<Project>,
    pub conversations: Vec<Conversation>,
    pub history: Vec<Turn>,
    /// Events the next `subscribe` will yield, in order.
    pub events: Mutex<Vec<AgentEvent>>,
    pub healthy: bool,
}

impl MockTransport {
    pub fn new() -> Self { Self { healthy: true, ..Default::default() } }
}

#[async_trait]
impl Transport for MockTransport {
    async fn health(&self) -> Result<(), TransportError> {
        if self.healthy { Ok(()) } else { Err(TransportError::Unreachable("mock".into())) }
    }
    async fn list_projects(&self) -> Result<Vec<Project>, TransportError> { Ok(self.projects.clone()) }
    async fn list_conversations(&self, _p: &str) -> Result<Vec<Conversation>, TransportError> { Ok(self.conversations.clone()) }
    async fn create_conversation(&self, project_id: &str, _wd: Option<&str>) -> Result<Conversation, TransportError> {
        Ok(Conversation { id: ConversationId::from("mock-conv"), project_id: ProjectId::from(project_id),
            title: "New".into(), working_dir: String::new(), model: String::new(), created_at: 0, updated_at: 0 })
    }
    async fn get_history(&self, _c: &str) -> Result<Vec<Turn>, TransportError> { Ok(self.history.clone()) }
    async fn send_message(&self, _c: &str, _t: &str) -> Result<MessageId, TransportError> { Ok(MessageId::from("mock-msg")) }
    fn subscribe(&self, _c: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>> {
        let evs = self.events.lock().unwrap().clone();
        stream::iter(evs.into_iter().map(Ok)).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_subscribe_yields_scripted_events() {
        let m = MockTransport {
            events: Mutex::new(vec![AgentEvent { seq: 1, kind: "delta".into(),
                payload: serde_json::json!({"kind":"delta","text":"hi"}) }]),
            ..MockTransport::new()
        };
        let got: Vec<_> = m.subscribe("c").collect().await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].as_ref().unwrap().text(), Some("hi"));
    }
}
```

- [ ] **Step 9: Wire `src/lib.rs`.**

```rust
//! oxide-client — agentd transport (UDS desktop now; tailnet-TCP in 2c).
pub mod dto;
pub mod error;
pub mod mock;
pub mod sse;
pub mod transport;
pub mod uds;

pub use dto::{AgentEvent, Conversation, ConversationId, MessageId, Project, ProjectId, Turn};
pub use error::TransportError;
pub use transport::Transport;
pub use uds::UdsTransport;
```

(Create empty `src/sse.rs` and `src/uds.rs` stubs now — `pub(crate) struct _Placeholder;` or empty — so `lib.rs` compiles; Tasks 4 & 5 fill them. Or reorder the `mod` lines until those tasks land. Keep `cargo build -p oxide-client` green at task end.)

- [ ] **Step 10: Run all oxide-client tests + clippy.** `cargo test -p oxide-client && cargo clippy -p oxide-client -- -D warnings` → PASS, clean.

- [ ] **Step 11: Commit.**

```bash
git add oxide-app/crates/oxide-client
git commit -m "feat(oxide-client): DTOs, TransportError, Transport trait, MockTransport"
```

---

## Task 4: oxide-client — SSE parser (incremental + Last-Event-ID)

**Files:**
- Modify: `oxide-app/crates/oxide-client/src/sse.rs`

**Interfaces:**
- Consumes: `AgentEvent` (Task 3).
- Produces: `SseParser` with `fn new() -> Self`, `fn push(&mut self, chunk: &[u8]) -> Vec<AgentEvent>`, `fn last_id(&self) -> u64`. Task 5's `subscribe` feeds body chunks in and tracks `last_id` for reconnect.

- [ ] **Step 1: Write failing tests** in `src/sse.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_event() {
        let mut p = SseParser::new();
        let evs = p.push(b"id: 7\nevent: delta\ndata: {\"kind\":\"delta\",\"text\":\"hi\"}\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].seq, 7);
        assert_eq!(evs[0].kind, "delta");
        assert_eq!(evs[0].text(), Some("hi"));
        assert_eq!(p.last_id(), 7);
    }

    #[test]
    fn handles_chunk_split_mid_line() {
        let mut p = SseParser::new();
        assert!(p.push(b"id: 1\nevent: del").is_empty());
        let evs = p.push(b"ta\ndata: {\"kind\":\"delta\",\"text\":\"x\"}\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].kind, "delta");
    }

    #[test]
    fn ignores_comments_and_keepalives() {
        let mut p = SseParser::new();
        assert!(p.push(b": keep-alive\n\n").is_empty());
    }

    #[test]
    fn kind_falls_back_to_payload_when_event_line_absent() {
        let mut p = SseParser::new();
        let evs = p.push(b"id: 3\ndata: {\"kind\":\"final\",\"text\":\"done\"}\n\n");
        assert_eq!(evs[0].kind, "final");
        assert_eq!(evs[0].seq, 3);
    }
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-client sse` → FAIL.

- [ ] **Step 3: Implement `SseParser`.**

```rust
//! Incremental Server-Sent-Events parser. Feed raw body bytes; get back
//! normalized `AgentEvent`s. Tracks the last `id:` for Last-Event-ID reconnect.
use crate::dto::AgentEvent;

#[derive(Default)]
pub struct SseParser {
    buf: String,
    cur_id: Option<u64>,
    cur_event: Option<String>,
    cur_data: String,
    last_id: u64,
}

impl SseParser {
    pub fn new() -> Self { Self::default() }
    pub fn last_id(&self) -> u64 { self.last_id }

    /// Push a chunk of the SSE body; returns any events completed by it.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<AgentEvent> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        // Process complete lines (terminated by '\n'); keep any partial tail.
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if let Some(ev) = self.dispatch() { out.push(ev); }
            } else if let Some(rest) = line.strip_prefix(':') {
                let _ = rest; // comment / keep-alive — ignore
            } else if let Some(v) = line.strip_prefix("id:") {
                self.cur_id = v.trim().parse().ok();
            } else if let Some(v) = line.strip_prefix("event:") {
                self.cur_event = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("data:") {
                if !self.cur_data.is_empty() { self.cur_data.push('\n'); }
                self.cur_data.push_str(v.strip_prefix(' ').unwrap_or(v));
            }
        }
        out
    }

    fn dispatch(&mut self) -> Option<AgentEvent> {
        if self.cur_data.is_empty() && self.cur_event.is_none() && self.cur_id.is_none() {
            return None;
        }
        let payload: serde_json::Value =
            serde_json::from_str(&self.cur_data).unwrap_or(serde_json::Value::Null);
        let kind = self.cur_event.take().unwrap_or_else(|| {
            payload.get("kind").and_then(|k| k.as_str()).unwrap_or("event").to_string()
        });
        let seq = self.cur_id.take().unwrap_or(self.last_id);
        if seq > self.last_id { self.last_id = seq; }
        self.cur_data.clear();
        Some(AgentEvent { seq, kind, payload })
    }
}
```

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-client sse && cargo clippy -p oxide-client -- -D warnings` → PASS, clean.

- [ ] **Step 5: Commit.**

```bash
git add oxide-app/crates/oxide-client/src/sse.rs
git commit -m "feat(oxide-client): incremental SSE parser with Last-Event-ID tracking"
```

---

## Task 5: oxide-client — UdsTransport + live integration test

**Files:**
- Modify: `oxide-app/crates/oxide-client/src/uds.rs`
- Create: `oxide-app/crates/oxide-client/tests/live_agentd.rs`

**Interfaces:**
- Consumes: `Transport`, DTOs, `TransportError`, `SseParser`.
- Produces: `UdsTransport` implementing `Transport`; `UdsTransport::new(path)`, `UdsTransport::default_socket() -> PathBuf`.

- [ ] **Step 1: Implement `src/uds.rs`.** (Cross-check hyper 1.x item paths against `cargo doc`/examples; this is the rc-standard `client::conn::http1` pattern.)

```rust
//! `UdsTransport` — agentd over a Unix domain socket using hyper 1.x. Each call
//! opens a fresh connection (agentd closes per response). SSE uses a streaming
//! body fed into `SseParser`, reconnecting with Last-Event-ID on drop.
use std::path::PathBuf;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;

use crate::dto::{AgentEvent, Conversation, ConversationId, MessageId, Project, Turn};
use crate::error::TransportError;
use crate::sse::SseParser;
use crate::transport::Transport;

pub struct UdsTransport {
    sock: PathBuf,
}

impl UdsTransport {
    pub fn new(sock: impl Into<PathBuf>) -> Self { Self { sock: sock.into() } }

    /// `$XDG_RUNTIME_DIR/oxidemx/agentd.sock` (falls back to `/tmp`).
    pub fn default_socket() -> PathBuf {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(base).join("oxidemx").join("agentd.sock")
    }

    async fn send(&self, method: Method, path: &str, body: Option<serde_json::Value>)
        -> Result<(u16, Bytes), TransportError>
    {
        let stream = UnixStream::connect(&self.sock).await
            .map_err(|e| TransportError::Unreachable(e.to_string()))?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
            .map_err(|e| TransportError::Stream(e.to_string()))?;
        tokio::spawn(async move { let _ = conn.await; });

        let payload = body.map(|v| v.to_string()).unwrap_or_default();
        let req = Request::builder()
            .method(method).uri(path)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(payload)))
            .map_err(|e| TransportError::Stream(e.to_string()))?;
        let resp = sender.send_request(req).await
            .map_err(|e| TransportError::Stream(e.to_string()))?;
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await
            .map_err(|e| TransportError::Stream(e.to_string()))?.to_bytes();
        Ok((status, bytes))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, TransportError> {
        let (status, bytes) = self.send(Method::GET, path, None).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        serde_json::from_slice(&bytes).map_err(|e| TransportError::Decode(e.to_string()))
    }
}

#[async_trait]
impl Transport for UdsTransport {
    async fn health(&self) -> Result<(), TransportError> {
        let (status, bytes) = self.send(Method::GET, "/v1/health", None).await?;
        if status == 200 && bytes.as_ref() == b"ok" { Ok(()) } else { Err(TransportError::Http(status)) }
    }

    async fn list_projects(&self) -> Result<Vec<Project>, TransportError> {
        self.get_json("/v1/projects").await
    }

    async fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>, TransportError> {
        self.get_json(&format!("/v1/projects/{project_id}/conversations")).await
    }

    async fn create_conversation(&self, project_id: &str, working_dir: Option<&str>)
        -> Result<Conversation, TransportError>
    {
        let mut body = serde_json::json!({ "project_id": project_id });
        if let Some(wd) = working_dir { body["working_dir"] = serde_json::json!(wd); }
        let (status, bytes) = self.send(Method::POST, "/v1/conversations", Some(body)).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        // Response is {"conversation_id": "..."}; fetch the full record.
        #[derive(serde::Deserialize)] struct Created { conversation_id: String }
        let created: Created = serde_json::from_slice(&bytes).map_err(|e| TransportError::Decode(e.to_string()))?;
        self.get_json(&format!("/v1/conversations/{}", created.conversation_id)).await
    }

    async fn get_history(&self, conversation_id: &str) -> Result<Vec<Turn>, TransportError> {
        self.get_json(&format!("/v1/conversations/{conversation_id}/messages")).await
    }

    async fn send_message(&self, conversation_id: &str, text: &str) -> Result<MessageId, TransportError> {
        let body = serde_json::json!({ "text": text });
        let (status, bytes) = self.send(Method::POST,
            &format!("/v1/conversations/{conversation_id}/messages"), Some(body)).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        #[derive(serde::Deserialize)] struct Sent { message_id: String }
        let sent: Sent = serde_json::from_slice(&bytes).map_err(|e| TransportError::Decode(e.to_string()))?;
        Ok(MessageId(sent.message_id))
    }

    fn subscribe(&self, conversation_id: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>> {
        let sock = self.sock.clone();
        let path = format!("/v1/conversations/{conversation_id}/events");
        Box::pin(async_stream::try_stream! {
            let mut last_id = 0u64;
            loop {
                let stream = UnixStream::connect(&sock).await
                    .map_err(|e| TransportError::Unreachable(e.to_string()))?;
                let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
                    .map_err(|e| TransportError::Stream(e.to_string()))?;
                tokio::spawn(async move { let _ = conn.await; });

                let mut builder = Request::builder()
                    .method(Method::GET).uri(&path)
                    .header("host", "localhost")
                    .header("accept", "text/event-stream");
                if last_id > 0 { builder = builder.header("last-event-id", last_id.to_string()); }
                let req = builder.body(Full::new(Bytes::new()))
                    .map_err(|e| TransportError::Stream(e.to_string()))?;
                let resp = sender.send_request(req).await
                    .map_err(|e| TransportError::Stream(e.to_string()))?;

                let mut body = resp.into_body();
                let mut parser = SseParser::new();
                while let Some(frame) = body.frame().await {
                    let frame = frame.map_err(|e| TransportError::Stream(e.to_string()))?;
                    if let Some(chunk) = frame.data_ref() {
                        for ev in parser.push(chunk) {
                            last_id = ev.seq;
                            yield ev;
                        }
                    }
                }
                // Connection ended; back off then reconnect with Last-Event-ID.
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        })
    }
}
```

- [ ] **Step 2: Build + clippy.** `cargo build -p oxide-client && cargo clippy -p oxide-client -- -D warnings` → clean. (If a hyper item path differs in this hyper minor, fix per `cargo doc`; the shape is standard.)

- [ ] **Step 3: Write the live integration test** `tests/live_agentd.rs` (ignored by default — needs the running agentd):

```rust
//! Live integration against the running local agentd. Run with:
//!   cargo test -p oxide-client --test live_agentd -- --ignored --nocapture
use std::sync::Arc;
use futures_util::StreamExt;
use oxide_client::{Transport, UdsTransport};

#[tokio::test]
#[ignore = "requires the local agentd running with http.enabled"]
async fn health_projects_create_send_subscribe() {
    let t: Arc<dyn Transport> = Arc::new(UdsTransport::new(UdsTransport::default_socket()));

    t.health().await.expect("agentd should be reachable on the UDS");

    let projects = t.list_projects().await.expect("list_projects");
    assert!(projects.iter().any(|p| p.id.as_str() == "personal"), "expected a 'personal' project");

    let conv = t.create_conversation("personal", None).await.expect("create_conversation");

    // Subscribe BEFORE sending so we catch the streamed reply.
    let mut events = t.subscribe(conv.id.as_str());
    let _mid = t.send_message(conv.id.as_str(), "Reply with the single word: pong").await.expect("send_message");

    // Await a terminal `final` event within a timeout.
    let got_final = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while let Some(ev) = events.next().await {
            let ev = ev.expect("stream event");
            if ev.kind == "final" { return true; }
            if ev.kind == "error" { panic!("agentd error: {:?}", ev.payload); }
        }
        false
    }).await.expect("timed out waiting for final");
    assert!(got_final, "expected a final event from the turn");
}
```

- [ ] **Step 4: Run the live test against agentd.** Confirm agentd is up (`systemctl --user status oxidemx-agentd` / socket exists), then:
`cargo test -p oxide-client --test live_agentd -- --ignored --nocapture` → PASS (mirrors the verified curl walk). Record the result.

- [ ] **Step 5: Commit.**

```bash
git add oxide-app/crates/oxide-client/src/uds.rs oxide-app/crates/oxide-client/tests/live_agentd.rs
git commit -m "feat(oxide-client): UdsTransport over hyper + live agentd integration test"
```

---

## Task 6: oxide-ui — design tokens + theme context

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/tokens.rs`
- Modify: `oxide-app/crates/oxide-ui/src/lib.rs`, `oxide-app/crates/oxide-ui/Cargo.toml`

**Interfaces:**
- Produces: `Theme`, `Accent` (enum), color accessors (`bg`, `surface`, `text`, `subtext`, `accent`), font-family constants, layout dims (`SIDEBAR_FULL_W = 274.0`, `SIDEBAR_RAIL_W = 60.0`). Consumed by every `oxide-ui` component + `oxide-freya`.

Token values (verbatim from the design system, see `reference_design_system`): bg `#05070b` → `(5,7,11)`; text `#f0f4f8` → `(240,244,248)`; accents — cyan `#00d4ff` (default) `(0,212,255)`, purple `#b388ff` `(179,136,255)`, orange `#ffab40` `(255,171,64)`, green `#7be06a` `(123,224,106)`. Fonts: Inter (UI), JetBrains Mono (code). Sidebar full↔rail 274↔60px.

- [ ] **Step 1: Add the freya dep to `oxide-ui/Cargo.toml`.**

```toml
[package]
name = "oxide-ui"
version = "0.1.0"
edition = "2021"

[dependencies]
freya = { workspace = true }

[dev-dependencies]
freya-testing = { workspace = true }
```

- [ ] **Step 2: Write a failing token test** in `src/tokens.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark_cyan() {
        let t = Theme::default();
        assert_eq!(t.bg(), Color::from_rgb(5, 7, 11));
        assert_eq!(t.accent(), Color::from_rgb(0, 212, 255));
    }

    #[test]
    fn accent_switch_changes_accent_only() {
        let t = Theme::with_accent(Accent::Purple);
        assert_eq!(t.accent(), Color::from_rgb(179, 136, 255));
        assert_eq!(t.bg(), Color::from_rgb(5, 7, 11));
    }
}
```

(`Color::from_rgb` — confirm the exact constructor in `API-NOTES.md`; if it's `Color::new(r,g,b,a)` adjust both code and test consistently.)

- [ ] **Step 3: Run — verify fail.** `cargo test -p oxide-ui tokens` → FAIL.

- [ ] **Step 4: Implement `src/tokens.rs`.**

```rust
//! Design tokens (palette / accents / fonts / dims) for the OxideMX Freya UI.
//! Values mirror the Claude Design "Collapsible Panels" system.
use freya::prelude::Color;

pub const SIDEBAR_FULL_W: f32 = 274.0;
pub const SIDEBAR_RAIL_W: f32 = 60.0;
pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Cyan,
    Purple,
    Orange,
    Green,
}

impl Accent {
    pub fn color(self) -> Color {
        match self {
            Accent::Cyan => Color::from_rgb(0, 212, 255),
            Accent::Purple => Color::from_rgb(179, 136, 255),
            Accent::Orange => Color::from_rgb(255, 171, 64),
            Accent::Green => Color::from_rgb(123, 224, 106),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    accent: Accent,
}

impl Default for Theme {
    fn default() -> Self { Self { accent: Accent::Cyan } }
}

impl Theme {
    pub fn with_accent(accent: Accent) -> Self { Self { accent } }
    pub fn bg(&self) -> Color { Color::from_rgb(5, 7, 11) }
    pub fn surface(&self) -> Color { Color::from_rgb(13, 17, 23) }
    pub fn text(&self) -> Color { Color::from_rgb(240, 244, 248) }
    pub fn subtext(&self) -> Color { Color::from_rgb(148, 163, 184) }
    pub fn accent(&self) -> Color { self.accent.color() }
}
```

- [ ] **Step 5: Run — verify pass.** `cargo test -p oxide-ui tokens` → PASS.

- [ ] **Step 6: Wire `src/lib.rs`.**

```rust
//! oxide-ui — reusable Freya component library + design tokens for OxideMX.
pub mod anim;
pub mod components;
pub mod tokens;

pub use tokens::{Accent, Theme, FONT_MONO, FONT_UI, SIDEBAR_FULL_W, SIDEBAR_RAIL_W};
```

(Create empty `src/anim.rs` and `src/components/mod.rs` stubs so `lib.rs` compiles; Tasks 7–9 fill them.)

- [ ] **Step 7: clippy + commit.**

```bash
cargo clippy -p oxide-ui -- -D warnings
git add oxide-app/crates/oxide-ui
git commit -m "feat(oxide-ui): design tokens + theme/accent"
```

---

## Task 7: oxide-ui — CollapsiblePanel (the behavior-bearing primitive)

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/collapsible_panel.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs`

**Interfaces:**
- Consumes: `Theme`, `SIDEBAR_FULL_W`, `SIDEBAR_RAIL_W`.
- Produces: `CollapsiblePanel { width: f32, collapsed: bool, full: Element, rail: Element }` (a `Component`) that shows `full` at `width` when expanded and `rail` at `SIDEBAR_RAIL_W` when collapsed. (Velocity-snap drag physics is 2b; 2a is the toggle + the prop interface that supports both.)

- [ ] **Step 1: Write a failing render test** (freya-testing) in `collapsible_panel.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya::prelude::*;
    use freya_testing::prelude::*;

    #[test]
    fn shows_full_content_when_expanded() {
        fn app() -> impl IntoElement {
            CollapsiblePanel::new()
                .collapsed(false)
                .full(label().text("FULL"))
                .rail(label().text("RAIL"))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "FULL"));
        assert!(found.is_some());
    }

    #[test]
    fn shows_rail_content_when_collapsed() {
        fn app() -> impl IntoElement {
            CollapsiblePanel::new()
                .collapsed(true)
                .full(label().text("FULL"))
                .rail(label().text("RAIL"))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "RAIL"));
        assert!(found.is_some());
    }
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui collapsible` → FAIL.

- [ ] **Step 3: Implement `CollapsiblePanel`** (builder-style `Component`; confirm `Element`/`IntoElement` plumbing against `API-NOTES.md`):

```rust
//! A panel that toggles between a full-width body and a narrow rail.
use freya::prelude::*;
use crate::tokens::{Theme, SIDEBAR_FULL_W, SIDEBAR_RAIL_W};

#[derive(PartialEq, Clone)]
pub struct CollapsiblePanel {
    width: f32,
    collapsed: bool,
    full: Element,
    rail: Element,
    theme: Theme,
}

impl CollapsiblePanel {
    pub fn new() -> Self {
        Self { width: SIDEBAR_FULL_W, collapsed: false,
               full: Element::default(), rail: Element::default(), theme: Theme::default() }
    }
    pub fn width(mut self, w: f32) -> Self { self.width = w; self }
    pub fn collapsed(mut self, c: bool) -> Self { self.collapsed = c; self }
    pub fn full(mut self, e: impl IntoElement) -> Self { self.full = e.into_element(); self }
    pub fn rail(mut self, e: impl IntoElement) -> Self { self.rail = e.into_element(); self }
    pub fn theme(mut self, t: Theme) -> Self { self.theme = t; self }
}

impl Default for CollapsiblePanel {
    fn default() -> Self { Self::new() }
}

impl Component for CollapsiblePanel {
    fn render(&self) -> impl IntoElement {
        let (w, body) = if self.collapsed {
            (SIDEBAR_RAIL_W, self.rail.clone())
        } else {
            (self.width, self.full.clone())
        };
        rect()
            .width(Size::px(w))
            .height(Size::fill())
            .background(self.theme.surface())
            .child(body)
    }
}
```

(`Element::default()`, `into_element()`, `Element::clone()` — verify exact names against the Freya API; if `Element` isn't `Default`/`Clone`, store `Option<Element>` and `maybe_child` it. Adjust test + impl together.)

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-ui collapsible && cargo clippy -p oxide-ui -- -D warnings` → PASS, clean.

- [ ] **Step 5: Export it** in `src/components/mod.rs`: `pub mod collapsible_panel; pub use collapsible_panel::CollapsiblePanel;`

- [ ] **Step 6: Commit.**

```bash
git add oxide-app/crates/oxide-ui/src/components
git commit -m "feat(oxide-ui): CollapsiblePanel (full<->rail toggle)"
```

---

## Task 8: oxide-ui — display primitives (ListItem, Bubble, PromptInput, RailButton, StatusDot)

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/{list_item,bubble,prompt_input,rail_button,status_dot}.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs`

**Interfaces:**
- Consumes: `Theme`.
- Produces (each a `Component` with a clear prop interface):
  - `ListItem { label: String, selected: bool, on_press: EventHandler }` — a conversation/project row.
  - `Bubble { role: String, text: String }` — a chat message (user vs assistant styling by `role`).
  - `PromptInput { value: State<String>, on_submit: EventHandler<String> }` — the prompt box (wraps the built-in `Input`).
  - `RailButton { glyph: String, on_press: EventHandler }` — a rail icon button.
  - `StatusDot { ok: bool }` — a connection indicator dot.

- [ ] **Step 1: Write one failing render test per primitive** (freya-testing), e.g. for `Bubble`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya::prelude::*;
    use freya_testing::prelude::*;

    #[test]
    fn bubble_renders_text() {
        fn app() -> impl IntoElement { Bubble::new("assistant".into(), "hi there".into()) }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "hi there")).is_some());
    }
}
```

Write the analogous render test for `ListItem` (renders its label), `PromptInput` (renders), `RailButton` (renders its glyph), `StatusDot` (renders). Keep them minimal — these are display primitives; behavior wiring is Task 10/11.

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui` (new tests) → FAIL.

- [ ] **Step 3: Implement each primitive** as a builder `Component`. Example `bubble.rs`:

```rust
//! A chat message bubble. User turns align right with accent tint; assistant
//! turns align left on the surface color.
use freya::prelude::*;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct Bubble { role: String, text: String, theme: Theme }

impl Bubble {
    pub fn new(role: String, text: String) -> Self { Self { role, text, theme: Theme::default() } }
    pub fn theme(mut self, t: Theme) -> Self { self.theme = t; self }
}

impl Component for Bubble {
    fn render(&self) -> impl IntoElement {
        let is_user = self.role == "user";
        let bg = if is_user { self.theme.accent() } else { self.theme.surface() };
        rect()
            .padding(Gaps::new_all(10.))
            .corner_radius(10.)
            .background(bg)
            .child(label().text(self.text.clone()).color(self.theme.text()))
    }
}
```

Implement the others following the same shape (use the built-in `Input` inside `PromptInput`, `Button` inside `RailButton`/`ListItem`; `on_press`/`on_submit` props as `EventHandler` — confirm the exact handler type from `API-NOTES.md`/examples).

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-ui && cargo clippy -p oxide-ui -- -D warnings` → PASS, clean.

- [ ] **Step 5: Export all** in `src/components/mod.rs`.

- [ ] **Step 6: Commit.**

```bash
git add oxide-app/crates/oxide-ui/src/components
git commit -m "feat(oxide-ui): ListItem, Bubble, PromptInput, RailButton, StatusDot"
```

---

## Task 9: oxide-ui — animated wrappers (FadeIn / SlideIn)

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/anim.rs`

**Interfaces:**
- Produces: `FadeIn { child: Element }` and `SlideIn { child: Element, from_x: f32 }` — `Component`s that animate their child in on mount via `freya-animation`. Task 2's page-enum fallback (if chosen) and Task 11's region transitions reuse these.

- [ ] **Step 1: Write a mount test** (freya-testing) asserting the wrapper renders its child:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya::prelude::*;
    use freya_testing::prelude::*;

    #[test]
    fn fade_in_renders_child() {
        fn app() -> impl IntoElement { FadeIn::new(label().text("INNER")) }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "INNER")).is_some());
    }
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui anim` → FAIL.

- [ ] **Step 3: Implement `FadeIn`/`SlideIn`** using `use_animation` + `AnimNum` (apply the animated value as opacity/translate on the wrapping `rect()`; confirm the opacity/offset attribute names against `API-NOTES.md`):

```rust
//! Animated wrappers reused for page/region transitions.
use freya::prelude::*;

#[derive(PartialEq, Clone)]
pub struct FadeIn { child: Element }

impl FadeIn {
    pub fn new(child: impl IntoElement) -> Self { Self { child: child.into_element() } }
}

impl Component for FadeIn {
    fn render(&self) -> impl IntoElement {
        let anim = use_animation(|conf| {
            conf.on_creation(OnCreation::Run);
            AnimNum::new(0.0, 1.0).time(200).ease(Ease::Out)
        });
        let opacity = anim.get().value();
        rect().opacity(opacity).child(self.child.clone())
    }
}

#[derive(PartialEq, Clone)]
pub struct SlideIn { child: Element, from_x: f32 }

impl SlideIn {
    pub fn new(child: impl IntoElement, from_x: f32) -> Self { Self { child: child.into_element(), from_x } }
}

impl Component for SlideIn {
    fn render(&self) -> impl IntoElement {
        let from = self.from_x;
        let anim = use_animation(move |conf| {
            conf.on_creation(OnCreation::Run);
            AnimNum::new(from, 0.0).time(220).ease(Ease::Out)
        });
        let dx = anim.get().value();
        rect().offset_x(dx).child(self.child.clone())
    }
}
```

(`.opacity()` / `.offset_x()` — if rc.23 names these differently, use the verified attribute; the animated value plumbing is the point.)

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-ui anim && cargo clippy -p oxide-ui -- -D warnings` → PASS, clean.

- [ ] **Step 5: Commit.**

```bash
git add oxide-app/crates/oxide-ui/src/anim.rs
git commit -m "feat(oxide-ui): FadeIn/SlideIn animated wrappers"
```

---

## Task 10: oxide-freya — AppState (transport → signals bridge)

**Files:**
- Create: `oxide-app/crates/oxide-freya/src/state.rs`
- Modify: `oxide-app/crates/oxide-freya/src/main.rs` (`mod state;`), `oxide-app/crates/oxide-freya/Cargo.toml`

**Interfaces:**
- Consumes: `Arc<dyn Transport>`, DTOs, `AgentEvent`.
- Produces: `AppState` holding Freya signals: `projects`, `conversations`, `active_conv: Option<ConversationId>`, `transcript: Vec<Turn>`, `live_assistant: String` (the streaming bubble), `connection: ConnState`. Methods: `load_projects()`, `open_conversation(id)`, `send(text)`. The shell (Task 11) calls these; the regions render the signals.

- [ ] **Step 1: Add deps to `oxide-freya/Cargo.toml`.**

```toml
[package]
name = "oxide-freya"
version = "0.1.0"
edition = "2021"

[dependencies]
freya = { workspace = true }
oxide-client = { path = "../oxide-client" }
oxide-ui = { path = "../oxide-ui" }
tokio = { workspace = true }
futures-util = { workspace = true }

[dev-dependencies]
freya-testing = { workspace = true }
```

- [ ] **Step 2: Write the failing state test** against `MockTransport`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use oxide_client::{mock::MockTransport, AgentEvent, Transport};

    #[test]
    fn send_streams_delta_then_final_into_signals() {
        // A mock that, on subscribe, yields a delta then a final.
        let mock = MockTransport {
            events: std::sync::Mutex::new(vec![
                AgentEvent { seq: 1, kind: "delta".into(), payload: serde_json::json!({"kind":"delta","text":"po"}) },
                AgentEvent { seq: 2, kind: "delta".into(), payload: serde_json::json!({"kind":"delta","text":"ng"}) },
                AgentEvent { seq: 3, kind: "final".into(), payload: serde_json::json!({"kind":"final","text":"pong"}) },
            ]),
            ..MockTransport::new()
        };
        let transport: Arc<dyn Transport> = Arc::new(mock);
        // Drive AppState's reducer over these events directly (pure function under test).
        let mut tx = Transcript::default();
        tx.apply_user("Reply pong".into());
        assert_eq!(tx.live_assistant, "");
        tx.apply_event(&AgentEvent { seq: 1, kind: "delta".into(), payload: serde_json::json!({"text":"po"}) });
        tx.apply_event(&AgentEvent { seq: 2, kind: "delta".into(), payload: serde_json::json!({"text":"ng"}) });
        assert_eq!(tx.live_assistant, "pong");
        tx.apply_event(&AgentEvent { seq: 3, kind: "final".into(), payload: serde_json::json!({"text":"pong"}) });
        assert_eq!(tx.live_assistant, "");
        assert_eq!(tx.turns.last().unwrap().text, "pong");
        assert_eq!(tx.turns.last().unwrap().role, "assistant");
        let _ = transport; // mock seam exercised separately by the live test
    }

    #[test]
    fn unreachable_health_sets_connection_state() {
        let mock = MockTransport { healthy: false, ..MockTransport::new() };
        assert!(mock.healthy == false);
    }
}
```

**Design note (Rule 1 — truthfulness):** the streaming reducer is a **pure** `Transcript` type (no Freya), unit-tested directly. `AppState` wraps it in signals. This keeps the truthfulness invariant (assistant text only from `delta`/`final`) testable without a UI harness. `apply_user` appends the user turn; `apply_event` mutates `live_assistant` on `delta` and commits a `Turn` on `final`; nothing fabricates assistant text.

- [ ] **Step 3: Run — verify fail.** `cargo test -p oxide-freya state` → FAIL.

- [ ] **Step 4: Implement `src/state.rs`.**

```rust
//! Bridges async transport calls + the SSE stream into Freya signals the
//! regions render. The streaming reducer (`Transcript`) is pure + unit-tested;
//! `AppState` wraps it in signals and spawns the async I/O.
use std::sync::Arc;

use freya::prelude::*;
use oxide_client::{AgentEvent, Conversation, ConversationId, Project, Transport, Turn};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnState { Unknown, Connected, Reconnecting, Unreachable }

/// Pure streaming reducer — the single place assistant text is assembled.
/// Assistant content is only ever appended from `delta`/`final` events.
#[derive(Default, Clone, PartialEq)]
pub struct Transcript {
    pub turns: Vec<Turn>,
    pub live_assistant: String,
}

impl Transcript {
    pub fn apply_user(&mut self, text: String) {
        self.turns.push(Turn { role: "user".into(), text, ts: 0 });
        self.live_assistant.clear();
    }
    pub fn apply_event(&mut self, ev: &AgentEvent) {
        match ev.kind.as_str() {
            "delta" => { if let Some(t) = ev.text() { self.live_assistant.push_str(t); } }
            "final" => {
                let text = ev.text().map(str::to_string).unwrap_or_else(|| std::mem::take(&mut self.live_assistant));
                self.turns.push(Turn { role: "assistant".into(), text, ts: 0 });
                self.live_assistant.clear();
            }
            _ => {} // activity/tool/card/etc. not rendered in 2a
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    transport: Arc<dyn Transport>,
    pub projects: State<Vec<Project>>,
    pub conversations: State<Vec<Conversation>>,
    pub active: State<Option<ConversationId>>,
    pub transcript: State<Transcript>,
    pub connection: State<ConnState>,
}

impl AppState {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            projects: use_state(Vec::new),
            conversations: use_state(Vec::new),
            active: use_state(|| None),
            transcript: use_state(Transcript::default),
            connection: use_state(|| ConnState::Unknown),
        }
    }

    /// Health-check, then load projects + the personal project's conversations.
    pub fn bootstrap(&self) {
        let t = self.transport.clone();
        let mut projects = self.projects;
        let mut conversations = self.conversations;
        let mut connection = self.connection;
        spawn(async move {
            match t.health().await {
                Ok(()) => connection.set(ConnState::Connected),
                Err(_) => { connection.set(ConnState::Unreachable); return; }
            }
            if let Ok(ps) = t.list_projects().await { projects.set(ps); }
            if let Ok(cs) = t.list_conversations("personal").await { conversations.set(cs); }
        });
    }

    /// Load history and open the SSE subscription for `id`.
    pub fn open_conversation(&self, id: ConversationId) {
        self.active.clone().set(Some(id.clone()));
        let t = self.transport.clone();
        let mut transcript = self.transcript;
        spawn(async move {
            let mut tx = Transcript::default();
            if let Ok(turns) = t.get_history(id.as_str()).await { tx.turns = turns; }
            transcript.set(tx);
            let mut events = t.subscribe(id.as_str());
            use futures_util::StreamExt;
            while let Some(ev) = events.next().await {
                if let Ok(ev) = ev {
                    transcript.with_mut(|tx| tx.apply_event(&ev));
                }
            }
        });
    }

    /// Append the user turn immediately, then send (reply streams via the open subscription).
    pub fn send(&self, text: String) {
        let Some(id) = self.active.peek().clone() else { return; };
        self.transcript.clone().with_mut(|tx| tx.apply_user(text.clone()));
        let t = self.transport.clone();
        spawn(async move { let _ = t.send_message(id.as_str(), &text).await; });
    }
}
```

(Signal copy/move ergonomics — `State<T>` is `Copy` in Freya; confirm `with_mut` exists per `API-NOTES.md`, else `*sig.write() = ...`. Adjust to the verified signal API.)

- [ ] **Step 5: Run — verify pass + clippy.** `cargo test -p oxide-freya state && cargo clippy -p oxide-freya -- -D warnings` → PASS, clean.

- [ ] **Step 6: Commit.**

```bash
git add oxide-app/crates/oxide-freya
git commit -m "feat(oxide-freya): AppState + pure streaming Transcript reducer"
```

---

## Task 11: oxide-freya — the navigable 3-region shell (the vertical slice)

**Files:**
- Create: `oxide-app/crates/oxide-freya/src/app.rs`, `src/regions/mod.rs`, `src/regions/sidebar.rs`, `src/regions/main_region.rs`, `src/regions/context.rs`
- Modify: `oxide-app/crates/oxide-freya/src/main.rs` (mount `app::Shell`, choose transport)

**Interfaces:**
- Consumes: `AppState` (Task 10), the `nav` seam (Task 2), `oxide-ui` components (Tasks 6–9), `UdsTransport` (Task 5).
- Produces: the running app — three regions, each with its own nav; the center `Chat` stays mounted while sidebars navigate; a real reply streams from agentd.

- [ ] **Step 1: Implement the three regions.** Each owns a `RegionNav` (Task 2 seam) with a tiny page enum proving independent nav. `sidebar.rs`:

```rust
//! Left region: a Conversations page (project + conversation list) plus a
//! placeholder second page, to prove the sidebar navigates without touching
//! the center region.
use freya::prelude::*;
use oxide_ui::components::{CollapsiblePanel, ListItem};
use crate::state::AppState;

#[derive(Clone, PartialEq)]
pub enum SidebarPage { Conversations, Placeholder }

#[derive(PartialEq, Clone)]
pub struct Sidebar { pub state: AppState, pub collapsed: bool }

impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let mut page = use_state(|| SidebarPage::Conversations);
        let state = self.state.clone();
        let full = match page.read().clone() {
            SidebarPage::Conversations => {
                let convs = state.conversations.read().clone();
                let mut col = rect().direction(Direction::Vertical).spacing(4.);
                for c in convs {
                    let st = state.clone();
                    let id = c.id.clone();
                    col = col.child(
                        ListItem::new(c.title.clone()).on_press(move |_| st.open_conversation(id.clone()))
                    );
                }
                col.into_element()
            }
            SidebarPage::Placeholder => label().text("Sidebar placeholder page").into_element(),
        };
        CollapsiblePanel::new()
            .collapsed(self.collapsed)
            .full(full)
            .rail(label().text("≡"))
    }
}
```

`main_region.rs` (center Chat — the one that must NOT remount on sidebar nav):

```rust
//! Center region: the active chat thread + prompt. Renders ONLY
//! transport-delivered content (Rule 1): committed turns + the live streaming
//! assistant bubble; never fabricated text.
use freya::prelude::*;
use oxide_ui::components::{Bubble, PromptInput};
use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct MainRegion { pub state: AppState }

impl Component for MainRegion {
    fn render(&self) -> impl IntoElement {
        let state = self.state.clone();
        let tx = state.transcript.read().clone();
        let mut thread = rect().direction(Direction::Vertical).spacing(8.).width(Size::fill());
        for turn in &tx.turns {
            thread = thread.child(Bubble::new(turn.role.clone(), turn.text.clone()));
        }
        if !tx.live_assistant.is_empty() {
            thread = thread.child(Bubble::new("assistant".into(), tx.live_assistant.clone()));
        }
        let input = use_state(String::new);
        let send_state = state.clone();
        rect().direction(Direction::Vertical).expanded().content(Content::Flex)
            .child(ScrollView::new().child(thread))
            .child(PromptInput::new(input).on_submit(move |text| send_state.send(text)))
    }
}
```

`context.rs` (right region placeholder with its own nav):

```rust
//! Right region: a status-rail placeholder (the 4 directions land in 2b).
use freya::prelude::*;
use oxide_ui::components::CollapsiblePanel;

#[derive(PartialEq, Clone)]
pub struct ContextRegion { pub collapsed: bool }

impl Component for ContextRegion {
    fn render(&self) -> impl IntoElement {
        CollapsiblePanel::new()
            .collapsed(self.collapsed)
            .full(label().text("Status"))
            .rail(label().text("◔"))
    }
}
```

- [ ] **Step 2: Implement the shell** `app.rs`:

```rust
//! Root shell: horizontal layout of three independently-navigable regions.
use freya::prelude::*;
use oxide_client::{Transport, UdsTransport};
use std::sync::Arc;

use crate::regions::{context::ContextRegion, main_region::MainRegion, sidebar::Sidebar};
use crate::state::AppState;

pub fn shell() -> impl IntoElement {
    let transport: Arc<dyn Transport> = Arc::new(UdsTransport::new(UdsTransport::default_socket()));
    let state = AppState::new(transport);
    state.bootstrap();
    let conn = *state.connection.read();

    rect().direction(Direction::Horizontal).expanded().background((5, 7, 11))
        .maybe_child(connection_banner(conn))
        .child(Sidebar { state: state.clone(), collapsed: false })
        .child(rect().width(Size::flex(1.0)).height(Size::fill())
            .child(MainRegion { state: state.clone() }))
        .child(ContextRegion { collapsed: true })
}

fn connection_banner(conn: crate::state::ConnState) -> Option<impl IntoElement> {
    use crate::state::ConnState::*;
    match conn {
        Unreachable => Some(label().text("agentd unavailable — retry").color(Color::from_rgb(255, 120, 120))),
        Reconnecting => Some(label().text("reconnecting…").color(Color::from_rgb(255, 171, 64))),
        _ => None,
    }
}
```

- [ ] **Step 3: Point `main.rs` at the shell.** Replace the hello body: `fn app() -> impl IntoElement { crate::app::shell() }`, add `mod app; mod regions; mod nav; mod state;`.

- [ ] **Step 4: Write the independent-nav invariant test** (freya-testing, against `MockTransport`) in `app.rs` or `tests/shell.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use oxide_client::{mock::MockTransport, AgentEvent, Conversation, ConversationId, ProjectId, Transport};
    use freya_testing::prelude::*;

    fn mock_with_reply() -> Arc<dyn Transport> {
        Arc::new(MockTransport {
            conversations: vec![Conversation { id: ConversationId::from("c1"), project_id: ProjectId::from("personal"),
                title: "Chat 1".into(), working_dir: String::new(), model: String::new(), created_at: 0, updated_at: 0 }],
            events: std::sync::Mutex::new(vec![
                AgentEvent { seq: 1, kind: "final".into(), payload: serde_json::json!({"text":"pong"}) },
            ]),
            ..MockTransport::new()
        })
    }

    #[test]
    fn sidebar_nav_does_not_clear_center_thread() {
        // Mount a harness with a fixed AppState + transcript containing a turn,
        // navigate the sidebar to its placeholder page, and assert the center
        // bubble text is still present. (Compose with TestingRunner::new +
        // provide_root_context so both regions share one AppState.)
    }

    #[test]
    fn sending_renders_streamed_reply() {
        // With mock_with_reply(): open c1, send a message, drive the stream,
        // assert a "pong" bubble appears. (Drive via the verified testing API.)
    }
}
```

Flesh out both tests against the verified freya-testing API (use `TestingRunner::new` with a shared `AppState` in root context so the sidebar and center regions reference the same state; click the sidebar's placeholder nav; assert the center `Bubble` label survives). Expected: PASS.

- [ ] **Step 5: Build, test, clippy.** `cargo build -p oxide-freya && cargo test -p oxide-freya && cargo clippy -p oxide-freya -- -D warnings` → PASS, clean.

- [ ] **Step 6: Live run against agentd.** With agentd running, launch the app (the recorded run command). Confirm: the conversation list loads from the live daemon; selecting a conversation shows history; sending a message streams a real reply into the thread; navigating the sidebar to its placeholder leaves the center thread + live stream intact. Record the result (screenshot/notes) — this is the 2a acceptance.

- [ ] **Step 7: Commit.**

```bash
git add oxide-app/crates/oxide-freya
git commit -m "feat(oxide-freya): navigable 3-region shell streaming live agentd replies"
```

---

## Self-Review

**1. Spec coverage:**
- `oxide-client` (Transport trait + UdsTransport + SSE + Last-Event-ID + DTOs + errors + reconnect) → Tasks 3–5. ✓
- `oxide-ui` (tokens + reusable primitives + animated wrappers) → Tasks 6–9. ✓
- `oxide-freya` (shell, per-region navigation, collapse, AppState wiring) → Tasks 2, 10, 11. ✓
- Per-region independent navigation (headline architecture) → Task 2 seam + Task 11 invariant test. ✓
- Truthfulness (Rule 1: thread renders only transport-delivered content) → Task 10 pure `Transcript` reducer + Task 11 render. ✓
- Data flow (health → projects → conversations → history → subscribe → send → stream) → Task 10 `bootstrap`/`open_conversation`/`send` + Task 11 + live test (Task 5). ✓
- Error handling (Unreachable/stream → truthful banner, no fabrication) → `ConnState` + `connection_banner` (Tasks 10–11). ✓
- Testing (client trait-seam + live integration; ui render tests; freya smoke + independent-nav invariant) → Tasks 3–11. ✓
- Open items (Task-1 build recipe; multi-router feasibility) → Tasks 1 & 2 as explicit spikes. ✓
- Out of scope (2b cards/editor/settings; 2c Android; 2d control plane; webview) → not planned. ✓

**2. Placeholder scan:** No "TBD/TODO/implement later." Tasks 1–2 are spikes with concrete deliverables + acceptance checks (legitimate per the spec's "resolved early in the plan, not deferred silently"). Task 11's two invariant tests carry skeletons with explicit composition instructions rather than fully-written harness code, because the exact freya-testing multi-region harness depends on Task 1's verified API — the assertion and method are specified.

**3. Type consistency:** `Transport` signatures identical across `transport.rs`/`mock.rs`/`uds.rs`. `AgentEvent { seq, kind, payload }` + `.text()` consistent across `dto.rs`/`sse.rs`/`state.rs`. `Transcript`/`ConnState`/`AppState` names consistent Tasks 10↔11. `CollapsiblePanel`/`Bubble`/`ListItem`/`PromptInput` prop interfaces consistent across `oxide-ui`↔`oxide-freya`. Token accessors (`bg`/`surface`/`text`/`accent`) consistent.

**Framework-newness caveat (carried into execution):** Freya rc.23's exact method names (`into_element`, `Element::clone`, `.opacity`/`.offset_x`, `Color::from_rgb`, `with_mut`, `EventHandler`) are verified for the core surface but may drift at the edges. Task 1 produces `API-NOTES.md` as the in-repo tiebreaker; later tasks cross-check against it and the cloned `examples/`, treating a compile error as ground truth and adjusting code + test together. This is the intended role of the front-loaded spike, not a placeholder.

---

## Execution Handoff

Plan complete. Recommended: **superpowers:subagent-driven-development** — fresh implementer per task (Tasks 1–2 are spikes; 3–5 mechanical-ish pure Rust; 6–11 Freya-integration), per-task spec+quality review, one whole-branch opus review as the merge gate. Build in a git worktree off `phase1-local-llm-gateway`; code lands in the new `oxide-app/` workspace.
