# Standalone Chat Window (Option D) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A reliably-opening, system-decorated standalone chat window — a sibling binary `oxidemx-chat` that reuses the overlay's existing chat in place (no extraction), launchable from the app menu, a hotkey, and the MX button, single-instance.

**Architecture:** Add one `RadialState` field (`chat_window_mode`) and one public entry `oxidemx_overlay::run_chat_window()` that mirrors `app::run()` but uses plain `iced::window::Settings` and renders chat-only; a new `[[bin]] oxidemx-chat` calls it. Single-instance via D-Bus name `org.oxidemx.Chat` with present-on-relaunch; the daemon gains a `ShowChat` signal; a `.desktop` entry + `install.sh` wiring.

**Tech Stack:** Rust, iced 0.14 (wayland/x11), zbus 5, oxidemx-overlay (overlay-rs), oxidemx-daemon, oxidemx-agent-proxy.

## Global Constraints

- Follow repo `CLAUDE.md`: no repo-wide `cargo fmt` (hand-match style); `cargo clippy` clean; no gold-plating; field-standard naming.
- **The overlay needs distrobox** (GTK/iced system libs). Build/test with:
  `distrobox enter claude_development -- bash -lc 'cd oxidemx-phase1 && <cargo cmd>'`.
- Reuse the chat in place — do NOT move `chat_ui/`, `RadialState` chat fields, or `update.rs` chat arms (that's the deferred extraction). This slice only ADDS a field, an entry fn, view/update branches, a binary, and glue.
- App id for the chat window: `org.oxidemx.Chat`. Single-instance must fall back to opening a plain window if the name can't be claimed (never exit silently).
- Every task: `cargo build -p oxidemx-overlay` (and the daemon where touched) clean before commit.

---

### Task 1: `chat_window_mode` field + `run_chat_window()` entry + view/update branches

**Files:**
- Modify: `overlay-rs/src/radial/mod.rs` (add field + default)
- Modify: `overlay-rs/src/app/mod.rs` (`run_chat_window()` + make it + `boot` reachable; `view`/`subscription` branch)
- Modify: `overlay-rs/src/app/update.rs` (guard daemon/radial arms in chat-window mode)
- Modify: `overlay-rs/src/lib.rs` (re-export `run_chat_window`)

**Interfaces:**
- Produces: `pub fn oxidemx_overlay::run_chat_window() -> iced::Result`; `RadialState.chat_window_mode: bool`.

- [ ] **Step 1: Add the `chat_window_mode` field (default false)**

In `overlay-rs/src/radial/mod.rs`, add to the `RadialState` struct (near the other top-level flags):
```rust
    /// When true, this RadialState drives the standalone chat WINDOW (a normal
    /// decorated toplevel), not the radial overlay: `view` renders chat-only and
    /// `update` ignores the daemon radial-show/puck/handoff paths.
    pub chat_window_mode: bool,
```
In `RadialState::new(...)` (the constructor that builds the struct literal), initialize `chat_window_mode: false,`. (Search the file for the struct literal that sets the other `ai_*` defaults and add it there.)

- [ ] **Step 2: Add `run_chat_window()` beside `run()`**

In `overlay-rs/src/app/mod.rs`, after `run()` (line ~340), add:
```rust
/// Run the chat as a normal, decorated toplevel WINDOW (the `oxidemx-chat`
/// sibling binary). Mirrors `run()` but: plain window settings (no frameless/
/// topmost/override_redirect, no cursor-helper), an opaque background, and a
/// boot that sets `chat_window_mode = true`.
pub fn run_chat_window() -> iced::Result {
    iced::application(boot_chat_window, update, view)
        .title("OxideMX Chat")
        .window(iced::window::Settings {
            size: iced::Size::new(520.0, 720.0),
            min_size: Some(iced::Size::new(380.0, 480.0)),
            decorations: true,
            transparent: false,
            ..Default::default()
        })
        .subscription(subscription)
        .run()
}

fn boot_chat_window() -> RadialState {
    let mut state = boot();
    state.chat_window_mode = true;
    state
}
```
(Leave `run()`'s transparent style as-is; the chat window uses iced's default opaque theme style by omitting `.style(...)`.)

- [ ] **Step 3: Branch `view()` for chat-only**

At the TOP of the overlay `view()` fn (`overlay-rs/src/app/mod.rs`), before the radial/morph assembly, add:
```rust
    if state.chat_window_mode {
        // Full-window chat; no radial canvas / disc / morph.
        return crate::chat_ui::view(state, 1.0);
    }
```
(Read the current `view` signature to match the exact param name/return type, e.g. `fn view(state: &RadialState) -> Element<'_, Message>`.)

- [ ] **Step 4: Guard the daemon/radial arms in `update()` when in chat-window mode**

In `overlay-rs/src/app/update.rs`, the arms driven by the daemon radial triggers + puck/handoff must no-op in chat-window mode. Add a guard near the top of `update()` (after the existing handoff-disarm guard, before the big match) OR inside each relevant arm. The minimal-risk version — a guard block right after the fn opens:
```rust
    // The chat WINDOW reuses this update fn but is not the radial overlay:
    // ignore the daemon's radial show/hide and the puck/handoff geometry.
    if state.chat_window_mode {
        match &message {
            Message::MenuRequested { .. }
            | Message::HideMenu
            | Message::HandoffPointer { .. }
            | Message::HandoffClick { .. }
            | Message::ChatHeaderPressed
            | Message::ChatResizeStart => return Task::none(),
            _ => {}
        }
    }
```
(Confirm the exact variant names/payloads from `app/mod.rs` — the inventory lists `MenuRequested`, `HideMenu`, `HandoffPointer{x,y}`, `HandoffClick{x,y}`, `ChatHeaderPressed`, `ChatResizeStart`. Include any other daemon/radial-only variants you find; do NOT guard `Ai*`/`Agentd*`.)

- [ ] **Step 5: Re-export from the lib**

In `overlay-rs/src/lib.rs`, ensure `pub use app::run_chat_window;` (or that `app` is `pub` and the binary can call `oxidemx_overlay::app::run_chat_window`). Match the existing `run` export style.

- [ ] **Step 6: Build (the overlay still builds; mode defaults false so existing behavior is unchanged)**

Run: `distrobox enter claude_development -- bash -lc 'cd oxidemx-phase1 && cargo build -p oxidemx-overlay 2>&1 | tail -5'`
Expected: clean build. Existing overlay tests unaffected: `... cargo test -p oxidemx-overlay 2>&1 | grep "test result"` → unchanged pass count.

- [ ] **Step 7: Commit**
```bash
git add overlay-rs/src/radial/mod.rs overlay-rs/src/app/mod.rs overlay-rs/src/app/update.rs overlay-rs/src/lib.rs
git commit -m "feat(overlay): chat_window_mode + run_chat_window() entry (normal window, chat-only view)"
```

---

### Task 2: `oxidemx-chat` sibling binary

**Files:**
- Create: `overlay-rs/src/bin/oxidemx-chat.rs`
- Modify: `overlay-rs/Cargo.toml` (add `[[bin]]`)

**Interfaces:**
- Consumes: `oxidemx_overlay::run_chat_window()` (Task 1).
- Produces: a `oxidemx-chat` binary.

- [ ] **Step 1: Add the binary target**

In `overlay-rs/Cargo.toml`, add (after the existing `[[bin]]` for the overlay, or create the section):
```toml
[[bin]]
name = "oxidemx-chat"
path = "src/bin/oxidemx-chat.rs"
```
(Check whether the crate currently defines the overlay binary via `[[bin]]` or `src/main.rs`. If it uses `src/main.rs` with an implicit bin name, ADD an explicit `[[bin]]` for the existing overlay binary too so both are named, OR keep the implicit main and only add the new `[[bin]]`. Match what compiles.)

- [ ] **Step 2: Write the binary**

Create `overlay-rs/src/bin/oxidemx-chat.rs`:
```rust
//! Standalone OxideMX chat window — a normal decorated toplevel that reuses the
//! overlay's chat in place (see run_chat_window). Single-instance is added in Task 3.
fn main() -> iced::Result {
    oxidemx_overlay::run_chat_window()
}
```

- [ ] **Step 3: Build the binary**

Run: `distrobox enter claude_development -- bash -lc 'cd oxidemx-phase1 && cargo build -p oxidemx-overlay --bin oxidemx-chat 2>&1 | tail -5'`
Expected: clean build, produces `target/debug/oxidemx-chat`.

- [ ] **Step 4: Manual launch check (documented; GUI can't be unit-tested)**

Run (host, with session env):
```bash
env XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus DISPLAY=:0 WAYLAND_DISPLAY=wayland-0 \
  ./target/debug/oxidemx-chat
```
Expected: a normal **decorated, resizable** window opens showing the chat UI, stays open (no dismiss-on-unfocus), and a message sent reaches agentd (watch `journalctl --user -u oxidemx-agentd -f`). Record the result in the task report.

- [ ] **Step 5: Commit**
```bash
git add overlay-rs/src/bin/oxidemx-chat.rs overlay-rs/Cargo.toml
git commit -m "feat(overlay): oxidemx-chat sibling binary (normal-window chat)"
```

---

### Task 3: Single-instance (`org.oxidemx.Chat`) + present-on-relaunch

**Files:**
- Modify: `overlay-rs/src/bin/oxidemx-chat.rs`
- Create: `overlay-rs/src/chat_window/single_instance.rs` (helper module)
- Modify: `overlay-rs/src/app/mod.rs` (subscription hook for present requests in chat-window mode)
- Modify: `overlay-rs/src/lib.rs` (module decl if needed)

**Interfaces:**
- Produces: `pub async fn oxidemx_overlay::chat_window::single_instance::acquire_or_present() -> SingleInstance` returning either `Primary(receiver)` (this is the only instance; receiver yields present requests) or `Secondary` (another instance was signaled to present — caller exits).

- [ ] **Step 1: Write the single-instance helper**

Create `overlay-rs/src/chat_window/single_instance.rs`:
```rust
//! Single-instance for the chat window via the well-known D-Bus name
//! `org.oxidemx.Chat`. The primary instance owns the name and serves a `Present`
//! method; a second launch calls `Present` on the primary and exits.
use zbus::{connection, interface};
use tokio::sync::mpsc;

const NAME: &str = "org.oxidemx.Chat";
const PATH: &str = "/org/oxidemx/Chat";

pub enum SingleInstance {
    /// This process owns the name; poll `present` for raise requests.
    Primary { present: mpsc::UnboundedReceiver<()> },
    /// Another instance is already running and was asked to present; exit.
    Secondary,
}

struct PresentService { tx: mpsc::UnboundedSender<()> }

#[interface(name = "org.oxidemx.Chat")]
impl PresentService {
    /// Ask the running chat window to raise/focus itself.
    async fn present(&self) { let _ = self.tx.send(()); }
}

/// Try to become the primary instance. On success, returns `Primary` with a
/// receiver of present-requests and keeps the connection alive (leaked into a
/// 'static task). On a name clash, calls `Present` on the existing instance and
/// returns `Secondary`. On any other error, returns `Primary` with a dead
/// receiver so the caller still opens a window (never silently exits).
pub async fn acquire_or_present() -> SingleInstance {
    let (tx, rx) = mpsc::unbounded_channel();
    let built = connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, PresentService { tx }));
    match built {
        Ok(builder) => match builder.build().await {
            Ok(conn) => { Box::leak(Box::new(conn)); SingleInstance::Primary { present: rx } }
            Err(zbus::Error::NameTaken) => { call_present().await; SingleInstance::Secondary }
            Err(_) => SingleInstance::Primary { present: rx } // degrade: open a window anyway
        },
        Err(_) => SingleInstance::Primary { present: rx },
    }
}

async fn call_present() {
    if let Ok(conn) = connection::Builder::session().and_then(|b| b.build()).await.map_err(|_| ()) {
        let _ = conn
            .call_method(Some(NAME), PATH, Some("org.oxidemx.Chat"), "Present", &())
            .await;
    }
}
```
(Confirm `zbus` 5 API names against the version in `overlay-rs/Cargo.toml`: `connection::Builder::session()`, `.name()`, `.serve_at()`, `Error::NameTaken`. Adjust to match — the daemon/agentd already use zbus 5, copy their builder idiom if these differ.)

Register the module: in `overlay-rs/src/lib.rs` add `pub mod chat_window { pub mod single_instance; }` (or match the crate's module style).

- [ ] **Step 2: Wire present-requests into the app (chat-window subscription)**

In `app/mod.rs`, add a `Message::PresentWindow` variant (chat-window only) and, in `subscription()`, when `state.chat_window_mode`, include a subscription that forwards the `present` receiver's items as `Message::PresentWindow`. In `update()`, handle `PresentWindow` → `iced::window::get_latest().and_then(iced::window::gain_focus)` (the iced 0.14 raise/focus task). Pass the receiver into the app via a `OnceCell`/`static` the binary fills before `run_chat_window()` (simplest: a `std::sync::Mutex<Option<Receiver>>` in the `single_instance` module that the subscription drains).
```rust
// app/mod.rs update arm:
Message::PresentWindow => {
    return iced::window::get_latest().and_then(iced::window::gain_focus);
}
```

- [ ] **Step 3: Use it from the binary**

Rewrite `overlay-rs/src/bin/oxidemx-chat.rs`:
```rust
fn main() -> iced::Result {
    // Single-instance: become primary or signal the existing window + exit.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let instance = rt.block_on(oxidemx_overlay::chat_window::single_instance::acquire_or_present());
    match instance {
        oxidemx_overlay::chat_window::single_instance::SingleInstance::Secondary => Ok(()),
        oxidemx_overlay::chat_window::single_instance::SingleInstance::Primary { present } => {
            oxidemx_overlay::chat_window::single_instance::stash_present_receiver(present);
            oxidemx_overlay::run_chat_window()
        }
    }
}
```
Add `stash_present_receiver` to the module (stores the receiver in the `Mutex<Option<…>>` the subscription drains). Keep the tokio runtime alive for the duration (the leaked connection needs the executor — if `run_chat_window` blocks the thread, spawn the zbus connection on a dedicated thread/runtime that stays alive; verify the connection survives by testing the second-launch path).

- [ ] **Step 4: Verify single-instance**

Run: `distrobox enter claude_development -- bash -lc 'cd oxidemx-phase1 && cargo build -p oxidemx-overlay --bin oxidemx-chat 2>&1 | tail -3'` → clean.
Manual: launch `oxidemx-chat`; launch it again → the second exits and the first window raises/focuses (not a duplicate). Confirm with `busctl --user list | grep org.oxidemx.Chat` (one owner) and `pgrep -c oxidemx-chat` → 1. Record in the report.

- [ ] **Step 5: Commit**
```bash
git add overlay-rs/src/bin/oxidemx-chat.rs overlay-rs/src/chat_window/ overlay-rs/src/app/mod.rs overlay-rs/src/lib.rs
git commit -m "feat(overlay): single-instance chat window (org.oxidemx.Chat present-on-relaunch)"
```

---

### Task 4: Daemon `ShowChat` signal + app subscribes

**Files:**
- Modify: the daemon D-Bus interface (find via `grep -rl "interface(name" oxidemx-daemon*/src` or wherever `org.oxidemx.Daemon` is defined — the unit runs `oxidemxd`)
- Modify: `overlay-rs/src/app/mod.rs` (chat-window subscription also listens for the daemon `ShowChat`)

**Interfaces:**
- Produces: a D-Bus signal `ShowChat` on `org.oxidemx.Daemon`; the chat window presents on it.

- [ ] **Step 1: Add the `ShowChat` signal to the daemon interface**

Locate the daemon's `#[interface(name = "org.oxidemx.Daemon")]` impl. Add a signal:
```rust
#[zbus(signal)]
async fn show_chat(ctxt: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
```
Emit it from wherever a "show chat" gesture/button is mapped (mirror how `MenuRequested`/`HideMenu` are emitted on the MX button — add a config-bound action that emits `ShowChat`). If button-mapping config is out of scope for this task, at minimum expose the signal so it can be emitted (a follow-up binds the MX button); note this in the report.

- [ ] **Step 2: Subscribe in the chat window**

In `app/mod.rs`'s `subscription()`, when `chat_window_mode`, add a zbus subscription to `org.oxidemx.Daemon`'s `ShowChat` signal that emits `Message::PresentWindow` (reuse the Task-3 present path). Build the proxy/connection the same way `agent_events.rs` does.

- [ ] **Step 3: Build both + verify the signal exists**

Run: build the daemon (`cargo build -p <daemon-crate>`) + the overlay; then `busctl --user introspect org.oxidemx.Daemon /org/oxidemx/Daemon | grep -i ShowChat` after restarting the daemon → the signal is listed. Manually: `busctl --user emit /org/oxidemx/Daemon org.oxidemx.Daemon ShowChat` (or trigger the bound action) with `oxidemx-chat` running → the window presents. Record results.

- [ ] **Step 4: Commit**
```bash
git add <daemon-files> overlay-rs/src/app/mod.rs
git commit -m "feat: daemon ShowChat signal + chat window presents on it"
```

---

### Task 5: Launch glue — `.desktop` + `install.sh` + keybinding doc

**Files:**
- Create: a desktop entry (generated by `install.sh`, mirroring the overlay's autostart `.desktop`)
- Modify: `install.sh` (install the `oxidemx-chat` binary + a menu `.desktop`; build it)
- Modify: build target list in `install.sh` (add `-p oxidemx-overlay --bin oxidemx-chat` or ensure the overlay build includes the new bin)

**Interfaces:** none (packaging).

- [ ] **Step 1: install.sh builds + installs `oxidemx-chat`**

In `install.sh`, where the overlay binary is built (`-p oxidemx-overlay`, ~line 924/992) ensure the `oxidemx-chat` bin is built too (the workspace `cargo build -p oxidemx-overlay --release` builds all its bins by default — verify). Where the overlay binary is installed (`install -Dm755 "$overlay_bin" "$BIN_DIR/oxidemx-overlay"`, ~line 1041), add the same for `oxidemx-chat`:
```sh
if chat_bin="$(pick_binary oxidemx-chat target/release/oxidemx-chat)"; then
    sudo install -Dm755 "$chat_bin" "$BIN_DIR/oxidemx-chat"
    log_success "Chat window binary installed"
fi
```

- [ ] **Step 2: Install a menu `.desktop` entry**

In `install.sh`, near the overlay autostart `.desktop` generation (~line 1130), add a **menu** entry (NoDisplay=false so it shows in the app grid) at `$HOME/.local/share/applications/oxidemx-chat.desktop`:
```sh
cat > "$HOME/.local/share/applications/oxidemx-chat.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=OxideMX Chat
Comment=AI chat window for OxideMX
Exec=$BIN_DIR/oxidemx-chat
Icon=oxidemx
Terminal=false
Categories=Utility;
EOF
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
log_success "Chat window menu entry installed"
```

- [ ] **Step 3: Document the keybinding**

In `install.sh`'s post-install summary (log output), print how to bind a hotkey:
```sh
log_dim "  Hotkey: Settings → Keyboard → Custom Shortcuts → command: $BIN_DIR/oxidemx-chat"
```

- [ ] **Step 4: Verify**

Run `bash -n install.sh` (syntax check). Manually run the relevant install.sh function (or the whole installer if safe) and confirm `/usr/local/bin/oxidemx-chat` + the `.desktop` exist and "OxideMX Chat" appears in the app menu. Record results.

- [ ] **Step 5: Commit**
```bash
git add install.sh
git commit -m "feat(install): install oxidemx-chat binary + app-menu .desktop + keybinding doc"
```

---

## Self-Review

**1. Spec coverage** (`docs/superpowers/specs/2026-06-20-standalone-chat-window-design.md`):
- §Approach/1 `run_chat_window()` + `chat_window_mode` + view/update branches → Task 1. ✅
- §Approach/2 `oxidemx-chat` binary + single-instance → Tasks 2-3. ✅
- §Approach/3 daemon `ShowChat`, `.desktop`, keybinding, install.sh → Tasks 4-5. ✅
- §Error handling (name-claim fallback opens a window) → Task 3 Step 1 (degrade path). ✅
- §Testing (boot mode, single-instance, overlay regression) → Tasks 1/2/3 verify steps. ✅
- §Incremental path (ChatState sub-struct, chat_update.rs) → explicitly OUT of scope (queued). ✅

**2. Placeholder scan:** The "confirm exact variant names / zbus API / module style" notes are accuracy guards against the live source (this is a refactor over existing code, not greenfield), each naming exactly what to confirm and where. No TBD/empty steps; code blocks are complete. GUI-only behaviors use documented manual-verify steps (iced windows can't be unit-tested headlessly).

**3. Type consistency:** `chat_window_mode: bool`, `run_chat_window()`, `boot_chat_window()`, `Message::PresentWindow`, `org.oxidemx.Chat` / `Present`, `org.oxidemx.Daemon` / `ShowChat`, `SingleInstance::{Primary{present},Secondary}` — consistent across Tasks 1-5.
