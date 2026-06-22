# HTTP/SSE + Tailscale Transport (1b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give agentd a capability-scoped HTTP/SSE surface — Unix-socket-local (Control scope) + tailnet-TCP-remote (Messaging scope) — so a cross-platform client can prompt, stream, and approve over a secure transport, leaving the D-Bus path untouched.

**Architecture:** A new `agentd/src/connector/` module. `caps.rs` is the durable capability contract; `auth.rs`/`tailnet.rs`/`event_hub.rs` are mockable building blocks; `http/` is an axum server delegating to the existing `Arc<AgentService>`. A `BroadcastEmitter` tees the existing `AgentEvent` stream to both the D-Bus drain and an in-process `EventHub` that SSE handlers subscribe to. Opt-in via `AppConfig.http.enabled` (default false) → zero behaviour change when off.

**Tech Stack:** Rust, agentd (host-side build, `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`), `axum` 0.8 (UnixListener + TcpListener via `axum::serve`), `tokio` (`net`, `sync::broadcast`), `tokio-stream`, `futures-util`, `serde`/`serde_json`, `oxidemx-shared` (config). Tailscale via `tailscaled` sidecar (`tailscale status --json`), not `tailscale-rs`.

## Global Constraints

- **Naming (Rule 0):** agent-protocol surface, `/v1`-prefixed (`conversations`/`messages`/`events`/`approvals`). Capability tiers use `ScopeTier` (`Messaging` < `Control`); descriptor is `ConnectorCaps`. Core stays connector-agnostic — `http/` is the only transport-aware code; do NOT touch `interface.rs` core logic or the D-Bus path.
- **Truthfulness (Rule 1):** every HTTP value comes from the authoritative store (`AgentService` reads) or the live `AgentEvent` stream. SSE re-emits `AgentEvent.payload` verbatim. `POST /messages` returns only `{message_id}` (an action ack), never a fabricated reply.
- **Security invariants (non-negotiable):** never bind `0.0.0.0`; tailnet listener binds the resolved tailnet IP only; `bind_override` rejecting `0.0.0.0`/unspecified. Control scope reachable ONLY over UDS — control routes never mounted on the TCP listener. Bearer token required on TCP; UDS uses filesystem perms (socket 0600, parent dir 0700). Token file mode 0600. Constant-time token compare.
- **Rust quality (Rule 2):** clippy-clean, hand-formatted (NO repo-wide `cargo fmt` — format only lines you add), `?` over unwrap in non-test code, `thiserror`, trait seams for mocks. No gold-plating: build Messaging in full + the Control seam (`/v1/auth/token` only). Do NOT build the connector registry, `InboundEvent`/`OutboundMessage`, or speculative control endpoints.
- **Build/process (Rule 3):** agentd builds host-side with `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd` (package name is `agentd`, binary is `oxidemx-agentd`). `oxidemx-shared` also builds host-side. ONLY Task 10 (settings UI) needs the `claude_development` distrobox. Isolate in a git worktree off `phase1-local-llm-gateway`. Commit each task.
- **Defaults:** port `8765`, SSE replay buffer `256`, settings toggle included (Task 10).

---

## File Structure

```
agentd/src/connector/
├── mod.rs                    # pub mod-s + re-exports
├── caps.rs                   # ScopeTier, ConnectorCaps                              (Task 1)
├── auth.rs                   # token load/generate/persist, constant-time verify     (Task 2)
├── tailnet.rs                # TailnetSource trait, CliTailnetSource, resolve_bind    (Task 3)
├── event_hub.rs              # EventHub (broadcast + ring + run→conv map), BroadcastEmitter (Task 4)
└── http/
    ├── mod.rs                # AppState, build_router, error→status, message-id gen   (Task 6)
    ├── routes_messaging.rs   # health/projects/conversations/messages/approvals       (Task 6)
    ├── sse.rs                # GET /v1/conversations/{id}/events                       (Task 7)
    ├── routes_control.rs     # GET /v1/auth/token                                      (Task 8)
    └── server.rs             # UDS+TCP listeners, scope-by-listener, serve(), shutdown (Task 8)
agentd/src/lib.rs             # add `pub mod connector;`                                (Task 1)
agentd/src/main.rs            # read AppConfig.http; wrap emitter; spawn serve          (Task 9)
agentd/Cargo.toml             # add axum, tower-http, tokio "net", tokio-stream, futures-util (Task 1)
oxidemx-shared/src/config.rs  # HttpConfig + AppConfig.http                             (Task 5)
oxidemx-settings/src/...      # HTTP-enable toggle in the AI tab                        (Task 10)
```

Dependency order: 1→(2,3,4,5 independent)→6→7→8→9→10.

---

### Task 1: Module scaffold + dependencies + `ScopeTier`/`ConnectorCaps`

**Files:**
- Modify: `agentd/Cargo.toml`
- Create: `agentd/src/connector/mod.rs`, `agentd/src/connector/caps.rs`
- Modify: `agentd/src/lib.rs` (add `pub mod connector;`)

**Interfaces:**
- Produces: `connector::caps::{ScopeTier, ConnectorCaps}`. `ScopeTier::{Messaging, Control}` (derives `PartialOrd`/`Ord` with `Messaging < Control`). `ConnectorCaps { scope: ScopeTier, can_stream: bool }`, consts `ConnectorCaps::UDS_LOCAL` / `ConnectorCaps::TAILNET_REMOTE`, method `allows(&self, required: ScopeTier) -> bool`.

- [ ] **Step 1: Add dependencies to `agentd/Cargo.toml`**

Under `[dependencies]`, add (keep existing entries):
```toml
axum = { version = "0.8", default-features = false, features = ["http1", "json", "tokio", "query"] }
tower-http = { version = "0.6", features = ["trace", "limit"] }
tokio-stream = { version = "0.1", features = ["sync"] }
futures-util = { version = "0.3", default-features = false, features = ["std"] }
```
Change the existing `tokio` line to add the `"net"` feature:
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "fs", "net"] }
```

- [ ] **Step 2: Write the failing test** — create `agentd/src/connector/caps.rs` with only the test module:

```rust
//! Capability contract: every transport declares what it may do; the HTTP edge enforces it.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_outranks_messaging() {
        assert!(ScopeTier::Control > ScopeTier::Messaging);
    }

    #[test]
    fn messaging_caps_deny_control_routes() {
        let caps = ConnectorCaps::TAILNET_REMOTE;
        assert_eq!(caps.scope, ScopeTier::Messaging);
        assert!(caps.allows(ScopeTier::Messaging));
        assert!(!caps.allows(ScopeTier::Control));
    }

    #[test]
    fn control_caps_allow_both_tiers() {
        let caps = ConnectorCaps::UDS_LOCAL;
        assert_eq!(caps.scope, ScopeTier::Control);
        assert!(caps.allows(ScopeTier::Messaging));
        assert!(caps.allows(ScopeTier::Control));
    }
}
```

- [ ] **Step 3: Create `agentd/src/connector/mod.rs`**

```rust
//! Connector layer: capability-scoped transports over the connector-agnostic core.
//!
//! 1b builds the HTTP connector (`http/`) + its building blocks. The full
//! `Connector` trait / registry / `InboundEvent` normalization is deferred to
//! the SP-Connectors slice; this module ships only what the HTTP transport needs.
#![forbid(unsafe_code)]

pub mod auth;
pub mod caps;
pub mod event_hub;
pub mod http;
pub mod tailnet;
```

> NOTE: `auth`, `event_hub`, `http`, `tailnet` modules are created in later tasks. To keep the crate compiling after Task 1, temporarily comment out the `pub mod` lines for modules not yet created, and uncomment each as its task lands. Re-add `auth` in Task 2, `tailnet` in Task 3, `event_hub` in Task 4, `http` in Task 6.

For Task 1, `mod.rs` should contain only:
```rust
//! Connector layer: capability-scoped transports over the connector-agnostic core.
#![forbid(unsafe_code)]

pub mod caps;
```

- [ ] **Step 4: Add `ScopeTier`/`ConnectorCaps` to `caps.rs`** (above the test module):

```rust
/// Ordered capability tier. `Control` implies `Messaging`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeTier {
    /// Conversation/messaging surface: safe to expose remotely.
    Messaging,
    /// Messaging + the high-security control plane (AI settings, skill/flow
    /// management + generation, conductor/orchestrator control + debug). Same-device only.
    Control,
}

/// What a given connector/connection is allowed to do.
#[derive(Debug, Clone, Copy)]
pub struct ConnectorCaps {
    pub scope: ScopeTier,
    pub can_stream: bool,
}

impl ConnectorCaps {
    /// Local Unix-socket connection: full control plane.
    pub const UDS_LOCAL: ConnectorCaps =
        ConnectorCaps { scope: ScopeTier::Control, can_stream: true };
    /// Remote tailnet connection: messaging only.
    pub const TAILNET_REMOTE: ConnectorCaps =
        ConnectorCaps { scope: ScopeTier::Messaging, can_stream: true };

    /// True if this connection may invoke a route requiring `required`.
    pub fn allows(&self, required: ScopeTier) -> bool {
        self.scope >= required
    }
}
```

- [ ] **Step 5: Add `pub mod connector;` to `agentd/src/lib.rs`** (alongside the other `pub mod` declarations).

- [ ] **Step 6: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::caps -- --nocapture`
Expected: 3 tests pass.
Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd`
Expected: no new warnings in `connector/`.

- [ ] **Step 7: Commit**

```bash
git add agentd/Cargo.toml agentd/Cargo.lock agentd/src/lib.rs agentd/src/connector/
git commit -m "feat(agentd): connector module scaffold + ScopeTier/ConnectorCaps capability contract"
```

---

### Task 2: `auth.rs` — bearer token generate/load/persist + constant-time verify

**Files:**
- Create: `agentd/src/connector/auth.rs`
- Modify: `agentd/src/connector/mod.rs` (uncomment/add `pub mod auth;`)

**Interfaces:**
- Consumes: nothing from prior tasks.
- Produces:
  - `auth::load_or_create_token(dir: &Path) -> std::io::Result<String>` — reads `<dir>/agentd-token`; if missing, generates 64 hex chars, writes mode 0600 (parent dir created 0700), returns it.
  - `auth::verify_bearer(header_value: Option<&str>, token: &str) -> bool` — constant-time compare of the `Bearer <token>` header against `token`.

- [ ] **Step 1: Write the failing tests** in `agentd/src/connector/auth.rs`:

```rust
//! Bearer-token provenance for the tailnet listener (the UDS listener needs none).
#![forbid(unsafe_code)]

use std::io;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_persist_and_reload_roundtrip() {
        let dir = tempdir().unwrap();
        let t1 = load_or_create_token(dir.path()).unwrap();
        assert_eq!(t1.len(), 64);
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
        // Second call loads the SAME token (no regeneration).
        let t2 = load_or_create_token(dir.path()).unwrap();
        assert_eq!(t1, t2);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        load_or_create_token(dir.path()).unwrap();
        let mode = std::fs::metadata(dir.path().join("agentd-token"))
            .unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn verify_bearer_accepts_correct_rejects_wrong_and_missing() {
        let token = "abc123";
        assert!(verify_bearer(Some("Bearer abc123"), token));
        assert!(!verify_bearer(Some("Bearer wrong"), token));
        assert!(!verify_bearer(Some("abc123"), token));      // missing scheme
        assert!(!verify_bearer(None, token));
        assert!(!verify_bearer(Some("Bearer "), token));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::auth`
Expected: FAIL — `load_or_create_token`/`verify_bearer` not found.

- [ ] **Step 3: Implement** (above the test module). Use the existing `rand` crate if present in the lock; otherwise read from `/dev/urandom` via `std::fs` to avoid a new dep — this implementation uses `getrandom` only if already a transitive dep. To stay dependency-free, read 32 bytes from `OsRng` is unavailable without `rand`; instead read `/dev/urandom`:

```rust
/// Load `<dir>/agentd-token`, or generate + persist a fresh 32-byte (64 hex) token.
pub fn load_or_create_token(dir: &Path) -> io::Result<String> {
    let path = dir.join("agentd-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    // Generate 32 random bytes from the OS CSPRNG.
    let mut buf = [0u8; 32];
    read_random(&mut buf)?;
    let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();

    // Ensure parent dir exists (0700) and write the token file (0600).
    std::fs::create_dir_all(dir)?;
    set_dir_mode(dir, 0o700)?;
    std::fs::write(&path, &token)?;
    set_file_mode(&path, 0o600)?;
    Ok(token)
}

/// Constant-time check of an `Authorization: Bearer <token>` header.
pub fn verify_bearer(header_value: Option<&str>, token: &str) -> bool {
    let Some(h) = header_value else { return false };
    let Some(presented) = h.strip_prefix("Bearer ") else { return false };
    constant_time_eq(presented.as_bytes(), token.as_bytes())
}

/// XOR-accumulate constant-time byte comparison (avoids a `subtle` dep).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn read_random(buf: &mut [u8]) -> io::Result<()> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(buf)
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> io::Result<()> { Ok(()) }
#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) -> io::Result<()> { Ok(()) }
```

Add `pub mod auth;` to `agentd/src/connector/mod.rs`.

- [ ] **Step 4: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::auth`
Expected: 4 tests pass (3 on non-unix — the 0600 test is `#[cfg(unix)]`).
Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add agentd/src/connector/auth.rs agentd/src/connector/mod.rs
git commit -m "feat(agentd): bearer token generate/load (0600) + constant-time verify"
```

---

### Task 3: `tailnet.rs` — `TailnetSource` seam + bind-address resolution

**Files:**
- Create: `agentd/src/connector/tailnet.rs`
- Modify: `agentd/src/connector/mod.rs` (add `pub mod tailnet;`)

**Interfaces:**
- Produces:
  - `trait TailnetSource: Send + Sync { fn tailnet_ip(&self) -> Option<IpAddr>; }`
  - `struct CliTailnetSource;` impl that shells `tailscale status --json` and reads `Self.TailscaleIPs[0]` (first IPv4).
  - `fn resolve_bind_addr(source: &dyn TailnetSource, bind_override: &str, port: u16) -> Result<Option<SocketAddr>, String>` — `bind_override` (non-empty) parsed + rejected if unspecified/`0.0.0.0`; else the tailnet IP; else `Ok(None)` (no TCP listener). Returns `Err` only for an invalid/unspecified override.

- [ ] **Step 1: Write the failing tests** in `agentd/src/connector/tailnet.rs`:

```rust
//! Tailscale bind-address resolution behind a mockable source.
#![forbid(unsafe_code)]

use std::net::{IpAddr, SocketAddr};

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSource(Option<IpAddr>);
    impl TailnetSource for MockSource {
        fn tailnet_ip(&self) -> Option<IpAddr> { self.0 }
    }

    fn ip(s: &str) -> IpAddr { s.parse().unwrap() }

    #[test]
    fn uses_tailnet_ip_when_present_and_no_override() {
        let src = MockSource(Some(ip("100.101.102.103")));
        let addr = resolve_bind_addr(&src, "", 8765).unwrap();
        assert_eq!(addr, Some(SocketAddr::new(ip("100.101.102.103"), 8765)));
    }

    #[test]
    fn none_when_no_tailnet_and_no_override() {
        let src = MockSource(None);
        assert_eq!(resolve_bind_addr(&src, "", 8765).unwrap(), None);
    }

    #[test]
    fn override_wins_over_tailnet() {
        let src = MockSource(Some(ip("100.1.1.1")));
        let addr = resolve_bind_addr(&src, "100.9.9.9", 8765).unwrap();
        assert_eq!(addr, Some(SocketAddr::new(ip("100.9.9.9"), 8765)));
    }

    #[test]
    fn rejects_unspecified_override() {
        let src = MockSource(None);
        assert!(resolve_bind_addr(&src, "0.0.0.0", 8765).is_err());
        assert!(resolve_bind_addr(&src, "::", 8765).is_err());
    }

    #[test]
    fn errors_on_garbage_override() {
        let src = MockSource(None);
        assert!(resolve_bind_addr(&src, "not-an-ip", 8765).is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::tailnet`
Expected: FAIL — items not found.

- [ ] **Step 3: Implement** (above the tests):

```rust
/// Source of the host's current tailnet IP. Seam so resolution is testable
/// without a live tailnet.
pub trait TailnetSource: Send + Sync {
    fn tailnet_ip(&self) -> Option<IpAddr>;
}

/// Production source: shells `tailscale status --json`.
pub struct CliTailnetSource;

impl TailnetSource for CliTailnetSource {
    fn tailnet_ip(&self) -> Option<IpAddr> {
        let out = std::process::Command::new("tailscale")
            .args(["status", "--json"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        // `Self.TailscaleIPs` is an array; take the first IPv4.
        let ips = v.get("Self")?.get("TailscaleIPs")?.as_array()?;
        ips.iter()
            .filter_map(|s| s.as_str())
            .filter_map(|s| s.parse::<IpAddr>().ok())
            .find(|ip| ip.is_ipv4())
    }
}

/// Resolve the TCP bind address. `Ok(None)` ⇒ no TCP listener (UDS still runs).
/// `Err` ⇒ a configured override is invalid/unspecified (caller logs + skips TCP).
pub fn resolve_bind_addr(
    source: &dyn TailnetSource,
    bind_override: &str,
    port: u16,
) -> Result<Option<SocketAddr>, String> {
    if !bind_override.trim().is_empty() {
        let ip: IpAddr = bind_override
            .trim()
            .parse()
            .map_err(|_| format!("invalid http.bind_override: {bind_override:?}"))?;
        if ip.is_unspecified() {
            return Err(format!("refusing to bind unspecified address {ip} (never 0.0.0.0)"));
        }
        return Ok(Some(SocketAddr::new(ip, port)));
    }
    Ok(source.tailnet_ip().map(|ip| SocketAddr::new(ip, port)))
}
```

Add `pub mod tailnet;` to `mod.rs`.

- [ ] **Step 4: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::tailnet`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add agentd/src/connector/tailnet.rs agentd/src/connector/mod.rs
git commit -m "feat(agentd): TailnetSource seam + bind resolution (rejects 0.0.0.0)"
```

---

### Task 4: `event_hub.rs` — `EventHub` (broadcast + ring + run→conv map) + `BroadcastEmitter`

**Files:**
- Create: `agentd/src/connector/event_hub.rs`
- Modify: `agentd/src/connector/mod.rs` (add `pub mod event_hub;`)

**Interfaces:**
- Consumes: `crate::seams::{AgentEvent, EventEmitter, RecordingEmitter}`.
- Produces:
  - `struct EventHub` (cloneable handle; internally `Arc`). Methods:
    - `EventHub::new(buffer_per_conv: usize) -> Self`
    - `fn publish(&self, ev: &AgentEvent)` — assigns a monotonic global seq, stores in the owning conversation's ring, broadcasts `SeqEvent { seq, conversation: String, ev: AgentEvent }`.
    - `fn link_run(&self, run_id: &str, conversation_id: &str)` — records a run→conversation mapping so run/flow events route to the right conversation.
    - `fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SeqEvent>`
    - `fn replay(&self, conversation_id: &str, after_seq: u64) -> Vec<SeqEvent>` — buffered events for that conversation with `seq > after_seq`, in order.
    - `fn owning_conversation(&self, ev: &AgentEvent) -> String` — `thread_or_run`, or the linked conversation if `thread_or_run` is a known run id.
  - `struct SeqEvent { pub seq: u64, pub conversation: String, pub ev: AgentEvent }` (Clone).
  - `struct BroadcastEmitter { ... }` implementing `EventEmitter`, constructed `BroadcastEmitter::new(inner: Arc<dyn EventEmitter>, hub: EventHub)`; `emit` publishes to the hub then forwards to `inner`.

- [ ] **Step 1: Write the failing tests** in `agentd/src/connector/event_hub.rs`:

```rust
//! In-process event fan-out: tees the AgentEvent stream to D-Bus (unchanged) and SSE subscribers.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::seams::{AgentEvent, EventEmitter};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seams::RecordingEmitter;

    fn ev(thread_or_run: &str, kind: &str) -> AgentEvent {
        AgentEvent {
            project: "/p".into(),
            thread_or_run: thread_or_run.into(),
            ts: 0,
            payload: serde_json::json!({ "kind": kind }),
        }
    }

    #[tokio::test]
    async fn subscriber_receives_published_event() {
        let hub = EventHub::new(16);
        let mut rx = hub.subscribe();
        hub.publish(&ev("conv-1", "delta"));
        let got = rx.recv().await.unwrap();
        assert_eq!(got.conversation, "conv-1");
        assert_eq!(got.seq, 1);
    }

    #[test]
    fn replay_returns_events_after_seq_for_that_conversation_only() {
        let hub = EventHub::new(16);
        hub.publish(&ev("conv-1", "a")); // seq 1
        hub.publish(&ev("conv-2", "b")); // seq 2
        hub.publish(&ev("conv-1", "c")); // seq 3
        let replayed = hub.replay("conv-1", 0);
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].seq, 1);
        assert_eq!(replayed[1].seq, 3);
        // after_seq filters:
        assert_eq!(hub.replay("conv-1", 1).len(), 1);
    }

    #[test]
    fn ring_buffer_evicts_past_capacity() {
        let hub = EventHub::new(2);
        for _ in 0..5 { hub.publish(&ev("conv-1", "x")); }
        assert_eq!(hub.replay("conv-1", 0).len(), 2);
    }

    #[test]
    fn run_events_route_to_linked_conversation() {
        let hub = EventHub::new(16);
        hub.link_run("run-9", "conv-7");
        hub.publish(&ev("run-9", "RunFinished")); // thread_or_run is the run id
        let replayed = hub.replay("conv-7", 0);
        assert_eq!(replayed.len(), 1);
        // and NOT under the raw run id:
        assert_eq!(hub.replay("run-9", 0).len(), 0);
    }

    #[test]
    fn broadcast_emitter_tees_to_inner_and_hub() {
        let rec = Arc::new(RecordingEmitter::default());
        let hub = EventHub::new(16);
        let emitter = BroadcastEmitter::new(rec.clone(), hub.clone());
        emitter.emit(ev("conv-1", "delta"));
        assert_eq!(rec.events().len(), 1);             // inner saw it
        assert_eq!(hub.replay("conv-1", 0).len(), 1);  // hub saw it
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::event_hub`
Expected: FAIL — items not found.

- [ ] **Step 3: Implement** (above the tests):

```rust
/// An AgentEvent tagged with a monotonic sequence and its owning conversation.
#[derive(Clone, Debug)]
pub struct SeqEvent {
    pub seq: u64,
    pub conversation: String,
    pub ev: AgentEvent,
}

struct Inner {
    seq: AtomicU64,
    tx: tokio::sync::broadcast::Sender<SeqEvent>,
    buffer_per_conv: usize,
    /// conversation_id -> bounded VecDeque of recent SeqEvents.
    rings: Mutex<HashMap<String, std::collections::VecDeque<SeqEvent>>>,
    /// run_id -> conversation_id, so run/flow events route correctly.
    run_links: Mutex<HashMap<String, String>>,
}

/// Cloneable fan-out hub. Cloning shares one underlying broadcast + buffers.
#[derive(Clone)]
pub struct EventHub(Arc<Inner>);

impl EventHub {
    pub fn new(buffer_per_conv: usize) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(1024);
        EventHub(Arc::new(Inner {
            seq: AtomicU64::new(0),
            tx,
            buffer_per_conv: buffer_per_conv.max(1),
            rings: Mutex::new(HashMap::new()),
            run_links: Mutex::new(HashMap::new()),
        }))
    }

    pub fn link_run(&self, run_id: &str, conversation_id: &str) {
        self.0.run_links.lock().unwrap_or_else(|e| e.into_inner())
            .insert(run_id.to_string(), conversation_id.to_string());
    }

    pub fn owning_conversation(&self, ev: &AgentEvent) -> String {
        let links = self.0.run_links.lock().unwrap_or_else(|e| e.into_inner());
        links.get(&ev.thread_or_run).cloned().unwrap_or_else(|| ev.thread_or_run.clone())
    }

    pub fn publish(&self, ev: &AgentEvent) {
        let seq = self.0.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let conversation = self.owning_conversation(ev);
        let item = SeqEvent { seq, conversation: conversation.clone(), ev: ev.clone() };
        {
            let mut rings = self.0.rings.lock().unwrap_or_else(|e| e.into_inner());
            let ring = rings.entry(conversation).or_default();
            ring.push_back(item.clone());
            while ring.len() > self.0.buffer_per_conv {
                ring.pop_front();
            }
        }
        let _ = self.0.tx.send(item); // Err only if no subscribers; fine.
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SeqEvent> {
        self.0.tx.subscribe()
    }

    pub fn replay(&self, conversation_id: &str, after_seq: u64) -> Vec<SeqEvent> {
        let rings = self.0.rings.lock().unwrap_or_else(|e| e.into_inner());
        rings.get(conversation_id)
            .map(|ring| ring.iter().filter(|e| e.seq > after_seq).cloned().collect())
            .unwrap_or_default()
    }
}

/// EventEmitter that tees every event to the in-process hub AND the inner emitter
/// (the production D-Bus `BusEmitter`, unchanged).
pub struct BroadcastEmitter {
    inner: Arc<dyn EventEmitter>,
    hub: EventHub,
}

impl BroadcastEmitter {
    pub fn new(inner: Arc<dyn EventEmitter>, hub: EventHub) -> Self {
        Self { inner, hub }
    }
}

impl EventEmitter for BroadcastEmitter {
    fn emit(&self, ev: AgentEvent) {
        self.hub.publish(&ev);
        self.inner.emit(ev);
    }
}
```

Add `pub mod event_hub;` to `mod.rs`.

- [ ] **Step 4: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::event_hub`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add agentd/src/connector/event_hub.rs agentd/src/connector/mod.rs
git commit -m "feat(agentd): EventHub fan-out (broadcast + per-conv ring + run links) + BroadcastEmitter"
```

---

### Task 5: `HttpConfig` in `oxidemx-shared` config

**Files:**
- Modify: `oxidemx-shared/src/config.rs` (add `HttpConfig`, add `http` field to `AppConfig` near line 1013)

**Interfaces:**
- Produces: `oxidemx_shared::config::HttpConfig { enabled: bool, port: u16, bind_override: String, event_buffer: usize }` with `Default` (enabled=false, port=8765, bind_override="", event_buffer=256). `AppConfig.http: HttpConfig` with `#[serde(default)]`.

- [ ] **Step 1: Write the failing test** — add to the `#[cfg(test)] mod tests` in `oxidemx-shared/src/config.rs`:

```rust
#[test]
fn http_config_defaults_off_and_roundtrips() {
    let c = HttpConfig::default();
    assert!(!c.enabled, "http transport is opt-in");
    assert_eq!(c.port, 8765);
    assert_eq!(c.event_buffer, 256);
    assert!(c.bind_override.is_empty());
    // Absent in JSON → defaults (serde default).
    let app: AppConfig = serde_json::from_str("{}").unwrap();
    assert!(!app.http.enabled);
    assert_eq!(app.http.port, 8765);
    // Round-trips.
    let json = serde_json::to_string(&app).unwrap();
    let back: AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.http.port, 8765);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-shared http_config`
Expected: FAIL — `HttpConfig` not found.

- [ ] **Step 3: Implement** — add the struct (match the file's existing serde-default style; check a neighbouring config struct like `AiConfig` near line 1203 for the exact `#[derive(...)]` + `#[serde(default)]` conventions used in this file, and mirror them):

```rust
/// HTTP/SSE transport (1b). Opt-in; the existing D-Bus path is unaffected when off.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    /// Master switch. When false, agentd starts no HTTP listeners (default).
    pub enabled: bool,
    /// TCP port for the tailnet listener.
    pub port: u16,
    /// Explicit bind IP override ("" = auto-detect the tailnet IP). Never 0.0.0.0.
    pub bind_override: String,
    /// Per-conversation SSE replay ring-buffer size.
    pub event_buffer: usize,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { enabled: false, port: 8765, bind_override: String::new(), event_buffer: 256 }
    }
}
```

In `AppConfig` (struct near line 1013, where `pub overlay: OverlayConfig` lives), add the field with the same serde-default attribute the sibling fields use:
```rust
    #[serde(default)]
    pub http: HttpConfig,
```
If `AppConfig` has a manual `Default` impl, add `http: HttpConfig::default()` there too.

- [ ] **Step 4: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-shared http_config`
Expected: PASS.
Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-shared`
Expected: existing config tests stay green (the `{}`→defaults round-trip still holds).

- [ ] **Step 5: Commit**

```bash
git add oxidemx-shared/src/config.rs
git commit -m "feat(shared): AppConfig.http (HttpConfig, opt-in, port 8765 / buffer 256)"
```

---

### Task 6: HTTP messaging router + `AppState` + handlers (no SSE yet)

**Files:**
- Create: `agentd/src/connector/http/mod.rs`, `agentd/src/connector/http/routes_messaging.rs`
- Modify: `agentd/src/connector/mod.rs` (add `pub mod http;`)

**Interfaces:**
- Consumes: `crate::interface::AgentService`, `connector::caps::{ScopeTier, ConnectorCaps}`, `connector::event_hub::EventHub`, `crate::error::AgentdError`. AgentService methods (all `async`, on `Arc<AgentService>`): `list_projects() -> Result<Vec<Project>>`, `list_conversations(project_id:&str) -> Result<Vec<Conversation>>`, `get_conversation(id:&str) -> Result<Option<Conversation>>`, `create_conversation(project_id:&str, working_dir:Option<&Path>) -> Result<Conversation>`, `get_transcript(project:&str, thread:&str) -> Result<Vec<TranscriptTurn>>`, `send_message(project:&str, thread:&str, text:&str, model_hint:Option<&str>) -> Result<String>`, `respond_approval(project:&str, request_id:&str, allow:bool, reason:Option<&str>) -> Result<()>`. NOTE: `send_message`/`get_transcript`'s `project` arg is the conversation's **working_dir path string**, and `thread` is the conversation id.
- Produces:
  - `http::AppState { svc: Arc<AgentService>, hub: EventHub, caps: ConnectorCaps, msg_seq: Arc<AtomicU64> }` (Clone).
  - `http::build_messaging_router(state: AppState) -> axum::Router` — mounts the messaging routes below.
  - `http::ApiError` (maps `AgentdError` → status + JSON body `{error:{code,message}}`), implementing `IntoResponse`.
  - `http::next_message_id(state: &AppState) -> String`.

- [ ] **Step 1: Write the failing test** — `agentd/src/connector/http/mod.rs` test module drives the router with `tower::ServiceExt::oneshot`. Reuse the crate's existing AgentService test constructor (see the `tests` module in `interface.rs`, e.g. a `TestHarness`/`with_*` builder — find the one that yields a working `Arc<AgentService>` with a mock model that returns a canned reply, and build `AppState` from it). Skeleton:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    // Build an AppState with Control caps over a test AgentService.
    // (Mirror interface.rs's test harness to construct Arc<AgentService>.)
    fn test_state() -> AppState { /* construct per interface.rs test harness */ }

    #[tokio::test]
    async fn health_is_ok() {
        let app = build_messaging_router(test_state());
        let res = app.oneshot(Request::get("/v1/health").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_projects_returns_personal() {
        let app = build_messaging_router(test_state());
        let res = app.oneshot(Request::get("/v1/projects").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.as_array().unwrap().iter().any(|p| p["id"] == "personal"));
    }

    #[tokio::test]
    async fn unknown_conversation_404() {
        let app = build_messaging_router(test_state());
        let res = app.oneshot(
            Request::get("/v1/conversations/does-not-exist/messages").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::http`
Expected: FAIL — items not found.

- [ ] **Step 3: Implement `http/mod.rs`** — AppState, error type, id helper, router assembly:

```rust
//! axum HTTP connector. Delegates to the connector-agnostic AgentService.
#![forbid(unsafe_code)]

pub mod routes_messaging;
pub mod sse;            // Task 7
pub mod routes_control; // Task 8
pub mod server;         // Task 8

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::connector::caps::ConnectorCaps;
use crate::connector::event_hub::EventHub;
use crate::error::AgentdError;
use crate::interface::AgentService;

#[derive(Clone)]
pub struct AppState {
    pub svc: Arc<AgentService>,
    pub hub: EventHub,
    pub caps: ConnectorCaps,
    pub msg_seq: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(svc: Arc<AgentService>, hub: EventHub, caps: ConnectorCaps) -> Self {
        Self { svc, hub, caps, msg_seq: Arc::new(AtomicU64::new(0)) }
    }
}

/// Monotonic-ish message id for action acks.
pub fn next_message_id(state: &AppState) -> String {
    let n = state.msg_seq.fetch_add(1, Ordering::Relaxed) + 1;
    format!("m{}-{}", crate::connector::http::now_ms(), n)
}

pub(crate) fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// HTTP error envelope mapping AgentdError → status + JSON.
pub struct ApiError(pub AgentdError);

impl From<AgentdError> for ApiError {
    fn from(e: AgentdError) -> Self { ApiError(e) }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            AgentdError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AgentdError::Project(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AgentdError::Io(_) | AgentdError::Dbus(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        let body = Json(serde_json::json!({ "error": { "code": code, "message": self.0.to_string() } }));
        (status, body).into_response()
    }
}

pub fn build_messaging_router(state: AppState) -> axum::Router {
    routes_messaging::router(state)
}
```

> The `sse`/`routes_control`/`server` `pub mod` lines reference modules created in Tasks 7–8. As in Task 1, comment them out until their task lands, or create empty stub files. Cleanest: create `sse.rs`, `routes_control.rs`, `server.rs` as empty (`#![forbid(unsafe_code)]` only) in this task so the module tree compiles, and fill them in their tasks.

- [ ] **Step 4: Implement `routes_messaging.rs`**:

```rust
//! Messaging-tier routes (mounted on both UDS and TCP listeners).
#![forbid(unsafe_code)]

use std::path::PathBuf;

use axum::extract::{Path as AxPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::{next_message_id, ApiError, AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/projects", get(list_projects))
        .route("/v1/projects/{project_id}/conversations", get(list_conversations))
        .route("/v1/conversations", post(create_conversation))
        .route("/v1/conversations/{id}", get(get_conversation))
        .route("/v1/conversations/{id}/messages", get(history).post(send_message))
        .route("/v1/conversations/{id}/approvals/{request_id}", post(respond_approval))
        .with_state(state)
}

async fn health() -> &'static str { "ok" }

async fn list_projects(State(st): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let projects = st.svc.list_projects().await?;
    Ok(Json(serde_json::to_value(projects).unwrap_or_default()))
}

async fn list_conversations(
    State(st): State<AppState>,
    AxPath(project_id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let convs = st.svc.list_conversations(&project_id).await?;
    Ok(Json(serde_json::to_value(convs).unwrap_or_default()))
}

#[derive(serde::Deserialize)]
struct CreateConversationBody { project_id: Option<String>, working_dir: Option<String> }

async fn create_conversation(
    State(st): State<AppState>,
    Json(body): Json<CreateConversationBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let project_id = body.project_id.unwrap_or_else(|| "personal".to_string());
    let wd = body.working_dir.map(PathBuf::from);
    let conv = st.svc.create_conversation(&project_id, wd.as_deref()).await?;
    Ok(Json(serde_json::json!({ "conversation_id": conv.id.as_str() })))
}

async fn get_conversation(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match st.svc.get_conversation(&id).await? {
        Some(c) => Ok(Json(serde_json::to_value(c).unwrap_or_default())),
        None => Err(ApiError(crate::error::AgentdError::NotFound(format!("conversation {id}")))),
    }
}

async fn history(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let conv = st.svc.get_conversation(&id).await?
        .ok_or_else(|| crate::error::AgentdError::NotFound(format!("conversation {id}")))?;
    let project = conv.working_dir.to_string_lossy().to_string();
    let turns = st.svc.get_transcript(&project, &id).await?;
    Ok(Json(serde_json::to_value(turns).unwrap_or_default()))
}

#[derive(serde::Deserialize)]
struct SendBody { text: String, model: Option<String> }

async fn send_message(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
    Json(body): Json<SendBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let conv = st.svc.get_conversation(&id).await?
        .ok_or_else(|| crate::error::AgentdError::NotFound(format!("conversation {id}")))?;
    let project = conv.working_dir.to_string_lossy().to_string();
    let message_id = next_message_id(&st);

    // Spawn the turn; deltas stream via the emitter→hub→SSE. Publish a terminal
    // `final`/`error` event so SSE carries the complete reply truthfully.
    let svc = st.svc.clone();
    let hub = st.hub.clone();
    let conv_id = id.clone();
    let model = body.model.clone();
    let text = body.text.clone();
    let mid = message_id.clone();
    tokio::spawn(async move {
        let payload = match svc.send_message(&project, &conv_id, &text, model.as_deref()).await {
            Ok(reply) => serde_json::json!({ "kind": "final", "message_id": mid, "text": reply }),
            Err(e) => serde_json::json!({ "kind": "error", "message_id": mid, "message": e.to_string() }),
        };
        hub.publish(&crate::seams::AgentEvent {
            project: String::new(), thread_or_run: conv_id, ts: super::now_ms(), payload,
        });
    });

    Ok(Json(serde_json::json!({ "message_id": message_id })))
}

#[derive(serde::Deserialize)]
struct ApprovalBody { allow: bool, reason: Option<String> }

async fn respond_approval(
    State(st): State<AppState>,
    AxPath((id, request_id)): AxPath<(String, String)>,
    Json(body): Json<ApprovalBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let conv = st.svc.get_conversation(&id).await?
        .ok_or_else(|| crate::error::AgentdError::NotFound(format!("conversation {id}")))?;
    let project = conv.working_dir.to_string_lossy().to_string();
    st.svc.respond_approval(&project, &request_id, body.allow, body.reason.as_deref()).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
```

Add `pub mod http;` to `connector/mod.rs`.

- [ ] **Step 5: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::http`
Expected: the 3 router tests pass.
Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add agentd/src/connector/http/ agentd/src/connector/mod.rs
git commit -m "feat(agentd): HTTP messaging router + AppState + AgentdError→status mapping"
```

---

### Task 7: SSE subscription endpoint

**Files:**
- Modify: `agentd/src/connector/http/sse.rs` (fill the stub), `agentd/src/connector/http/routes_messaging.rs` (mount the route)

**Interfaces:**
- Consumes: `AppState`, `EventHub::{subscribe, replay}`, `SeqEvent`.
- Produces: `sse::events_handler` mounted at `GET /v1/conversations/{id}/events`. Honors `Last-Event-ID` header: replays buffered events for `id` with `seq > last_event_id`, then streams live `SeqEvent`s filtered to `conversation == id`. Each SSE event: `.id(seq)`, `.event(payload["kind"])`, `.data(payload json)`. KeepAlive enabled.

- [ ] **Step 1: Write the failing test** — in `sse.rs`, an integration-style test: build a router, open the SSE response, publish an event to the hub, assert it arrives. Because driving a live SSE body in a unit test is fiddly, test the **replay + filter logic** directly via a pure helper `collect_replay(hub, id, last) -> Vec<(u64,String)>` plus one live-stream smoke test using `axum::serve` over an ephemeral TCP port (or `tower::oneshot` reading the first chunk). Minimum:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::event_hub::EventHub;
    use crate::seams::AgentEvent;

    fn ev(c: &str, kind: &str) -> AgentEvent {
        AgentEvent { project: "/p".into(), thread_or_run: c.into(), ts: 0,
                     payload: serde_json::json!({"kind": kind}) }
    }

    #[test]
    fn replay_after_last_event_id_filters_by_conversation_and_seq() {
        let hub = EventHub::new(16);
        hub.publish(&ev("c1", "a")); // 1
        hub.publish(&ev("c2", "b")); // 2
        hub.publish(&ev("c1", "c")); // 3
        let got = replay_frames(&hub, "c1", 1); // after seq 1
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 3);
        assert_eq!(got[0].1, "c"); // kind
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::http::sse`
Expected: FAIL — `replay_frames` not found.

- [ ] **Step 3: Implement `sse.rs`**:

```rust
//! SSE subscription: GET /v1/conversations/{id}/events
#![forbid(unsafe_code)]

use std::convert::Infallible;

use axum::extract::{Path as AxPath, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};

use crate::connector::event_hub::{EventHub, SeqEvent};
use super::AppState;

/// (seq, kind) frames buffered for `conversation` with seq > after. Test seam.
pub(crate) fn replay_frames(hub: &EventHub, conversation: &str, after: u64) -> Vec<(u64, String)> {
    hub.replay(conversation, after).into_iter()
        .map(|se| (se.seq, kind_of(&se)))
        .collect()
}

fn kind_of(se: &SeqEvent) -> String {
    se.ev.payload.get("kind").and_then(|k| k.as_str()).unwrap_or("event").to_string()
}

fn to_event(se: &SeqEvent) -> Event {
    Event::default()
        .id(se.seq.to_string())
        .event(kind_of(se))
        .data(se.ev.payload.to_string())
}

pub async fn events_handler(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let last: u64 = headers.get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let rx = st.hub.subscribe();
    let replay = st.hub.replay(&id, last);
    let conv = id.clone();

    let replay_stream = stream::iter(replay.into_iter().map(|se| Ok(to_event(&se))));
    let live_stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(move |res| {
            let conv = conv.clone();
            async move {
                match res {
                    Ok(se) if se.conversation == conv => Some(Ok(to_event(&se))),
                    _ => None, // lagged or other conversation
                }
            }
        });

    Sse::new(replay_stream.chain(live_stream)).keep_alive(KeepAlive::default())
}
```

- [ ] **Step 4: Mount the route** in `routes_messaging.rs` `router()`:
```rust
        .route("/v1/conversations/{id}/events", get(super::sse::events_handler))
```

- [ ] **Step 5: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::http`
Expected: SSE replay test + Task 6 router tests pass.

- [ ] **Step 6: Commit**

```bash
git add agentd/src/connector/http/sse.rs agentd/src/connector/http/routes_messaging.rs
git commit -m "feat(agentd): SSE subscription endpoint with Last-Event-ID replay"
```

---

### Task 8: Listeners (UDS + TCP), scope-by-listener, control router (`/v1/auth/token`), serve()

**Files:**
- Modify: `agentd/src/connector/http/routes_control.rs` (fill stub), `agentd/src/connector/http/server.rs` (fill stub), `agentd/src/connector/http/mod.rs` (export `serve`, `ServeConfig`)

**Interfaces:**
- Consumes: `AppState`, `ConnectorCaps::{UDS_LOCAL, TAILNET_REMOTE}`, `auth::{load_or_create_token, verify_bearer}`, `tailnet::{CliTailnetSource, resolve_bind_addr}`, `build_messaging_router`.
- Produces:
  - `server::ServeConfig { svc, hub, token: String, uds_path: PathBuf, tcp_addr: Option<SocketAddr>, shutdown: CancellationToken }`.
  - `server::serve(cfg) -> ` spawns UDS listener (full router, `UDS_LOCAL` caps) + TCP listener (messaging router + bearer layer, `TAILNET_REMOTE` caps) when `tcp_addr.is_some()`.
  - `routes_control::router(state) -> Router` mounting `GET /v1/auth/token` (returns the token; only ever mounted on the UDS/Control listener).

- [ ] **Step 1: Write the failing tests** — `server.rs` test module: live-wire over a tempfile UDS, asserting health + that the control route is present on UDS. And a TCP-side scope test: build the TCP router (messaging only) and assert `/v1/auth/token` is **absent** (404) there, while a UDS router serves it 200. Use `tower::oneshot`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn state(caps: crate::connector::caps::ConnectorCaps) -> AppState { /* per interface.rs harness */ }

    #[tokio::test]
    async fn control_route_present_on_uds_absent_on_tcp() {
        // UDS (Control) router includes auth/token.
        let uds = full_router(state(crate::connector::caps::ConnectorCaps::UDS_LOCAL), "tok");
        let r = uds.oneshot(Request::get("/v1/auth/token").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        // TCP (Messaging) router does NOT mount control routes.
        let tcp = messaging_only_router(state(crate::connector::caps::ConnectorCaps::TAILNET_REMOTE));
        let r = tcp.oneshot(Request::get("/v1/auth/token").body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }
}
```

> `full_router(state, token)` = messaging routes + control routes; `messaging_only_router(state)` = messaging routes only. Define both in `server.rs` so the listener setup and the tests share them.

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::http::server`
Expected: FAIL — items not found.

- [ ] **Step 3: Implement `routes_control.rs`**:

```rust
//! Control-tier routes — mounted ONLY on the UDS (Control-scope) listener.
#![forbid(unsafe_code)]

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use super::AppState;

/// Control router. The token is captured so the local app can fetch it for pairing.
pub fn router(state: AppState, token: String) -> Router {
    Router::new()
        .route("/v1/auth/token", get(move || {
            let token = token.clone();
            async move { Json(serde_json::json!({ "token": token })) }
        }))
        .with_state(state)
}
```

- [ ] **Step 4: Implement `server.rs`** — router assembly + listeners + bearer layer + shutdown:

```rust
//! Listener setup: UDS (Control) + tailnet TCP (Messaging), scope-by-listener.
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tokio_util::sync::CancellationToken;

use crate::connector::auth::verify_bearer;
use crate::connector::caps::ConnectorCaps;
use crate::connector::event_hub::EventHub;
use crate::interface::AgentService;
use super::{build_messaging_router, routes_control, AppState};

pub struct ServeConfig {
    pub svc: Arc<AgentService>,
    pub hub: EventHub,
    pub token: String,
    pub uds_path: PathBuf,
    pub tcp_addr: Option<SocketAddr>,
    pub shutdown: CancellationToken,
}

/// Messaging routes only (used by the TCP listener + tests).
pub fn messaging_only_router(state: AppState) -> Router {
    build_messaging_router(state)
}

/// Messaging + control routes (used by the UDS listener + tests).
pub fn full_router(state: AppState, token: String) -> Router {
    build_messaging_router(state.clone())
        .merge(routes_control::router(state, token))
}

/// Spawn the listeners. Returns immediately; tasks run until `shutdown` fires.
pub async fn serve(cfg: ServeConfig) -> std::io::Result<()> {
    // ── UDS (Control scope) ──
    let uds_state = AppState::new(cfg.svc.clone(), cfg.hub.clone(), ConnectorCaps::UDS_LOCAL);
    let uds_router = full_router(uds_state, cfg.token.clone());
    if let Some(parent) = cfg.uds_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let _ = std::fs::remove_file(&cfg.uds_path); // clear stale socket
    let uds = tokio::net::UnixListener::bind(&cfg.uds_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cfg.uds_path, std::fs::Permissions::from_mode(0o600))?;
    }
    let uds_shutdown = cfg.shutdown.clone();
    tokio::spawn(async move {
        let r = axum::serve(uds, uds_router)
            .with_graceful_shutdown(async move { uds_shutdown.cancelled().await });
        if let Err(e) = r.await { tracing::warn!("uds serve ended: {e}"); }
    });
    tracing::info!("HTTP UDS listener on {} (Control scope)", cfg.uds_path.display());

    // ── TCP (Messaging scope, bearer auth) ──
    if let Some(addr) = cfg.tcp_addr {
        let tcp_state = AppState::new(cfg.svc.clone(), cfg.hub.clone(), ConnectorCaps::TAILNET_REMOTE);
        let token = cfg.token.clone();
        let tcp_router = messaging_only_router(tcp_state).layer(
            axum::middleware::from_fn(move |req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| {
                let token = token.clone();
                async move {
                    // /v1/health is exempt.
                    if req.uri().path() == "/v1/health" {
                        return next.run(req).await;
                    }
                    let ok = verify_bearer(
                        req.headers().get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()),
                        &token,
                    );
                    if ok { next.run(req).await }
                    else { axum::http::StatusCode::UNAUTHORIZED.into_response() }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let tcp_shutdown = cfg.shutdown.clone();
        tokio::spawn(async move {
            let r = axum::serve(listener, tcp_router)
                .with_graceful_shutdown(async move { tcp_shutdown.cancelled().await });
            if let Err(e) = r.await { tracing::warn!("tcp serve ended: {e}"); }
        });
        tracing::info!("HTTP TCP listener on {addr} (Messaging scope)");
    }
    Ok(())
}
```
(Add `use axum::response::IntoResponse;` where needed for `.into_response()`.)

- [ ] **Step 5: Run tests + clippy**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd connector::http`
Expected: scope test + all prior router/SSE tests pass.
Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add agentd/src/connector/http/server.rs agentd/src/connector/http/routes_control.rs agentd/src/connector/http/mod.rs
git commit -m "feat(agentd): UDS+TCP listeners, scope-by-listener, bearer layer, /v1/auth/token"
```

---

### Task 9: Wire into `main.rs` + end-to-end UDS live test

**Files:**
- Modify: `agentd/src/main.rs`
- Create: `agentd/tests/http_uds_e2e.rs` (integration test)

**Interfaces:**
- Consumes: `AppConfig.http` (via `oxidemx_shared::config`), `connector::{event_hub::{EventHub, BroadcastEmitter}, http::server::{serve, ServeConfig}, auth::load_or_create_token, tailnet::{CliTailnetSource, resolve_bind_addr}}`.

- [ ] **Step 1: Modify `main.rs`** — after the emitter channel is built (around the current line 104) and before `AgentService::new`, conditionally build the hub + wrap the emitter:

```rust
    // ── HTTP transport (1b), opt-in via AppConfig.http.enabled ──
    let http_cfg = oxidemx_shared::config::default_config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<oxidemx_shared::config::AppConfig>(&s).ok())
        .map(|c| c.http)
        .unwrap_or_default();

    let (emitter, http_hub): (Arc<dyn EventEmitter>, Option<EventHub>) = if http_cfg.enabled {
        let bus: Arc<dyn EventEmitter> = Arc::new(BusEmitter { tx });
        let hub = EventHub::new(http_cfg.event_buffer);
        let wrapped: Arc<dyn EventEmitter> = Arc::new(BroadcastEmitter::new(bus, hub.clone()));
        (wrapped, Some(hub))
    } else {
        (Arc::new(BusEmitter { tx }), None)
    };
```
Replace the existing `let emitter: Arc<dyn EventEmitter> = Arc::new(BusEmitter { tx });` with the block above. Add imports:
```rust
use agentd::connector::auth::load_or_create_token;
use agentd::connector::event_hub::{BroadcastEmitter, EventHub};
use agentd::connector::http::server::{serve, ServeConfig};
use agentd::connector::tailnet::{resolve_bind_addr, CliTailnetSource};
use tokio_util::sync::CancellationToken;
```

After `svc` is built + migration runs (current line ~139), before building the zbus connection, spawn the HTTP server when enabled:

```rust
    if let Some(hub) = http_hub {
        // Token dir = user config dir (XDG_CONFIG_HOME or ~/.config) / oxidemx.
        let cfg_dir = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| std::path::PathBuf::from(".config"))
            .join("oxidemx");
        let token = load_or_create_token(&cfg_dir).unwrap_or_default();

        let uds_path = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| std::path::PathBuf::from(format!("/run/user/{}", unsafe_uid())))
            .join("oxidemx").join("agentd.sock");

        let tcp_addr = match resolve_bind_addr(&CliTailnetSource, &http_cfg.bind_override, http_cfg.port) {
            Ok(a) => a,
            Err(e) => { tracing::warn!("http tcp bind disabled: {e}"); None }
        };

        let serve_cfg = ServeConfig {
            svc: svc.clone(),
            hub,
            token,
            uds_path,
            tcp_addr,
            shutdown: CancellationToken::new(),
        };
        if let Err(e) = serve(serve_cfg).await {
            tracing::warn!("http serve failed to start (non-fatal): {e}");
        }
    }
```
Add a small helper for the uid fallback (only used when `XDG_RUNTIME_DIR` is unset):
```rust
fn unsafe_uid() -> u32 {
    // Best-effort: parse from $UID or default 1000. Avoids a libc dep.
    std::env::var("UID").ok().and_then(|s| s.parse().ok()).unwrap_or(1000)
}
```

- [ ] **Step 2: Write the e2e test** `agentd/tests/http_uds_e2e.rs` — start `serve()` with a tempfile UDS + a test `AgentService`, then drive it with a UDS HTTP client. Use `hyper-util`/`reqwest`? To avoid new deps, connect a raw `tokio::net::UnixStream` and speak HTTP/1.1 by hand for `GET /v1/health`, asserting `200 OK` in the response head. Skeleton:

```rust
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn uds_health_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("agentd.sock");
    // Build ServeConfig with a test AgentService + EventHub (mirror interface.rs harness;
    // expose a pub(crate) test constructor or a small test-only helper as needed).
    // ... start serve(cfg).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
    stream.write_all(b"GET /v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf);
    assert!(head.starts_with("HTTP/1.1 200"), "got: {head}");
}
```
> If constructing a full `AgentService` from an integration test (outside the crate's `#[cfg(test)]`) is impractical, instead keep this as a `#[cfg(test)] mod` inside `server.rs` reusing the in-crate harness, and assert health over a real bound UDS there. Either location is acceptable; the requirement is: a real `serve()` over a real UDS answers `GET /v1/health` with 200.

- [ ] **Step 3: Run the full suite + clippy + build the binary**

Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd -- --test-threads=1`
Expected: all green (single-threaded avoids the pre-existing FLOW-test env-var flake).
Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build -p agentd --bin oxidemx-agentd`
Expected: builds clean.
Run: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd`
Expected: clean.

- [ ] **Step 4: Verify the disabled path is unchanged** — add/confirm a test or manual check: with `http.enabled=false`, `main` selects the plain `BusEmitter` (no `BroadcastEmitter`). A focused assertion: factor the emitter-selection into a small `fn select_emitter(cfg:&HttpConfig, tx) -> (Arc<dyn EventEmitter>, Option<EventHub>)` and unit-test that `enabled=false` ⇒ `None` hub.

- [ ] **Step 5: Commit**

```bash
git add agentd/src/main.rs agentd/tests/http_uds_e2e.rs
git commit -m "feat(agentd): wire HTTP transport into main (opt-in) + UDS e2e test"
```

---

### Task 10: Settings UI — HTTP-enable toggle (distrobox build)

**Files:**
- Modify: the AI/settings tab in `oxidemx-settings/src/` that hosts the existing `use_agentd` toggle (locate it: `grep -rn "use_agentd" oxidemx-settings/src/`).

**Interfaces:**
- Consumes: `oxidemx_shared::config::AppConfig.http.enabled`.

> **BUILD ENV:** this task builds in the `claude_development` distrobox (GTK), NOT host-side. Do not use `CARGO_TARGET_DIR=/tmp/oxidemx-host-target` here.

- [ ] **Step 1: Locate the existing toggle** — `grep -rn "use_agentd" oxidemx-settings/src/` to find the widget + the load/save plumbing for an `AiConfig` bool. The new toggle mirrors it exactly but binds `config.http.enabled`.

- [ ] **Step 2: Add the toggle** — in the same settings section, add a labeled switch "Enable HTTP/SSE transport (remote access)" bound to `config.http.enabled`, wired into the same load-from-config / write-to-config path the `use_agentd` switch uses. Match the surrounding widget-construction style of that file.

- [ ] **Step 3: Build + smoke-test in distrobox**

Run (inside distrobox): `cargo build -p oxidemx-settings`
Expected: builds clean.
Manual: launch settings, confirm the toggle renders, flip it, confirm `~/.config/oxidemx/config.json` gains `"http": { "enabled": true }`.

- [ ] **Step 4: Commit**

```bash
git add oxidemx-settings/src/
git commit -m "feat(settings): toggle for the HTTP/SSE transport (config.http.enabled)"
```

---

## Self-Review

**Spec coverage:**
- Capability contract (`ScopeTier`/`ConnectorCaps`) → Task 1. ✅
- UDS-local Control + tailnet-TCP Messaging, scope-by-listener, never 0.0.0.0 → Tasks 3, 8. ✅
- Endpoint surface (health/projects/conversations/messages/events/approvals + `/v1/auth/token`) → Tasks 6, 7, 8. ✅
- Subscription streaming + `BroadcastEmitter` fan-out + Last-Event-ID + run→conv linkage → Tasks 4, 7. ✅
- Tailscale bind resolution behind a mock seam → Task 3. ✅
- Bearer token provenance (0600, constant-time) → Tasks 2, 8. ✅
- `[http]` config opt-in default-off + settings toggle → Tasks 5, 10. ✅
- main.rs wiring + disabled-path-unchanged → Task 9. ✅
- D-Bus untouched → no task modifies `interface.rs` core or the D-Bus drain (only `main.rs` emitter selection, additively). ✅
- Testing (pure/mock + live-wire UDS + no-regression) → each task's tests + Task 9 e2e. ✅
- Deferred (registry/InboundEvent, control endpoints beyond auth/token, live tailnet verification, PassKeys) → not in any task, per spec Out-of-scope. ✅

**Placeholder scan:** the `test_state()`/harness construction in Tasks 6/8/9 references the crate's existing `interface.rs` test harness rather than reproducing it — the implementer must read that harness (named in the task) to build `Arc<AgentService>`. This is a deliberate "consume the existing pattern" instruction, not a content gap; every other step has concrete code. The Task 1/6 `pub mod` stub note is explicit about creating empty module files to keep the tree compiling.

**Type consistency:** `AppState`, `EventHub`, `ConnectorCaps`, `SeqEvent`, `ServeConfig`, `ApiError` names are consistent across Tasks 4/6/7/8/9. `send_message(project=working_dir, thread=conversation_id, text, model_hint)` and `respond_approval(project, request_id, allow, reason)` match the real signatures pulled from `interface.rs`. `resolve_bind_addr` / `load_or_create_token` / `verify_bearer` signatures match their definitions and uses.

---

## Execution Handoff

Recommended: subagent-driven-development (fresh implementer per task, per-task spec+quality review, one whole-branch opus review = merge gate). Isolate in a worktree off `phase1-local-llm-gateway`.
