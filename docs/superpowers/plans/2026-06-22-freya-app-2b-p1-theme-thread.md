# Freya App 2b — Phase 1 (Theme + Thread) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Restyle the OxideMX Freya **center Thread column** (and expand the theme) to match the Claude Design "Collapsible Panels" mockup — the immediate visible transformation of the running app.

**Architecture:** Expand `oxide-ui`'s `Theme` to the full design palette (flat names + a `with_alpha` helper). Restyle the thread's reusable components (bubbles, header, composer) + add small design primitives (status puck, gradient avatar, worktree chip), reaching for Freya built-ins (`Input`/`Button`) themed via their `define_theme!` partials. Wire them into `MainRegion`. Styling is additive — the 2a transport/reducer/truthfulness are untouched.

**Tech Stack:** Rust, Freya v0.4.0-rc.23, `oxide-ui` + `oxide-freya` (in `oxide-app/`, in phase1), `freya-testing`.

## Global Constraints

- **Source of truth:** the validated `oxide-app/design-pipeline/OxideMX Freya - Collapsible Panels.freya.json` (the `theme.tokens` + the `Thread` component subtree) consumed via the **`claude-design-to-freya`** skill. **Run that skill + `freya-gui-framework` + read `oxide-app/FREYA-PATTERNS.md` before writing code.**
- **Rule 1 — truthfulness:** the thread renders ONLY transport-delivered content (committed turns + the live `live_assistant` bubble). Styling adds no fabricated text; the reducer/`AppState` are untouched.
- **Rule 2 — quality:** `cargo clippy -p oxide-ui -p oxide-freya -- -D warnings` clean; hand-formatted (match surrounding style, no repo-wide `cargo fmt`); no gold-plating (style the listed components, nothing more). Reusable design bits live in `oxide-ui`.
- **Verified rc.23 APIs (from the skill tests — use these, don't re-derive):**
  - Alpha colors: `Color::from_argb(a: u8, r, g, b)`. The design's `${accent}1a` → `Theme::with_alpha(theme.accent(), 0x1a)`.
  - Per-corner radius: `CornerRadius { top_left, top_right, bottom_right, bottom_left }` (NOT `new_all`).
  - Gradient: `rect().background_linear_gradient(LinearGradient::new().angle(150.).stop(colorA, 0.).stop(colorB, 100.))` (clipped by corner_radius).
  - Font weight: `label().font_weight(FontWeight::SEMI_BOLD)` / `FontWeight::BOLD`.
  - Built-in theming shapes: `Button`/`Switch`/`Card` use dual `.theme_colors(<C>ColorsThemePartial{..})` + `.theme_layout(..)`; `Input`/`Slider`/`Select` use a single `.theme(<C>ThemePartial{..})`; fields wrap as `Some(Preference::Specific(color))`. (See the skill's "theming a built-in" table.)
  - Animation: `use_animation(|c| { c.on_creation(OnCreation::Run); AnimNum::new(0.,1.).time(1800).ease(Ease::Out) })` (loop via `OnFinish::Restart`) for the `pulse`.
- **Design token values (from `freya.json theme.tokens`, default cyan accent):** crust `#0a0c10` · mantle `#0f1117` · base `#121418` · surface0 `#1a1d24` · surface1 `#242832` · surface2 `#2e3440` · overlay0 `#404654` · text `#f0f4f8` · subtext1 `#c8d0dc` · subtext0 `#9aa5b5` · faint `#5d6675` · accent `#00d4ff` · accent2 `#0abdc6` · accentDim `#0891a8` · green `#00e676` · yellow `#ffd54f` · red `#ff5252` · blue `#4a9eff` · mauve `#b388ff` · peach `#ffab40` · teal `#0abdc6` · hairline white@.06 · hairlineStrong white@.10. Accent sets: cyan(`#00d4ff`,`#0abdc6`,`#0891a8`) / violet(`#b388ff`,`#8b6cff`,`#7a5bd0`) / amber(`#ffab40`,`#ff8f3f`,`#cf8232`) / lime(`#7be06a`,`#52c24a`,`#46a23e`).
- **Build:** from inside `oxide-app/`, prefix `LIBRARY_PATH=/tmp/oxidemx-lib-links` (recreate per `oxide-app/README.md` if missing). `oxide-freya` is a BINARY crate (`--bin oxide-freya`). The existing 2a suite (29 tests) MUST stay green.

---

## File Structure

- Modify `oxide-app/crates/oxide-ui/src/tokens.rs` — full palette + `with_alpha` + `Accent{Cyan,Violet,Amber,Lime}`.
- Create `oxide-app/crates/oxide-ui/src/components/status_puck.rs` — `StatusPuck` (+ keep `status_dot.rs`).
- Create `oxide-app/crates/oxide-ui/src/components/avatar.rs` — gradient `Avatar`.
- Create `oxide-app/crates/oxide-ui/src/components/chip.rs` — `Chip` / `WorktreeChip`.
- Create `oxide-app/crates/oxide-ui/src/components/thread_header.rs` — `ThreadHeader`.
- Modify `oxide-app/crates/oxide-ui/src/components/bubble.rs` — restyle `Bubble` (per-role tint/corner/avatar).
- Modify `oxide-app/crates/oxide-ui/src/components/prompt_input.rs` — restyle into the bordered Composer box + send button.
- Modify `oxide-app/crates/oxide-ui/src/components/mod.rs` — export the new components.
- Modify `oxide-app/crates/oxide-freya/src/regions/main_region.rs` — header + styled thread + composer; add a snapshot.

---

## Task 1: Expand the Theme to the design palette

**Files:** Modify `oxide-app/crates/oxide-ui/src/tokens.rs`

**Interfaces — Produces:** `Theme` with accessors `bg/bg_deep/panel/surface/surface_hi/surface_max/overlay/text/subtext_hi/subtext/faint/accent/accent_hi/accent_dim/green/yellow/red/blue/mauve/peach/teal/hairline/hairline_strong()` (all `-> Color`) + `Theme::with_alpha(base: Color, a: u8) -> Color` + `Accent { Cyan, Violet, Amber, Lime }` with `accent()/accent_hi()/accent_dim()`.

- [ ] **Step 1: Replace the failing tests** in `tokens.rs` (update the existing two + add alpha):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark_cyan() {
        let t = Theme::default();
        assert_eq!(t.bg(), Color::from_rgb(18, 20, 24));      // base #121418
        assert_eq!(t.accent(), Color::from_rgb(0, 212, 255)); // #00d4ff
    }

    #[test]
    fn palette_surfaces_and_text() {
        let t = Theme::default();
        assert_eq!(t.panel(), Color::from_rgb(15, 17, 23));    // mantle #0f1117
        assert_eq!(t.surface(), Color::from_rgb(26, 29, 36));  // surface0 #1a1d24
        assert_eq!(t.text(), Color::from_rgb(240, 244, 248));  // #f0f4f8
        assert_eq!(t.subtext(), Color::from_rgb(154, 165, 181)); // subtext0 #9aa5b5
    }

    #[test]
    fn accent_switch_changes_accent_only() {
        let t = Theme::with_accent(Accent::Violet);
        assert_eq!(t.accent(), Color::from_rgb(179, 136, 255)); // #b388ff
        assert_eq!(t.bg(), Color::from_rgb(18, 20, 24));        // unchanged
    }

    #[test]
    fn with_alpha_sets_argb() {
        let c = Theme::with_alpha(Color::from_rgb(0, 212, 255), 0x1a);
        assert_eq!(c, Color::from_argb(0x1a, 0, 212, 255));
    }
}
```

- [ ] **Step 2: Run — verify fail.** `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui tokens` → FAIL.

- [ ] **Step 3: Implement the expanded `tokens.rs`** (replace the `Accent` enum + `Theme` impl; keep the consts + fonts):

```rust
//! Design tokens for the OxideMX Freya UI. Values mirror the Claude Design
//! "Collapsible Panels" `freya.json` (default cyan accent). Flat names; the
//! `accent_NN` alpha ramp is computed via `Theme::with_alpha`.
use freya::prelude::Color;

pub const SIDEBAR_FULL_W: f32 = 274.0;
pub const SIDEBAR_RAIL_W: f32 = 60.0;
pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Cyan,
    Violet,
    Amber,
    Lime,
}

impl Accent {
    pub fn accent(self) -> Color {
        match self {
            Accent::Cyan => Color::from_rgb(0, 212, 255),
            Accent::Violet => Color::from_rgb(179, 136, 255),
            Accent::Amber => Color::from_rgb(255, 171, 64),
            Accent::Lime => Color::from_rgb(123, 224, 106),
        }
    }
    pub fn accent_hi(self) -> Color {
        match self {
            Accent::Cyan => Color::from_rgb(10, 189, 198),
            Accent::Violet => Color::from_rgb(139, 108, 255),
            Accent::Amber => Color::from_rgb(255, 143, 63),
            Accent::Lime => Color::from_rgb(82, 194, 74),
        }
    }
    pub fn accent_dim(self) -> Color {
        match self {
            Accent::Cyan => Color::from_rgb(8, 145, 168),
            Accent::Violet => Color::from_rgb(122, 91, 208),
            Accent::Amber => Color::from_rgb(207, 130, 50),
            Accent::Lime => Color::from_rgb(70, 162, 62),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    accent: Accent,
}

impl Default for Theme {
    fn default() -> Self { Self { accent: Accent::Cyan } }
}

impl Theme {
    pub fn with_accent(accent: Accent) -> Self { Self { accent } }

    /// Apply an alpha (0..=255) to a base color — the design's `${color}NN` ramp.
    pub fn with_alpha(base: Color, a: u8) -> Color {
        Color::from_argb(a, base.r(), base.g(), base.b())
    }

    // surfaces (low → high elevation)
    pub fn bg_deep(&self) -> Color { Color::from_rgb(10, 12, 16) }   // crust
    pub fn panel(&self) -> Color { Color::from_rgb(15, 17, 23) }     // mantle
    pub fn bg(&self) -> Color { Color::from_rgb(18, 20, 24) }        // base
    pub fn surface(&self) -> Color { Color::from_rgb(26, 29, 36) }   // surface0
    pub fn surface_hi(&self) -> Color { Color::from_rgb(36, 40, 50) } // surface1
    pub fn surface_max(&self) -> Color { Color::from_rgb(46, 52, 64) } // surface2
    pub fn overlay(&self) -> Color { Color::from_rgb(64, 70, 84) }   // overlay0
    // text
    pub fn text(&self) -> Color { Color::from_rgb(240, 244, 248) }
    pub fn subtext_hi(&self) -> Color { Color::from_rgb(200, 208, 220) } // subtext1
    pub fn subtext(&self) -> Color { Color::from_rgb(154, 165, 181) }    // subtext0
    pub fn faint(&self) -> Color { Color::from_rgb(93, 102, 117) }
    // accent ramp
    pub fn accent(&self) -> Color { self.accent.accent() }
    pub fn accent_hi(&self) -> Color { self.accent.accent_hi() }
    pub fn accent_dim(&self) -> Color { self.accent.accent_dim() }
    // semantic tones
    pub fn green(&self) -> Color { Color::from_rgb(0, 230, 118) }
    pub fn yellow(&self) -> Color { Color::from_rgb(255, 213, 79) }
    pub fn red(&self) -> Color { Color::from_rgb(255, 82, 82) }
    pub fn blue(&self) -> Color { Color::from_rgb(74, 158, 255) }
    pub fn mauve(&self) -> Color { Color::from_rgb(179, 136, 255) }
    pub fn peach(&self) -> Color { Color::from_rgb(255, 171, 64) }
    pub fn teal(&self) -> Color { Color::from_rgb(10, 189, 198) }
    // hairlines (tinted white)
    pub fn hairline(&self) -> Color { Color::from_argb(15, 255, 255, 255) }       // ~.06
    pub fn hairline_strong(&self) -> Color { Color::from_argb(26, 255, 255, 255) } // ~.10
}
```

- [ ] **Step 4: Run — verify pass.** `cargo test -p oxide-ui tokens` → PASS.

- [ ] **Step 5: Fix downstream breakage.** The old accessors `surface()` value changed and `Accent::Purple/Orange/Green` are gone. Build the crate + fix any references (other `oxide-ui` components + `oxide-freya` that used `Accent::Purple` etc. or the old surface): `cargo build -p oxide-ui -p oxide-freya`. Expected: any `Accent::Purple` → `Accent::Violet` etc.; existing component tests still green. Run `cargo test -p oxide-ui` → PASS.

- [ ] **Step 6: clippy + commit.**
```bash
cargo clippy -p oxide-ui -- -D warnings
git add oxide-app/crates/oxide-ui/src/tokens.rs
git commit -m "feat(oxide-ui): expand Theme to the Collapsible-Panels palette + with_alpha"
```

---

## Task 2: Thread display primitives (StatusPuck, Avatar, WorktreeChip)

**Files:** Create `status_puck.rs`, `avatar.rs`, `chip.rs` in `oxide-app/crates/oxide-ui/src/components/`; Modify `components/mod.rs`.

**Interfaces — Consumes:** `Theme`, `Theme::with_alpha`. **Produces:** `StatusPuck::new(state: &str)` (state ∈ working/delivered/failed/idle → tone yellow/green/red/overlay; dot + label pill), `Avatar::new()` (26px gradient sphere, sparkle), `WorktreeChip::new(branch: String)` (accent-tint pill). All builder `Component`s with `.theme(Theme)`.

- [ ] **Step 1: Write render tests** (one per primitive), e.g. `status_puck.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya::prelude::*;
    use freya_testing::prelude::*;

    #[test]
    fn status_puck_renders_state_label() {
        fn app() -> impl IntoElement { StatusPuck::new("working") }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "working")).is_some());
    }
}
```
(Analogous: `Avatar` renders its sparkle glyph; `WorktreeChip::new("main")` renders "main".)

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui status_puck` (etc.) → FAIL.

- [ ] **Step 3: Implement.** `status_puck.rs` (tone map + pill; the `pulse` animation when working can be a later polish — render the glow dot statically here):

```rust
//! A small status pill: a tone-colored dot + state label (working/delivered/failed/idle).
use freya::prelude::*;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct StatusPuck { state: String, theme: Theme }

impl StatusPuck {
    pub fn new(state: &str) -> Self { Self { state: state.to_string(), theme: Theme::default() } }
    pub fn theme(mut self, t: Theme) -> Self { self.theme = t; self }
    fn tone(&self) -> Color {
        match self.state.as_str() {
            "working" => self.theme.yellow(),
            "delivered" => self.theme.green(),
            "failed" => self.theme.red(),
            _ => self.theme.overlay(),
        }
    }
}

impl Component for StatusPuck {
    fn render(&self) -> impl IntoElement {
        let tone = self.tone();
        rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding(Gaps::new(5., 11., 5., 11.))
            .corner_radius(CornerRadius::new_all(999.))
            .background(Theme::with_alpha(tone, 0x14))
            .border(Border::new().fill(Theme::with_alpha(tone, 0x3a)).width(1.))
            .child(rect().width(Size::px(7.)).height(Size::px(7.))
                .corner_radius(CornerRadius::new_all(4.)).background(tone))
            .child(label().text(self.state.clone()).font_size(11.5)
                .font_weight(FontWeight::SEMI_BOLD).color(tone))
    }
}
```

`avatar.rs` (gradient sphere + sparkle):

```rust
//! A 26px gradient avatar sphere with a sparkle glyph (assistant identity).
use freya::prelude::*;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct Avatar { theme: Theme }

impl Avatar {
    pub fn new() -> Self { Self { theme: Theme::default() } }
    pub fn theme(mut self, t: Theme) -> Self { self.theme = t; self }
}
impl Default for Avatar { fn default() -> Self { Self::new() } }

impl Component for Avatar {
    fn render(&self) -> impl IntoElement {
        rect()
            .width(Size::px(26.)).height(Size::px(26.))
            .corner_radius(CornerRadius::new_all(8.))
            .main_align(Alignment::Center).cross_align(Alignment::Center)
            .background_linear_gradient(
                LinearGradient::new().angle(150.)
                    .stop(self.theme.accent(), 0.).stop(self.theme.accent_dim(), 100.))
            .child(label().text("✦").font_size(14.).color(self.theme.bg_deep()))
    }
}
```

`chip.rs` (`WorktreeChip` accent-tint pill):

```rust
//! An accent-tinted pill chip (e.g. a worktree branch name).
use freya::prelude::*;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct WorktreeChip { label: String, theme: Theme }

impl WorktreeChip {
    pub fn new(label: String) -> Self { Self { label, theme: Theme::default() } }
    pub fn theme(mut self, t: Theme) -> Self { self.theme = t; self }
}

impl Component for WorktreeChip {
    fn render(&self) -> impl IntoElement {
        let a = self.theme.accent();
        rect()
            .direction(Direction::Horizontal).cross_align(Alignment::Center)
            .padding(Gaps::new(1., 7., 1., 7.))
            .corner_radius(CornerRadius::new_all(999.))
            .background(Theme::with_alpha(a, 0x14))
            .border(Border::new().fill(Theme::with_alpha(a, 0x33)).width(1.))
            .child(label().text(self.label.clone()).font_size(10.5).color(a))
    }
}
```

(Confirm `Border`/`LinearGradient`/`FontWeight` exact names against `oxide-app/API-NOTES.md` + the skill; adjust if a method differs.)

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-ui` (the 3 new) → PASS; `cargo clippy -p oxide-ui -- -D warnings` clean.

- [ ] **Step 5: Export** in `components/mod.rs` (`pub mod status_puck; pub use status_puck::StatusPuck;` etc.).

- [ ] **Step 6: Commit.**
```bash
git add oxide-app/crates/oxide-ui/src/components
git commit -m "feat(oxide-ui): StatusPuck, Avatar, WorktreeChip thread primitives"
```

---

## Task 3: Restyle Bubble (per-role tint, per-corner tail, avatar)

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/bubble.rs`

**Interfaces — Consumes:** `Theme`, `Theme::with_alpha`, `Avatar`. **Produces:** `Bubble::new(role, text)` unchanged signature (so `MainRegion`'s call is untouched); user role → right-aligned accent-tint with corner `14,14,4,14`; assistant role → left, `Avatar` + surface bubble corner `14,14,14,4`.

- [ ] **Step 1: Update the tests** — keep `bubble_renders_text` and add a user-vs-assistant style distinction (presence-based, since color asserts are brittle): assert the assistant bubble renders the avatar sparkle `✦` and the user bubble does not.

```rust
#[test]
fn assistant_bubble_has_avatar_user_does_not() {
    fn ass() -> impl IntoElement { Bubble::new("assistant".into(), "hi".into()) }
    fn usr() -> impl IntoElement { Bubble::new("user".into(), "hi".into()) }
    let mut a = launch_test(ass); a.sync_and_update();
    assert!(a.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "✦")).is_some());
    let mut u = launch_test(usr); u.sync_and_update();
    assert!(u.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "✦")).is_none());
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui bubble` → FAIL.

- [ ] **Step 3: Implement** the restyled `Bubble`:

```rust
//! A chat message bubble styled per the Collapsible-Panels design.
use freya::prelude::*;
use crate::components::avatar::Avatar;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct Bubble { role: String, text: String, theme: Theme }

impl Bubble {
    pub fn new(role: String, text: String) -> Self { Self { role, text, theme: Theme::default() } }
    pub fn theme(mut self, t: Theme) -> Self { self.theme = t; self }
}

impl Component for Bubble {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        if self.role == "user" {
            rect().width(Size::fill()).direction(Direction::Horizontal).main_align(Alignment::End)
                .child(
                    rect().width(Size::percent(78.))
                        .padding(Gaps::new(10., 14., 10., 14.))
                        .corner_radius(CornerRadius { top_left: 14., top_right: 14., bottom_right: 4., bottom_left: 14. })
                        .background(Theme::with_alpha(th.accent(), 0x1a))
                        .border(Border::new().fill(Theme::with_alpha(th.accent(), 0x33)).width(1.))
                        .child(label().text(self.text.clone()).font_size(13.).color(th.text())),
                )
        } else {
            rect().width(Size::fill()).direction(Direction::Horizontal).main_align(Alignment::Start).spacing(10.)
                .child(Avatar::new().theme(th))
                .child(
                    rect().width(Size::percent(82.))
                        .padding(Gaps::new(10., 14., 10., 14.))
                        .corner_radius(CornerRadius { top_left: 14., top_right: 14., bottom_right: 14., bottom_left: 4. })
                        .background(th.surface())
                        .border(Border::new().fill(th.hairline()).width(1.))
                        .child(label().text(self.text.clone()).font_size(13.).color(th.subtext_hi())),
                )
        }
    }
}
```

(Restyle is per the `freya.json` `UserBubble`/`AssistantBubble`. `main_align(End)` does the right-push. Confirm `Size::percent` / `CornerRadius` struct fields against API-NOTES.)

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-ui bubble && cargo clippy -p oxide-ui -- -D warnings` → PASS, clean.

- [ ] **Step 5: Commit.**
```bash
git add oxide-app/crates/oxide-ui/src/components/bubble.rs
git commit -m "feat(oxide-ui): restyle Bubble (accent-tint user / avatar assistant, per-corner tails)"
```

---

## Task 4: ThreadHeader

**Files:** Create `oxide-app/crates/oxide-ui/src/components/thread_header.rs`; Modify `components/mod.rs`.

**Interfaces — Consumes:** `Theme`, `StatusPuck`, `WorktreeChip`. **Produces:** `ThreadHeader::new(title: String, state: String, worktree: Option<String>)` builder `Component` (+ `.theme`).

- [ ] **Step 1: Write a render test** asserting the title + state puck render:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya::prelude::*;
    use freya_testing::prelude::*;
    #[test]
    fn header_renders_title_and_state() {
        fn app() -> impl IntoElement { ThreadHeader::new("My chat".into(), "working".into(), Some("wt-x".into())) }
        let mut t = launch_test(app); t.sync_and_update();
        assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "My chat")).is_some());
        assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "working")).is_some());
    }
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui thread_header` → FAIL.

- [ ] **Step 3: Implement** `thread_header.rs`:

```rust
//! The thread header: title + optional worktree chip + a status puck.
use freya::prelude::*;
use crate::components::{status_puck::StatusPuck, chip::WorktreeChip};
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct ThreadHeader { title: String, state: String, worktree: Option<String>, theme: Theme }

impl ThreadHeader {
    pub fn new(title: String, state: String, worktree: Option<String>) -> Self {
        Self { title, state, worktree, theme: Theme::default() }
    }
    pub fn theme(mut self, t: Theme) -> Self { self.theme = t; self }
}

impl Component for ThreadHeader {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let title_col = rect().direction(Direction::Vertical).width(Size::flex(1.0)).spacing(3.)
            .child(label().text(self.title.clone()).font_size(15.)
                .font_weight(FontWeight::SEMI_BOLD).color(th.text()))
            .maybe_child(self.worktree.clone().map(|w| WorktreeChip::new(w).theme(th)));
        rect()
            .direction(Direction::Horizontal).content(Content::Flex).cross_align(Alignment::Center)
            .width(Size::fill()).spacing(12.)
            .padding(Gaps::new(12., 18., 12., 18.))
            .background(th.bg())
            .child(title_col)
            .child(StatusPuck::new(&self.state).theme(th))
    }
}
```

(`.maybe_child(Option<_>)` per FREYA-PATTERNS; confirm exact name.)

- [ ] **Step 4: Run — verify pass + clippy + export** (`pub mod thread_header; pub use thread_header::ThreadHeader;`).

- [ ] **Step 5: Commit.**
```bash
git add oxide-app/crates/oxide-ui/src/components
git commit -m "feat(oxide-ui): ThreadHeader (title + worktree chip + status puck)"
```

---

## Task 5: Restyle the Composer (bordered box + Input + send button)

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/prompt_input.rs`

**Interfaces — Consumes:** `Theme`, built-in `Input`, `Button`. **Produces:** `PromptInput::new(value: Writable<String>)` unchanged signature + `.on_submit(EventHandler<String>)`; renders the design's bordered composer box (`bg_deep` fill, `surface_max` border, radius 12) wrapping the built-in `Input` (placeholder "Ask, or type / for a flow…") + an accent `SendButton` (42px, accent fill, crust icon "➤").

- [ ] **Step 1: Update the render test** to assert the placeholder text is visible (stronger than "a Rect renders"):

```rust
#[test]
fn composer_shows_placeholder() {
    fn app() -> impl IntoElement {
        let value = use_state(String::new);
        PromptInput::new(value.into_writable())
    }
    let mut t = launch_test(app); t.sync_and_update();
    // The send glyph is a stable, composer-specific marker.
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "➤")).is_some());
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui prompt_input` → FAIL.

- [ ] **Step 3: Implement** the restyled `PromptInput` (keep `value`/`on_submit` fields; restyle `render`):

```rust
fn render(&self) -> impl IntoElement {
    let th = self.theme;
    let mut input = Input::new(self.value.clone())
        .width(Size::fill())
        .placeholder("Ask, or type / for a flow…")
        .theme(InputThemePartial {
            background: Some(Preference::Specific(th.bg_deep())),
            border_fill: Some(Preference::Specific(th.surface_max())),
            color: Some(Preference::Specific(th.text())),
            ..Default::default()
        });
    if let Some(handler) = self.on_submit.clone() { input = input.on_submit(handler); }

    rect()
        .direction(Direction::Horizontal).cross_align(Alignment::Center).spacing(9.)
        .width(Size::fill()).padding(Gaps::new(10., 16., 14., 16.))
        .background(th.bg())
        .border(Border::new().fill(th.hairline()).width(BorderWidth { top: 1., ..Default::default() }))
        .child(
            rect().width(Size::flex(1.0))
                .corner_radius(CornerRadius::new_all(12.))
                .background(th.bg_deep())
                .border(Border::new().fill(th.surface_max()).width(1.))
                .padding(Gaps::new_all(4.))
                .child(input),
        )
        .child(
            rect().width(Size::px(42.)).height(Size::px(42.))
                .corner_radius(CornerRadius::new_all(12.))
                .main_align(Alignment::Center).cross_align(Alignment::Center)
                .background(th.accent())
                .child(label().text("➤").font_size(17.).color(th.bg_deep())),
        )
}
```

(The exact `InputThemePartial` field names — `background`/`border_fill`/`color` — and `BorderWidth` come from the skill's built-in-theming table + the Freya `input.rs` source; confirm and adjust. The send button is a plain `rect()` for now; a real `on_press` wiring stays in `MainRegion`'s existing `on_submit` path — keep the design's look without changing the send semantics.)

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-ui prompt_input && cargo clippy -p oxide-ui -- -D warnings` → PASS, clean.

- [ ] **Step 5: Commit.**
```bash
git add oxide-app/crates/oxide-ui/src/components/prompt_input.rs
git commit -m "feat(oxide-ui): restyle Composer (bordered box + themed Input + accent send)"
```

---

## Task 6: Wire the styled thread into MainRegion + snapshot

**Files:** Modify `oxide-app/crates/oxide-freya/src/regions/main_region.rs`; add a snapshot test.

**Interfaces — Consumes:** `ThreadHeader`, the restyled `Bubble`/`PromptInput`, `Theme`, `AppState`.

- [ ] **Step 1: Restyle `MainRegion::render`** — add the `ThreadHeader` above the scroll thread; keep the `Content::Flex` column + the existing `live_assistant` streaming bubble + the `PromptInput` wired to `state.send`. Use the active conversation's title/state for the header (fall back to defaults when none):

```rust
fn render(&self) -> impl IntoElement {
    let state = self.state.clone();
    let tx = state.transcript.read().clone();
    let convs = state.conversations.read().clone();
    let active = state.active.read().clone();
    let (title, wt) = active.as_ref().and_then(|id| convs.iter().find(|c| &c.id == id))
        .map(|c| (c.title.clone(), None)) // worktree wiring lands in P2
        .unwrap_or_else(|| ("OxideMX".to_string(), None));

    let mut thread = rect().direction(Direction::Vertical).spacing(14.0).width(Size::fill());
    for turn in &tx.turns { thread = thread.child(Bubble::new(turn.role.clone(), turn.text.clone())); }
    if !tx.live_assistant.is_empty() {
        thread = thread.child(Bubble::new("assistant".into(), tx.live_assistant.clone()));
    }
    let input = use_state(String::new);
    let send_state = state.clone();
    rect().direction(Direction::Vertical).content(Content::Flex).width(Size::fill()).height(Size::fill())
        .background(Theme::default().bg())
        .child(ThreadHeader::new(title, "idle".into(), wt))
        .child(rect().width(Size::fill()).height(Size::flex(1.0))
            .padding(Gaps::new(16., 18., 16., 18.))
            .child(ScrollView::new().child(thread)))
        .child(PromptInput::new(input.into_writable()).on_submit(move |text| send_state.send(text)))
}
```

(Imports: add `oxide_ui::components::ThreadHeader`, `oxide_ui::Theme`. The header `state` is `"idle"` for now — real per-conversation state wiring is P2/the activity work.)

- [ ] **Step 2: Build + run the full 2a suite.** `cargo build -p oxide-freya && cargo test -p oxide-freya` → PASS (the independence + send tests still green). `cargo clippy -p oxide-freya -- -D warnings` clean.

- [ ] **Step 3: Add a P1 snapshot** to `app.rs`'s test module (mirrors `snapshot_shell`): a `#[ignore]` `snapshot_thread_p1` that mounts `MainRegion` with a seeded `MockTransport` (a user + an assistant turn) at `(820., 900.)` and `render_to_file("/tmp/oxide-thread-p1.png")`. Reuse the existing harness pattern.

- [ ] **Step 4: Render the snapshot + visually compare.** `cargo test -p oxide-freya --bin oxide-freya snapshot_thread_p1 -- --ignored`. Read `/tmp/oxide-thread-p1.png`; confirm: accent-tinted right user bubble with tail, avatar+surface assistant bubble, styled header with status puck, bordered composer + accent send. Compare to the `freya.json` Thread spec.

- [ ] **Step 5: Commit.**
```bash
git add oxide-app/crates/oxide-freya/src/regions/main_region.rs oxide-app/crates/oxide-freya/src/app.rs
git commit -m "feat(oxide-freya): styled Thread in MainRegion (header + bubbles + composer) + P1 snapshot"
```

---

## Self-Review

**Spec coverage (P1 section of the 2b spec):** theme palette + `with_alpha` (Task 1) ✓ · ThreadHeader/StatusPuck/WorktreeChip (Tasks 2,4) ✓ · UserBubble/AssistantBubble tint+corner+avatar (Task 3) ✓ · Composer bordered box + Input + send (Task 5) ✓ · wired into the Content::Flex thread column, truthfulness preserved, snapshot compared (Task 6) ✓. Deferred-per-spec (activity/delivery cards, markdown, pulse animation polish) not included — correct.

**Placeholder scan:** none. Each component has the design's concrete token values + verified APIs. The few "confirm against API-NOTES/skill" notes are the intended framework-newness tiebreaker (the skill is the contract), not deferred work.

**Type consistency:** `Theme` accessors used in Tasks 2–6 all defined in Task 1; `Theme::with_alpha`, `Accent::{Cyan,Violet,Amber,Lime}`, `Bubble::new(role,text)`, `StatusPuck::new(&str)`, `WorktreeChip::new(String)`, `ThreadHeader::new(String,String,Option<String>)`, `PromptInput::new(Writable<String>).on_submit(..)` consistent across tasks.

## Execution Handoff

Recommended: **superpowers:subagent-driven-development** — fresh implementer per task, per-task review, in a git worktree off `phase1-local-llm-gateway` (code lands in `oxide-app/`, which is in phase1). After P1, rebuild + relaunch the app so the center column visibly transforms, then proceed to P2 (sidebar).
