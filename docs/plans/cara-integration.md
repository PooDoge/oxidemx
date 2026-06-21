# Cara (carapace) integration — Design & Implementation Plan

> Status: **draft for review** — no code written yet.
> Design truth: `docs/design-system/cara-page.jsx` rendered by
> `docs/design-system/cara-preview.html` (standalone 1:1) and the
> "Cara integration" section of `docs/design-system/index.html`
> (serve the dir: `python3 -m http.server 8741`).
> Upstream source audited: `/run/media/system/fastdrive/Games/carapace` (v0.8.0, Apache-2.0).
> Parent plan: [`radial-ai-implementation.md`](radial-ai-implementation.md) — this plan
> **composes with it**; shared contracts are called out explicitly below.

**Goal:** the radial menu's AI shell gains a third backend — **Cara**, the local
carapace daemon — so one assistant with one memory answers on Signal/Telegram/Matrix
*and* in the overlay chat; Cara's tool runs are approved from the desktop
(chat card or popup-rs toast); a new radial page drives Cara without opening chat.

**What carapace verifiably ships for us** (audited in source, not docs):

| Surface | Where | Used for |
|---|---|---|
| `POST /v1/chat/completions` (+ SSE streaming) | `src/server/openai.rs` | Phase 1 chat transport. **No `tools` field** — request is `{model, messages, stream, user}` only; tools run server-side inside Cara |
| WS JSON-RPC | `src/server/ws/handlers/{sessions,exec,cron,channels,usage,system}.rs` | Phase 2 control plane: session list/attach, exec-approval events, cron, usage |
| Auth | deny-by-default, loopback binding, CSRF on control routes; OS keyring (keyutils on Linux) | token onboarding + storage story |
| Sandboxing | Landlock subprocess sandboxing on Linux | what the approval card's "sandboxed" badge means |
| Plugins | wasmtime component-model, Ed25519-signed (`wit/`) | Phase 4: OxideMX plugin inside Cara |

---

## 1 · UX design (what the preview shows)

1. **Backend pill** — the chat thread strip (parent plan T3 `chat_ui/threads.rs`) gains a
   third chip: `General · Menu setup · Cara`. Cara chip carries a shell glyph; header
   status line becomes `cara 0.8 · 127.0.0.1:18789 · <provider:model>` with green/red dot.
   Offline ⇒ red dot + automatic fallback to the direct Gemini path (status line says so).
2. **Cross-channel session rule** — a horizontal rule "session continued from Signal · 14:32"
   when the attached Cara session has turns from other channels; inbound bubbles from
   other channels get a small channel tag above the bubble (Phase 2 — needs WS sessions).
3. **Exec-approval card** — new agent card (parent plan §4 `AgentCardData`): shield icon,
   yellow tone while pending, mono command line, `Approve once / Always allow / Deny`,
   origin-channel line ("asked from the Signal session — approving here answers everywhere"),
   TTL progress bar. Resolves in place to green "Tool run approved".
4. **Approval toast** — when the chat shell is closed, the same approval renders as a
   popup-rs layer-shell toast (384 px wide, yellow top border, `Super+A` accelerator).
5. **Radial Cara page** — built-in page: New session · Approvals (badge "1 pending") ·
   Talk mode · Sessions (submenu of recent sessions w/ origin + age) · Channels ·
   Usage ($ today) · Memory · Stop run.
6. **Mode split preserved** — `SettingsCustomizer` stays on the direct Gemini path:
   its tools (`set_menu_config`, `list_system_apps`, MCQ) execute *inside the overlay
   process* and cannot ride Cara's OpenAI endpoint (no client-tool passthrough, see table).
   GeneralChat is what routes to Cara.

---

## 2 · Architecture

```
oxidemx-overlay                         cara (carapace, systemd --user)
┌──────────────────────────┐            ┌─────────────────────────────┐
│ chat shell (chat_ui)     │  SSE       │ /v1/chat/completions        │
│  ai_client.rs            │───────────►│  agent loop · server tools  │
│   Backend::Gemini        │            │  sessions · memory · cron   │
│   Backend::Cara ────────┐│  WS JSONRPC│                             │
│ cara_control.rs (new) ──┼┼───────────►│ sessions.* exec.* usage.*   │
│   approval events ◄─────┘│            │ channels: Signal/TG/Matrix  │
│   → Card / popup toast   │            │ providers: anthropic/gemini │
└──────────────────────────┘            └─────────────────────────────┘
        127.0.0.1:18789 only · bearer token from keyring · no new open ports
```

- **`Backend` seam** in `ai_client.rs`: `ask_ai()` grows a match on the active backend.
  Cara arm maps OpenAI SSE chunks → existing `StreamEvent::{Delta,Activity,Card}` —
  the chat UI is backend-agnostic and unchanged.
- **`cara_control.rs`** (new): one persistent WS connection (tokio-tungstenite is already
  in carapace's stack; overlay side uses `async-tungstenite` on the iced executor).
  Subscribes to exec-approval + session events; exposes `approve(id, scope)`, `deny(id)`,
  `sessions()`, `attach(session_id)`, `usage()`. Reconnect with backoff; connection state
  feeds the header dot.
- **Approvals fan-out**: WS event → if chat shell open ⇒ `StreamEvent::Card(ExecApproval{..})`
  into the active thread; else ⇒ D-Bus to `oxidemx-popup` (toast) + indicator badge count
  via the existing daemon `org.oxidemx.Daemon` property the extension already watches.
- **Daemon untouched** in Phases 1–3 (same posture as the parent plan).

### New `AgentCardData` variants (extends parent plan §4 — requires its T4)

```rust
ExecApproval { id: String, command: String, origin: Option<String>,   // channel name
               expires_at: Option<Instant>, state: ApprovalState },   // Pending/Approved/Denied/Expired
CaraCron     { id: String, label: String, schedule: String, enabled: bool },
```

### Config additions (additive, serde-default, lossless — parent plan §3 idiom)

```rust
pub struct CaraConfig {                      // overlay.ai.cara
    pub enabled: bool,                       // default false
    pub endpoint: String,                    // default "http://127.0.0.1:18789"
    pub model: Option<String>,               // None → "carapace" (Cara routes per-session)
    pub fallback_to_gemini: bool,            // default true (offline ⇒ direct path)
}
```

Token is **never** in config.json: read from the Secret Service / keyutils entry
`oxidemx/cara-token` (same store family carapace itself uses); settings-rs gets a
"paste token" field that writes the keyring entry. Never logged, zeroized after use.

---

## 3 · File-level diff map

| File | Change | Phase |
|---|---|---|
| `oxidemx-shared/src/config.rs` | `CaraConfig` under `AiConfig` (parent plan adds `AiConfig` in T1) | 1 |
| `overlay-rs/src/ai_client.rs` | `Backend` enum + OpenAI-SSE adapter (~250 loc); health probe; fallback | 1 |
| `overlay-rs/src/chat_ui/threads.rs` | Cara chip + linked-channels caption (file created by parent T3) | 1 |
| `overlay-rs/src/chat_ui/header.rs` | status line variants (online/offline/fallback) | 1 |
| `settings-rs` (AI page) | enable toggle, endpoint, token→keyring, connection test button | 2 |
| `overlay-rs/src/cara_control.rs` | **new** — WS JSON-RPC client, event stream, approve/deny API | 2 |
| `overlay-rs/src/chat_ui/cards.rs` | `ExecApproval` + `CaraCron` card renderers (per `cara-page.jsx`) | 2 |
| `popup-rs` | approval toast surface (per `CaraApprovalPopup` mock) + `Super+A` accept | 2 |
| `oxidemx-shared/default-config.json` | built-in `cara` radial page (fresh seeds only, parent Q8 applies) | 3 |
| `overlay-rs/src/actions.rs` | `ActionKind::Cara` (payload in `command`: `new-session`, `talk`, `stop`, `approvals`, …) → calls `cara_control` | 3 |
| `packaging/systemd/cara.service` *(or doc-only)* | run/verify recipe for the cara user service; install.sh opt-in prompt | 2 |
| carapace plugin (`wit` component, separate crate) | `set_menu_config`/`get_menu_config`/`list_system_apps` exposed *inside Cara* | 4 |

---

## 4 · Phases & task graph

```
P1 chat MVP (no new protocol): CaraConfig → Backend seam + SSE adapter →
   chips/status UI → offline fallback → manual smoke vs running cara
   depends on: parent T1 (config section). Parent T3/T4 NOT required if cards deferred.
 ├─► P2 control plane: keyring onboarding (settings-rs) → cara_control.rs WS client →
 │      exec-approval events → ExecApproval card (needs parent T4 Card plumbing) →
 │      popup toast + indicator badge → session attach/list (cross-channel rule UI)
 ├─► P3 radial page: ActionKind::Cara + executors → built-in page seed → badge wedge
 └─► P4 (design-spike first): signed WASM plugin in Cara exposing menu-config tools;
        then "Cara configures the menu from any channel". Separate plan when reached.
P5 verification: vision loop vs cara-preview.html boards 1–5 (OxideMX + Dracula themes);
   clippy -D warnings; approval-flow integration test against a local cara with a
   scripted exec request; offline/fallback test (stop the service mid-stream).
```

Estimates: P1 ≈ 1–2 days · P2 ≈ 3–4 days · P3 ≈ 1 day · P4 spike before sizing.

---

## 5 · Security posture

- Loopback-only endpoint; we never expose or proxy it. Token via OS keyring, 0-length
  logs, zeroize. Header dot ≠ token validity (separate 401 surface in status line).
- Approvals are **explicit-deny-default** in UI: Esc/timeout = deny; "Always allow"
  writes a carapace-side policy, not an overlay-side bypass.
- `SettingsCustomizer` keeps local tools out of Cara's reach until P4's *signed* plugin —
  no generic "run anything in the overlay" bridge.
- carapace itself: pinned-rev build from the local checkout or release binary from
  getcara.io with signature check (its install doc covers `cara verify`).

## 6 · Risks

- **Session semantics mismatch** — the OpenAI endpoint is stateless (full `messages`
  resend, like our Gemini path); true cross-channel continuity only arrives with P2
  WS `sessions.attach`. The P1 UI must not *claim* continuity (no channel rule until P2).
- **WS protocol drift** — carapace is pre-1.0; JSON-RPC method shapes may move. Pin a
  tested cara version in docs; `cara_control` degrades to "chat-only" on method errors.
- **Approval latency path** — toast must work when the overlay process is closed entirely;
  if so the watcher needs to live in the daemon or a tiny `oxidemx-cara-bridge` —
  decide at P2 start (current lean: daemon owns the WS client, overlay/popup subscribe
  over D-Bus; contradicts "daemon untouched" — flagged as the one daemon change).
- **Double-assistant confusion** — Gemini fallback answers must be visually distinct
  (status line + chip state) so users know when memory/channels are NOT in play.

## 7 · Open questions (answer before P2)

1. Who owns the WS client when the overlay is closed — daemon, or accept "no approvals
   while overlay closed" for P2 and revisit? (toast value drops a lot without it)
2. Token onboarding: paste-into-settings (proposed) vs `cara pair`-style QR/device flow
   (carapace has a device-pairing surface in `src/devices/`)?
3. Does the Cara chip replace GeneralChat when enabled (proposed: yes, with fallback
   badge) or coexist as a fourth mode?
4. Radial Approvals wedge badge: poll usage/exec count on page-open only (proposed) or
   live subscription while disc visible?
5. P4 plugin signing: self-signed Ed25519 key checked into the repo is useless —
   document a per-user key ceremony, or ship unsigned + local allowlist?
