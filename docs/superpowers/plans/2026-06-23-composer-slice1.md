# OxideMX Composer — Slice 1 Implementation Plan (chassis + functional editor)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-line `PromptInput` at the bottom of the chat thread with the full **Composer** chassis from the design study — a multi-line editor, prediction strip, attach menu, grouped provider menu, attachment chips, send/working/disabled states, and an activity line — matching `design/composer/OxideMX - Composer.freya.json`.

**Architecture:** A new `composer/` module in the `oxide-ui` crate (one file per sub-component) plus a top-level `Composer` orchestrator that owns local UI state and is mounted in `oxide-freya`'s `main_region.rs` footer. Built entirely on Freya's builder API (`rect().child()`) and built-in components (`Card`/`Button`/`Chip`/`Menu`/`Select`/`SegmentedButton`/`Switch`/`RadioItem`/`Attached`/`ScrollView`). The editor is a custom widget on the `freya-edit` `use_editable` engine (the `Input` component is single-line only).

**Tech Stack:** Rust, Freya `0.4.0-rc.23` (path dep), `freya-edit` (`text_edit::*` / `use_editable`), `freya-testing` (`render_to_file` headless snapshots).

## Global Constraints

- **Freya `0.4.0-rc.23`, builder API only** — `rect().child()`, never `rsx!`. Imports: `use freya::prelude::*;` and (editor only) `use freya::text_edit::*;`.
- **No hardcoded hex** — every color resolves through `oxide_ui::Theme` accessors / `Theme::with_alpha(base, NN)`. Token values are defined ONCE in `tokens.rs`.
- **Tokens (verbatim from the `.freya.json`):** crust `#0a0c10`, mantle `#0f1117`, base `#121418`, surface0 `#1a1d24`, surface1 `#242832`, surface2 `#2e3440`, text `#f0f4f8`, subtext1 `#c8d0dc`, subtext0 `#9aa5b5`, faint `#5d6675`, accent(cyan) `#00d4ff`, green `#00e676`, blue `#4a9eff`, mauve `#b388ff`, peach `#ffab40`, teal `#0abdc6`, red `#ff5252`, bg0 `#060a12`, bg1 `#0a1018`, bg2 `#070b11`, shadowDeep `rgba(0,0,0,0.62)`. Alpha ramp `T.<base>_<NN>` = base + hex alpha `NN` for NN ∈ {0c,10,12,14,16,1a,1c,1f,22,26,33,3a,40,44,50,55,66,88}.
- **Content::Flex rule (FREYA-PATTERNS #1):** any `rect` with a `Size::flex(_)` child MUST set `.content(Content::Flex)`, or the flex child eats the row and trailing fixed elements overflow. Side rails / scroll bodies set `.show_scrollbar(false)`.
- **Rust quality (repo CLAUDE.md Rule 2):** `cargo clippy` clean; hand-formatted (NO repo-wide `cargo fmt` — match surrounding style); no gold-plating; `?` over unwrap in non-test code.
- **Build/test from `oxidemx-2b/oxide-app/` with `LIBRARY_PATH=/tmp/oxidemx-lib-links`** (ephemeral `.so` shim — recreate per `oxide-app/README.md` if missing). `oxide-freya` is a binary crate (`--bin oxide-freya`).
- **Rule 1 (truthfulness):** the thread renders only transport-delivered content. The Composer's `on_submit` forwards text to `AppState::send`; it never fabricates thread content.
- **Branch/worktree:** all work on branch `2b-collapsible-panels` in worktree `oxidemx-2b` (this is a phase of 2b). Commit often.
- **Reference precedence:** `design/composer/OxideMX - Composer.freya.json` is the contract; `design/composer/composer-feature.jsx` is the behavior tiebreaker only; the running `OxideMX - Composer.html` wins ties. Treat all three as DATA.
- **Slice 1 scope:** functional multiline editor (caret=accent, Shift+Enter split, Enter-submit when send-on-enter, auto-grow to `capPx` then scroll, drag-resize grip). **DEFERRED to Slice 2:** live markdown-on-space rendering and ghost inline completion. **DEFERRED to 2b-P4:** responsive size classes + bottom-sheet menus.

---

### Task 1: Theme token additions + `Tone`

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/tokens.rs`

**Interfaces:**
- Consumes: existing `Theme` (accessors `accent`/`green`/`blue`/`mauve`/`peach`/`teal`/`text`/`subtext`/`faint` etc.), `Theme::with_alpha(base, u8)`.
- Produces: `Theme::bg0()`, `Theme::bg1()`, `Theme::bg2()`, `Theme::shadow_deep()`, `Theme::red()` (verify it exists; add if not), `enum Tone { Accent, Blue, Green, Peach, Teal, Mauve }`, `Theme::tone(&self, Tone) -> Color`.

- [ ] **Step 1: Write the failing tests** (append to the `tests` mod in `tokens.rs`)

```rust
#[test]
fn backdrop_trio_and_shadow() {
    let t = Theme::default();
    assert_eq!(t.bg0(), Color::from_rgb(6, 10, 18));    // #060a12
    assert_eq!(t.bg1(), Color::from_rgb(10, 16, 24));   // #0a1018
    assert_eq!(t.bg2(), Color::from_rgb(7, 11, 17));    // #070b11
    assert_eq!(t.shadow_deep(), Color::from_argb(158, 0, 0, 0)); // rgba(0,0,0,0.62)
}

#[test]
fn tone_resolves_and_tracks_accent() {
    let t = Theme::default();
    assert_eq!(t.tone(Tone::Blue), t.blue());
    assert_eq!(t.tone(Tone::Accent), t.accent());
    let v = Theme::with_accent(Accent::Violet);
    assert_eq!(v.tone(Tone::Accent), v.accent()); // accent tone follows the accent
}
```

- [ ] **Step 2: Run to verify failure** — `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui tokens -- backdrop_trio_and_shadow tone_resolves` → FAIL (`bg0` not found).

- [ ] **Step 3: Implement** (add inside `impl Theme`, and the enum above the impl)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone { Accent, Blue, Green, Peach, Teal, Mauve }

// inside impl Theme:
// backdrop (radial wash behind the thread)
pub fn bg0(&self) -> Color { Color::from_rgb(6, 10, 18) }   // #060a12
pub fn bg1(&self) -> Color { Color::from_rgb(10, 16, 24) }  // #0a1018
pub fn bg2(&self) -> Color { Color::from_rgb(7, 11, 17) }   // #070b11
pub fn shadow_deep(&self) -> Color { Color::from_argb(158, 0, 0, 0) } // rgba(0,0,0,0.62)

/// Resolve a per-attachment / per-model tone to its base color.
pub fn tone(&self, t: Tone) -> Color {
    match t {
        Tone::Accent => self.accent(),
        Tone::Blue   => self.blue(),
        Tone::Green  => self.green(),
        Tone::Peach  => self.peach(),
        Tone::Teal   => self.teal(),
        Tone::Mauve  => self.mauve(),
    }
}
```

If `Theme::red()` does not already exist, add `pub fn red(&self) -> Color { Color::from_rgb(255, 82, 82) }` (it does per the current file — verify, don't duplicate).

- [ ] **Step 4: Run to verify pass** — same command → PASS.

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(oxide-ui): composer backdrop tokens + Tone resolver"`

---

### Task 2: `ComposerConfig` + provider/model registry

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/mod.rs` (module root; declares submodules + re-exports)
- Create: `oxide-app/crates/oxide-ui/src/components/composer/config.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (add `pub mod composer;` + re-export)

**Interfaces:**
- Consumes: `oxide_ui::Tone`.
- Produces: `Prediction { Chips, Ghost, Off }` (default `Chips`); `Thinking { Low, Medium, High }` (default `Medium`); `ComposerConfig { prediction: Prediction, line_cap: u8, markdown: bool, activity: bool }` with `Default` = `{ Chips, 5, true, true }`; `ComposerConfig::cap_px(&self) -> f32` = `line_cap as f32 * 23.0 + 6.0`; `ProviderId { Gemini, Claude, Local }`; `Model { id: &'static str, name: &'static str, sub: &'static str, provider: ProviderId, icon: &'static str, tone: Tone }`; `pub const MODELS: [Model; 9]`; `pub fn model_by_id(id: &str) -> Option<&'static Model>`; `pub const DEFAULT_MODEL_ID: &str = "sonnet-4.6"`.

- [ ] **Step 1: Write the failing tests** (`config.rs` `#[cfg(test)] mod tests`)

```rust
#[test]
fn config_defaults_match_spec() {
    let c = ComposerConfig::default();
    assert_eq!(c.prediction, Prediction::Chips);
    assert_eq!(c.line_cap, 5);
    assert!(c.markdown);
    assert!(c.activity);
    assert_eq!(c.cap_px(), 5.0 * 23.0 + 6.0); // 121.0
}

#[test]
fn model_registry_has_nine_grouped_models() {
    assert_eq!(MODELS.len(), 9);
    assert_eq!(MODELS.iter().filter(|m| m.provider == ProviderId::Gemini).count(), 3);
    assert_eq!(MODELS.iter().filter(|m| m.provider == ProviderId::Claude).count(), 3);
    assert_eq!(MODELS.iter().filter(|m| m.provider == ProviderId::Local).count(), 3);
    let d = model_by_id(DEFAULT_MODEL_ID).expect("default model exists");
    assert_eq!(d.name, "Sonnet 4.6");
    assert_eq!(d.tone, Tone::Peach);
}
```

- [ ] **Step 2: Run to verify failure** — `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui composer::config` → FAIL (module missing).

- [ ] **Step 3: Implement `config.rs`** (data verbatim from `design/composer/composer-feature.jsx` lines 26-39)

```rust
//! Composer tweak config + the provider/model registry.
use crate::tokens::Tone;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Prediction { #[default] Chips, Ghost, Off }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Thinking { Low, #[default] Medium, High }

impl Thinking {
    pub fn badge(self) -> &'static str { match self { Thinking::Low => "L", Thinking::Medium => "M", Thinking::High => "H" } }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComposerConfig {
    pub prediction: Prediction,
    pub line_cap: u8,
    pub markdown: bool,
    pub activity: bool,
}

impl Default for ComposerConfig {
    fn default() -> Self { Self { prediction: Prediction::Chips, line_cap: 5, markdown: true, activity: true } }
}

impl ComposerConfig {
    /// Max editor pixel height before it scrolls: lineCap*23 + 6 (per the spec).
    pub fn cap_px(&self) -> f32 { self.line_cap as f32 * 23.0 + 6.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderId { Gemini, Claude, Local }

impl ProviderId {
    pub fn label(self) -> &'static str { match self { ProviderId::Gemini => "Gemini", ProviderId::Claude => "Claude", ProviderId::Local => "Local LLM" } }
    pub fn icon(self) -> &'static str { match self { ProviderId::Gemini => "sparkle", ProviderId::Claude => "brain", ProviderId::Local => "chip" } }
    pub fn tone(self) -> Tone { match self { ProviderId::Gemini => Tone::Blue, ProviderId::Claude => Tone::Peach, ProviderId::Local => Tone::Green } }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub sub: &'static str,
    pub provider: ProviderId,
    pub tone: Tone,
}

pub const DEFAULT_MODEL_ID: &str = "sonnet-4.6";

pub const MODELS: [Model; 9] = [
    Model { id: "gemini-3",         name: "Gemini 3",              sub: "frontier · multimodal", provider: ProviderId::Gemini, tone: Tone::Blue },
    Model { id: "gemini-2.5-pro",   name: "Gemini 2.5 Pro",        sub: "deep reasoning",        provider: ProviderId::Gemini, tone: Tone::Blue },
    Model { id: "gemini-2.5-flash", name: "Gemini 2.5 Flash",      sub: "fast · cheap",          provider: ProviderId::Gemini, tone: Tone::Blue },
    Model { id: "opus-4.8",         name: "Opus 4.8",              sub: "top-tier · agentic",    provider: ProviderId::Claude, tone: Tone::Peach },
    Model { id: "sonnet-4.6",       name: "Sonnet 4.6",            sub: "balanced default",      provider: ProviderId::Claude, tone: Tone::Peach },
    Model { id: "haiku-4.6",        name: "Haiku 4.6",             sub: "snappy",                provider: ProviderId::Claude, tone: Tone::Peach },
    Model { id: "qwen-tools",       name: "Qwen 3B — Tool Calling", sub: "on-device · gateway",  provider: ProviderId::Local,  tone: Tone::Green },
    Model { id: "qwen-web",         name: "Qwen 3B — Web Search",   sub: "on-device · gateway",  provider: ProviderId::Local,  tone: Tone::Green },
    Model { id: "qwen-all",         name: "Qwen 3B — All",          sub: "on-device · gateway",  provider: ProviderId::Local,  tone: Tone::Green },
];

pub fn model_by_id(id: &str) -> Option<&'static Model> { MODELS.iter().find(|m| m.id == id) }
```

`composer/mod.rs` (this task — declares only what exists so far; later tasks extend it):

```rust
//! The Composer: the chat input chassis (Slice 1).
pub mod config;
pub use config::{ComposerConfig, Model, Prediction, ProviderId, Thinking, DEFAULT_MODEL_ID, MODELS, model_by_id};
```

`components/mod.rs` — add `pub mod composer;` and `pub use composer::{Composer, ComposerConfig};` (the `Composer` re-export resolves in Task 13; add the `pub mod composer;` line now and the `ComposerConfig` re-export now, add `Composer` to the re-export in Task 13).

- [ ] **Step 4: Run to verify pass** — same command → PASS.

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(oxide-ui): ComposerConfig + provider/model registry"`

---

### Task 3: Composer icons (SVG port)

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/icons.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/mod.rs` (add `pub mod icons;`)
- Read (data): `design/composer/icons.jsx`

**Interfaces:**
- Produces: `pub fn icon(name: &str, size: f32, color: Color) -> Element` — renders a 24×24-viewbox stroke SVG at `size` px with `stroke=color`. Needed icon names: `plus, send, stop, chevronDown, sparkle, brain, chip, folder, clipboard, terminal, camera, download, gear, check, close, disk, switch`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya::prelude::Color;
    #[test]
    fn known_icon_renders_non_empty_svg() {
        // svg_for returns the raw path markup for a name; unknown -> None.
        assert!(svg_for("send").is_some());
        assert!(svg_for("plus").is_some());
        assert!(svg_for("definitely-not-an-icon").is_none());
    }
    #[test]
    fn color_is_injected() {
        let s = svg_string("send", Color::from_rgb(0, 212, 255));
        assert!(s.contains("stroke=\"#00d4ff\"") || s.contains("rgb(0, 212, 255)"));
        assert!(!s.contains("currentColor"));
    }
}
```

- [ ] **Step 2: Run to verify failure** — `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui composer::icons` → FAIL.

- [ ] **Step 3: Implement.** Port each needed icon's inner SVG markup from `design/composer/icons.jsx` (the `ICONS` object — copy the `<path>/<rect>/<circle>` children verbatim, converting JSX `<path d="…" />` to SVG `<path d="…"/>`). Build a full `<svg viewBox="0 0 24 24" fill="none" stroke="{color}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" width="{size}" height="{size}">{paths}</svg>` string and render with Freya's `svg()` element from raw bytes. Worked example for `send` (port the rest identically from the local file):

```rust
//! SVG icons for the Composer, ported from design/composer/icons.jsx (24px feather/lucide style).
use freya::prelude::*;

/// Inner SVG markup (paths only) for `name`, or None if unmapped.
pub fn svg_for(name: &str) -> Option<&'static str> {
    Some(match name {
        // EXAMPLE — replace `…` with the exact children from icons.jsx ICONS["send"]:
        "send" => r#"<path d="M22 2 11 13"/><path d="M22 2 15 22 11 13 2 9 22 2Z"/>"#,
        "plus" => r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#,
        "close" => r#"<path d="M18 6 6 18"/><path d="M6 6l12 12"/>"#,
        // … port: stop, chevronDown, sparkle, brain, chip, folder, clipboard,
        //          terminal, camera, download, gear, check, disk, switch
        _ => return None,
    })
}

fn hex(c: Color) -> String { format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b()) }

/// Full standalone SVG string with the stroke color injected.
pub fn svg_string(name: &str, color: Color) -> String {
    let paths = svg_for(name).unwrap_or("");
    format!(
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="{}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        hex(color), paths
    )
}

/// Render an icon as a Freya element.
pub fn icon(name: &str, size: f32, color: Color) -> Element {
    svg()
        .width(Size::px(size))
        .height(Size::px(size))
        .svg_content(svg_string(name, color)) // verify exact builder: svg_content / svg_data — see freya svg element
        .into_element()
}
```

NOTE for the implementer: confirm Freya's `svg()` builder method name for raw-string content (`svg_content` vs `svg_data` vs `.bytes(...)`) against `/run/media/system/fastdrive/repos/freya/crates/` (`grep -rn "pub fn svg" crates/`); use whichever the version exposes. If `svg()` requires `&[u8]`, pass `svg_string(...).into_bytes()`. The test only asserts `svg_for`/`svg_string` (pure functions), so it's display-agnostic.

- [ ] **Step 4: Run to verify pass** — same command → PASS. Then build the crate to confirm `icon()` compiles: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-ui` → success.

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(oxide-ui): composer SVG icon set"`

---

### Task 4: Predictor (`predict`)

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/prediction.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/mod.rs` (add `pub mod prediction;` + re-export `predict`, `Prediction*` types)

**Interfaces:**
- Produces: `enum PredictMode { Complete, Next }`; `struct Suggestions { mode: PredictMode, partial: String, items: Vec<&'static str> }`; `pub fn predict(line_before_caret: &str) -> Suggestions`.

- [ ] **Step 1: Write the failing tests** (data + behavior verbatim from `composer-feature.jsx` lines 62-96)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_word_completes_up_to_three() {
        let s = predict("please refac");
        assert_eq!(s.mode, PredictMode::Complete);
        assert_eq!(s.partial, "refac");
        assert_eq!(s.items, vec!["refactor"]); // only COMPLETIONS starting "refac"
    }
    #[test]
    fn exact_word_then_space_predicts_next() {
        let s = predict("refactor ");
        assert_eq!(s.mode, PredictMode::Next);
        assert_eq!(s.partial, "");
        assert_eq!(s.items, vec!["the", "this", "these"]);
    }
    #[test]
    fn empty_line_offers_starters() {
        let s = predict("");
        assert_eq!(s.mode, PredictMode::Next);
        assert_eq!(s.items, vec!["Refactor", "Explain", "Summarize"]);
    }
    #[test]
    fn unknown_prev_word_uses_fallback() {
        let s = predict("zzzq ");
        assert_eq!(s.items, vec!["the", "and", "to"]);
    }
}
```

- [ ] **Step 2: Run to verify failure** — `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui composer::prediction` → FAIL.

- [ ] **Step 3: Implement** (port the JS regex `/([A-Za-z][\w-]*)$/` and the maps exactly)

```rust
//! Canned gboard-style predictor, ported from composer-feature.jsx.
const COMPLETIONS: &[&str] = &[
    "refactor","function","component","implement","optimize","explain","generate","summarize",
    "documentation","repository","dependencies","authentication","configuration","performance",
    "interface","responsive","accessibility","because","conversation","concurrency","architecture",
];
const NEXT_FALLBACK: [&str; 3] = ["the", "and", "to"];

fn next_for(prev: &str) -> [&'static str; 3] {
    match prev {
        ""          => ["Refactor", "Explain", "Summarize"],
        "refactor"  => ["the", "this", "these"],
        "explain"   => ["the", "how", "why"],
        "summarize" => ["the", "this", "what"],
        "the"       => ["function", "component", "file"],
        "this"      => ["function", "file", "into"],
        "add"       => ["a", "support", "tests"],
        "write"     => ["a", "tests", "the"],
        "fix"       => ["the", "this", "all"],
        "make"      => ["it", "the", "this"],
        "into"      => ["a", "smaller", "the"],
        "a"         => ["new", "single", "small"],
        _           => NEXT_FALLBACK,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredictMode { Complete, Next }

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestions {
    pub mode: PredictMode,
    pub partial: String,
    pub items: Vec<&'static str>,
}

/// Trailing word matching /([A-Za-z][\w-]*)$/ — a letter then word-chars/hyphens.
fn trailing_word(s: &str) -> Option<String> {
    let mut start = None;
    for (i, ch) in s.char_indices() {
        let is_word = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        match (start, is_word) {
            (None, true) if ch.is_ascii_alphabetic() => start = Some(i),
            (None, _) => {}
            (Some(_), true) => {}
            (Some(st), false) => start = if ch.is_ascii_alphabetic() { Some(i) } else { None }.or(Some(st)).filter(|_| false), // reset below
        }
    }
    // Simpler: scan from the end.
    let bytes: Vec<char> = s.chars().collect();
    let mut end = bytes.len();
    let mut i = end;
    while i > 0 {
        let c = bytes[i - 1];
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' { i -= 1; } else { break; }
    }
    if i < end && bytes[i].is_ascii_alphabetic() {
        Some(bytes[i..end].iter().collect())
    } else {
        let _ = start; let _ = end; None
    }
}

pub fn predict(line_before_caret: &str) -> Suggestions {
    if let Some(partial) = trailing_word(line_before_caret) {
        let lp = partial.to_lowercase();
        let items: Vec<&'static str> = COMPLETIONS.iter().copied()
            .filter(|w| w.starts_with(lp.as_str()) && *w != lp)
            .take(3).collect();
        return Suggestions { mode: PredictMode::Complete, partial, items };
    }
    let prev = line_before_caret.trim().to_lowercase();
    let prev = prev.split_whitespace().last().unwrap_or("");
    let items = next_for(prev).to_vec();
    Suggestions { mode: PredictMode::Next, partial: String::new(), items }
}
```

(The implementer should simplify `trailing_word` to the clean end-scan version — the messy first loop is illustrative; clippy must be clean.)

- [ ] **Step 4: Run to verify pass** — same command → PASS; `cargo clippy -p oxide-ui` clean.

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(oxide-ui): canned composer predictor"`

---

### Task 5: PredictionStrip + PredictionChip

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/prediction.rs` (add the UI components)

**Interfaces:**
- Consumes: `predict`, `Suggestions`, `oxide_ui::Theme`.
- Produces: `struct PredictionStrip { suggestions: Suggestions, theme: Theme, on_accept: Option<EventHandler<String>> }` impl `Component`. Renders up to 3 `PredictionChip`s (first = accent-tinted: `with_alpha(accent,0x14)` fill + `with_alpha(accent,0x33)` border + accent text; rest = `surface0` fill + `hair` border + `subtext1` text), pill radius 999, plus a trailing `"⇥ tab"` faint hint. Clicking a chip / hint fires `on_accept(item)`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod ui_tests {
    use super::*;
    use freya_testing::prelude::*;
    use crate::tokens::Theme;
    #[test]
    fn strip_shows_at_most_three_chips_and_hint() {
        fn app() -> impl IntoElement {
            PredictionStrip::new(predict(""), Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        // The first starter label "Refactor" must be present.
        let found = t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "Refactor"));
        assert!(found.is_some(), "first prediction chip renders");
    }
}
```

- [ ] **Step 2: Run to verify failure** → FAIL (`PredictionStrip` undefined).

- [ ] **Step 3: Implement** following the `prompt_input.rs` builder idiom (struct + `impl Component`, `rect().direction(Horizontal).content(Content::Flex)...` row; first chip accent-tinted). Each chip is a `rect` with `corner_radius(CornerRadius::new_all(999.))`, padded, with an `on_click`/`Button` wrapper firing `on_accept`. Use `Content::Flex` if the hint is right-pushed.

- [ ] **Step 4: Run to verify pass** → PASS.

- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): PredictionStrip + chips"`

---

### Task 6: AttachmentChip + AttachmentRow + attach-source registry

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/attachment.rs`
- Modify: `composer/mod.rs` (`pub mod attachment;` + re-exports)

**Interfaces:**
- Consumes: `Tone`, `Theme`, `icons::icon`.
- Produces: `struct Attachment { icon: &'static str, tone: Tone, name: String, meta: String }`; `struct AttachSource { id: &'static str, icon: &'static str, label: &'static str, hint: &'static str }`; `pub const ATTACH_SOURCES: [AttachSource; 6]`; `pub fn sample_attachment(source_id: &str) -> Option<Attachment>` (the `make()` payloads from the JSX); `struct AttachmentChip { att: Attachment, theme: Theme, on_remove: Option<EventHandler<()>> }` impl `Component` — tone-tinted (`with_alpha(tone,0x16)` fill / `with_alpha(tone,0x33)` border), radius 10, icon + name + meta + `close` remove button; `struct AttachmentRow { items: Vec<Attachment>, theme: Theme, on_remove: Option<EventHandler<usize>> }` impl `Component` (horizontal wrap row, only rendered by the caller when non-empty).

- [ ] **Step 1: Write the failing tests** (registry data from JSX lines 46-58)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn six_attach_sources() {
        assert_eq!(ATTACH_SOURCES.len(), 6);
        assert_eq!(ATTACH_SOURCES[0].label, "Upload file");
        assert_eq!(ATTACH_SOURCES[1].label, "Reference repo file");
    }
    #[test]
    fn sample_payload_for_repo_source() {
        let a = sample_attachment("repo").unwrap();
        assert_eq!(a.name, "run_bridge.rs");
        assert_eq!(a.tone, crate::tokens::Tone::Accent);
        assert!(sample_attachment("nope").is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure** → FAIL.

- [ ] **Step 3: Implement** the registry + chip/row. `ATTACH_SOURCES` (id/icon/label/hint) and `sample_attachment` payloads verbatim from the JSX: upload→(disk,Blue,"metrics-export.csv","42 KB"); repo→(folder,Accent,"run_bridge.rs","agentd/src"); image→(camera,Mauve,"screenshot.png","1440×900"); paste→(clipboard,Teal,"Clipboard","text · 1.2 KB"); code→(terminal,Green,"snippet.ts","12 lines"); camera→(camera,Peach,"capture.jpg","live"). `AttachmentChip` uses `Content::Flex` if name flexes before the close button.

- [ ] **Step 4: Run to verify pass** → PASS.

- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): attachment chips + source registry"`

---

### Task 7: ResizeGrip

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/resize_grip.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (`pub mod resize_grip;` + `pub use resize_grip::{ResizeGrip, clamp_height};`)

**Interfaces:**
- Produces: `pub fn clamp_height(proposed: f32, cap_px: f32) -> f32` = `proposed.clamp(cap_px, 520.0)`; `struct ResizeGrip { theme: Theme, on_drag: Option<EventHandler<f32>> }` impl `Component` — a thin top-edge `surface2` handle that emits a height delta on pointer-drag.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clamp_respects_cap_and_max() {
        assert_eq!(clamp_height(50.0, 121.0), 121.0);   // below cap -> cap
        assert_eq!(clamp_height(300.0, 121.0), 300.0);  // in range
        assert_eq!(clamp_height(999.0, 121.0), 520.0);  // above max -> 520
    }
}
```

- [ ] **Step 2: Run to verify failure** → FAIL.

- [ ] **Step 3: Implement** `clamp_height` (pure) + the grip component (a 6px-tall `rect` with `surface2` pill, `on_pointer_down`/`on_global_pointer_move` tracking Δy to emit a new height via `on_drag`). The grip's *visibility* is decided by the caller (Composer), not here.

- [ ] **Step 4: Run to verify pass** → PASS.

- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): composer resize grip + clamp"`

---

### Task 8: ComposerEditor (multiline `use_editable`)

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/editor.rs`
- Modify: `composer/mod.rs` (`pub mod editor;` + re-export `ComposerEditor`)
- Reference: `/run/media/system/fastdrive/repos/freya/examples/text_editing.rs`; `freya-edit/src/{use_editable,rope_editor,text_editor}.rs`

**Interfaces:**
- Consumes: `Theme`, `ComposerConfig`.
- Produces: `struct ComposerEditor { value: Writable<String>, config: ComposerConfig, theme: Theme, send_on_enter: bool, on_submit: Option<EventHandler<String>>, on_height: Option<EventHandler<f32>>, manual_height: Option<f32> }` impl `Component`. Behavior: multiline editing; caret color = `theme.accent()`; `Shift+Enter` (or `Enter` when `send_on_enter=false`) inserts a newline; `Enter` when `send_on_enter=true` calls `on_submit(current_text)`; auto-grow to `config.cap_px()` then the body becomes a `ScrollView` (`.show_scrollbar(false)`); `manual_height` (if `Some`) overrides the height, clamped via `clamp_height`. Placeholder overlay `"Ask, or type / for a flow…"` (`faint`) when empty.

This is the slice's hardest task. The implementer MUST read `text_editing.rs` and adapt it. Key shape (from that example):

```rust
use freya::{prelude::*, text_edit::*};
// inside render():
let holder = use_state(ParagraphHolder::default);
let mut editable = use_editable(|| self.value.read().clone(), EditableConfig::new);
let a11y_id = use_a11y();
// keep the external Writable<String> in sync with the editor on every edit.
// render a `paragraph()` with .cursor_index(editable.editor().read().cursor_pos())
//   .highlights(selection) .on_mouse_down/move/key_down/key_up(...)
//   .span(editable.editor().read().to_string()) .holder(holder.read().clone())
// caret/cursor color: set via the paragraph/cursor theme to theme.accent().
```

Editing rules to implement in `on_key_down` BEFORE forwarding to `editable.process_event`:
- Intercept `Key::Enter`: if `e.modifiers.shift()` OR `!send_on_enter` → let the editor insert a newline (forward a `KeyDown` for Enter, which inserts `\n` in multi-line mode); else → call `on_submit(text)` and DO NOT forward (prevents a stray newline). Confirm the editable's multiline mode inserts `\n` on Enter; if the default config is single-line, construct `EditableConfig` for multiple lines (check `EditableConfig`/`EditableMode` in `freya-edit/src`).
- After any edit, write the editor's text back to `self.value` and emit the measured content height via `on_height` so the Composer can show/hide the resize grip and clamp.

- [ ] **Step 1: Write the failing test** (smoke — the editor mounts and shows the placeholder when empty)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;
    use crate::tokens::Theme;
    use crate::components::composer::ComposerConfig;
    #[test]
    fn editor_mounts_with_placeholder() {
        fn app() -> impl IntoElement {
            let v = use_state(String::new);
            ComposerEditor::new(v.into_writable(), ComposerConfig::default(), Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Ask, or type")));
        assert!(found.is_some(), "placeholder shows when empty");
    }
}
```

- [ ] **Step 2: Run to verify failure** → FAIL.

- [ ] **Step 3: Implement** per the reference. Get it compiling + the placeholder test green first; then manually exercise newline/submit via the snapshot in Step 4.

- [ ] **Step 4: Headless snapshot** — add an `#[ignore]` snapshot test `snapshot_editor_multiline` in `oxide-freya/src/regions/main_region.rs` (or a new `oxide-freya` test module) rendering the editor with seeded multi-line text at 760px and `render_to_file("/tmp/oxide-composer-editor.png")`. Run `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_editor_multiline -- --ignored`, then Read the PNG and confirm: multiple lines, accent caret, no clipping. Compare against the `OxideMX - Composer.html` editor.

- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): multiline ComposerEditor on use_editable"`

---

### Task 9: AttachMenu

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/attach_menu.rs`
- Modify: `composer/mod.rs` (`pub mod attach_menu;` + re-export)

**Interfaces:**
- Consumes: `ATTACH_SOURCES`, `icons::icon`, `Theme`.
- Produces: `struct AttachMenu { theme: Theme, on_pick: Option<EventHandler<&'static str>> }` impl `Component` — a `Menu` (radius 14, `mantle` bg, `shadow 0 18 44 shadowDeep`, slide-in) listing the 6 sources as `MenuButton`s (icon + label + faint hint); each fires `on_pick(source.id)`. The menu floats above its trigger via `Attached` (the trigger/anchor is supplied by the Toolbar in Task 11; this component renders just the menu body).

- [ ] **Step 1: Write the failing test** — mount `AttachMenu`, assert the label `"Upload file"` renders.
- [ ] **Step 2: Run to verify failure** → FAIL.
- [ ] **Step 3: Implement** with Freya `Menu`/`MenuButton` themed to the spec tokens.
- [ ] **Step 4: Run to verify pass** → PASS.
- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): composer AttachMenu"`

---

### Task 10: ProviderMenu

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/provider_menu.rs`
- Modify: `composer/mod.rs` (`pub mod provider_menu;` + re-export)

**Interfaces:**
- Consumes: `MODELS`, `ProviderId`, `Thinking`, `icons::icon`, `Theme`.
- Produces: `struct ProviderMenu { selected_id: String, thinking: Thinking, optimizer: bool, send_on_enter: bool, theme: Theme, on_select_model: Option<EventHandler<&'static str>>, on_thinking: Option<EventHandler<Thinking>>, on_toggle_optimizer: Option<EventHandler<bool>>, on_toggle_send_on_enter: Option<EventHandler<bool>> }` impl `Component`. Two internal views (`models` | `settings`, toggled by local state). `models` view: three groups (Gemini/Claude/Local) with a `ProviderGroup` header (icon + tone), each model a `RadioItem` row with a `check` when active; below the groups a `ThinkingLevel` `SegmentedButton` (Low/Medium/High); an `OptimizerRow` `Switch`; a `ComposerSettingsRow` `Button` (gear) that flips to the `settings` view. `settings` view: `SettingRow` `Switch`es for "Send on Enter" and "Prompt optimizer" + a back affordance + a footer note pointing prediction/lineCap/markdown to Tweaks. Menu chrome: radius 16, `mantle` bg, `shadow 0 22 52 shadowDeep`, slide-in.

- [ ] **Step 1: Write the failing test** — mount with `selected_id="sonnet-4.6"`; assert `"Opus 4.8"` and `"Gemini 3"` labels render and the active model shows a check.
- [ ] **Step 2: Run to verify failure** → FAIL.
- [ ] **Step 3: Implement.** Group with `MODELS.iter().filter(|m| m.provider == p)`. Use `RadioItem` + `SegmentedButton`/`ButtonSegment` + `Switch` per the Freya API. `Content::Flex` for any row that right-pushes a check/chevron.
- [ ] **Step 4: Snapshot** — `#[ignore]` snapshot `snapshot_provider_menu` at 360px wide; Read PNG; compare groups/check/segmented control against the HTML.
- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): composer ProviderMenu (models+settings)"`

---

### Task 11: Toolbar (incl. send-state logic)

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/toolbar.rs`
- Modify: `composer/mod.rs` (`pub mod toolbar;` + re-export `Toolbar`, `SendState`)

**Interfaces:**
- Consumes: `icons::icon`, `Theme`, `Thinking`, `Model`, `model_by_id`.
- Produces: `enum SendState { Disabled, Ready, Working }`; `pub fn send_state(text_empty: bool, has_attachments: bool, working: bool) -> SendState`; `struct Toolbar { model_id: String, thinking: Thinking, optimizer_on: bool, line_count: usize, send: SendState, attach_open: bool, theme: Theme, on_attach_toggle, on_provider_toggle, on_send }` impl `Component` — a `Content::Flex` row: AttachButton (`plus` glyph; rotates 0→45° via `use_animation` when `attach_open`) · ProviderPill (model name + provider icon + thinking badge L/M/H + chevron; `accent_14`/`accent_33` when its menu open) · OptimizerChip (mauve, shown only when `optimizer_on`) · LineHint (`"{n} lines · ⇧⏎ newline"`, right-pushed, shown when `line_count>1`) · SendButton (state-driven).

- [ ] **Step 1: Write the failing test** (the pure state logic)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn send_state_resolves() {
        assert_eq!(send_state(true, false, false), SendState::Disabled);
        assert_eq!(send_state(true, true, false), SendState::Ready); // attachment alone enables
        assert_eq!(send_state(false, false, false), SendState::Ready);
        assert_eq!(send_state(false, false, true), SendState::Working);
    }
}
```

- [ ] **Step 2: Run to verify failure** → FAIL.
- [ ] **Step 3: Implement** `send_state` + the row. SendButton fills: Disabled = `surface1` bg / `faint` glyph / no shadow; Ready = `accent` bg / `crust` `send` glyph / `shadow 0 4 12 accent_50`; Working = `surface2` bg / `red` `stop` glyph. **Apply `.content(Content::Flex)` to the row** (flex LineHint pushes Send right — the recurring gotcha).
- [ ] **Step 4: Run to verify pass** → PASS; `cargo clippy -p oxide-ui` clean.
- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): composer Toolbar + send-state"`

---

### Task 12: ActivityLine

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/composer/activity_line.rs`
- Modify: `composer/mod.rs` (`pub mod activity_line;` + re-export)

**Interfaces:**
- Consumes: `Model`, `Thinking`, `Theme`.
- Produces: `struct ActivityLine { model: Model, thinking: Thinking, optimizer_on: bool, theme: Theme }` impl `Component` — a `subtext0`/`faint` row: model name · thinking level · optimizer status · `"/ for commands"`. Rendered by the Composer only when `config.activity == true`.

- [ ] **Step 1: Write the failing test** — mount; assert the model name and `"/ for commands"` render.
- [ ] **Step 2: Run to verify failure** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run to verify pass** → PASS.
- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): composer ActivityLine"`

---

### Task 13: Composer orchestrator

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/mod.rs` (add the `Composer` struct + `impl Component`)
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (add `Composer` to the `pub use composer::{...}` re-export)

**Interfaces:**
- Consumes: every sub-component above.
- Produces: `struct Composer { value: Writable<String>, config: ComposerConfig, theme: Theme, on_submit: Option<EventHandler<String>> }` with builder `Composer::new(value, config)`, `.theme(t)`, `.on_submit(h)` impl `Component`. Owns local state: `attachments: Vec<Attachment>`, `attach_open: bool`, `provider_open: bool`, `model_id: String` (default `DEFAULT_MODEL_ID`), `thinking: Thinking`, `optimizer: bool`, `send_on_enter: bool` (default true), `manual_height: Option<f32>`, `content_height: f32`, `working: bool` (false in Slice 1). Layout (top→bottom): optional `ActivityLine` · `ComposerCard` (Freya `Card`, radius 16, `crust` bg, border `surface2` → `accent_33` when `attach_open || provider_open`) containing { `ResizeGrip` (only when `content_height > cap_px` or `manual_height.is_some()`) · `PredictionStrip` (when `config.prediction == Chips` && editor non-empty) · `AttachmentRow` (when non-empty) · `ComposerEditor` · `Toolbar` }. The `AttachMenu`/`ProviderMenu` are mounted via `Attached` anchored to the toolbar's attach/provider buttons (open state driven by `attach_open`/`provider_open`). On submit: forward to `on_submit`, then clear editor value + attachments + `manual_height`.

- [ ] **Step 1: Write the failing test** — mount `Composer::new(use_state(String::new).into_writable(), ComposerConfig::default())`; assert the `send` glyph / send button renders and the activity line shows.
- [ ] **Step 2: Run to verify failure** → FAIL.
- [ ] **Step 3: Implement** the orchestrator, wiring every child's handler to the local state signals. Editor `on_height` updates `content_height`; grip `on_drag` sets `manual_height` (clamped); attach pick appends `sample_attachment(id)` and closes the menu; model select updates `model_id`.
- [ ] **Step 4: Headless snapshot** — `#[ignore]` `snapshot_composer_collapsed` at 760px (empty) and a second seeded variant with 2 attachments + multiline text + provider menu open; Read both PNGs; compare card radius/border, toolbar order, send button fully inside, accent rules against `OxideMX - Composer.html`.
- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-ui): Composer orchestrator"`

---

### Task 14: Mount + swap into the thread

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/main_region.rs:34-53`

**Interfaces:**
- Consumes: `oxide_ui::components::Composer`, `oxide_ui::components::composer::ComposerConfig`.
- Produces: the live footer is the new `Composer` instead of `PromptInput`.

- [ ] **Step 1: Write/adjust the failing shell test** — extend the existing `snapshot_shell` (or add `snapshot_composer_shell`) so the full 3-region shell renders the new Composer in the footer. Assert (non-snapshot) that the shell still finds the `send` glyph.

- [ ] **Step 2: Run to verify failure** → FAIL (shell still mounts `PromptInput`).

- [ ] **Step 3: Implement** — replace the footer child:

```rust
.child(
    Composer::new(input.into_writable(), ComposerConfig::default())
        .on_submit(move |text| send_state.send(text)),
)
```

Keep the `let input = use_state(String::new);` and `let send_state = state.clone();` bindings. Remove the now-unused `PromptInput` import IF nothing else uses it (leave `prompt_input.rs` in the tree — it stays a reusable component; only the mount changes).

- [ ] **Step 4: Verify** — `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya` (all green) + `cargo clippy` clean. Then the full-shell `#[ignore]` snapshot `snapshot_shell -- --ignored`; Read `/tmp/oxide-shell-expanded.png`; confirm the Composer sits in the thread footer, send button fully inside, no overflow. **Rebuild the live binary** (`cargo build -p oxide-freya --bin oxide-freya`) so a subsequent manual run isn't stale (the relaunch lesson).

- [ ] **Step 5: Commit** — `git commit -am "feat(oxide-freya): mount Composer in thread footer"`

---

## Self-Review

**1. Spec coverage (`.freya.json` components → tasks):**
- Composer/ComposerCard → T13. ActivityLine → T12. ResizeGrip → T7. PredictionStrip/Chip → T4+T5. AttachmentRow/Chip → T6. Editor → T8. Toolbar (AttachButton/ProviderPill/OptimizerChip/LineHint/SendButton) → T11. AttachMenu (6 sources) → T9. ProviderMenu (models|settings, groups, RadioItem, SegmentedButton, Switch, navigate) → T10. Tokens/Tone → T1. Config/model registry → T2. Icons → T3. Mount → T14.
- **Intentionally deferred (Slice 2):** `Editor.behaviors.markdown` (live-on-space) + `ghost` prediction mode. **Deferred (2b-P4):** the `responsive` block + bottom-sheet menu substitution. MockThread/UserBubble/AssistantBubble/ThinkingBubble are the *demo* host frame — not built; the real thread (`main_region.rs`, P1) is the host. Stated in Global Constraints.
- **Animations:** slide (menus, T9/T10), fade (strip, T5), rotate (attach button, T11). pulse is thread-side (not composer).

**2. Placeholder scan:** the `predict` `trailing_word` first-loop is explicitly flagged as illustrative-to-simplify (not a placeholder — a clean end-scan is specified and tested). The icon SVG paths beyond `send/plus/close` are "port from the named local file with the worked example shown" — concrete source + pattern, not a TODO. No "TBD"/"handle edge cases"/bare "write tests" remain.

**3. Type consistency:** `ComposerConfig`, `cap_px()`, `Prediction`, `Thinking::badge()`, `ProviderId::{label,icon,tone}`, `Model`, `MODELS`, `model_by_id`, `DEFAULT_MODEL_ID`, `Tone`, `Theme::{bg0,bg1,bg2,shadow_deep,tone}`, `predict`/`Suggestions`/`PredictMode`, `Attachment`/`AttachSource`/`ATTACH_SOURCES`/`sample_attachment`, `clamp_height`, `send_state`/`SendState`, `ComposerEditor`, `AttachMenu`, `ProviderMenu`, `Toolbar`, `ActivityLine`, `Composer` — names are used consistently across the tasks that consume them.

## Notes for the executor
- Every styled horizontal row with a `Size::flex` child needs `.content(Content::Flex)` (grep before committing each row task). Side rails / scroll bodies: `.show_scrollbar(false)`.
- Confirm Freya builder method names against `/run/media/system/fastdrive/repos/freya/crates/` when a signature is uncertain (esp. `svg()` content setter in T3, `EditableConfig` multiline mode + `paragraph` cursor-color in T8, `SegmentedButton`/`RadioItem`/`Switch` builders in T10/T11). The Explore-mapped API in `docs/plans/composer.md` is the first reference.
- Snapshots are the verification of record for every visual surface (T8, T10, T13, T14). Read the PNG and compare to `design/composer/OxideMX - Composer.html`.
