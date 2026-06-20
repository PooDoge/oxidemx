//! iced application: state, view, update, subscription wiring.
//!
//! The overlay is a single iced application that owns:
//!   * a `RadialState` (active theme + slices + per-slice highlight
//!     + visible? toggle for the menu),
//!   * a `Painter` (struct that implements `canvas::Program` and
//!     does the cairo-style frame painting via iced primitives),
//!   * a tokio-free zbus listener that forwards daemon signals into
//!     `Message::Overlay(OverlayEvent)`.
//!
//! Positioning happens after each `Show` event: we fire-and-forget a
//! D-Bus call to the GNOME extension's `MoveOverlay` method. The
//! extension places our xdg-toplevel at the cursor; iced never sees
//! coordinates the way the gtk4-layer-shell prototype tried to.

use iced::{Color, Size};
use tracing::{error, info, warn};

use oxidemx_shared::AppConfig;

use crate::dbus::OverlayEvent;
use crate::radial::RadialState;

pub mod agent_events;
mod subscriptions;
mod update;
mod view;

pub(crate) use subscriptions::rel_time;

use subscriptions::subscription;
use update::update;
use view::view;

const APP_ID: &str = "org.oxidemx.overlay";

/// The four primary chat views the header segmented switcher selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatView {
    Conversation,
    Skills,
    Memory,
    Tasks,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    /// Sentinel for fire-and-forget Tasks whose completion we
    /// don't need to react to (e.g. haptic pulses).
    Noop,
    Overlay(OverlayEvent),
    /// Result of asking the GNOME extension to position the window.
    /// Used only for logging / future retries.
    Positioned(bool),
    /// Toggle-mode cursor moved over the canvas. Coords are widget-
    /// local pixels (origin at canvas top-left).
    ToggleCursor {
        x: f64,
        y: f64,
    },
    /// Toggle-mode left-click — dispatch the highlighted slice and
    /// close the menu.
    ToggleClickSelect,
    /// Toggle-mode dismiss without dispatching (right-click / Esc /
    /// click outside the menu).
    ToggleDismiss,
    /// Mouse-wheel scroll over the centre puck — cycles to the
    /// next/previous radial-menu page (positive = next, negative
    /// = previous). Only emitted in toggle mode where there's a
    /// real cursor over the canvas.
    CyclePage(i32),
    /// Result of asking the GNOME extension for the currently
    /// focused window's class. Fired immediately after a `Show`
    /// event so the menu can swap to the matching app-context
    /// page. `None` = extension missing or no app focused.
    FocusedClassResolved(Option<String>),
    /// Config reload from inotify watcher — replace theme + slices
    /// in the live state without restarting the overlay.
    ConfigReloaded(Box<oxidemx_shared::AppConfig>),
    WindowOpened(iced::window::Id),
    /// Result of querying the monitor size for centering fallback.
    CenterOverlay(Option<Size>),

    // AI Assistant Messages
    AiEditorAction(iced::widget::text_editor::Action),
    AiSubmitPrompt,
    /// Live event from the in-flight agent turn: text delta or
    /// tool-activity label, tagged with the requesting thread.
    AiStream((usize, crate::ai_client::StreamEvent)),
    /// Stop button — abort the in-flight request, keep the partial.
    AiStopRequest,
    /// A link inside a rendered markdown bubble was clicked.
    AiLinkClicked(String),
    /// Copy arbitrary text (bubble ⧉ button / Copy chat).
    AiCopyText(String),
    /// Pointer entered/left a chat bubble (reveals its copy button).
    AiBubbleHover(Option<usize>),
    /// Open the right-click context menu for bubble `Some(i)`, or close
    /// it (`None`).
    AiBubbleMenu(Option<usize>),
    /// Switch bubble `i` into text-selection mode (read-only editor).
    AiBubbleSelect(usize),
    /// Edits from a bubble in selection mode — only selection/copy
    /// actions are applied (the bubble is read-only).
    AiSelectAction(iced::widget::text_editor::Action),
    /// Leave selection mode, restoring the rendered-markdown bubble.
    AiSelectExit,
    /// Paste the clipboard into the prompt input (context-menu Paste).
    AiPasteToInput,
    /// Clipboard contents arrived for `AiPasteToInput`.
    AiPasteReceived(Option<String>),
    /// Jump the conversation scroll back to the newest message.
    AiScrollToBottom,
    /// Timer to clear the transient confirmation toast.
    AiToastExpire,
    /// Open/close the Skills management panel.
    AiToggleSkills,
    /// Enable/disable a skill by name.
    AiSkillEnable(String, bool),
    /// Search filter in the Skills panel.
    AiSkillsSearch(String),
    /// Header segmented switcher selected a view.
    AiShowView(ChatView),
    /// Expand/collapse an agent/tool card by history index.
    AiCardToggle(usize),
    /// Open a reply image in the full-window lightbox.
    AiLightboxOpen(String),
    /// Close the lightbox.
    AiLightboxClose,
    /// Slash palette: click row `i`.
    AiPaletteSelect(usize),
    /// Slash palette: move selection by delta (Up/Down).
    AiPaletteNav(i32),
    /// Slash palette: run the selected row (Enter).
    AiPaletteRun,
    /// Slash palette: dismiss (Esc).
    AiPaletteClose,
    /// Retry the last failed turn (Retry button on an error bubble).
    AiRetryLast,
    /// Search filter in the thread-list view.
    AiThreadsSearch(String),
    /// Export a thread (by index) to a Markdown file.
    AiExportThread(usize),
    /// A file was dropped onto the overlay window — stage it as an
    /// attachment for the next prompt.
    AiFileDropped(std::path::PathBuf),
    /// Open the native file picker to attach a file.
    AiAttachPick,
    /// Picker result.
    AiAttachReceived(Option<std::path::PathBuf>),
    /// Clear the staged attachment.
    AiAttachClear,
    /// A rolling thread summary finished: `(thread, summary, upto)`.
    AiSummaryUpdated(usize, Option<String>, usize),
    /// A remote AI-reply image finished fetching: `(url, bytes)`.
    AiImageFetched(String, Option<Vec<u8>>),
    /// The conversation was scrolled (tracks whether the "jump to
    /// latest" affordance should show).
    AiChatScrolled(iced::widget::scrollable::Viewport),
    /// Cycle the active thread's model Flash ↔ Pro.
    AiModelToggled,
    AiRenameStart(usize),
    AiRenameInput(String),
    AiRenameCommit,
    AiDeleteThread(usize),
    /// Reply for the prompt sent from thread `.0` — indexed so a
    /// response landing after the user switched/created threads
    /// still files into the conversation that asked.
    AiResponseReceived(usize, Result<(String, Option<String>), String>),
    /// `/optimize` finished: the rewritten prompt (or an error) to drop back
    /// into the chat input for review.
    AiPromptOptimized(Result<String, String>),
    AiChooseOption(String),
    AiQuestionReceived(crate::ai_client::PendingQuestion),
    /// Chat strip action icons (replace the old mode toggle): open
    /// the Command Center (Mission Control), the Agents & skills
    /// config (settings Agents tab), and MCP servers.
    AiOpenCommandCenter,
    AiOpenAgentsConfig,
    AiOpenMcpConfig,
    /// "+ New" — start a fresh conversation thread.
    AiNewChat,
    /// Toggle the previous-conversations list.
    AiToggleThreads,
    /// Open thread `.0` from the list.
    AiSelectThread(usize),
    /// Mouse-down on the chat shell's header arc (outside the ×) —
    /// starts a native compositor window move, like grabbing a
    /// titlebar.
    ChatHeaderPressed,
    /// Pointer motion over the chat shell while the puck is armed —
    /// feeds the deliberate-travel activation rule (see
    /// `crate::handoff`).
    HandoffPointer {
        x: f64,
        y: f64,
    },
    /// Click over the chat shell while the puck is armed — activates
    /// the chat unless it landed on the puck's hit circle.
    HandoffClick {
        x: f64,
        y: f64,
    },
    /// Mouse-down on the chat's bottom-right resize grip — starts a
    /// native compositor interactive resize.
    ChatResizeStart,
    /// Compositor reported a window resize — keep `win_size` true.
    WindowResized(Size),
    /// Fresh live-data sample from the 1 s widget sampler.
    WidgetSample(crate::sampler::WidgetSnapshot),
    /// Wheel over a Dial slice — adjust its target by ±1 step.
    DialAdjust {
        idx: usize,
        direction: i32,
    },
    /// Event from the widget-host worker: a fresh scene for the
    /// painter's replay store, an instance failure, or the
    /// installed-widget registry (Plan 3's picker).
    WidgetHost(oxidemx_widget_host::HostEvent),
    /// Wheel over a Custom-widget slice — forwarded to the guest as
    /// `Event::Scroll { delta }` via the host worker.
    WidgetScroll {
        idx: usize,
        delta: f32,
    },
    /// Monitor size arrived for the debounced chat-size persist —
    /// clamp below the output before writing the config.
    ChatSizePersist((f32, f32), Option<Size>),
    /// Vision-loop dev hook: the window screenshot arrived.
    VisionShot(iced::window::Screenshot),
    /// Toggle the memories management view (header brain button).
    AiToggleMemories,
    /// Live edit of the memories view's search filter.
    AiMemorySearch(String),
    /// Delete a memory by id (row 🗑 / card "Forget" chip).
    AiMemoryDelete(String),
    /// Open Mission Control on a flow (Flow card "Watch" chip).
    AiWatchFlow(String),
    /// Pin/unpin a memory by id.
    AiMemoryPin(String, bool),
    /// Toggle the scheduled-tasks view (header clock button).
    AiToggleTasks,
    /// Async task-list refresh finished.
    AiTasksLoaded(Vec<crate::agent::tasks::TaskInfo>),
    /// Enable/disable a task's timer (card / row switch).
    AiTaskToggle(String, bool),
    /// "Run now" on a task (unit base name).
    AiTaskRun(String),
    /// Delete a task's units entirely.
    AiTaskDelete(String),
    /// "Edit" chip — drops an edit prompt into the input.
    AiTaskEdit(String),
    /// The overlay window lost keyboard focus. Dismisses the disc
    /// (click-elsewhere-to-close), but is IGNORED while the chat
    /// shell is up: Mutter drops keyboard focus the moment an
    /// interactive move grab starts, so dismissing here would close
    /// the chat the instant the user tries to drag it.
    WindowUnfocused,
    /// The overlay window gained keyboard focus. Diagnostic only —
    /// confirms the extension's RaiseOverlay activation actually
    /// landed (a Wayland client can't overcome this any other way).
    WindowFocused,

    /// A second `oxidemx-chat` launch signalled the running instance to
    /// raise/focus via `org.oxidemx.Chat Present`. Best-effort on Wayland
    /// (focus-stealing prevention); the primary goal is preventing duplicates.
    PresentWindow,

    // ── agentd (use_agentd = true) path ────────────────────────────────────

    /// A conductor run-kind event, parsed into a flat view for the activity dock.
    RunEvent(crate::activity::RunEventView),

    // ── Activity bubble action messages ────────────────────────────────────────

    /// Expand the cluster for `run_id` (show all bubbles).
    ActivityExpand(String),
    /// Collapse all clusters + close any open peek popover.
    ActivityCollapse,
    /// Open or close the peek popover for `(run_id, step)`.
    BubblePeekToggle(String, String),
    /// Dismiss (remove) the finished cluster for `run_id`.
    BubbleDismiss(String, String),
    /// Cancel the in-flight run via D-Bus.
    RunCancel(String),
    /// Re-launch a flow: `(run_id, flow_id)`.  `run_id` is the old id (informational).
    RunRetry(String, String),
    /// Open an artifact path with xdg-open.
    RunOpenArtifact(String),
    /// Append the run's handoff / bubble logs into the active chat thread.
    RunTranscript(String),

    /// A D-Bus event from agentd matched to an overlay chat thread.
    ///
    /// `session_id` is the raw thread id from D-Bus (`thread_or_run`).
    /// `update.rs` resolves it to an `ai_threads` index and then fans out
    /// to the appropriate `AiStream` or `AgentdFinal` logic.
    AgentdEvent {
        session_id: String,
        inner: crate::app::agent_events::AgentdInner,
    },

    /// agentd signals that the turn is complete; the full reply text is
    /// carried here.  Parallel to `AiResponseReceived` for the in-proc path.
    AgentdFinal {
        thread_idx: usize,
        text: String,
    },

    /// agentd is waiting for user approval of a tool call.
    ///
    /// The overlay shows an approval card; the user clicks Allow/Deny
    /// which fires `AgentdRespondApproval`.
    AgentdApprovalRequested {
        thread: String,
        request_id: String,
        card_json: String,
    },

    /// User responded to an agentd approval card.
    /// Constructed from the approval card view (Task 8 wires the button).
    #[allow(dead_code)]
    AgentdRespondApproval {
        request_id: String,
        allow: bool,
    },

    /// Agentd model lifecycle change (load / unload / set-active).
    AgentdModelStatus(String, String),

    /// History loaded from agentd for the given thread index.
    AgentdHistoryLoaded {
        thread_idx: usize,
        turns: Vec<(bool, String)>,
    },
}

pub fn run() -> iced::Result {
    // The window is created at the CHAT height from the start and
    // never resized: programmatic xdg resizes on Wayland (winit
    // 0.30 + Mutter + fractional scaling) progressively desync the
    // wgpu surface size from the layout/pointer space — squashed
    // disc, stretched chat, hit-boxes offset from visuals. The disc
    // renders in the top WINDOW_SIZE×WINDOW_SIZE square; the strip
    // below is transparent until the AI chat morph uses it.
    // Size from the persisted `overlay.chat_size` (clamped so the
    // disc square + chat minimums always fit). A resize-grip commit
    // updates the config and issues one window resize; the next
    // launch starts at the committed size.
    let initial_size = crate::chat_shell::effective_window_size(
        crate::config::load()
            .map(|c| c.overlay.chat_size)
            .unwrap_or(None),
    );
    let window = oxidemx_window::frameless_topmost(APP_ID, initial_size);

    iced::application(boot, update, view)
        .title("OxideMX")
        .window(window)
        .style(|_state, _theme| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: Color::WHITE,
        })
        .subscription(subscription)
        .run()
}

fn boot() -> RadialState {
    let config = match crate::config::load() {
        Ok(c) => c,
        Err(e) => {
            warn!("could not load config ({e}); using defaults");
            AppConfig::default()
        }
    };
    RadialState::new(&config)
}

/// Run the chat as a normal, decorated toplevel WINDOW (the `oxidemx-chat`
/// sibling binary). Mirrors `run()` but: plain window settings (no frameless/
/// topmost/override_redirect, no cursor-helper), an opaque background, and a
/// boot that sets `chat_window_mode = true`.
pub fn run_chat_window() -> iced::Result {
    iced::application(boot_chat_window, update, view)
        .title("OxideMX Chat")
        .window(iced::window::Settings {
            size: iced::Size::new(520.0, 720.0),
            min_size: Some(iced::Size::new(380.0, 480.0)),
            decorations: true,
            transparent: false,
            // Wayland app_id — must match StartupWMClass in
            // packaging/org.oxidemx.chat.desktop so the WM groups the window
            // under the launcher entry (correct taskbar icon + grouping).
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "org.oxidemx.Chat".to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
        .subscription(subscription)
        .run()
}

fn boot_chat_window() -> RadialState {
    let mut state = boot();
    state.chat_window_mode = true;
    // Stable show_time lets the status shader animate (the 16 ms Tick re-renders);
    // morph parked at 1.0 is the "chat fully open" pose.
    state.ai_morph = crate::anim::Tween::at(1.0);
    state.show_time = Some(std::time::Instant::now());
    state
}

#[allow(dead_code)]
fn _ensure_link(_e: &OverlayEvent) {
    error!("only here so the OverlayEvent path is referenced from app");
    info!("ditto");
}
