//! Chat persistence types for the AI Assistant: `ChatMessage`,
//! `ChatThread`, and the load/save round-trip to
//! `~/.config/oxidemx/ai-chats.json`.

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub is_user: bool,
    pub text: String,
    /// Parsed markdown for AI messages — rebuilt on construction
    /// and on load (never serialized), so the 60 fps view never
    /// re-parses.
    #[serde(skip)]
    pub md: Vec<iced::widget::markdown::Item>,

    /// Structured agent-feature card attached to this message
    /// (command executed / task scheduled / memory saved). `None`
    /// for plain text bubbles — and for every message written
    /// before this field existed, so old `ai-chats.json` files
    /// load unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<crate::ai_client::AgentCardData>,

    /// A failed turn — rendered as a red error bubble with a Retry
    /// button. `false` for everything written before this field existed.
    #[serde(default)]
    pub is_error: bool,

    /// Path to an image the user attached to this (user) message —
    /// rendered as a thumbnail in the bubble. `None` for text-only and
    /// pre-existing messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,

    /// Unix seconds the message was created — drives the per-bubble
    /// timestamp in the meta line. `0` for pre-existing messages.
    #[serde(default)]
    pub created_at: u64,
    /// Provider token usage for the turn that produced this (assistant)
    /// message — `(prompt, completion)`. Drives the per-bubble cost meta.
    #[serde(default)]
    pub tokens: (u32, u32),
}

impl ChatMessage {
    pub fn user(text: String) -> Self {
        ChatMessage {
            is_user: true,
            text,
            md: Vec::new(),
            card: None,
            is_error: false,
            image_path: None,
            created_at: now_secs(),
            tokens: (0, 0),
        }
    }

    pub fn assistant(text: String) -> Self {
        let md = iced::widget::markdown::parse(&text).collect();
        ChatMessage {
            is_user: false,
            text,
            md,
            card: None,
            is_error: false,
            image_path: None,
            created_at: now_secs(),
            tokens: (0, 0),
        }
    }

    /// A failed turn. Rendered distinctly with a Retry affordance.
    pub fn error(text: String) -> Self {
        ChatMessage {
            is_user: false,
            text,
            md: Vec::new(),
            card: None,
            is_error: true,
            image_path: None,
            created_at: now_secs(),
            tokens: (0, 0),
        }
    }

    /// An agent-feature card message. `text` carries a plain-text
    /// rendering for "Copy chat" and for clients without card
    /// rendering; the view draws from `card`.
    pub fn agent_card(card: crate::ai_client::AgentCardData) -> Self {
        let text = match &card {
            crate::ai_client::AgentCardData::Command {
                command, exit_code, ..
            } => {
                format!("[command executed: {command} (exit {exit_code})]")
            }
            crate::ai_client::AgentCardData::Task { name, schedule, .. } => {
                format!("[task scheduled: {name} — {schedule}]")
            }
            crate::ai_client::AgentCardData::Memory { text, .. } => {
                format!("[memory saved: {text}]")
            }
            crate::ai_client::AgentCardData::Flow {
                flow_id,
                success,
                steps,
                ..
            } => {
                let done = steps.iter().filter(|s| s.status == "done").count();
                let verb = if *success { "completed" } else { "run failed" };
                format!("[flow {verb}: {flow_id} — {done}/{} steps]", steps.len())
            }
        };
        ChatMessage {
            is_user: false,
            text,
            md: Vec::new(),
            card: Some(card),
            is_error: false,
            image_path: None,
            created_at: now_secs(),
            tokens: (0, 0),
        }
    }
}

/// One AI conversation thread. The chat shell can hold several and
/// switch between them; non-empty threads persist to
/// `~/.config/oxidemx/ai-chats.json` so conversations survive
/// overlay restarts. `session_id` is the server-side
/// `previous_interaction_id` thread — it may expire upstream, in
/// which case the next prompt simply starts fresh server context
/// (the visible history is display-only either way).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChatThread {
    /// First user prompt, truncated — shown in the thread list.
    #[serde(default)]
    pub title: String,
    pub mode: crate::ai_client::AgentMode,
    #[serde(default)]
    pub history: Vec<ChatMessage>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Gemini model id for this thread (Flash by default; the
    /// toolbar pill can switch to Pro for harder prompts).
    #[serde(default = "default_chat_model")]
    pub model: String,
    /// Unix seconds of the last message — drives the "2h ago"
    /// labels in the thread list.
    #[serde(default)]
    pub updated_at: u64,
    /// Cumulative provider token usage for this thread (prompt /
    /// completion), summed across turns for the usage + cost readout.
    #[serde(default)]
    pub tokens_prompt: u64,
    #[serde(default)]
    pub tokens_completion: u64,
    /// Rolling summary of messages `[0, summary_upto)` — keeps long
    /// threads in context without shipping every turn. Empty until the
    /// thread grows past the summarization threshold.
    #[serde(default)]
    pub summary: String,
    /// How many leading history messages `summary` already covers.
    #[serde(default)]
    pub summary_upto: usize,
}

fn default_chat_model() -> String {
    crate::ai_client::DEFAULT_MODEL.to_string()
}

/// Current unix time in whole seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Default for ChatThread {
    fn default() -> Self {
        ChatThread {
            title: String::new(),
            mode: crate::ai_client::AgentMode::Agentic,
            history: Vec::new(),
            session_id: None,
            model: default_chat_model(),
            updated_at: 0,
            tokens_prompt: 0,
            tokens_completion: 0,
            summary: String::new(),
            summary_upto: 0,
        }
    }
}

/// Most threads kept on disk — oldest beyond this are dropped on save.
const MAX_SAVED_CHATS: usize = 30;

fn chats_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::Path::new(&home).join(".config/oxidemx/ai-chats.json")
}

/// Load persisted chat threads; always returns at least one (empty)
/// thread so `RadialState::chat()` is total.
pub fn load_chat_threads() -> Vec<ChatThread> {
    let mut threads: Vec<ChatThread> = std::fs::read_to_string(chats_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    threads.retain(|t: &ChatThread| !t.history.is_empty());
    // Rebuild the serde-skipped parsed-markdown for AI messages.
    for thread in &mut threads {
        for msg in &mut thread.history {
            if !msg.is_user {
                msg.md = iced::widget::markdown::parse(&msg.text).collect();
            }
        }
    }
    threads.push(ChatThread::default());
    threads
}

/// Best-effort persist of all non-empty threads (newest kept when
/// over the cap). Small file, sync write — called on response /
/// thread-management events, not per keystroke.
pub fn save_chat_threads(threads: &[ChatThread]) {
    let keep: Vec<&ChatThread> = threads.iter().filter(|t| !t.history.is_empty()).collect();
    let start = keep.len().saturating_sub(MAX_SAVED_CHATS);
    let path = chats_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string(&keep[start..]) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!(error = %e, "failed to persist AI chats");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to serialise AI chats"),
    }
}
