//! [`Mode`] — request routing modes with required-capability + sampling mappings.

use oxidemx_shared::config::{Capabilities, SamplingConfig};
use crate::guard::{Action, Check, GuardConfig, SchemaKind};
use serde::{Deserialize, Serialize};

/// Selects the operational intent of a [`crate::types::ChatRequest`].
///
/// Each variant maps to a required capability set and a sensible default
/// [`SamplingConfig`]. The [`Mode::guard_config`] method returns the
/// appropriate [`crate::guard::GuardConfig`] for post-inference checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Classify input into a discrete category; requires structured output.
    Classify,
    /// General conversational turn; no special capabilities required.
    Chat,
    /// Tool/function-calling turn.
    ToolUse,
    /// Web-search-augmented generation.
    WebSearch,
    /// Summarise, rewrite, or otherwise transform text.
    Transform,
    /// Vision (image-grounded) generation.
    Vision,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Chat
    }
}

impl Mode {
    /// Capability bits that the chosen model **must** advertise for this mode.
    pub fn required(&self) -> Capabilities {
        match self {
            Mode::Classify => Capabilities::STRUCTURED_OUTPUT,
            Mode::Chat => Capabilities::empty(),
            Mode::ToolUse => Capabilities::TOOLS,
            Mode::WebSearch => Capabilities::WEB_SEARCH,
            Mode::Transform => Capabilities::empty(),
            Mode::Vision => Capabilities::VISION,
        }
    }

    /// Default sampling parameters for this mode.
    ///
    /// Classify / Transform → low temperature (deterministic).
    /// Everything else → balanced.
    pub fn sampling(&self) -> SamplingConfig {
        match self {
            Mode::Classify | Mode::Transform => SamplingConfig {
                temperature: Some(0.1),
                top_p: Some(0.9),
                top_k: None,
                max_tokens: None,
            },
            _ => SamplingConfig {
                temperature: Some(0.7),
                top_p: Some(0.95),
                top_k: None,
                max_tokens: None,
            },
        }
    }

    /// Per-mode [`GuardConfig`].
    ///
    /// | Mode       | Checks                                              | Action       |
    /// |------------|-----------------------------------------------------|--------------|
    /// | Classify   | Schema(Json) + Grounding (OneOf injected per-call)  | Escalate     |
    /// | Transform  | NoNewFacts + TermPreservation + LengthBounds        | Escalate     |
    /// | Chat       | NonEmptyNonRefusal + Grounding                      | PassFlagged  |
    /// | ToolUse    | NonEmptyNonRefusal + Schema(Json)                   | Escalate     |
    /// | WebSearch  | NonEmptyNonRefusal + Grounding                      | Escalate     |
    /// | Vision     | NonEmptyNonRefusal                                  | Escalate     |
    ///
    /// **Note:** `Classify` returns `Schema(Json)` as a structural fallback.
    /// The real `Schema(OneOf { … })` check is supplied by the request handler
    /// at call-site, where the valid label list is known.
    pub fn guard_config(&self) -> GuardConfig {
        match self {
            Mode::Classify => GuardConfig {
                checks: vec![
                    // Caller provides the actual OneOf list at call-site; we
                    // use Json as a structural fallback here at the mode level.
                    // The real schema check is injected by the request handler.
                    Check::Schema(SchemaKind::Json),
                    Check::Grounding { min_overlap: 0.10 },
                ],
                action_on_fail: Action::Escalate,
            },
            Mode::Transform => GuardConfig {
                checks: vec![
                    Check::NoNewFacts,
                    Check::TermPreservation { min_keep: 0.5 },
                    Check::LengthBounds { max_ratio: 3.0 },
                ],
                action_on_fail: Action::Escalate,
            },
            Mode::Chat => GuardConfig {
                checks: vec![
                    Check::NonEmptyNonRefusal,
                    Check::Grounding { min_overlap: 0.05 },
                ],
                action_on_fail: Action::PassFlagged,
            },
            Mode::ToolUse => GuardConfig {
                checks: vec![
                    Check::NonEmptyNonRefusal,
                    Check::Schema(SchemaKind::Json),
                ],
                action_on_fail: Action::Escalate,
            },
            Mode::WebSearch => GuardConfig {
                checks: vec![
                    Check::NonEmptyNonRefusal,
                    Check::Grounding { min_overlap: 0.05 },
                ],
                action_on_fail: Action::Escalate,
            },
            Mode::Vision => GuardConfig {
                checks: vec![Check::NonEmptyNonRefusal],
                action_on_fail: Action::Escalate,
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_required_caps_and_default() {
        assert_eq!(Mode::default(), Mode::Chat);
        assert_eq!(Mode::Chat.required(), Capabilities::empty()); // non-mandatory
        assert_eq!(Mode::ToolUse.required(), Capabilities::TOOLS);
        assert_eq!(Mode::WebSearch.required(), Capabilities::WEB_SEARCH);
        assert_eq!(Mode::Vision.required(), Capabilities::VISION);
        assert_eq!(Mode::Classify.required(), Capabilities::STRUCTURED_OUTPUT);
        assert_eq!(Mode::Transform.required(), Capabilities::empty());
    }
}
