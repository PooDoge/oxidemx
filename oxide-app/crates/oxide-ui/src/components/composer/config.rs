//! Composer tweak config + the provider/model registry.
use crate::tokens::Tone;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Prediction { #[default] Chips, Ghost, Off }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Thinking { Low, #[default] Medium, High }

impl Thinking {
    pub fn badge(self) -> &'static str { match self { Thinking::Low => "L", Thinking::Medium => "M", Thinking::High => "H" } }
    /// Lowercase word for the thinking level (used in the ActivityLine status text).
    pub fn word(self) -> &'static str { match self { Thinking::Low => "low", Thinking::Medium => "medium", Thinking::High => "high" } }
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn thinking_word_is_lowercase() {
        assert_eq!(Thinking::Low.word(), "low");
        assert_eq!(Thinking::Medium.word(), "medium");
        assert_eq!(Thinking::High.word(), "high");
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
}
