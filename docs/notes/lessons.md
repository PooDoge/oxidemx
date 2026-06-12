# Lessons — radial-ai implementation run (2026-06-11)

One entry per correction; read at session start per the workflow.

- **iced 0.14 `Space::new()` takes no args.** `Space::new(w, h)` is older API; build `Space::new().width(..).height(..)`.
- **`iced::window::Screenshot` field is `rgba`, not `bytes`.** `{ rgba, size, scale_factor }`; size is in *physical* px (× scale factor on fractional-scaling displays).
- **Emoji glyphs (🧠 🕓 📌 🗑) don't render in iced's default font** — they silently produce empty space. Stick to glyphs in DejaVu/Cantarell coverage (✱ ◔ ✦ ✕ ➤ ⧉ ▸).
- **`df` on `/` reports 0 available on atomic/ostree systems** (read-only composefs). Measure `/home` (fall back `/var`, `/`) for a disk-free widget.
- **Don't regex-patch Rust with `[^}]*` across match blocks** — it eats nested braces. Patch with exact unique strings.
- **Clippy 1.96 default lints now include** `doc_lazy_continuation` (a doc line starting with `+` reads as a list item), `doc_overindented_list_items`, and `manual_is_multiple_of` (`x % n == 0` → `x.is_multiple_of(n)`). Pre-existing code needed a sweep before `-D warnings` could gate.
- **The Interactions API can't mix the built-in `google_search` with custom function declarations.** Adding custom tools to GeneralChat forced its search through the nested-custom `grounded_search` pattern SettingsCustomizer already used.
- **Vision shots on a live desktop race the real mouse**: pointer travel over the popped overlay legitimately activates the chat (the deliberate-travel rule working as specified). Capture the armed state with short delays, or read state from logs.
- **GNOME 50 blocks `org.gnome.Shell.Screenshot` for arbitrary callers** and the portal stalls without interactive consent. An env-gated in-process `iced::window::screenshot` hook (`OXIDEMX_VISION_SHOT`/`OXIDEMX_START_PAGE`/`OXIDEMX_VISION_DELAY_MS`) is the reliable capture path — and doubles as a future CI visual-test hook.
- **`resizable: false` window settings + one-shot `iced::window::resize`** on release-commit worked in testing where per-frame resizes historically desynced the wgpu surface; keep resizes rare and single-shot.
