use tokio::sync::mpsc;

/// A question pending user response, sent to the UI event loop.
#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub question: String,
    pub options: Vec<String>,
    pub response_tx: mpsc::Sender<String>,
}

/// Live progress events for an in-flight agent turn, tagged with the
/// chat-thread index that issued the request so late events file
/// into the right conversation.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of the model's text reply, in order. Not produced in
    /// the current non-streaming runtime (the reply arrives complete
    /// via `AiResponseReceived`); the variant + its UI scaffolding are
    /// kept so token streaming can be re-added without rewiring.
    #[allow(dead_code)]
    Delta(String),
    /// What the agent is doing right now ("Searching the web…",
    /// "Scheduling task — writing systemd unit…"). Dynamic so tool
    /// executors can interpolate the target into the label.
    Activity(String),
    /// A structured agent-feature card to append to the
    /// conversation (command executed / task scheduled / memory
    /// saved). Rendered by `chat_ui::cards` and persisted on the
    /// owning `ChatMessage`.
    Card(AgentCardData),
    /// Token usage reported by the provider for this turn (prompt,
    /// completion). Accumulated into the thread for the usage/cost
    /// readout.
    Usage { prompt: u32, completion: u32 },
}

/// Payload for the three agent-feature card types. Serialized into
/// `ai-chats.json` as part of `ChatMessage`, so every field is
/// plain data (chips/buttons are derived in the view).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentCardData {
    /// `execute_command` ran. `stdout` is trimmed to the card cap.
    Command {
        command: String,
        stdout: String,
        exit_code: i32,
    },
    /// `schedule_task` created/changed a systemd user timer.
    Task {
        name: String,
        /// Unit base name ("oxidemx-task-<slug>").
        unit: String,
        /// OnCalendar expression as written to the timer.
        schedule: String,
        /// Human "next run" from `systemctl --user list-timers`,
        /// `None` when the timer is disabled.
        next_run: Option<String>,
        enabled: bool,
    },
    /// `memory` saved an entry.
    Memory {
        id: String,
        text: String,
        /// "until changed" (pinned) or "auto · 90d" (unpinned).
        retention: String,
    },
    /// `run_flow` ran a conductor flow. Live per-step progress streams
    /// as `Activity` while it runs; this card is the final summary,
    /// with a "Watch" chip that opens Mission Control on the flow.
    Flow {
        flow_id: String,
        run_id: String,
        success: bool,
        steps: Vec<FlowStep>,
        artifacts: Vec<String>,
    },
}

/// One step's terminal status inside a `Flow` card.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlowStep {
    pub step: String,
    /// `pending` | `running` | `done` | `failed` | `skipped`.
    pub status: String,
}

/// Channel type for thread-tagged stream events flowing into the
/// iced subscription.
pub type StreamEventTx = mpsc::Sender<(usize, StreamEvent)>;

/// Per-request handle for forwarding stream events. Cheap to clone.
#[derive(Clone, Debug)]
pub struct StreamSink {
    pub thread: usize,
    pub tx: mpsc::Sender<(usize, StreamEvent)>,
}

impl StreamSink {
    pub async fn send(&self, event: StreamEvent) {
        let _ = self.tx.send((self.thread, event)).await;
    }
}
