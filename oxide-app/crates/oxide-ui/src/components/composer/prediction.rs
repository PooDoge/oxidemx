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
