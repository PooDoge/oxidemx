# Attachments Transport (client + agentd multimodal) — design

**Date 2026-06-25.** Make composer attachments actually travel with a sent message, all the way to the
LLM. Completes the attachments feature (UI shipped; transport was deferred). Spans the Freya app
(`oxide-client`/`oxide-ui`/`oxide-freya`) and the backend (`agentd` + `oxidemx-agent-core`).

## Scope (locked, from user: "A+B now")

- **Slice A — client-side transport:** thread attachments from the composer through to the request body.
- **Slice B — agentd receipt + LLM multimodal:** decode, persist, and feed attachments to the LLM.
- **Verification:** every layer is headless-testable (`MockTransport` for A; `MockTurnRunner` for B —
  both in-process, no daemon/API keys). The ONLY un-verifiable-while-away part is a **real LLM
  multimodal response** (needs a live provider key) and the live GUI round-trip — those are the user's
  on-PC check. We verify the WIRING (attachment reaches `route_turn` as `MessageType::Image`), not the
  model's answer.

## Key facts (from scoping, file:line)

- Send path today: composer `on_send`→`submit(text)` (`composer/mod.rs:275`) → `AppState::send(text)`
  (`oxide-freya/src/state.rs:183`) → `Transport::send_message(id, text)` (`oxide-client/src/transport.rs:16`)
  → UDS POST `/v1/conversations/{id}/messages` body `{text}` (`oxide-client/src/uds.rs:98`) → agentd
  `routes_messaging.rs:75` `SendBody{text,model}` → `interface.rs:374 send_message` → `route_turn(...,None,...)`
  (`interface.rs:175`). **Attachments are dropped on send** (`composer/mod.rs:132` clears them, never read).
- The agent core ALREADY supports multimodal: `route_turn(image: Option<(String,Vec<u8>)>)`
  (`oxidemx-agent-core/src/runtime.rs:672`) injects a `ChatMessage{ message_type: MessageType::Image((mime,bytes)) }`
  (`runtime.rs:304`). Providers encode it: Gemini `inlineData` base64 (`vendor/AutoAgents/.../google.rs:985`),
  Anthropic `ImageSource` base64 (`anthropic.rs:350`), OpenAI `data:` URL (`openai_compatible.rs:998`).
  Ollama/MistralRs/ClaudeCode: no image encoding → degrade.
- Persistence: per-thread JSONL `TranscriptTurn{role,text,ts}` (`agentd/src/sessions.rs:28`/`:68`). No blob store.
- Headless test seam: `MockTurnRunner` (`interface.rs:1579`) + `TestEnv` — full in-process send_message tests,
  no daemon/keys. `tests/live_agentd.rs` is `#[ignore]` (needs running daemon) — that's the on-PC path.

## Wire format (the contract)

Extend the existing JSON body (NOT multipart — simpler, fits the current transport; our attachments are
pasted images / small files, base64 is fine):
```
SendBody { text: String, model: Option<String>, attachments: Vec<AttachmentPayload> }   // attachments serde-default []
AttachmentPayload { name: String, mime: String, kind: String /* "image"|"text"|"file" */, data_b64: Option<String> }
```
- `data_b64 = Some(base64(bytes))` for `AttachData::Image(bytes)` (mime e.g. `image/png`) and
  `AttachData::Text(s)` (utf-8 bytes, `text/plain`); `None` for `AttachData::None` (mock `+`-menu sources —
  metadata only, no bytes, so they cannot become LLM image input).
- Back-compat: `attachments` defaults to `[]`, so old clients / the agentd `SendBody` before B still parse.

## Slice A — client-side (build env: `LIBRARY_PATH=/tmp/oxidemx-lib-links`)

### A1. `oxide-client` DTO + transport
- `dto.rs`: add `AttachmentPayload { name, mime, kind, data_b64: Option<String> }` (Serialize/Deserialize, PartialEq).
- `transport.rs`: change `send_message(&self, conversation_id, text)` →
  `send_message(&self, conversation_id, text, attachments: &[AttachmentPayload])`. (One trait, one extra param —
  there's a single production call site.)
- `uds.rs`: serialize `attachments` into the POST body alongside `text`/`model`.
- `mock.rs`: `MockTransport` captures the last `attachments` (e.g. into an `Arc<Mutex<Vec<…>>>` or a recorded-calls
  vec) so tests can assert they arrived. Keep returning `Ok(MessageId)`.

### A2. `oxide-freya` AppState wiring
- `state.rs`: `AppState::send(text)` → `AppState::send(text, attachments: Vec<AttachmentPayload>)`; pass to
  `transport.send_message(id, &text, &attachments)`.

### A3. `oxide-ui` composer → forward attachments (currently dropped)
- The composer's `on_submit` is `EventHandler<String>`. Change the submit payload to carry attachments:
  `on_submit: EventHandler<(String, Vec<Attachment>)>` (or a small `SubmitPayload{text, attachments}` struct —
  decide in plan; a struct reads better). `submit` closure (`mod.rs:123`) passes `(text, attachments.read().clone())`
  BEFORE clearing. The attachment→`AttachmentPayload` conversion (base64-encode `AttachData`) happens at the
  oxide-freya boundary (A2) so `oxide-ui` stays transport-agnostic — `oxide-ui` emits `Vec<Attachment>`, `oxide-freya`
  maps to `Vec<AttachmentPayload>`. Add `fn to_payload(&Attachment) -> AttachmentPayload` in oxide-freya (uses base64).
- Update `main_region.rs:134` `.on_submit(move |(text, atts)| send_state.send(text, map_payloads(atts)))`.

## Slice B — agentd receipt + multimodal (build env: host-side `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`)

### B1. `agentd` SendBody + decode
- `routes_messaging.rs`: extend `SendBody` with `attachments: Vec<AttachmentPayload>` (serde-default). Decode
  `data_b64` → bytes. Pass to the service.

### B2. `AttachmentStore` (new blob store) — `agentd/src/`
- New module parallel to `TranscriptStore`: blobs on disk at `<project_store>/attachments/<thread>/<id>.<ext>`.
  `write(thread, mime, bytes) -> Result<AttachmentRef>` (id = content hash or counter), `read(thread, id) -> Result<Vec<u8>>`.
  Cleanup on session/thread delete (wire into the existing end-session path). Size cap (e.g. 20 MB/attachment) → reject oversize.
- `AttachmentRef { id: String, mime: String, name: String }`.

### B3. `TranscriptTurn` + persistence
- Add `attachments: Vec<AttachmentRef>` to `TranscriptTurn` (`sessions.rs:28`), serde-default `[]` (back-compat with
  existing JSONL). The user turn records its attachment refs.

### B4. `send_message` + `route_turn` wiring
- `interface.rs send_message`: accept decoded attachments; for each image attachment, `attachment_store.write()`,
  collect `AttachmentRef`s into the user `TranscriptTurn`; pass the image bytes to the turn runner.
- `route_turn` (`runtime.rs:672`) currently takes ONE `image: Option<(String,Vec<u8>)>`. Extend to multiple:
  `images: Vec<(String /*mime*/, Vec<u8>)>` (inject one `MessageType::Image` ChatMessage per image at
  `runtime.rs:304`). Update the single agentd caller (`interface.rs:175`, was `None`) + any other `route_turn` caller
  (grep — the overlay app may call it; if so, pass `vec![]` to keep it compiling). **Confirm all callers in the plan.**

### B5. Provider capability (graceful degrade)
- Gemini/Anthropic/OpenAI encode images (vendor) → pass images through. Ollama/MistralRs/ClaudeCode don't → do NOT
  send `MessageType::Image`; instead append a short text note to the user message
  (`"[N image attachment(s) omitted — current model can't read images]"`) so the turn is coherent. Determine the
  provider's image capability from the resolved `AiProvider` (factory `oxidemx-agent/src/factory.rs`). Add an
  `fn supports_images(&AiProvider) -> bool` (Gemini/Anthropic/OpenAi = true; others = false).

### B6. Emit attachment metadata
- The `Turn` SSE event for the user turn includes its `AttachmentRef`s (so a future UI can render them). Minimal:
  include refs in the event payload that `interface.rs` already emits.

## Data flow
compose + attach → send → `(text, Vec<Attachment>)` → oxide-freya maps to `Vec<AttachmentPayload>` (base64) →
`Transport::send_message(id, text, payloads)` → UDS JSON body → agentd `SendBody` decode → `AttachmentStore.write` +
`TranscriptTurn.attachments` → `route_turn(text, images)` → provider (image blocks if supported, else text note) → LLM.

## Error handling
- Base64 decode failure (agentd) / oversize → skip that attachment + log; never panic, never fail the whole turn.
- `Clipboard`/encode errors stay client-side (already handled). `AttachmentStore` IO errors → propagate as a
  turn-level error event, don't crash the daemon.
- Empty `attachments` → behaves exactly as today (text-only turn).

## Testing (headless)
- **A:** `MockTransport` records `attachments` → unit test asserts they arrive with correct name/mime/kind/b64.
  Composer freya test: submitting with seeded attachments fires `on_submit` with the attachments (not dropped).
  oxide-freya: `to_payload` base64 round-trips an image Attachment.
- **B:** `AttachmentStore` write/read round-trip + oversize-reject unit tests. `TranscriptTurn` serde back-compat
  (old line without `attachments` parses). `send_message` (via `TestEnv` + a `MockTurnRunner` that records the
  images it received) asserts: attachment persisted + ref in transcript + image passed to the runner. `supports_images`
  truth table. Provider-degrade: a non-image provider gets the text note + no image.
- **Deferred to on-PC (live):** a real Gemini/Anthropic multimodal response; the GUI send→render round-trip.

## Non-goals (this round)
- Multipart/streaming upload (JSON+base64 is the contract this round).
- Rendering received attachments in the chat transcript UI (emit metadata only; UI render is a later slice).
- Image input for Ollama/MistralRs/ClaudeCode (degrade with a note).
- Audio/PDF parts (the model type supports Pdf, but scope is images + the existing text/file metadata).
