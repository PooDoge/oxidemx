//! Activity-bubble data model: a run is a cluster, each step is a bubble.
//! Pure data — no iced widgets here (those live in `dock.rs`). Colors are
//! resolved from the live `Kit` at render time; never hardcode hex.

use std::time::Instant;
use oxidemx_widgets::kit::Kit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleState { Pending, Working, Done, Failed, Skipped }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterStatus { Running, Finished, Failed, Cancelled }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneKey { Blue, Peach, Mauve, Teal, Green, Accent }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTone(pub ToneKey);

impl AgentTone {
    /// Map an agent/archetype name to a slice tone (substring match, lowercased).
    pub fn for_agent(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        let key = if n.contains("research") || n.contains("fact") || n.contains("source") {
            ToneKey::Blue
        } else if n.contains("shell") || n.contains("index") || n.contains("repo") || n.contains("ops") {
            ToneKey::Peach
        } else if n.contains("writ") || n.contains("draft") || n.contains("author") {
            ToneKey::Mauve
        } else if n.contains("summ") || n.contains("digest") || n.contains("condense") {
            ToneKey::Teal
        } else if n.contains("brows") || n.contains("web") || n.contains("fetch") {
            ToneKey::Green
        } else {
            ToneKey::Accent
        };
        AgentTone(key)
    }

    pub fn icon(&self) -> &'static str {
        match self.0 {
            ToneKey::Blue => "search",
            ToneKey::Peach => "terminal",
            ToneKey::Mauve => "pencil",
            ToneKey::Teal => "clipboard",
            ToneKey::Green => "globe",
            ToneKey::Accent => "sparkle",
        }
    }

    pub fn color(&self, kit: &Kit) -> iced::Color {
        match self.0 {
            ToneKey::Blue => kit.blue,
            ToneKey::Peach => kit.peach,
            ToneKey::Mauve => kit.mauve,
            ToneKey::Teal => kit.teal,
            ToneKey::Green => kit.green,
            ToneKey::Accent => kit.accent,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentBubble {
    pub step: String,
    pub agent: String,
    pub state: BubbleState,
    pub tone: AgentTone,
    pub logs: Vec<String>,
    pub unread: u32,
    pub artifact: Option<String>,
    pub summary: String,
    pub started_at: Option<Instant>,
}

impl AgentBubble {
    pub fn new(step: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            agent: String::new(),
            state: BubbleState::Pending,
            tone: AgentTone(ToneKey::Accent),
            logs: Vec::new(),
            unread: 0,
            artifact: None,
            summary: String::new(),
            started_at: None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, BubbleState::Done | BubbleState::Failed | BubbleState::Skipped)
    }

    /// Append a log line, keeping the tail bounded (the peek shows the last ~6;
    /// 40 retained). Used by every event that writes to the log.
    pub fn push_log(&mut self, line: String) {
        self.logs.push(line);
        if self.logs.len() > 40 { self.logs.remove(0); }
    }
}

#[derive(Debug, Clone)]
pub struct RunCluster {
    pub run_id: String,
    pub flow_id: String,
    pub status: ClusterStatus,
    pub bubbles: Vec<AgentBubble>,
    pub artifacts: Vec<String>,
    pub handoff: String,
    pub finished_at: Option<Instant>,
}

impl RunCluster {
    pub fn new(run_id: impl Into<String>, flow_id: impl Into<String>, steps: Vec<String>) -> Self {
        Self {
            run_id: run_id.into(),
            flow_id: flow_id.into(),
            status: ClusterStatus::Running,
            bubbles: steps.into_iter().map(AgentBubble::new).collect(),
            artifacts: Vec::new(),
            handoff: String::new(),
            finished_at: None,
        }
    }

    pub fn bubble_mut(&mut self, step: &str) -> Option<&mut AgentBubble> {
        self.bubbles.iter_mut().find(|b| b.step == step)
    }

    pub fn progress(&self) -> f32 {
        if self.bubbles.is_empty() { return 0.0; }
        let done = self.bubbles.iter().filter(|b| b.is_terminal()).count();
        done as f32 / self.bubbles.len() as f32
    }

    pub fn running_count(&self) -> usize {
        self.bubbles.iter().filter(|b| matches!(b.state, BubbleState::Working)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_maps_known_archetypes() {
        assert!(matches!(AgentTone::for_agent("web-researcher").0, ToneKey::Blue));
        assert!(matches!(AgentTone::for_agent("shell-op").0, ToneKey::Peach));
        assert!(matches!(AgentTone::for_agent("writer").0, ToneKey::Mauve));
        assert!(matches!(AgentTone::for_agent("summarizer").0, ToneKey::Teal));
        assert!(matches!(AgentTone::for_agent("browser").0, ToneKey::Green));
        assert!(matches!(AgentTone::for_agent("coordinator").0, ToneKey::Accent));
        // unknown → Accent (default)
        assert!(matches!(AgentTone::for_agent("totally-unknown").0, ToneKey::Accent));
    }

    #[test]
    fn progress_counts_terminal_bubbles() {
        let mut c = RunCluster::new("run-1", "flow-x", vec!["a".into(), "b".into()]);
        assert_eq!(c.progress(), 0.0);
        c.bubbles[0].state = BubbleState::Done;
        assert_eq!(c.progress(), 0.5);
    }
}
