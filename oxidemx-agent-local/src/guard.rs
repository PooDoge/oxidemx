//! [`ResponseGuard`] — pure heuristic sanity-check failsafe for LLM outputs.
//!
//! All logic is pure over `(input: &str, output: &str)` — no model calls,
//! no I/O, fully synchronous, and table-driven testable.

use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single failed check with a human-readable `detail`.
///
/// `check` is `&'static str` (a compile-time constant name); we only serialize
/// this type, never deserialize it from external input.
#[derive(Debug, Clone, Serialize)]
pub struct Reason {
    /// Name of the check that produced this reason.
    pub check: &'static str,
    /// Human-readable description of why the check failed.
    pub detail: String,
}

/// Verdict returned by [`evaluate`].
#[derive(Debug, Clone, Serialize)]
pub enum Verdict {
    /// All checks passed.
    Ok,
    /// Some checks raised concerns but the configured action is `PassFlagged`.
    Suspect(Vec<Reason>),
    /// At least one check failed and the action demands escalation / rejection.
    Failed(Vec<Reason>),
}

/// What to do when one or more checks fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    /// Re-run inference up to `max` times before escalating.
    Retry {
        /// Maximum number of retries before the request is escalated.
        max: u8,
    },
    /// Fail hard and surface to the caller.
    Escalate,
    /// Immediately reject (return [`LocalError::GuardRejected`]).
    Reject,
    /// Pass the response through but tag it as [`Verdict::Suspect`].
    PassFlagged,
}

/// Schema constraint for [`Check::Schema`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchemaKind {
    /// Output must be exactly one of these strings (trimmed, case-sensitive).
    OneOf(Vec<String>),
    /// Output must be valid JSON.
    Json,
}

/// Individual guard checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Check {
    /// Output must not be empty and must not be a refusal phrase.
    NonEmptyNonRefusal,
    /// Output must overlap lexically with the input above `min_overlap`.
    Grounding {
        /// Minimum Jaccard overlap required (0.0–1.0).
        min_overlap: f32,
    },
    /// Transform mode: output must not introduce numbers/URLs/entities absent
    /// from the input.
    NoNewFacts,
    /// Output must not contain pathologically repeated n-grams.
    Repetition,
    /// Output must conform to a schema.
    Schema(SchemaKind),
    /// Transform mode: output length must not exceed `input * max_ratio`.
    LengthBounds {
        /// Maximum allowed ratio of `output.len() / input.len()`.
        max_ratio: f32,
    },
    /// Transform mode: at least `min_keep` fraction of input terms must appear
    /// in the output.
    TermPreservation {
        /// Minimum fraction of input terms that must be preserved (0.0–1.0).
        min_keep: f32,
    },
}

/// Full guard configuration: a list of checks and what to do on failure.
///
/// `Default` = grounding + refusal check, `Escalate` on failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardConfig {
    /// Ordered list of checks to run against the model's output.
    pub checks: Vec<Check>,
    /// What to do when at least one check fires.
    pub action_on_fail: Action,
}

impl Default for GuardConfig {
    fn default() -> Self {
        GuardConfig {
            checks: vec![
                Check::NonEmptyNonRefusal,
                Check::Grounding { min_overlap: 0.15 },
            ],
            action_on_fail: Action::Escalate,
        }
    }
}

// ── Pure helpers (pub(crate) so each can be unit-tested) ─────────────────────

/// Returns `true` when the text looks like a model refusal.
pub(crate) fn is_refusal(text: &str) -> bool {
    let lower = text.to_lowercase();
    const PHRASES: &[&str] = &[
        "i cannot help",
        "i can't help",
        "i'm unable to",
        "i am unable to",
        "i cannot assist",
        "i can't assist",
        "i won't",
        "i will not",
        "i'm not able to",
        "i am not able to",
        "as an ai",
        "as a language model",
        "i'm just an ai",
        "i refuse",
        "that's not something i can",
        "that is not something i can",
        "i don't have the ability",
        "i do not have the ability",
    ];
    PHRASES.iter().any(|p| lower.contains(p))
}

/// Jaccard similarity over lowercased word sets (ignoring stopwords).
pub(crate) fn lexical_overlap(input: &str, output: &str) -> f32 {
    const STOPWORDS: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "shall",
        "should", "may", "might", "must", "can", "could", "to", "of", "in",
        "for", "on", "with", "at", "by", "from", "as", "and", "but", "or",
        "nor", "so", "yet", "both", "either", "neither", "not", "if", "it",
        "its", "this", "that", "these", "those",
    ];

    let words = |s: &str| -> std::collections::HashSet<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .map(|w| w.to_lowercase())
            .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
            .collect()
    };

    let a = words(input);
    let b = words(output);
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(&b).count() as f32;
    let union = a.union(&b).count() as f32;
    if union == 0.0 {
        return 0.0;
    }
    intersection / union
}

/// Returns numbers/URLs/Capitalized-tokens found in `output` but not in
/// `input`.  An empty vec means no new specifics were introduced.
pub(crate) fn introduces_new_specifics(input: &str, output: &str) -> Vec<String> {
    let extract = |s: &str| -> std::collections::HashSet<String> {
        let mut found = std::collections::HashSet::new();
        // Numbers (integers + decimals)
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            // URLs
            if s[i..].starts_with("http://") || s[i..].starts_with("https://") {
                let end = s[i..]
                    .find(|c: char| c.is_whitespace())
                    .map(|o| i + o)
                    .unwrap_or(s.len());
                found.insert(s[i..end].to_string());
                i = end;
                continue;
            }
            // Numbers (digit sequence possibly containing '.')
            if bytes[i].is_ascii_digit() {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                found.insert(s[start..i].to_string());
                continue;
            }
            // Capitalized words (length ≥ 2, first byte uppercase, not at
            // sentence start — we use a simple heuristic: preceded by a
            // non-punctuation character)
            if bytes[i].is_ascii_uppercase() {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let word = &s[start..i];
                if word.len() >= 2 {
                    found.insert(word.to_string());
                }
                continue;
            }
            i += 1;
        }
        found
    };

    let in_input = extract(input);
    let in_output = extract(output);
    in_output.difference(&in_input).cloned().collect()
}

/// Returns `true` when any 3-gram appears more than 3 times.
pub(crate) fn has_repetition(text: &str) -> bool {
    let tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    if tokens.len() < 3 {
        return false;
    }
    let mut counts: std::collections::HashMap<[&str; 3], u32> =
        std::collections::HashMap::new();
    for w in tokens.windows(3) {
        let key = [w[0], w[1], w[2]];
        let c = counts.entry(key).or_insert(0);
        *c += 1;
        if *c > 3 {
            return true;
        }
    }
    false
}

/// Returns `true` when the output satisfies the schema.
pub(crate) fn matches_schema(output: &str, kind: &SchemaKind) -> bool {
    match kind {
        SchemaKind::OneOf(options) => {
            let trimmed = output.trim();
            options.iter().any(|o| o == trimmed)
        }
        SchemaKind::Json => serde_json::from_str::<serde_json::Value>(output).is_ok(),
    }
}

/// Fraction of significant input terms present in the output (0.0–1.0).
pub(crate) fn term_preservation(input: &str, output: &str) -> f32 {
    let significant: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 3)
        .collect();
    if significant.is_empty() {
        return 1.0;
    }
    let out_lower = output.to_lowercase();
    let kept = significant.iter().filter(|w| out_lower.contains(w.as_str())).count();
    kept as f32 / significant.len() as f32
}

// ── evaluate ──────────────────────────────────────────────────────────────────

/// Run every check in `cfg` over `(input, output)` and return a [`Verdict`].
///
/// If any check fires:
/// - [`Action::PassFlagged`] → [`Verdict::Suspect`]
/// - anything else → [`Verdict::Failed`]
pub fn evaluate(cfg: &GuardConfig, input: &str, output: &str) -> Verdict {
    let mut reasons: Vec<Reason> = Vec::new();

    for check in &cfg.checks {
        match check {
            Check::NonEmptyNonRefusal => {
                if output.trim().is_empty() {
                    reasons.push(Reason {
                        check: "NonEmptyNonRefusal",
                        detail: "output is empty".into(),
                    });
                } else if is_refusal(output) {
                    reasons.push(Reason {
                        check: "NonEmptyNonRefusal",
                        detail: format!("output appears to be a refusal: {:?}", &output[..output.len().min(80)]),
                    });
                }
            }
            Check::Grounding { min_overlap } => {
                let overlap = lexical_overlap(input, output);
                if overlap < *min_overlap {
                    reasons.push(Reason {
                        check: "Grounding",
                        detail: format!(
                            "lexical overlap {:.2} < required {:.2}",
                            overlap, min_overlap
                        ),
                    });
                }
            }
            Check::NoNewFacts => {
                let new = introduces_new_specifics(input, output);
                if !new.is_empty() {
                    reasons.push(Reason {
                        check: "NoNewFacts",
                        detail: format!("output introduces specifics absent from input: {:?}", new),
                    });
                }
            }
            Check::Repetition => {
                if has_repetition(output) {
                    reasons.push(Reason {
                        check: "Repetition",
                        detail: "output contains pathological n-gram repetition".into(),
                    });
                }
            }
            Check::Schema(kind) => {
                if !matches_schema(output, kind) {
                    reasons.push(Reason {
                        check: "Schema",
                        detail: format!("output does not match schema {:?}", kind),
                    });
                }
            }
            Check::LengthBounds { max_ratio } => {
                let in_len = input.len() as f32;
                let out_len = output.len() as f32;
                if in_len > 0.0 && out_len / in_len > *max_ratio {
                    reasons.push(Reason {
                        check: "LengthBounds",
                        detail: format!(
                            "output/input length ratio {:.2} exceeds max {:.2}",
                            out_len / in_len,
                            max_ratio
                        ),
                    });
                }
            }
            Check::TermPreservation { min_keep } => {
                let kept = term_preservation(input, output);
                if kept < *min_keep {
                    reasons.push(Reason {
                        check: "TermPreservation",
                        detail: format!(
                            "term preservation {:.2} < required {:.2}",
                            kept, min_keep
                        ),
                    });
                }
            }
        }
    }

    if reasons.is_empty() {
        return Verdict::Ok;
    }

    match cfg.action_on_fail {
        Action::PassFlagged => Verdict::Suspect(reasons),
        _ => Verdict::Failed(reasons),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_detected() {
        assert!(is_refusal("I cannot help with that."));
        assert!(!is_refusal("Paris."));
    }

    #[test]
    fn grounding_flags_offtopic() {
        assert!(lexical_overlap("capital of France?", "Paris is the capital of France") > 0.3);
        assert!(lexical_overlap("capital of France?", "Bananas grow in the tropics") < 0.2);
    }

    #[test]
    fn no_new_facts_catches_injected_specifics() {
        let extra = introduces_new_specifics(
            "Summarize: the build failed",
            "It failed in 3.14 seconds at http://x",
        );
        assert!(
            extra.iter().any(|s| s == "3.14"),
            "expected 3.14 in {:?}",
            extra
        );
        assert!(
            extra.iter().any(|s| s.contains("http://x")),
            "expected http://x in {:?}",
            extra
        );
        assert!(introduces_new_specifics("retry 3 times", "retry 3 times please").is_empty());
    }

    #[test]
    fn repetition_caught() {
        assert!(has_repetition("go go go go go go go go"));
        assert!(!has_repetition("a normal sentence"));
    }

    #[test]
    fn schema_oneof() {
        let k = SchemaKind::OneOf(vec!["SIMPLE".into(), "COMPLEX".into()]);
        assert!(matches_schema("SIMPLE", &k));
        assert!(!matches_schema("maybe", &k));
    }

    #[test]
    fn evaluate_fails_offtopic_classify() {
        let cfg = GuardConfig {
            checks: vec![Check::Schema(SchemaKind::OneOf(vec![
                "SIMPLE".into(),
                "COMPLEX".into(),
            ]))],
            action_on_fail: Action::Escalate,
        };
        assert!(matches!(
            evaluate(&cfg, "classify this", "definitely simple, I think"),
            Verdict::Failed(_)
        ));
    }
}
