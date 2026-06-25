# Feasibility: OpenUI generative-UI in a Freya webview for the OxideMX chat-output window

- **Date:** 2026-06-25
- **Target stack:** Rust + Freya `v0.4.0-rc.23`, Bazzite Linux, GNOME on **Wayland**
- **Question:** Can the chat **output** thread be an embedded webview that renders rich/interactive UI via OpenUI (thesysdev)?
- **Verdict:** **NO-GO** for an *embedded* Freya webview on Wayland today (hard blocker). **CONDITIONAL GO** for the *generative-UI* goal via the recommended alternatives.

---

## 1. THE BLOCKER — Freya webview on Wayland (verified against the clone)

**Confirmed: Freya's webview cannot render on Wayland.** This is stated by Freya itself and dictated by how it calls `wry`.

Evidence from the current clone `/run/media/system/fastdrive/repos/freya/crates/freya-webview` (version `0.4.0-rc.23`):

- `Cargo.toml` pins **`wry = "0.54"`** and, on Linux, **`gtk = "0.18"`** (GTK3 → WebKitGTK 4.x via webkit2gtk-4.x). It does **not** depend on GTK4/WebKitGTK 6.0.
- `src/lib.rs` crate docs spell out the platform matrix verbatim:
  > Windows: WebView2. macOS: WKWebView. **Linux (x11): WebKitGTK. Linux (Wayland): Not supported. Android: Not supported.**
- `src/plugin.rs:133` builds the webview with **`builder.build_as_child(window)`** — the single code path. There is **no** `new_gtk` / `gtk::Fixed` usage anywhere in `crates/` (grep returned only the one `build_as_child` hit).

Why `build_as_child` is the blocker — from `wry` docs (`docs.rs/wry/latest/wry/struct.WebViewBuilder.html`, confirmed on 0.55.1):
> "This will create the webview as a child window of the `parent` window. **Only X11 is supported. This method won't work on Wayland.**"
> "If you want to support child webviews on X11 and Wayland at the same time, we recommend using `WebViewBuilderExtUnix::new_gtk` with `gtk::Fixed`."

**State of `wry` + WebKitGTK on Wayland (2025–2026):**
- A *standalone, top-level* webview window (own GTK window) renders fine on Wayland; the limitation is specifically the **embedded child** case that Freya uses.
- `new_gtk` + `gtk::Fixed` is the documented way to get an embedded child webview working on **both** X11 and Wayland — but it requires hosting the webview inside a real GTK widget hierarchy, which Freya (a Skia/winit renderer) does not provide. Freya did not adopt it.
- Known workarounds for the broader WebKitGTK-on-Wayland blank-screen class of bugs: `GDK_BACKEND=x11` (forces XWayland — degrades the experience and re-enters the X11-only `build_as_child` path, so it *can* make Freya's webview appear, but only via XWayland), `WEBKIT_DISABLE_COMPOSITING_MODE=1`, `WEBKIT_DISABLE_DMABUF_RENDERER=1` (the DMA-BUF renderer is flaky on NVIDIA). These address rendering glitches, not the architectural child-window-on-Wayland gap.
- GTK4 + WebKitGTK 6.0 gives native Wayland rendering (e.g. Wails v3 experimental `-tags gtk4`), but Freya's webview is on GTK3/wry-0.54 and would need a non-trivial upstream port.

**Plain answer:** An embedded Freya webview will **not display** on this user's GNOME/Wayland desktop. Forcing `GDK_BACKEND=x11` could make it appear under XWayland, but that defeats the point and is fragile (NVIDIA/compositing caveats). This determines everything downstream.

## 2. What OpenUI actually is (thesysdev/openui)

- **Category:** (a) — an **LLM-returns-UI-spec → client renders interactive components** framework. The model emits **OpenUI Lang** (a compact, streaming-first DSL, claimed ~67% fewer tokens than equivalent JSON); a client runtime parses it progressively and renders to the DOM. Not a hosted server product; not merely a static component renderer.
- **Runtime:** JavaScript/TypeScript + **React** (Vue/Svelte bindings also exist). Packages:
  - `@openuidev/lang-core` — framework-agnostic parser + prompt generation + runtime-evaluation/type layer.
  - `@openuidev/react-lang` — React renderer for streamed OpenUI Lang; component libraries declared with Zod schemas.
  - `@openuidev/react-headless` / `@openuidev/react-ui` — chat state/streaming + prebuilt chat layouts.
  - **`@openuidev/browser-bundle`** — **prebuilt browser bundle shipping the renderer + UI library + React + styles as plain `<script>`/`<link>` assets** for CDN / iframe / **no-build** scenarios. **This is the key package: it needs no Node server at runtime.**
  - `@openuidev/cli`, plus `react-email`, `vue-lang`, `svelte-lang`.
- **Agent skill:** Yes. OpenUI ships an **Agent Skill** for coding assistants (Claude Code/Cursor/Copilot): `npx skills add thesysdev/openui --skill openui`. It is a *developer-assistant* skill (helps you build a generative-UI app); it is **not** a runtime tool the agentd agent would call. The runtime artifact the *LLM* produces is **OpenUI Lang text**, which the renderer turns into components.
- **License/maturity:** **MIT**, actively maintained (~7.4k stars, ~594 commits on main at time of fetch). Reasonably mature but young; pre-1.0 API churn likely. Note: `thesysdev` also sells a commercial hosted "C1/GenUI" product — the open-source OpenUI is the relevant piece here and is self-hostable/embeddable.

## 3. No-server local embed (the right pattern *if* a webview were viable)

You do **not** need an HTTP server. Two viable mechanisms, both reachable because Freya's `WebView::on_created(...)` hands you the raw **`wry::WebViewBuilder`** (`component.rs`), so every `wry` builder method is available:

1. **`wry` custom protocol (recommended).** Build the OpenUI `browser-bundle` app to static `dist/` (HTML/JS/CSS), embed the bytes (`include_dir!`/`rust-embed`) or read from disk, and register `with_custom_protocol("oxide", handler)` (or `with_asynchronous_custom_protocol`). Load `oxide://localhost/index.html`. The handler maps request paths → embedded bytes with correct MIME types. Caveat from wry docs: custom-protocol pages get a platform-specific `Origin`, so set CORS/CSP accordingly.
2. **`with_html(...)` inline / `data:`** — fine for a tiny shell, awkward for a full React bundle (better to use the custom protocol).

**Data plane (no server):**
- Push the agent's OpenUI-Lang spec into the page by evaluating JS (`WebView::evaluate_script`) or via an injected init script that exposes a receiver; the React renderer streams/renders it.
- Send user interactions back out via wry's **IPC handler** (`with_ipc_handler`), wired through Freya's `on_created` hook → forward to agentd.

This part is clean and well-trodden. **It is moot on Wayland** because the embedded webview never paints (Section 1).

## 4. Integration shape for OxideMX (if it worked)

- **Today:** chat output is Freya-native. `oxide-ui/src/components/bubble.rs` renders role-aware `Bubble` components (rect/label/Avatar, per-corner tails, theme tokens); Freya has the **`markdown`** feature (`freya-components` → `pulldown-cmark`) available.
- **Proposed flow:** agentd streams an OpenUI-Lang spec → webview (custom-protocol bundle) renders interactive components → user clicks/inputs post back via wry IPC → Freya `on_created` bridge → agentd. **Composer INPUT stays Freya; only the thread OUTPUT becomes a webview** (as scoped).
- **Wayland multi-window/positioning concerns:** even a *standalone top-level* wry window (the only Wayland-capable webview shape) cannot be positioned/parented programmatically under Wayland (no global coordinates, no client-side window placement) and would be a separate OS window, not an in-thread surface — so it can't visually sit inside the Freya thread. This kills the "in-app rich output region" feel regardless.

## 5. Alternatives (since the webview is blocked on Wayland)

**(b) Richer Freya-native output — RECOMMENDED.** Strongly favored.
- Freya already has the `markdown` feature (code blocks, lists, emphasis) and `Card`/rect/label primitives; the chat already renders structured `Bubble`s.
- OxideMX already owns a **WASM widget-slice system** (`oxidemx-widget-api`/`-host`/`-proto`, `wasmi` runtime, guest PDK emitting a `Scene`/`WedgeGeom` proto). That is *already* an in-app "agent emits a UI spec → host renders it" pipeline — the same conceptual model as OpenUI, but rendered natively (no DOM, no webview, Wayland-clean).
- **Tradeoff:** you build the component vocabulary yourself (cards, forms, charts, buttons) instead of inheriting React ecosystem widgets; more upfront work, but native perf, native theming, no XWayland, no WebKitGTK driver roulette, and it reuses infrastructure you already have. You could even define an OpenUI-Lang-*inspired* spec the agent emits and map it to Freya/WASM widgets.

**(a) External browser window.** Open a generated local page (custom-protocol or a throwaway localhost) in the user's default browser. Works on Wayland (it's the browser's problem), supports full OpenUI/React. **Tradeoff:** loses all in-app feel, context switch, no tight IPC, awkward lifecycle — acceptable only as an "open rich view in browser" escape hatch.

**(c) Wait for Freya Wayland-webview support.** Would require Freya to port its webview to `new_gtk`+`gtk::Fixed` (and likely GTK4/WebKitGTK 6.0) and embed it in a GTK hierarchy it doesn't currently have. **Tradeoff:** zero effort for us but indefinite timeline and not on Freya's roadmap as of this clone; do not block on it.

## 6. Verdict + rough effort

- **Embedded Freya webview on Wayland: NO-GO.** Deciding factor: Freya uses `wry::build_as_child` (X11-only) and its own docs say "Linux (Wayland): Not supported." Confirmed in the clone at `crates/freya-webview/{Cargo.toml,src/lib.rs,src/plugin.rs}`. No flag or env var makes the embedded child webview render natively on Wayland (only `GDK_BACKEND=x11`/XWayland, which is a regression).
- **Generative-UI goal: CONDITIONAL GO** via **Alternative (b) — Freya-native + the existing WASM widget system.** Reuse what you have; optionally borrow OpenUI Lang's *spec idea* (agent emits a typed UI spec) without its React/webview runtime.
- **Rough effort:**
  - Webview-on-Wayland path: not worth spiking for *embedded*; **NO-GO**.
  - If you want to *prove* it for completeness: ~0.5–1 day spike — register a `wry` **custom-protocol static bundle** via Freya's `on_created`, launch on this Wayland box, and confirm whether it paints (expect blank/X11-only). Do this **before** committing anything.
  - Native generative-UI (recommended): medium effort — design a small UI-spec schema, extend the agentd→chat contract, and render via Freya widgets and/or the WASM widget host. Reuses existing crates; weeks not months for a first useful slice.
- **Recommended next step:** Skip the embedded-webview route. Prototype an **agent-emitted UI-spec → Freya-native renderer** (lean on `markdown` + `Card` first, escalate to the WASM widget host for interactive pieces). Keep "open in external browser" as an optional escape hatch for truly web-heavy output.

---

## Sources / things I couldn't fully verify

- Freya clone files (authoritative for the blocker): `crates/freya-webview/Cargo.toml` (`wry = "0.54"`, `gtk = "0.18"`), `src/lib.rs` (platform matrix), `src/plugin.rs:133` (`build_as_child`), `src/component.rs` (`on_created` exposes raw `wry::WebViewBuilder`).
- wry docs: https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html (build_as_child X11-only; new_gtk + gtk::Fixed; custom protocols). Latest wry = 0.55.1; Freya pins 0.54 — behavior identical for this limitation.
- WebKitGTK/Wayland workarounds: tauri issue #12361, Arch forums, yaak.app feedback (GDK_BACKEND=x11, WEBKIT_DISABLE_COMPOSITING_MODE, WEBKIT_DISABLE_DMABUF_RENDERER).
- OpenUI: https://github.com/thesysdev/openui + its README (MIT, browser-bundle no-server package, OpenUI Lang, agent skill `npx skills add thesysdev/openui`).
- **Not verified:** exact OpenUI Lang event-callback wiring (README didn't detail interaction-return semantics); whether `browser-bundle` runs cleanly entirely offline behind a custom protocol with no network calls (needs a hands-on test); precise WebKitGTK package version Bazzite ships. None of these change the verdict, since the embedded-webview path is blocked upstream of them.
