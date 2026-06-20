//! Background-run activity bubbles for the standalone chat window (spec
//! `2026-06-20-agent-activity-bubbles-design.md`). Gated on `chat_window_mode`.

pub mod model;

pub use model::{AgentBubble, AgentTone, BubbleState, ClusterStatus, RunCluster, ToneKey};
