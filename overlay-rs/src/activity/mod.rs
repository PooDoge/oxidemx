//! Background-run activity bubbles for the standalone chat window (spec
//! `2026-06-20-agent-activity-bubbles-design.md`). Gated on `chat_window_mode`.

pub mod dock;
pub mod model;

pub use model::{AgentBubble, AgentTone, BubbleState, ClusterStatus, RunCluster, ToneKey};

use std::time::Instant;

/// A flattened, connector-agnostic view of one conductor run event, parsed
/// from the agentd `run`-kind payload (`variant` + `details`). Defaulted
/// fields are simply absent for variants that don't carry them.
#[derive(Debug, Clone, Default)]
pub struct RunEventView {
    pub run_id: String,
    pub conversation_id: String,
    pub variant: String,
    pub flow_id: String,
    pub steps: Vec<String>,
    pub step: String,
    pub agent: String,
    pub message: String,
    pub success: bool,
    pub artifact: Option<String>,
    pub summary: String,
    pub artifacts: Vec<String>,
    pub handoff: String,
}

#[derive(Debug, Default)]
pub struct ActivityState {
    pub clusters: Vec<RunCluster>,
    /// run_id of the currently-expanded cluster (None = all collapsed).
    pub expanded: Option<String>,
    /// (run_id, step) of the open peek popover, if any.
    pub peek: Option<(String, String)>,
}

impl ActivityState {
    pub fn cluster(&self, run_id: &str) -> Option<&RunCluster> {
        self.clusters.iter().find(|c| c.run_id == run_id)
    }
    fn cluster_mut(&mut self, run_id: &str) -> Option<&mut RunCluster> {
        self.clusters.iter_mut().find(|c| c.run_id == run_id)
    }
    fn peek_is(&self, run_id: &str, step: &str) -> bool {
        self.peek.as_ref().is_some_and(|(r, s)| r == run_id && s == step)
    }

    /// (active_or_just_finished, recent) split. A cluster is "recent" once it
    /// finished/failed/cancelled more than 6s ago. Recent is capped at 5
    /// (oldest dropped, logged — never silently truncated).
    pub fn partition(&self) -> (Vec<&RunCluster>, Vec<&RunCluster>) {
        let mut active = Vec::new();
        let mut recent = Vec::new();
        for c in &self.clusters {
            let is_recent = !matches!(c.status, ClusterStatus::Running)
                && c.finished_at.is_some_and(|t| t.elapsed().as_secs() >= 6);
            if is_recent { recent.push(c); } else { active.push(c); }
        }
        if recent.len() > 5 {
            let dropped = recent.len() - 5;
            tracing::debug!("activity: dropping {dropped} old recent run(s) past the cap of 5");
            recent.truncate(5);
        }
        (active, recent)
    }

    pub fn apply_run_event(&mut self, ev: &RunEventView) {
        match ev.variant.as_str() {
            "RunStarted" => {
                if self.cluster(&ev.run_id).is_none() {
                    self.clusters.push(RunCluster::new(
                        ev.run_id.clone(), ev.flow_id.clone(), ev.steps.clone(),
                    ));
                }
            }
            "TaskAssigned" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.agent = ev.agent.clone();
                        b.tone = AgentTone::for_agent(&ev.agent);
                    }
                }
            }
            "TaskStarted" => {
                let now = Instant::now();
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Working;
                        b.started_at.get_or_insert(now);
                    }
                }
            }
            "AgentMessage" => {
                let open = self.peek_is(&ev.run_id, &ev.step);
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.push_log(ev.message.clone());
                        if !open { b.unread = b.unread.saturating_add(1); }
                    }
                }
            }
            "TaskFinished" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = if ev.success { BubbleState::Done } else { BubbleState::Failed };
                        b.artifact = ev.artifact.clone();
                        b.summary = ev.summary.clone();
                    }
                }
            }
            "TaskError" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Failed;
                        b.push_log(format!("error: {}", ev.message));
                    }
                }
            }
            "StepRetrying" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Working;
                        b.push_log("retrying\u{2026}".into());
                    }
                }
            }
            "StepSkipped" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    if let Some(b) = c.bubble_mut(&ev.step) {
                        b.state = BubbleState::Skipped;
                    }
                }
            }
            "RunFinished" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    c.status = ClusterStatus::Finished;
                    c.artifacts = ev.artifacts.clone();
                    c.handoff = ev.handoff.clone();
                    c.finished_at = Some(Instant::now());
                }
            }
            "RunFailed" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    c.status = ClusterStatus::Failed;
                    c.finished_at = Some(Instant::now());
                }
            }
            "RunCancelled" => {
                if let Some(c) = self.cluster_mut(&ev.run_id) {
                    c.status = ClusterStatus::Cancelled;
                    c.finished_at = Some(Instant::now());
                }
            }
            // v1 ignores per-step approvals (spec §10).
            _ => {}
        }
    }
}

/// The assistant-message body posted to a conversation when its flow run ends.
/// Header line (flow · status · counts) + the full handoff markdown on success,
/// or the failure reason on failure/cancel. Artifact cards are rendered by the
/// chat from `RunEventView.artifacts` separately (Task 8).
pub fn delivery_message(v: &RunEventView) -> String {
    match v.variant.as_str() {
        "RunFinished" => {
            let n = v.artifacts.len();
            let head = format!("**{}** · ✓ · {n} artifact(s)", v.flow_id);
            if v.handoff.trim().is_empty() {
                format!("{head}\n\n_(flow produced no inline answer; see artifacts)_")
            } else {
                format!("{head}\n\n{}", v.handoff)
            }
        }
        "RunFailed" => format!("**{}** · ✗ failed\n\n{}", v.flow_id,
            if v.message.is_empty() { "(no reason reported)".into() } else { v.message.clone() }),
        "RunCancelled" => format!("**{}** · ⊘ cancelled", v.flow_id),
        _ => String::new(),
    }
}

#[cfg(test)]
mod reducer_tests {
    use super::*;

    fn ev(variant: &str) -> RunEventView {
        RunEventView { run_id: "run-1".into(), variant: variant.into(), ..Default::default() }
    }

    #[test]
    fn full_lifecycle_builds_cluster() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView {
            steps: vec!["a".into(), "b".into()], flow_id: "research".into(), ..ev("RunStarted")
        });
        assert_eq!(s.clusters.len(), 1);
        assert_eq!(s.cluster("run-1").unwrap().bubbles.len(), 2);

        s.apply_run_event(&RunEventView { step: "a".into(), agent: "web-researcher".into(), ..ev("TaskAssigned") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].tone.0, ToneKey::Blue));

        s.apply_run_event(&RunEventView { step: "a".into(), ..ev("TaskStarted") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Working));

        s.apply_run_event(&RunEventView { step: "a".into(), message: "google_search · x".into(), ..ev("AgentMessage") });
        assert_eq!(s.cluster("run-1").unwrap().bubbles[0].logs.len(), 1);
        assert_eq!(s.cluster("run-1").unwrap().bubbles[0].unread, 1);

        s.apply_run_event(&RunEventView {
            step: "a".into(), success: true, artifact: Some("/tmp/out.md".into()), summary: "ok".into(), ..ev("TaskFinished")
        });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Done));
        assert_eq!(s.cluster("run-1").unwrap().progress(), 0.5);

        s.apply_run_event(&RunEventView {
            artifacts: vec!["/tmp/out.md".into()], handoff: "# done".into(), ..ev("RunFinished")
        });
        assert!(matches!(s.cluster("run-1").unwrap().status, ClusterStatus::Finished));
    }

    #[test]
    fn unknown_step_and_unknown_run_are_ignored() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { step: "ghost".into(), ..ev("TaskStarted") }); // no cluster yet
        assert!(s.clusters.is_empty());
        s.apply_run_event(&RunEventView { steps: vec!["a".into()], ..ev("RunStarted") });
        s.apply_run_event(&RunEventView { step: "ghost".into(), ..ev("TaskStarted") }); // unknown step
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Pending));
    }

    #[test]
    fn failure_and_skip_and_cancel() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { steps: vec!["a".into(), "b".into()], ..ev("RunStarted") });
        s.apply_run_event(&RunEventView { step: "a".into(), message: "boom".into(), ..ev("TaskError") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[0].state, BubbleState::Failed));
        s.apply_run_event(&RunEventView { step: "b".into(), ..ev("StepSkipped") });
        assert!(matches!(s.cluster("run-1").unwrap().bubbles[1].state, BubbleState::Skipped));
        s.apply_run_event(&ev("RunCancelled"));
        assert!(matches!(s.cluster("run-1").unwrap().status, ClusterStatus::Cancelled));
    }

    #[test]
    fn unread_does_not_bump_while_peek_open() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { steps: vec!["a".into()], ..ev("RunStarted") });
        s.peek = Some(("run-1".into(), "a".into()));
        s.apply_run_event(&RunEventView { step: "a".into(), message: "m".into(), ..ev("AgentMessage") });
        assert_eq!(s.cluster("run-1").unwrap().bubbles[0].unread, 0);
    }

    #[test]
    fn partition_splits_active_and_recent() {
        use std::time::{Duration, Instant};
        let mut s = ActivityState::default();
        // active: still running
        s.apply_run_event(&RunEventView { run_id: "r-run".into(), variant: "RunStarted".into(), steps: vec!["a".into()], ..Default::default() });
        // just finished (active — <6s)
        s.apply_run_event(&RunEventView { run_id: "r-new".into(), variant: "RunStarted".into(), steps: vec!["a".into()], ..Default::default() });
        s.apply_run_event(&RunEventView { run_id: "r-new".into(), variant: "RunFinished".into(), ..Default::default() });
        // old finished (recent — >6s)
        s.apply_run_event(&RunEventView { run_id: "r-old".into(), variant: "RunStarted".into(), steps: vec!["a".into()], ..Default::default() });
        s.apply_run_event(&RunEventView { run_id: "r-old".into(), variant: "RunFinished".into(), ..Default::default() });
        if let Some(c) = s.clusters.iter_mut().find(|c| c.run_id == "r-old") {
            c.finished_at = Some(Instant::now() - Duration::from_secs(7));
        }
        let (active, recent) = s.partition();
        assert_eq!(active.len(), 2, "running + just-finished are active");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].run_id, "r-old");
    }

    #[test]
    fn approval_requested_is_ignored() {
        let mut s = ActivityState::default();
        s.apply_run_event(&RunEventView { steps: vec!["a".into()], ..ev("RunStarted") });
        let before = format!("{:?}", s.cluster("run-1").unwrap().bubbles[0].state);
        s.apply_run_event(&RunEventView { step: "a".into(), ..ev("ApprovalRequested") });
        let after = format!("{:?}", s.cluster("run-1").unwrap().bubbles[0].state);
        assert_eq!(before, after); // no state change (v1 ignores approvals)
    }

    #[test]
    fn delivered_message_has_header_and_handoff() {
        let v = RunEventView {
            variant: "RunFinished".into(), flow_id: "doc-digest".into(),
            handoff: "# Answer\nkey points".into(),
            artifacts: vec!["ANSWER.md".into(), "debug/digest.md".into()],
            ..Default::default()
        };
        let body = delivery_message(&v);
        assert!(body.starts_with("**doc-digest** · ✓"));
        assert!(body.contains("# Answer"));
        let f = RunEventView { variant: "RunFailed".into(), flow_id: "x".into(),
            message: "step boom".into(), ..Default::default() };
        assert!(delivery_message(&f).contains("✗"));
        assert!(delivery_message(&f).contains("step boom"));
    }
}
