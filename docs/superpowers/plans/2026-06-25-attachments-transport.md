# Attachments Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use `- [ ]`.

**Goal:** Composer attachments travel with a sent message → agentd → the LLM as multimodal image input (Gemini/Anthropic/OpenAI), degrading to a text note for image-incapable providers.

**Architecture:** Client (A) base64-encodes attachments into the JSON send body; agentd (B) decodes, persists to a blob store, records refs on the turn, and passes images to the already-multimodal `route_turn`. Wire contract: `attachments: [{name, mime, kind, data_b64?}]` (serde-default `[]`, back-compat). Each layer is headless-testable; the live LLM response is the user's on-PC check.

**Tech Stack:** Rust, Freya app (`oxide-client`/`oxide-ui`/`oxide-freya`), `agentd` + `oxidemx-agent-core` + vendored AutoAgents (`MessageType::Image` already wired through Gemini/Anthropic/OpenAI), `base64`.

## Global Constraints

- **Build environments (Rule 3 — never mix toolchains over one target):**
  - **A-side** (`oxide-client`/`oxide-ui`/`oxide-freya`, in `oxide-app/`): `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo …`.
  - **B-side** (`agentd`, `oxidemx-agent-core`, `oxidemx-agent`): host-side `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo …` (rustup toolchain; no distrobox).
- **Wire contract (must match byte-for-byte across A and B, by JSON field name):**
  `AttachmentPayload { name: String, mime: String, kind: String, data_b64: Option<String> }` where `kind ∈ {"image","text","file"}`. `attachments` is serde-`#[serde(default)]` on both `SendBody`s.
- Headless only this round: `MockTransport` (A) + `MockTurnRunner`/`TestEnv` (B). Do NOT add live-daemon tests (the `live_agentd.rs` `#[ignore]` path is the user's on-PC check). No real API keys.
- `cargo clippy` clean; hand-formatted (no `cargo fmt`); `?`/`.ok()` over unwrap in non-test; never panic on decode/IO errors (skip + log). Field-standard naming (Rule 0).
- Confirm exact signatures against source before editing (cited file:line in the spec). Use real type names; note any correction in the task report.

---

### Task 1 (A): `oxide-client` DTO + transport signature + mock capture

**Build env:** `LIBRARY_PATH=/tmp/oxidemx-lib-links` in `oxide-app/`.
**Files:** `crates/oxide-client/src/dto.rs`, `transport.rs`, `uds.rs`, `mock.rs`, `Cargo.toml` (add `base64` if not present — check workspace deps first).

**Interfaces:**
- Produces: `pub struct AttachmentPayload { name: String, mime: String, kind: String, data_b64: Option<String> }` (Serialize, Deserialize, Clone, Debug, PartialEq) in `dto.rs`.
- `transport.rs`: `async fn send_message(&self, conversation_id: &str, text: &str, attachments: &[AttachmentPayload]) -> Result<MessageId, TransportError>` (add the param to the trait).
- `uds.rs`: serialize `attachments` into the POST JSON body (`{ text, model?, attachments }`).
- `mock.rs`: `MockTransport` records the attachments of the last/most-recent `send_message` (e.g. `recorded: Arc<Mutex<Vec<AttachmentPayload>>>` + a getter), still returns `Ok(MessageId)`.

- [ ] **Step 1: failing test** — in `mock.rs`/a test mod: call `MockTransport::send_message("c1","hi", &[AttachmentPayload{name:"a.png".into(),mime:"image/png".into(),kind:"image".into(),data_b64:Some("AAAA".into())}])`, assert the mock recorded 1 attachment with those fields. Also a `dto` serde round-trip test for `AttachmentPayload`.
- [ ] **Step 2: run → FAIL** (`LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-client`).
- [ ] **Step 3: implement** the DTO + trait param + uds body + mock recording. Update ALL `send_message` impls/callers in the crate to the new signature (the test mock + uds). (The oxide-freya caller is updated in Task 2.)
- [ ] **Step 4: run → PASS**; clippy clean.
- [ ] **Step 5: commit** `feat(oxide-client): AttachmentPayload + attachments in send_message transport`

---

### Task 2 (A): `oxide-freya` AppState wiring + attachment→payload mapping

**Build env:** `LIBRARY_PATH=/tmp/oxidemx-lib-links` in `oxide-app/`.
**Files:** `crates/oxide-freya/src/state.rs` (+ wherever `Attachment` is convertible — add a `to_payload` helper, e.g. a new `crates/oxide-freya/src/attachment_payload.rs` or inline in state.rs).

**Interfaces:**
- Consumes: `AttachmentPayload` (Task 1), `oxide_ui::components::composer::{Attachment, AttachKind, AttachData}`.
- Produces: `pub fn to_payload(att: &Attachment) -> AttachmentPayload` — maps `AttachData::Image(bytes)`→`{mime: "image/png" (or att-derived), kind:"image", data_b64: Some(base64(bytes))}`; `AttachData::Text(s)`→`{mime:"text/plain", kind:"text", data_b64: Some(base64(s.as_bytes()))}`; `AttachData::None`→`{kind: by AttachKind, data_b64: None}`. `name` from `att.name`.
- `AppState::send(&self, text)` → `AppState::send(&self, text, attachments: Vec<AttachmentPayload>)`; pass `&attachments` to `transport.send_message(id, &text, &attachments)`.

- [ ] **Step 1: failing test** — unit test: `to_payload(&image_attachment_with_bytes)` yields `kind=="image"`, `data_b64` decodes back to the original bytes; `to_payload(&none_data_attachment)` yields `data_b64==None`. (Construct `Attachment` via `oxide_ui` `sample_attachment` / a literal.)
- [ ] **Step 2: run → FAIL** (`cargo test -p oxide-freya`).
- [ ] **Step 3: implement** `to_payload` + the `AppState::send` signature change (update its internal `transport.send_message` call). Leave the composer→AppState wiring change to Task 3.
- [ ] **Step 4: run → PASS**; clippy clean.
- [ ] **Step 5: commit** `feat(oxide-freya): AppState::send carries attachments + Attachment→AttachmentPayload mapping`

---

### Task 3 (A): composer forwards attachments on submit (currently dropped)

**Build env:** `LIBRARY_PATH=/tmp/oxidemx-lib-links` in `oxide-app/`.
**Files:** `crates/oxide-ui/src/components/composer/mod.rs` (submit payload + `on_submit` type), `crates/oxide-freya/src/regions/main_region.rs` (the `.on_submit` wiring at ~line 134).

**Interfaces:**
- `Composer`'s `on_submit` changes from `EventHandler<String>` to `EventHandler<SubmitPayload>` where `pub struct SubmitPayload { pub text: String, pub attachments: Vec<Attachment> }` (new, in composer `mod.rs` or `config.rs`; Clone, PartialEq). The `submit` closure (`mod.rs:123`) builds `SubmitPayload { text, attachments: attachments.read().clone() }` and fires it BEFORE clearing `value`/`attachments`.
- `main_region.rs`: `.on_submit(move |p: SubmitPayload| send_state.send(p.text, p.attachments.iter().map(to_payload).collect()))`.

- [ ] **Step 1: failing test** — freya test: mount `Composer` with `on_submit` capturing into a signal; seed an attachment (via the paste handler or a test seam) + text, trigger send (drive the send button press or call the submit path), assert the captured `SubmitPayload` has the text AND the attachment (i.e. attachments are NOT dropped). If seeding internal `attachments` state isn't reachable from a test, assert at minimum that `on_submit` now carries a `SubmitPayload` with the typed text and an (empty) attachments vec, and cover the non-drop via the existing composer tests + the Task-2 mapping.
- [ ] **Step 2: run → FAIL** (`cargo test -p oxide-ui composer`).
- [ ] **Step 3: implement** `SubmitPayload` + the on_submit type change + main_region wiring. Keep all existing composer tests green (update any that construct `on_submit` with a `String` handler).
- [ ] **Step 4: run → PASS** (`cargo test -p oxide-ui -p oxide-freya`); clippy clean.
- [ ] **Step 5: snapshot/build** — `cargo build -p oxide-freya --bin oxide-freya` (whole A-side compiles). No new snapshot needed (no visual change).
- [ ] **Step 6: commit** `feat(oxide-ui): composer forwards attachments on submit (SubmitPayload)`

---

### Task 4 (B): `AttachmentStore` blob store + `AttachmentRef`

**Build env:** host-side `CARGO_TARGET_DIR=/tmp/oxidemx-host-target` (build `agentd`).
**Files:** new `agentd/src/attachments.rs` (parallel to `sessions.rs`'s `TranscriptStore`); `pub mod attachments;` in `agentd/src/lib.rs` (or wherever modules are declared).

**Interfaces:**
- Produces: `pub struct AttachmentRef { pub id: String, pub mime: String, pub name: String }` (Serialize, Deserialize, Clone, Debug, PartialEq, serde for the transcript). `pub struct AttachmentStore { root: PathBuf }` with `pub fn new(root) -> Self`, `pub fn write(&self, thread: &str, name: &str, mime: &str, bytes: &[u8]) -> Result<AttachmentRef, AttachmentError>` (id = a content/counter id; file at `<root>/<thread>/<id>`; reject bytes over a `MAX_ATTACHMENT_BYTES` const e.g. 20 MB), `pub fn read(&self, thread: &str, id: &str) -> Result<Vec<u8>, AttachmentError>`, `pub fn remove_thread(&self, thread: &str) -> Result<(), AttachmentError>` (cleanup). `thiserror` error enum.

- [ ] **Step 1: failing tests** — `write` then `read` round-trips bytes + returns a ref with the mime/name; `write` of oversize (`MAX+1`) returns `Err`; `remove_thread` deletes the dir; `read` of a missing id returns `Err` (no panic). Use a `tempfile::tempdir()` root (check if `tempfile` is a dev-dep in agentd; the existing `TestEnv` likely uses one — mirror it).
- [ ] **Step 2: run → FAIL** (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd attachments`).
- [ ] **Step 3: implement** the store.
- [ ] **Step 4: run → PASS**; `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd` clean.
- [ ] **Step 5: commit** `feat(agentd): AttachmentStore blob store + AttachmentRef`

---

### Task 5 (B): `route_turn` multi-image + provider image-capability

**Build env:** host-side `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`.
**Files:** `oxidemx-agent-core/src/runtime.rs` (`route_turn` ~line 672 + the image-injection ~line 304); `oxidemx-agent/src/factory.rs` (or wherever `AiProvider` is defined) for the capability fn; ALL `route_turn` callers.

**Interfaces:**
- Change `route_turn(..., image: Option<(String, Vec<u8>)>, ...)` → `route_turn(..., images: Vec<(String /*mime*/, Vec<u8>)>, ...)`. At the injection site, push ONE `ChatMessage { message_type: MessageType::Image((mime,bytes)) }` per image (preserve order, before the text message — match how the single-image path ordered it).
- Produces: `pub fn supports_images(provider: &AiProvider) -> bool` — `Gemini | OpenAi | Anthropic => true`; `Ollama | MistralRs | ClaudeCode => false`. (Locate the `AiProvider` enum; co-locate the fn.)
- **Confirm every `route_turn` caller** (grep `route_turn(` across the repo — agentd `interface.rs:175` + possibly the overlay app). Update each: agentd passes the images (Task 6); any other caller passes `vec![]` to compile unchanged.

- [ ] **Step 1: failing tests** — `supports_images` truth table (all 6 `AiProvider` variants). If `route_turn` has a unit test with a stub provider, add one asserting N images → N `MessageType::Image` messages injected (else cover via Task 6's `MockTurnRunner` test and keep this task to the signature + capability fn + a `supports_images` unit test).
- [ ] **Step 2: run → FAIL** (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p oxidemx-agent -p oxidemx-agent-core`).
- [ ] **Step 3: implement** the signature change + multi-image injection + `supports_images` + update all callers (vec![] for non-agentd).
- [ ] **Step 4: run → PASS**; clippy clean on the touched crates.
- [ ] **Step 5: commit** `feat(agent-core): route_turn multi-image + provider image-capability`

---

### Task 6 (B): agentd `send_message` integration (decode + persist + route + degrade + emit)

**Build env:** host-side `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`.
**Files:** `agentd/src/connector/http/routes_messaging.rs` (`SendBody`), `agentd/src/interface.rs` (`send_message` ~374, the `route_turn` call ~175), `agentd/src/sessions.rs` (`TranscriptTurn`).

**Interfaces:** Consumes Task 4 (`AttachmentStore`/`AttachmentRef`), Task 5 (`route_turn(images)`, `supports_images`).

**Changes:**
- `routes_messaging.rs`: `SendBody` gains `#[serde(default)] attachments: Vec<AttachmentPayload>` (define a matching `AttachmentPayload{name,mime,kind,data_b64:Option<String>}` in agentd — same JSON shape as oxide-client's). Decode each `data_b64` (base64) → bytes; skip (log) on decode error or oversize.
- `sessions.rs`: `TranscriptTurn` gains `#[serde(default)] attachments: Vec<AttachmentRef>` (back-compat: old JSONL lines without the field still parse).
- `interface.rs send_message(project, thread, text, model, attachments: Vec<(name,mime,bytes)>)`: for each image attachment → `AttachmentStore.write()` → collect `AttachmentRef`; store refs on the user `TranscriptTurn`. Build the `images: Vec<(mime,bytes)>` for image-kind attachments. If `supports_images(provider)` → pass `images` to `route_turn`; else pass `vec![]` AND append a text note to the user text (`"\n\n[{n} image attachment(s) omitted — current model can't read images]"`). Emit the user `Turn` event including its `attachments` refs.
- Thread the decoded attachments from the route handler into `send_message`.

- [ ] **Step 1: failing tests** (headless, via `TestEnv` + a `MockTurnRunner` recording the images it received — extend the mock to capture the `images` arg):
  - back-compat: a `TranscriptTurn` JSON line WITHOUT `attachments` deserializes (empty vec).
  - `send_message` with one image attachment + an image-capable stub provider → attachment persisted in `AttachmentStore`, a ref on the user transcript turn, and the `MockTurnRunner` received 1 image.
  - degrade: with an image-incapable provider → `MockTurnRunner` received 0 images AND the user text got the omitted-note.
- [ ] **Step 2: run → FAIL** (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd`).
- [ ] **Step 3: implement** the SendBody decode + TranscriptTurn field + send_message wiring + degrade + emit. Wire the decoded attachments from the HTTP handler through.
- [ ] **Step 4: run → PASS** (`CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd`); clippy clean. Build agentd: `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build -p agentd`.
- [ ] **Step 5: commit** `feat(agentd): receive + persist attachments, route images to multimodal LLM (degrade for image-incapable providers)`

---

## Self-Review

- **Spec coverage:** wire contract (T1+T6 matching structs); A1 client DTO/transport (T1); A2 AppState+mapping (T2); A3 composer forward (T3); B1 SendBody decode (T6); B2 AttachmentStore (T4); B3 TranscriptTurn (T6); B4 route_turn wiring (T5+T6); B5 provider degrade (T5 cap fn + T6 note); B6 emit refs (T6). Headless testing per layer. Build-env split honored per task.
- **Placeholder scan:** "confirm route_turn callers / AiProvider location / tempfile dev-dep" are concrete verify-against-source steps. Error handling specified (skip+log on decode/oversize, no panic). No vague TODOs.
- **Type consistency:** `AttachmentPayload{name,mime,kind,data_b64}` identical JSON in T1 (oxide-client) + T6 (agentd); `AttachmentRef{id,mime,name}` T4→T6; `to_payload` T2→T3; `SubmitPayload{text,attachments}` T3; `route_turn(images: Vec<(String,Vec<u8>)>)` T5→T6; `supports_images` T5→T6. Consistent.

## Notes for the executor
- A-side (T1–T3) and B-side (T4–T6) are independent until the wire contract — they can't share a Rust dependency, only the JSON field names. Keep the two `AttachmentPayload` structs byte-identical on the wire.
- B-side multimodal infra already exists (vendor AutoAgents `MessageType::Image` + Gemini/Anthropic/OpenAI encoders) — do NOT touch vendor code; only wire agentd→`route_turn`.
- The live LLM multimodal response + GUI round-trip are the user's on-PC verification — every task here is proven via Mock seams.
