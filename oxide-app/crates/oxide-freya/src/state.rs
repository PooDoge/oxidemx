//! Bridges async transport calls + the SSE stream into Freya signals the
//! regions render. The streaming reducer (`Transcript`) is pure + unit-tested;
//! `AppState` wraps it in signals and spawns the async I/O.
//!
//! # Truthfulness invariant (Rule 1)
//!
//! `Transcript` is the ONLY place assistant text is assembled.
//! Assistant content is only ever appended from `delta`/`final` events —
//! never fabricated by any other path.
use std::collections::HashMap;
use std::sync::Arc;

use freya::prelude::*;
use oxide_client::{AgentEvent, Conversation, ConversationId, Project, ProjectId, Transport, Turn};
use oxide_client::dto::AttachmentPayload;

use crate::conversation_meta::ConvMeta;

// ── Connection state ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnState {
    Unknown,
    Connected,
    Reconnecting,
    Unreachable,
}

// ── Status directions (right-panel facets) ──────────────────────────────────

/// The four right-panel "directions". Named `StatusDirection` to avoid colliding
/// with Freya's layout `Direction`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StatusDirection {
    #[default]
    Spec,
    Mission,
    Workbench,
    Ambient,
}

impl StatusDirection {
    pub const ALL: [StatusDirection; 4] =
        [Self::Spec, Self::Mission, Self::Workbench, Self::Ambient];

    pub fn label(self) -> &'static str {
        match self {
            Self::Spec => "Spec",
            Self::Mission => "Mission",
            Self::Workbench => "Workbench",
            Self::Ambient => "Ambient",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Spec => "S",
            Self::Mission => "M",
            Self::Workbench => "W",
            Self::Ambient => "A",
        }
    }
}

/// Which right-panel tab is active (Run / Worktree / .oxide settings).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RightTab {
    #[default]
    Run,
    Worktree,
    Settings,
}

/// Responsive size class derived from the LOGICAL window width (design's four
/// breakpoints). `Compact` and narrower force both side panels to icon rails.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SizeClass {
    #[default]
    Wide,
    Compact,
    Tablet,
    Phone,
}

impl SizeClass {
    pub fn from_logical_width(w: f32) -> SizeClass {
        if w >= 1180.0 {
            SizeClass::Wide
        } else if w >= 920.0 {
            SizeClass::Compact
        } else if w >= 600.0 {
            SizeClass::Tablet
        } else {
            SizeClass::Phone
        }
    }

    pub fn is_compact_or_narrower(self) -> bool {
        !matches!(self, SizeClass::Wide)
    }
}

// ── Pure streaming reducer ──────────────────────────────────────────────────

/// Pure streaming reducer — the single place assistant text is assembled.
///
/// `apply_user` appends a user turn and clears `live_assistant`.
/// `apply_event` appends to `live_assistant` on `delta` and commits an
/// assistant `Turn` (clearing `live_assistant`) on `final`.
/// Nothing else touches assistant text — the truthfulness invariant is
/// structural, not aspirational.
#[derive(Default, Clone, PartialEq)]
pub struct Transcript {
    pub turns: Vec<Turn>,
    /// In-flight assistant text (the streaming "bubble").  Empty between turns.
    pub live_assistant: String,
}

impl Transcript {
    /// Append a user message and reset the streaming bubble.
    pub fn apply_user(&mut self, text: String) {
        self.turns.push(Turn { role: "user".into(), text, ts: 0 });
        self.live_assistant.clear();
    }

    /// Process one SSE event.
    ///
    /// - `delta` — append `text` to `live_assistant`.
    /// - `final` — commit an assistant `Turn` (text from event or accumulated
    ///   `live_assistant`) and clear the bubble.
    /// - anything else — ignored (activity/tool/card events are not rendered in 2a).
    pub fn apply_event(&mut self, ev: &AgentEvent) {
        match ev.kind.as_str() {
            "delta" => {
                if let Some(t) = ev.text() {
                    self.live_assistant.push_str(t);
                }
            }
            "final" => {
                // ev.text() is canonical; accumulated live_assistant is intentionally
                // discarded when the final event provides its own text field.
                let text = ev
                    .text()
                    .map(str::to_string)
                    .unwrap_or_else(|| std::mem::take(&mut self.live_assistant));
                self.turns.push(Turn { role: "assistant".into(), text, ts: 0 });
                self.live_assistant.clear();
            }
            _ => {} // activity/tool/card — not rendered in 2a
        }
    }
}

// ── AppState ────────────────────────────────────────────────────────────────

/// Freya signal bridge to the agentd transport.
///
/// Must be constructed inside a Freya component (calls `use_state`).
/// Call `bootstrap` once on mount; call `open_conversation` and `send`
/// from event handlers.
#[derive(Clone)]
pub struct AppState {
    transport: Arc<dyn Transport>,
    pub projects: State<Vec<Project>>,
    pub conversations: State<Vec<Conversation>>,
    pub active: State<Option<ConversationId>>,
    pub transcript: State<Transcript>,
    pub connection: State<ConnState>,
    pub current_project: State<Option<ProjectId>>,
    pub sidebar_collapsed: State<bool>,
    pub context_collapsed: State<bool>,
    /// Retained for slice 2 (rail agent-status visual style); no longer selects
    /// right-panel content.
    pub active_direction: State<StatusDirection>,
    pub right_tab: State<RightTab>,
    pub size_class: State<SizeClass>,
    pub conversation_meta: State<HashMap<String, ConvMeta>>,
}

impl PartialEq for AppState {
    /// Two `AppState` handles are equal when they share the same underlying
    /// signals — i.e., they were created in the same component invocation.
    fn eq(&self, other: &Self) -> bool {
        self.projects == other.projects
            && self.conversations == other.conversations
            && self.active == other.active
            && self.transcript == other.transcript
            && self.connection == other.connection
            && self.current_project == other.current_project
            && self.sidebar_collapsed == other.sidebar_collapsed
            && self.context_collapsed == other.context_collapsed
            && self.active_direction == other.active_direction
            && self.right_tab == other.right_tab
            && self.size_class == other.size_class
            && self.conversation_meta == other.conversation_meta
    }
}

impl AppState {
    /// Create `AppState`.  Must be called from inside a Freya component body
    /// (same restriction as `use_state`).
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            projects: use_state(Vec::new),
            conversations: use_state(Vec::new),
            active: use_state(|| None),
            transcript: use_state(Transcript::default),
            connection: use_state(|| ConnState::Unknown),
            current_project: use_state(|| None),
            sidebar_collapsed: use_state(|| false),
            context_collapsed: use_state(|| true),
            active_direction: use_state(StatusDirection::default),
            right_tab: use_state(RightTab::default),
            size_class: use_state(SizeClass::default),
            conversation_meta: use_state(HashMap::new),
        }
    }

    /// Health-check → load projects → load personal project's conversations.
    ///
    /// Call once on component mount (e.g. from `use_future`).
    pub fn bootstrap(&self) {
        let t = self.transport.clone();
        let mut projects = self.projects;
        let mut conversations = self.conversations;
        let mut connection = self.connection;
        let mut current_project = self.current_project;
        let this = self.clone();
        spawn(async move {
            match t.health().await {
                Ok(()) => connection.set(ConnState::Connected),
                Err(_) => {
                    connection.set(ConnState::Unreachable);
                    return;
                }
            }
            if let Ok(ps) = t.list_projects().await {
                let first = ps.first().map(|p| p.id.clone());
                projects.set(ps);
                let pid = first.unwrap_or_else(|| ProjectId::from("personal"));
                current_project.set(Some(pid.clone()));
                if let Ok(cs) = t.list_conversations(pid.as_str()).await {
                    conversations.set(cs);
                }
                this.reload_conversation_meta();
            }
        });
    }

    /// Select a project: record it, clear the active conversation, and load the
    /// project's conversations. Transport errors leave `conversations` unchanged
    /// (same tolerant pattern as `bootstrap`).
    pub fn open_project(&self, id: ProjectId) {
        let mut current = self.current_project;
        let mut active = self.active;
        let mut conversations = self.conversations;
        current.set(Some(id.clone()));
        active.set(None);
        let t = self.transport.clone();
        let this = self.clone();
        spawn(async move {
            if let Ok(cs) = t.list_conversations(id.as_str()).await {
                conversations.set(cs);
            }
            this.reload_conversation_meta();
        });
    }

    /// Load history and subscribe to the SSE stream for `id`.
    pub fn open_conversation(&self, id: ConversationId) {
        // Synchronously set active + reset transcript BEFORE the spawn so that
        // `send()` calls during the `get_history` await window append to the
        // fresh (empty) transcript, not the previous conversation's turns.
        let mut active = self.active;
        active.set(Some(id.clone()));
        let mut transcript = self.transcript;
        transcript.set(Transcript::default());

        let t = self.transport.clone();
        let mut connection = self.connection;
        spawn(async move {
            // Fetch history, then merge so any turns `send()` appended locally
            // during the await are preserved AFTER the history turns.
            let history = t.get_history(id.as_str()).await.unwrap_or_default();
            transcript.with_mut(|mut tx| {
                let locals = std::mem::take(&mut tx.turns);
                tx.turns = history;
                tx.turns.extend(locals);
            });
            let mut events = t.subscribe(id.as_str());
            use futures_util::StreamExt;
            while let Some(ev) = events.next().await {
                match ev {
                    Ok(ev) => {
                        connection.set(ConnState::Connected);
                        transcript.with_mut(|mut tx| tx.apply_event(&ev));
                    }
                    Err(_) => {
                        connection.set(ConnState::Reconnecting);
                    }
                }
            }
        });
    }

    /// Create a new conversation in the current project, then make it active.
    /// Project comes from `current_project`; model + working_dir default server-side.
    /// On success (after the transport responds) the conversation is inserted and
    /// opened; transport errors are logged and leave the UI unchanged.
    pub fn create_conversation(&self) {
        let Some(project_id) = self.current_project.peek().clone() else { return };
        let mut conversations = self.conversations;
        let this = self.clone();
        let t = self.transport.clone();
        spawn(async move {
            match t.create_conversation(project_id.as_str(), None).await {
                Ok(conv) => {
                    let id = conv.id.clone();
                    conversations.with_mut(|mut cs| cs.push(conv));
                    this.open_conversation(id);
                }
                Err(e) => eprintln!("create_conversation failed: {e}"),
            }
        });
    }

    /// Append the user turn immediately, then fire `send_message` to the
    /// transport.  The assistant reply streams back via the open subscription.
    ///
    /// `attachments` are forwarded verbatim to the transport; callers are
    /// responsible for mapping `Attachment → AttachmentPayload` via
    /// [`crate::attachment_payload::to_payload`] before calling this.
    pub fn send(&self, text: String, attachments: Vec<AttachmentPayload>) {
        let Some(id) = self.active.peek().clone() else { return };
        let is_first_user_turn = !self.transcript.peek().turns.iter().any(|t| t.role == "user");
        let has_title = self.conversation_meta.peek().get(id.as_str()).and_then(|m| m.title.as_ref()).is_some();
        if is_first_user_turn && !has_title {
            if let Some(title) = crate::conversation_meta::derive_title(&text) {
                self.rename_conversation(id.as_str(), title);
            }
        }
        let mut transcript = self.transcript;
        transcript.with_mut(|mut tx| tx.apply_user(text.clone()));
        let t = self.transport.clone();
        spawn(async move {
            let _ = t.send_message(id.as_str(), &text, &attachments).await;
        });
    }

    /// Load the current project's title/icon overrides into the signal.
    pub fn reload_conversation_meta(&self) {
        let (dir, pid) = {
            let cur = self.current_project.peek().clone();
            let projects = self.projects.peek().clone();
            match cur.and_then(|id| projects.iter().find(|p| p.id == id).cloned()) {
                Some(p) => (p.default_working_dir.clone(), p.id.0.clone()),
                None => return,
            }
        };
        let store = crate::conversation_meta::ConversationMetaStore::load(&dir, &pid);
        let mut sig = self.conversation_meta;
        sig.set(store.as_map().clone());
    }

    pub fn rename_conversation(&self, id: &str, title: String) {
        self.with_meta_store(|s| s.set_title(id, title));
    }

    pub fn set_conversation_icon(&self, id: &str, icon: String) {
        self.with_meta_store(|s| s.set_icon(id, icon));
    }

    fn with_meta_store(&self, f: impl FnOnce(&mut crate::conversation_meta::ConversationMetaStore)) {
        // Whole-file read-modify-write (load → edit → persist → refresh signal). This is
        // lost-update-safe ONLY because every caller runs synchronously on the UI thread,
        // so two edits never interleave. If this ever moves into a `spawn`, switch to a
        // single owned store or per-key locking to avoid clobbering concurrent edits.
        let (dir, pid) = {
            let cur = self.current_project.peek().clone();
            let projects = self.projects.peek().clone();
            match cur.and_then(|id| projects.iter().find(|p| p.id == id).cloned()) {
                Some(p) => (p.default_working_dir.clone(), p.id.0.clone()),
                None => return,
            }
        };
        let mut store = crate::conversation_meta::ConversationMetaStore::load(&dir, &pid);
        f(&mut store);
        let mut sig = self.conversation_meta;
        sig.set(store.as_map().clone());
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use oxide_client::{mock::MockTransport, AgentEvent};

    use super::*;

    /// delta → delta → final accumulates "pong" then commits an assistant turn.
    #[test]
    fn send_streams_delta_then_final_into_signals() {
        // Only the pure Transcript reducer is tested here — no Freya context needed.
        let _transport: Arc<dyn oxide_client::Transport> = Arc::new(MockTransport {
            events: std::sync::Mutex::new(vec![
                AgentEvent {
                    seq: 1,
                    kind: "delta".into(),
                    payload: serde_json::json!({"kind":"delta","text":"po"}),
                },
                AgentEvent {
                    seq: 2,
                    kind: "delta".into(),
                    payload: serde_json::json!({"kind":"delta","text":"ng"}),
                },
                AgentEvent {
                    seq: 3,
                    kind: "final".into(),
                    payload: serde_json::json!({"kind":"final","text":"pong"}),
                },
            ]),
            ..MockTransport::new()
        });

        let mut tx = Transcript::default();
        tx.apply_user("Reply pong".into());
        assert_eq!(tx.live_assistant, "");
        assert_eq!(tx.turns.len(), 1);
        assert_eq!(tx.turns[0].role, "user");

        tx.apply_event(&AgentEvent {
            seq: 1,
            kind: "delta".into(),
            payload: serde_json::json!({"text":"po"}),
        });
        tx.apply_event(&AgentEvent {
            seq: 2,
            kind: "delta".into(),
            payload: serde_json::json!({"text":"ng"}),
        });
        assert_eq!(tx.live_assistant, "pong");

        tx.apply_event(&AgentEvent {
            seq: 3,
            kind: "final".into(),
            payload: serde_json::json!({"text":"pong"}),
        });
        assert_eq!(tx.live_assistant, "", "live_assistant must be cleared after final");
        assert_eq!(
            tx.turns.last().unwrap().text,
            "pong",
            "final must commit a Turn with the assistant text"
        );
        assert_eq!(
            tx.turns.last().unwrap().role,
            "assistant",
            "committed Turn must have role=assistant"
        );
    }

    /// An unhealthy `MockTransport` must return `Err` from `health()` — the
    /// seam `AppState::bootstrap` relies on to transition to `Unreachable`.
    #[tokio::test]
    async fn unhealthy_transport_health_returns_err() {
        let mock = MockTransport { healthy: false, ..MockTransport::new() };
        assert!(
            mock.health().await.is_err(),
            "unhealthy mock must return Err from health()"
        );
    }

    /// apply_user clears live_assistant; a second user turn appends.
    #[test]
    fn apply_user_appends_and_clears_live() {
        let mut tx = Transcript::default();
        tx.live_assistant = "leftover".into();
        tx.apply_user("hello".into());
        assert_eq!(tx.live_assistant, "", "apply_user must clear live_assistant");
        assert_eq!(tx.turns.len(), 1);
        assert_eq!(tx.turns[0].text, "hello");
        assert_eq!(tx.turns[0].role, "user");
    }

    /// final with no preceding deltas uses its own text field (not empty live_assistant).
    #[test]
    fn final_without_deltas_commits_event_text() {
        let mut tx = Transcript::default();
        tx.apply_event(&AgentEvent {
            seq: 1,
            kind: "final".into(),
            payload: serde_json::json!({"text":"direct"}),
        });
        assert_eq!(tx.turns.last().unwrap().text, "direct");
        assert_eq!(tx.live_assistant, "");
    }

    /// final with no text field falls back to accumulated live_assistant.
    #[test]
    fn final_without_text_field_falls_back_to_live() {
        let mut tx = Transcript::default();
        tx.apply_event(&AgentEvent {
            seq: 1,
            kind: "delta".into(),
            payload: serde_json::json!({"text":"acc"}),
        });
        // Final has no "text" key — should drain live_assistant.
        tx.apply_event(&AgentEvent {
            seq: 2,
            kind: "final".into(),
            payload: serde_json::json!({}),
        });
        assert_eq!(tx.turns.last().unwrap().text, "acc");
        assert_eq!(tx.live_assistant, "");
    }

    #[test]
    fn size_class_thresholds_and_right_tab_default() {
        use super::{SizeClass, RightTab};
        assert_eq!(SizeClass::from_logical_width(1280.0), SizeClass::Wide);
        assert_eq!(SizeClass::from_logical_width(1000.0), SizeClass::Compact);
        assert_eq!(SizeClass::from_logical_width(700.0), SizeClass::Tablet);
        assert_eq!(SizeClass::from_logical_width(420.0), SizeClass::Phone);
        // boundaries (inclusive lower)
        assert_eq!(SizeClass::from_logical_width(1180.0), SizeClass::Wide);
        assert_eq!(SizeClass::from_logical_width(920.0), SizeClass::Compact);
        assert!(!SizeClass::Wide.is_compact_or_narrower());
        assert!(SizeClass::Compact.is_compact_or_narrower());
        assert!(SizeClass::Phone.is_compact_or_narrower());
        assert_eq!(RightTab::default(), RightTab::Run);
    }

    #[test]
    fn status_direction_all_has_four_with_labels_and_icons() {
        assert_eq!(StatusDirection::ALL.len(), 4);
        assert_eq!(StatusDirection::ALL[0], StatusDirection::Spec);
        let labels: Vec<_> = StatusDirection::ALL.iter().map(|d| d.label()).collect();
        assert_eq!(labels, ["Spec", "Mission", "Workbench", "Ambient"]);
        // Every direction has a non-empty icon glyph.
        assert!(StatusDirection::ALL.iter().all(|d| !d.icon().is_empty()));
        assert_eq!(StatusDirection::default(), StatusDirection::Spec);
    }

    #[test]
    fn create_conversation_ok_inserts_and_activates() {
        use freya_testing::prelude::*;
        use oxide_client::mock::MockTransport;
        fn app() -> impl IntoElement {
            let state = AppState::new(Arc::new(MockTransport::new()));
            let st = state.clone();
            use_hook(move || {
                st.current_project.clone().set(Some(ProjectId::from("personal")));
                st.create_conversation();
            });
            let n = state.conversations.read().len();
            let active = state.active.read().clone().map(|i| i.as_str().to_string()).unwrap_or_default();
            label().text(format!("n={n} active={active}"))
        }
        let mut runner = launch_test(app);
        runner.poll_n(Duration::from_millis(5), 12);
        let found = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("n=1") && l.text.as_ref().contains("active=mock-conv"))
        });
        assert!(found.is_some(), "create_conversation should optimistically insert + activate the new conversation");
    }

    /// Auto-title: first `send` on a blank transcript derives a title and
    /// stores it in `conversation_meta`.
    ///
    /// The mock project has a real temp dir so persistence works end-to-end
    /// (same path `with_meta_store` uses).  We drive `use_hook` synchronously
    /// then poll for the signal update.
    #[test]
    fn send_first_message_auto_titles_conversation() {
        use freya_testing::prelude::*;
        use oxide_client::mock::MockTransport;
        use std::sync::Arc;

        let tmp = std::env::temp_dir()
            .join(format!("oxide-meta-autotitle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("temp dir");
        let tmp_str = tmp.to_string_lossy().to_string();

        let project = Project {
            id: ProjectId::from("test-proj"),
            name: "Test".into(),
            default_working_dir: tmp_str.clone(),
            created_at: 0,
        };
        let project_cap = project.clone();

        fn app(project: Project) -> impl IntoElement {
            let state = AppState::new(Arc::new(MockTransport::new()));
            let st = state.clone();
            use_hook(move || {
                st.projects.clone().set(vec![project.clone()]);
                st.current_project.clone().set(Some(project.id.clone()));
                st.active.clone().set(Some(ConversationId::from("conv-1")));
                st.send("hello there".into(), vec![]);
            });
            let meta = state.conversation_meta.read();
            let title = meta
                .get("conv-1")
                .and_then(|m| m.title.as_deref())
                .unwrap_or("")
                .to_string();
            label().text(format!("title={title}"))
        }

        let mut runner = launch_test(move || app(project_cap.clone()));
        runner.poll_n(Duration::from_millis(5), 12);
        let found = runner.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("title=hello there"))
        });
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(found.is_some(), "first send must auto-title the conversation in conversation_meta");
    }

    #[test]
    fn create_conversation_err_leaves_unchanged() {
        use freya_testing::prelude::*;
        use oxide_client::mock::MockTransport;
        fn app() -> impl IntoElement {
            let mock = MockTransport { create_conversation_fails: true, ..MockTransport::new() };
            let state = AppState::new(Arc::new(mock));
            let st = state.clone();
            use_hook(move || {
                st.current_project.clone().set(Some(ProjectId::from("personal")));
                st.create_conversation();
            });
            let n = state.conversations.read().len();
            label().text(format!("n={n}"))
        }
        let mut runner = launch_test(app);
        runner.poll_n(Duration::from_millis(5), 12);
        assert!(runner.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("n=0"))).is_some(),
            "a failed create must leave conversations empty");
    }
}
