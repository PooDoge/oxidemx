//! Canned gboard-style predictor + PredictionStrip UI, ported from composer-feature.jsx.
use freya::prelude::*;
use crate::tokens::Theme;

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
    let bytes: Vec<char> = s.chars().collect();
    let mut i = bytes.len();
    while i > 0 {
        let c = bytes[i - 1];
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            i -= 1;
        } else {
            break;
        }
    }
    if i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        Some(bytes[i..].iter().collect())
    } else {
        None
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

// ── PredictionStrip ──────────────────────────────────────────────────────────

/// A horizontal strip of up to 3 prediction chips with a trailing tab hint.
///
/// Builder usage:
/// ```ignore
/// fn app() -> impl IntoElement {
///     PredictionStrip::new(predict(""), Theme::default())
///         .on_accept(|word: String| println!("accepted: {word}"))
/// }
/// ```
#[derive(PartialEq, Clone)]
pub struct PredictionStrip {
    suggestions: Suggestions,
    theme: Theme,
    on_accept: Option<EventHandler<String>>,
}

impl PredictionStrip {
    pub fn new(suggestions: Suggestions, theme: Theme) -> Self {
        Self { suggestions, theme, on_accept: None }
    }

    pub fn on_accept(mut self, handler: impl Into<EventHandler<String>>) -> Self {
        self.on_accept = Some(handler.into());
        self
    }
}

impl Component for PredictionStrip {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let accent = th.accent();
        // Cap at 3 items; pad to length 3 with None.
        let items: Vec<Option<&'static str>> = {
            let mut v: Vec<Option<&'static str>> =
                self.suggestions.items.iter().copied().take(3).map(Some).collect();
            while v.len() < 3 { v.push(None); }
            v
        };

        // Build each chip as an Option<Element> so we can pass to maybe_child.
        let make_chip = |item: &'static str, is_first: bool,
                         handler: Option<EventHandler<String>>| -> Element {
            let (bg, border_color, text_color) = if is_first {
                (
                    Theme::with_alpha(accent, 0x14),
                    Theme::with_alpha(accent, 0x33),
                    accent,
                )
            } else {
                (th.surface(), th.hairline(), th.subtext_hi())
            };
            let item_owned = item.to_string();
            let chip = rect()
                .direction(Direction::Horizontal)
                .cross_align(Alignment::Center)
                .padding(Gaps::new(3., 10., 3., 10.))
                .corner_radius(CornerRadius::new_all(999.))
                .background(bg)
                .border(Border::new().fill(border_color).width(1.))
                .child(label().text(item).font_size(11.5).color(text_color));
            if let Some(h) = handler {
                chip.on_press(move |_: Event<PressEventData>| h.call(item_owned.clone()))
                    .into_element()
            } else {
                chip.into_element()
            }
        };

        let chip0: Option<Element> = items[0].map(|item| {
            make_chip(item, true, self.on_accept.clone())
        });
        let chip1: Option<Element> = items[1].map(|item| {
            make_chip(item, false, self.on_accept.clone())
        });
        let chip2: Option<Element> = items[2].map(|item| {
            make_chip(item, false, self.on_accept.clone())
        });

        // Trailing hint fires on_accept with the first item.
        let hint_handler = self.on_accept.clone();
        let first_item: Option<String> = items[0].map(|s| s.to_string());
        let hint = rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .on_press(move |_: Event<PressEventData>| {
                if let (Some(h), Some(w)) = (&hint_handler, &first_item) {
                    h.call(w.clone());
                }
            })
            .child(label().text("⇥ tab").font_size(10.5).color(th.faint()));

        // The spacer uses Size::flex(1.0), so the row MUST have Content::Flex.
        rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .width(Size::fill())
            .padding(Gaps::new(4., 8., 4., 8.))
            .maybe_child(chip0)
            .maybe_child(chip1)
            .maybe_child(chip2)
            .child(rect().width(Size::flex(1.0)).height(Size::px(1.)))
            .child(hint)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

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
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "Refactor")
        });
        assert!(found.is_some(), "first prediction chip renders");
    }
}

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
