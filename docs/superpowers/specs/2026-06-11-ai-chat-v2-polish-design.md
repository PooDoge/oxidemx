# AI Chat v2 Polish — Design

**Date:** 2026-06-11 · **Status:** Approved (brainstormed; user selected full A–D package + SSE streaming + per-thread model picker)

## Scope

Improvements to the radial menu's AI chat shell (overlay-rs), building on the Interactions-API transport:

1. **Rich responses** — AI bubbles render markdown via `iced::widget::markdown` (+ `highlighter` feature for code blocks), themed to the active palette. User bubbles stay plain. Parsed markdown cached per message in a `RefCell` side-cache keyed by message content hash (icon-cache pattern); streaming partials render plain and switch to markdown on completion. Link clicks → `AiLinkClicked(url)` → `xdg-open`.
2. **Copy** — `mouse_area` hover per bubble reveals a ⧉ copy button (`iced::clipboard::write`); toolbar "Copy chat" exports the conversation as markdown. Per-code-block copy only if the markdown `Viewer` hook is cheap (bubble-level copy is the contract).
3. **Streaming** — `"stream": true` Interactions request; SSE parsed from `reqwest::bytes_stream`; text deltas forwarded over an mpsc channel into the subscription mesh (`AiStreamDelta(thread_idx, delta)` / `AiStreamDone(thread_idx, result)`). **Any stream-parse failure falls back transparently to the existing blocking call.** SSE event schema verified by live probe before implementation.
4. **Stop / in-flight control** — Send becomes Stop while loading; `Task::abortable` handle stored in state; abort labels the partial "(stopped)" and keeps it.
5. **Tool activity** — `STATUS_TX` channel (QUESTION_TX pattern): agent loop reports Thinking / Searching the web / Reading menu config / Writing config / Listing apps; UI shows the specific label while loading.
6. **Input** — `text_editor` multi-line input (Enter sends, Shift+Enter newline, grows to ~4 lines).
7. **Threads** — rename (inline edit) + delete (active-thread delete falls back to a fresh thread) + `updated_at` ("2h ago") in the list. New fields serde-defaulted; existing `ai-chats.json` loads unchanged.
8. **Model picker** — toolbar pill cycling Flash (`gemini-2.5-flash`, default) ↔ Pro (`gemini-2.5-pro`), stored per thread; session id kept on switch, reset + retried once if the API rejects the continuation.
9. **Auto-scroll** — history scrollable gets an `Id`; new messages/deltas snap to END.

## Error handling

Stream failure → blocking fallback; abort → partial kept + labeled; model-switch session error → one silent session reset/retry; markdown parse never fails (graceful plaintext).

## Testing

Unit: SSE line-parser (event framing, partial chunks, [DONE]); conversation→markdown export. Manual checklist: stream/stop/copy/links/rename/delete/model-switch/legacy chats file/auto-scroll.

## Out of scope (recorded for later)

Edit-and-resend; per-code-block copy if Viewer hook proves deep; token-usage display; screenshot input (needs portal); AI disc-page slice refresh.
