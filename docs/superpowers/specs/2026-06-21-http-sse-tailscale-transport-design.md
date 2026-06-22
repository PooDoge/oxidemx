# HTTP/SSE + Tailscale Transport — Design (Sub-project 1b)

**Status:** Approved for planning (2026-06-21).
**Branch:** new branch off `phase1-local-llm-gateway` (1a merged at `bc575fd`, resume anchor `58ea9b6`).
**Program context:** Second sub-project of the **AI-chat rewrite program**:
1. **Projects · Conversations · Worktrees model** (1a — DONE + MERGED `bc575fd`).
2. **HTTP/SSE + Tailscale transport** (this doc = phase **1b**).
3. **Freya chat application** (desktop + Android, own spec) — consumes this transport.
4. **Overlay chat decommission** (own spec).
Related memory: [[project_connector_architecture]], [[project_ai_chat_rewrite]], [[project_flow_delivery_s1]], [[feedback_best_practices_rule]].

**Goal:** Give agentd a single capability-scoped HTTP/SSE surface — served locally over a Unix domain socket (full **Control** scope) and remotely over the Tailscale interface (**Messaging** scope) — so one cross-platform Freya client (desktop + Android) can prompt, stream, and approve over a secure transport, with the high-security control plane structurally confined to the same device. The live D-Bus path is left untouched.

**Architecture:** A new connector-agnostic `connector` module in agentd. `caps.rs` holds the durable capability contract (`ConnectorCaps`, `ScopeTier`). `http/` is an axum server that delegates to the existing `Arc<AgentService>` (the transport-agnostic core from 1a). A `BroadcastEmitter` tees the existing `AgentEvent` stream to both the D-Bus drain (unchanged) and a `tokio::sync::broadcast` that SSE handlers subscribe to. No connector registry, no `InboundEvent`/`OutboundMessage` normalization, no D-Bus refactor — those stay deferred to the later SP-Connectors slice.

**Tech Stack:** Rust, agentd (host-side build), `axum` 0.8 + `tokio` (`net` UnixListener/TcpListener, `sync::broadcast`), `tower-http` (trace/limit), `serde`/JSON, `zbus` (unchanged). Tailscale via the `tailscaled` sidecar (LocalAPI / `tailscale status --json`), **not** `tailscale-rs`.

---

## Global Constraints (bind every task)

- **Rule 0 — field-standard naming.** Transport surface follows the **agent-protocol** shape (conversations + messages + SSE), `/v1`-prefixed. Capability tiers use the OAuth-/API-token-standard term **`scope`** (`ScopeTier`); the connector capability descriptor is **`ConnectorCaps`** (the name fixed in `docs/research/connector-architecture.md`). Core stays **connector-agnostic** — the HTTP layer is the only transport-aware code, exactly as the D-Bus `AgentInterface` is today.
- **Rule 1 — truthfulness is structural.** Every value returned over HTTP (conversation list, message history, run/flow status, SSE events) comes from the authoritative store (`ProjectRegistry`/`ConversationIndex`/`TranscriptStore`/`run.json`) or the live `AgentEvent` stream — never a fabricated or cached-in-the-handler value. SSE event payloads are re-emitted verbatim from the same `AgentEvent`s the D-Bus drain sees.
- **Rule 2 — Rust quality bar.** clippy-clean, hand-formatted (no repo-wide `cargo fmt`), `?` over unwrap in non-test code, newtypes over primitive obsession, `thiserror` errors, trait seams (`Arc<dyn Trait>`) for anything mocked (the Tailnet source, the clock). No gold-plating: build the Messaging tier in full + the Control-tier seam; do **not** build the connector registry, `InboundEvent` normalization, or speculative control endpoints.
- **Rule 3 — builds + process.** agentd builds **host-side** (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target`, rustup). Isolate work in a git worktree off `phase1-local-llm-gateway` (Jim edits the main checkout concurrently). Commit each task. Brainstorm→spec→plan→subagent-driven build.
- **Security invariants (non-negotiable):** never bind `0.0.0.0`; the tailnet listener binds the resolved tailnet IP only. The **Control** scope is reachable **only** over the UDS listener — control routes are never mounted on the TCP listener. Bearer token required on the TCP listener; the UDS listener authenticates by filesystem permission (0600) + same-user peer credential. The token file is mode 0600.

---

## Background — current state (from the codebase map)

- **Core is already transport-agnostic (1a).** `AgentService` ([interface.rs]) owns the Project registry, per-project Conversation index, transcripts, `.oxide` resolver, worktree mechanics, `send_message`, and the conductor run launcher. The D-Bus `AgentInterface` is a thin wrapper that delegates to it. This sub-project adds a **second** thin wrapper (HTTP) over the same `Arc<AgentService>` — it does not change the core.
- **Single event side-channel today.** `main.rs` builds one `Arc<dyn EventEmitter>` = `BusEmitter` → one `mpsc` channel → one drain task → D-Bus `event`/`model_status_changed`/`approval_requested` signals. `AgentEvent { project, thread_or_run, ts, payload: Value }` ([seams.rs]) is the unit. SSE needs to observe this **same** stream → a fan-out emitter is the one genuinely-new piece of shared infrastructure.
- **No `Connector` trait/registry exists in code** — only designed in `docs/research/connector-architecture.md`. 1b intentionally does **not** build it; it builds the capability contract (`ConnectorCaps`) that the registry would later consume.
- **Tailscale:** `tailscale`/`tailscaled` binaries are installed on the dev box but `tailscaled` is not running / not logged into a tailnet. Live remote verification is therefore **deferred** to a manual step; everything else is built + tested over the UDS listener, which needs no tailscale.

This sub-project **adds a transport**; it does not start over and does not refactor the live D-Bus path.

---

## The capability contract (`connector/caps.rs`)

The durable, expensive-to-retrofit part. Every transport declares what it may do; the HTTP edge enforces it.

```rust
/// Ordered capability tier. `Control` implies `Messaging`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeTier {
    /// Conversation/messaging surface: safe to expose remotely.
    Messaging,
    /// Messaging + the high-security control plane (AI settings, skill/flow
    /// management + generation, conductor/orchestrator control + debug).
    /// Same-device only.
    Control,
}

/// What a given connector/connection is allowed to do.
#[derive(Debug, Clone, Copy)]
pub struct ConnectorCaps {
    pub scope: ScopeTier,
    pub can_stream: bool,
}

impl ConnectorCaps {
    pub const UDS_LOCAL: ConnectorCaps =
        ConnectorCaps { scope: ScopeTier::Control, can_stream: true };
    pub const TAILNET_REMOTE: ConnectorCaps =
        ConnectorCaps { scope: ScopeTier::Messaging, can_stream: true };

    /// True if this connection may invoke a route requiring `required`.
    pub fn allows(&self, required: ScopeTier) -> bool {
        self.scope >= required
    }
}
```

- The scope of a request is **fixed by the listener it arrived on** (see below), injected as an axum request extension. No per-request IP sniffing.
- Control routes are mounted only on the UDS router. A Messaging-scope request that somehow reaches a control route is rejected `403` by a scope-guard layer (defense in depth even though the route isn't mounted on TCP).

---

## Listeners → scope (`connector/http/server.rs`)

**Two listeners, one shared `AppState`, scope-by-listener:**

| Listener | Address | Scope | Auth | When it runs |
|----------|---------|-------|------|--------------|
| **UDS** | `$XDG_RUNTIME_DIR/oxidemx/agentd.sock` (0600) | `Control` | filesystem perm + same-user peer cred (no token) | whenever HTTP enabled |
| **Tailnet TCP** | `<resolved-tailnet-ip>:<port>` (never `0.0.0.0`) | `Messaging` | `Authorization: Bearer <token>` | only when a tailnet is up |

- The UDS router = full router (messaging + control). The TCP router = messaging router only, wrapped in the bearer-auth layer.
- Both share the same `AppState { svc: Arc<AgentService>, events: EventHub, caps: ConnectorCaps }` (caps differs per listener) and the same handlers; the handlers read `ConnectorCaps` from the request extension and consult `allows(...)` for any control-scoped action.
- axum serves each listener with `axum::serve(listener, router)` on its own `tokio::spawn`ed task; shutdown via a shared `CancellationToken`.

`$XDG_RUNTIME_DIR` resolution: env var if set + absolute, else `/run/user/<uid>`; the `oxidemx/` parent dir is created 0700, the socket removed-if-stale then bound, then `chmod` 0600.

---

## Endpoint surface (`/v1`, agent-protocol shape)

**Messaging tier — built in full (mounted on both listeners):**
```
GET  /v1/health                                    liveness (no auth)
GET  /v1/projects                                  list projects (read)
GET  /v1/projects/{project_id}/conversations       list conversations (read)
POST /v1/conversations                             create  body {project_id?, working_dir?} → {conversation_id}
GET  /v1/conversations/{id}                         conversation metadata
GET  /v1/conversations/{id}/messages               transcript history (read)
POST /v1/conversations/{id}/messages               enqueue user turn  body {text, model?} → {message_id}
GET  /v1/conversations/{id}/events                  SSE subscription (Last-Event-ID supported)
POST /v1/conversations/{id}/approvals/{request_id}  respond to a gated tool call  body {verdict: allow|deny|always|edit, reason?, args?}
```

**Control tier — UDS listener only; seam built, near-empty in 1b:**
```
GET  /v1/auth/token                                fetch the bearer token (for displaying/pairing the remote client)
```
> The settings/skills/flows/conductor control endpoints are **out of scope for 1b** — they land in sub-project 2 as the Freya control surface needs them. Each is then a one-route addition under the already-gated control group, requiring no core or contract change. 1b ships exactly one control route (`/v1/auth/token`) to exercise and prove the gating seam end-to-end.

**Health is unauthenticated** (liveness probes); every other route requires the listener's auth. Errors use a consistent JSON body `{error: {code, message}}` with appropriate status (400/401/403/404/409/500), mapped from `AgentdError` via a `thiserror`→status table.

`POST /v1/conversations/{id}/messages` returns `{message_id}` **immediately** after enqueuing; the assistant reply, tool calls, run/flow events, auto-deliveries, and approval requests all arrive on the conversation's `/events` SSE — see streaming.

---

## Streaming — subscription model + event fan-out

**Why subscription, not per-turn inline:** only a persistent per-conversation subscription handles (a) cross-device — phone and desktop both attached to the same conversation, (b) S1 flow auto-delivery — results pushed into a conversation by a background run, and (c) remote approvals — an `approval_request` surfacing mid-turn. A per-turn `messages/stream` response cannot carry events that originate outside that one request.

**Fan-out (`connector/event_hub.rs` + a `BroadcastEmitter`):**
```rust
/// Tees every AgentEvent to the existing D-Bus drain AND an in-process broadcast.
pub struct BroadcastEmitter {
    inner: Arc<dyn EventEmitter>,            // the existing BusEmitter (D-Bus)
    hub: EventHub,
}
impl EventEmitter for BroadcastEmitter {
    fn emit(&self, ev: AgentEvent) {
        self.hub.publish(&ev);               // SSE subscribers
        self.inner.emit(ev);                 // D-Bus drain — unchanged
    }
}
```
- `main.rs` wraps the production `BusEmitter` in `BroadcastEmitter` **only when HTTP is enabled**; when disabled, the emitter is the plain `BusEmitter` (zero behavior change, D-Bus path bit-identical).
- `EventHub` keeps a `tokio::sync::broadcast::Sender<Arc<AgentEvent>>` plus a small per-conversation **ring buffer** (last N events, default 256, sequence-numbered) for `Last-Event-ID` replay.
- An SSE handler for conversation `C`: on connect, if `Last-Event-ID` is present, first replays buffered events with seq > that id, then forwards live events whose **owning conversation** is `C`. An event's owning conversation is its `thread_or_run` for chat-thread events, or — for conductor run/flow events whose `thread_or_run` is a run id — the S1 `conversation_id` linkage (a run launched from a conversation delivers its events there). The `EventHub` resolves ownership via a `run_id → conversation_id` map populated when a run is launched with a `conversation_id` (the same linkage S1's auto-delivery uses); chat events resolve directly. Each SSE frame: `id: <seq>`, `event: <kind>`, `data: <the AgentEvent.payload JSON, verbatim>`. A periodic `: keep-alive` comment prevents idle-connection timeouts.
- SSE event `kind`s are exactly the existing payload kinds (`delta`/token, `tool_call`, the conductor `RunEvent` kinds, `delivery`, `ApprovalRequest`, `model_status`) — the wire stays identical to D-Bus so clients written against one match the other.

Scope note: the broadcast is in-process; a Messaging-scope subscriber only ever receives events for conversations it can name, and conversation IDs are unguessable (existing id scheme). No control-plane events flow over the messaging SSE.

---

## Tailscale bind resolution (`connector/tailnet.rs`)

```rust
/// Seam so bind resolution is unit-testable without a live tailnet.
pub trait TailnetSource: Send + Sync {
    /// The host's current tailnet IPv4, or None if no tailnet is up.
    fn tailnet_ip(&self) -> Option<IpAddr>;
}
```
- **`CliTailnetSource`** (production): shells `tailscale status --json` and reads `Self.TailscaleIPs[0]` (IPv4). Absent/error/`tailscaled` down ⇒ `None` (logged, non-fatal).
- Resolution order for the TCP bind address: explicit `[http].bind_override` (if set) → `TailnetSource::tailnet_ip()` → **none** (TCP listener simply does not start; UDS still runs).
- The bind address is **never** `0.0.0.0`; `bind_override` is validated to reject `0.0.0.0`/unspecified addresses at startup (error + skip TCP, keep UDS).
- Live remote reachability over the tailnet is a **deferred manual verification** step (tailscaled not yet up on the dev box); the resolution logic is covered by `TailnetSource`-mock tests now.

---

## Auth + token provenance (`connector/auth.rs`)

- **Token:** 32 bytes from the OS CSPRNG, hex-encoded, generated on first run when missing, persisted to `~/.config/oxidemx/agentd-token` mode 0600 (parent dir 0700). Loaded on start.
- **TCP listener:** a `tower` middleware checks `Authorization: Bearer <token>` in constant time; missing/mismatch ⇒ `401`. `/v1/health` is exempt.
- **UDS listener:** no token (file-perm + peer-cred auth); requests are injected `ConnectorCaps::UDS_LOCAL`.
- **Pairing the remote client:** the local desktop app reads the token via the control-tier `GET /v1/auth/token` over the UDS and displays it (settings); the Android client is configured with it. (QR/PassKey pairing is deferred — PassKeys blocked on the `.well-known` public-fetch requirement on a tailnet-only host.)

---

## Config + default (`[http]` in agentd config)

```toml
[http]
enabled = false          # opt-in; nothing consumes HTTP yet (mirrors the use_agentd precedent)
port = 8765              # TCP port for the tailnet listener
bind_override = ""       # optional explicit IP; "" = auto (tailnet IP). 0.0.0.0/unspecified rejected.
event_buffer = 256      # per-conversation SSE replay ring size
```
- When `enabled = false` (default): no listeners, emitter is the plain `BusEmitter`, **zero** change from today.
- When `enabled = true`: wrap emitter in `BroadcastEmitter`, start the UDS listener always, start the TCP listener iff a bind address resolves.
- The settings app gets an "HTTP transport" toggle (mirrors the existing `use_agentd` toggle wiring). Settings UI work beyond the toggle is sub-project 2.

---

## Module layout

```
agentd/src/connector/
├── mod.rs            # re-exports; `pub mod`s
├── caps.rs           # ScopeTier, ConnectorCaps
├── event_hub.rs      # EventHub (broadcast + ring buffer), BroadcastEmitter
├── tailnet.rs        # TailnetSource trait, CliTailnetSource
├── auth.rs           # token gen/load, bearer middleware
└── http/
    ├── mod.rs        # AppState, build_router (messaging|control), serve(listeners)
    ├── server.rs     # UDS + TCP listener setup, scope-by-listener, shutdown
    ├── routes_messaging.rs   # health, projects, conversations, messages, events(SSE), approvals
    └── routes_control.rs     # auth/token (+ seam for sub-project 2 control endpoints)
```
`main.rs` gains: read `[http]` config; if enabled, build `EventHub`, wrap emitter, and spawn `connector::http::serve(...)` with the `Arc<AgentService>` it already constructs. The D-Bus setup is unchanged.

---

## Migration / back-compat

- **Additive only.** No on-disk format changes, no D-Bus contract changes, no migration. With `[http].enabled=false` (default) the daemon behaves exactly as `bc575fd`.
- The overlay + GNOME extension continue on D-Bus, untouched. HTTP and D-Bus coexist over the same `Arc<AgentService>` and the same event stream.

---

## Out of scope (other specs / later slices)

- The `Connector` trait, `ConnectorRegistry`, `InboundEvent`/`OutboundMessage` normalization, and refactoring D-Bus into `OverlayConnector` — the **SP-Connectors** slice.
- Control-plane endpoints beyond `/v1/auth/token` (settings/skills/flows/conductor over HTTP) — **sub-project 2**, as the Freya control surface needs them.
- The Freya client itself (desktop + Android) — **sub-project 2**.
- PassKeys; QR pairing; multi-user/multi-tenant tokens; token rotation UI.
- Live tailnet reachability verification — deferred manual step (tailscaled not yet up).

---

## Testing

- **Pure/mock (unit):**
  - `ConnectorCaps::allows` ordering (Messaging denied Control; Control allows both).
  - `TailnetSource`-mocked bind resolution: tailnet present → that IP; absent → None (no TCP listener); `bind_override` set → override; `bind_override = 0.0.0.0` → rejected, TCP skipped, UDS still up.
  - Token: generate-when-missing, persist 0600, load-existing round-trip; bearer middleware accept/reject (constant-time compare).
  - `EventHub`: publish → subscriber receives filtered-by-conversation; ring-buffer `Last-Event-ID` replay returns events with seq > id in order; buffer eviction past capacity.
  - `BroadcastEmitter` tees to both the inner emitter (RecordingEmitter) and the hub.
- **Live-wire (loopback/UDS — no tailscale needed):** start the server with a `tempfile` UDS path → `GET /v1/health` 200 → `POST /v1/conversations` → `POST .../messages` returns `{message_id}` → SSE `GET .../events` receives the assistant turn + a `delivery`/run event → `POST .../approvals/{id}` resolves a gated tool call. Assert a Messaging-scope (simulated TCP) request to `/v1/auth/token` is `403`, and a UDS request is `200`.
- **No-regression:** with `[http].enabled=false`, the existing agentd test suite stays green and the emitter is the plain `BusEmitter` (assert the wrap is bypassed).
- **Deferred manual:** bring up `tailscaled`, enable `[http]`, hit `https`/`http` over the tailnet IP from a second device, confirm Messaging works and control routes are absent.
