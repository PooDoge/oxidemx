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

use iced::widget::canvas::Canvas;
use iced::widget::{container, Space};
use iced::{Color, Element, Length, Size, Subscription, Task};
use tracing::{debug, error, info, warn};

use oxidemx_shared::AppConfig;

use crate::dbus::OverlayEvent;
use crate::geometry::WINDOW_SIZE;
use crate::radial::{ChatMessage, Painter, RadialState};

const APP_ID: &str = "org.oxidemx.overlay";

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
    AiChooseOption(String),
    AiQuestionReceived(crate::ai_client::PendingQuestion),
    /// Agent-mode pill clicked (Menu Setup ↔ General). Switching
    /// resets the server-side session thread — the two modes'
    /// tool configurations can't share one Interactions thread —
    /// but keeps the visible history.
    AiModeSelected(crate::ai_client::AgentMode),
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
    /// Mouse-down on the chat's bottom-right resize grip.
    ChatResizeStart {
        x: f64,
        y: f64,
    },
    /// Pointer motion during a resize drag — updates the ghost.
    ChatResizeMove {
        x: f64,
        y: f64,
    },
    /// Button release ends the drag: persist `overlay.chat_size`
    /// and issue the single real window resize.
    ChatResizeEnd,
    /// Compositor reported a window resize — keep `win_size` true.
    WindowResized(Size),
    /// Toggle the memories management view (header brain button).
    AiToggleMemories,
    /// Live edit of the memories view's search filter.
    AiMemorySearch(String),
    /// Delete a memory by id (row 🗑 / card "Forget" chip).
    AiMemoryDelete(String),
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
    /// landed (a Wayland client can't observe this any other way).
    WindowFocused,
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

fn update(state: &mut RadialState, message: Message) -> Task<Message> {
    // A chat-widget interaction arriving while the puck is armed is
    // a click the caps canvas never saw (the widget captured it) —
    // it still counts as "clicked in the chat outside the puck", so
    // disarm before processing. Hover/stream messages don't count.
    if state.ai_handoff.is_armed()
        && matches!(
            message,
            Message::AiEditorAction(_)
                | Message::AiSubmitPrompt
                | Message::AiModelToggled
                | Message::AiRenameStart(_)
                | Message::AiDeleteThread(_)
                | Message::AiModeSelected(_)
                | Message::AiNewChat
                | Message::AiToggleThreads
                | Message::AiSelectThread(_)
                | Message::AiChooseOption(_)
                | Message::AiStopRequest
                | Message::AiLinkClicked(_)
                | Message::AiCopyText(_)
                | Message::AiToggleMemories
                | Message::AiMemorySearch(_)
                | Message::AiMemoryDelete(_)
                | Message::AiMemoryPin(_, _)
                | Message::AiToggleTasks
                | Message::AiTaskToggle(_, _)
                | Message::AiTaskRun(_)
                | Message::AiTaskDelete(_)
                | Message::AiTaskEdit(_)
        )
    {
        state.activate_chat();
    }
    match message {
        Message::Tick => {
            state.advance_animations();
            // Focus the chat's text input as soon as its widgets are
            // actually mounted in the tree (they fade in mid-morph —
            // a focus operation issued at page-change time would
            // find nothing focusable).
            if state.chat_focus_pending
                && crate::chat_shell::chat_alpha(state.ai_morph_progress()) > 0.05
            {
                state.chat_focus_pending = false;
                info!("chat shell mounted — focusing text input");
                return iced::widget::operation::focus_next();
            }
            Task::none()
        }
        Message::Noop => Task::none(),
        Message::Overlay(OverlayEvent::Show { x, y }) => {
            debug!(x, y, "Show event from daemon");
            state.show();
            // Daemon's cursor coord is the *cursor position*; we
            // want the window *centred* on it. MoveOverlay places
            // the window's top-left, so subtract half the window
            // size before sending. Pass monitor=-1 (absolute stage
            // coords) — the extension figures out which monitor
            // contains the requested point.
            // Centre the DISC (not the window) on the cursor: the
            // disc sits centred horizontally in the top square of a
            // possibly-wider/taller chat-sized window.
            let half_x = (state.win_size.0 / 2.0) as i32;
            let half_y = (WINDOW_SIZE / 2.0) as i32;
            // Three parallel D-Bus round-trips: window position,
            // focus query, and the menu_appear haptic pulse.
            // Batching keeps perceived open latency bounded by
            // the slowest of the three.
            // menu_appear haptic always fires on Show → kick the
            // ripple too so visual feedback synchronises with the
            // motor pulse.
            state.trigger_ripple();

            let window_tasks = if let Some(id) = state.window_id {
                Task::batch(vec![
                    iced::window::move_to(
                        id,
                        iced::Point::new((x - half_x) as f32, (y - half_y) as f32),
                    ),
                    iced::window::minimize(id, false),
                    iced::window::gain_focus(id),
                ])
            } else {
                Task::none()
            };

            Task::batch(vec![
                window_tasks,
                Task::perform(
                    oxidemx_window::cursor_helper::move_overlay(
                        APP_ID.to_string(),
                        x - half_x,
                        y - half_y,
                        -1,
                    ),
                    Message::Positioned,
                ),
                Task::perform(
                    oxidemx_window::cursor_helper::get_focused_window_class(APP_ID.to_string()),
                    Message::FocusedClassResolved,
                ),
                Task::perform(
                    crate::haptic_client::trigger_haptic("menu_appear".to_string()),
                    |_| Message::Noop,
                ),
            ])
        }
        Message::Overlay(OverlayEvent::Hide) => {
            debug!("Hide event from daemon");
            // Decide haptic outcome BEFORE hide() resets target.
            // - actionable target → confirm
            // - target that can't actually dispatch → invalid
            // - no target (tap to toggle) → silent (menu_appear
            //   already played; nothing was attempted)
            let outcome = haptic_outcome_for(state);
            // Both Actionable and Unactionable outcomes fire a
            // haptic (confirm / invalid) → ripple in both cases.
            // NoTarget is silent on the haptic side, so skip the
            // ripple too.
            if !matches!(outcome, DispatchOutcome::NoTarget) {
                state.trigger_ripple();
            }
            // Dispatch burst only fires on Actionable — it's a
            // visual confirmation of a successful dispatch, not
            // a "you tried" notification (the ripple covers the
            // invalid case).
            if matches!(outcome, DispatchOutcome::Actionable) {
                if let Some(idx) = dispatch_origin_for(state) {
                    state.trigger_dispatch_burst(idx);
                }
            }
            state.hide();
            haptic_outcome_task(outcome)
        }
        Message::Overlay(OverlayEvent::CursorMoved { dx, dy }) => {
            // Compare target_slice before/after so we can fire
            // a NotifySliceHover only when the user crosses into
            // a *new* slot — otherwise the daemon would get a
            // spam of pulses on every mouse-move event.
            let before = state.target_slice();
            let before_sub = submenu_parent_of(state);
            state.on_cursor_moved(dx, dy);
            let after = state.target_slice();
            let after_sub = submenu_parent_of(state);
            // Slice-change ripple fires on every crossing into a
            // new slot (matches haptic_on_target_change). Submenu
            // open/close ripple fires whenever the submenu state
            // transitions. Both compounding events still get one
            // ripple — looks more deliberate than two stacked.
            if (before != after && after.is_some()) || (before_sub != after_sub) {
                state.trigger_ripple();
            }
            Task::batch([
                haptic_on_target_change(before, after),
                haptic_on_submenu_change(before_sub, after_sub),
            ])
        }
        Message::Positioned(success) => {
            info!("Positioned event received: success={}", success);
            if !success {
                warn!(
                    "MoveOverlay failed — extension couldn't find window with app_id={}; \
                     check that oxidemx-indicator extension is enabled. Querying monitor size for fallback centering.",
                    APP_ID
                );
                if let Some(id) = state.window_id {
                    info!("Querying monitor size for window ID {:?}", id);
                    iced::window::monitor_size(id).map(Message::CenterOverlay)
                } else {
                    warn!("MoveOverlay failed, but state.window_id is None!");
                    Task::none()
                }
            } else {
                Task::none()
            }
        }
        Message::CenterOverlay(Some(size)) => {
            info!("CenterOverlay triggered with monitor size: {:?}", size);
            if let Some(id) = state.window_id {
                let x = (size.width - state.win_size.0) / 2.0;
                let y = (size.height - state.win_size.1) / 2.0;
                info!("Centering window on monitor: x={}, y={}", x, y);
                iced::window::move_to(id, iced::Point::new(x, y))
            } else {
                Task::none()
            }
        }
        Message::CenterOverlay(None) => {
            warn!("Could not determine monitor size for fallback centering");
            Task::none()
        }
        Message::ToggleCursor { x, y } => {
            // Same slice-change debounce as drag-mode CursorMoved.
            let before = state.target_slice();
            let before_sub = submenu_parent_of(state);
            state.on_toggle_cursor(x, y);
            let after = state.target_slice();
            let after_sub = submenu_parent_of(state);
            if (before != after && after.is_some()) || (before_sub != after_sub) {
                state.trigger_ripple();
            }
            Task::batch([
                haptic_on_target_change(before, after),
                haptic_on_submenu_change(before_sub, after_sub),
            ])
        }
        Message::ToggleClickSelect => {
            debug!("Toggle-mode click select");
            // Same outcome split as Hide — confirm if a real
            // action will dispatch, invalid if the targeted slice
            // can't actually do anything, silent if no slice.
            let outcome = haptic_outcome_for(state);
            if !matches!(outcome, DispatchOutcome::NoTarget) {
                state.trigger_ripple();
            }
            if matches!(outcome, DispatchOutcome::Actionable) {
                if let Some(idx) = dispatch_origin_for(state) {
                    state.trigger_dispatch_burst(idx);
                }
            }
            state.click_select();
            haptic_outcome_task(outcome)
        }
        Message::ToggleDismiss => {
            debug!("Toggle-mode dismiss");
            state.dismiss();
            Task::none()
        }
        Message::CyclePage(direction) => {
            debug!(direction, "Cycle radial-menu page");
            // cycle_page is a no-op when fewer than two pages are
            // in the cycle, so detect actual transitions by
            // comparing the active page index before/after.
            let before = state.active_page;
            state.cycle_page(direction);
            if state.active_page != before {
                state.trigger_ripple();
                let mut tasks = vec![Task::perform(
                    crate::haptic_client::trigger_haptic("page_change".to_string()),
                    |_| Message::Noop,
                )];
                // Landing on the AI page: keyboard focus for the
                // chat's text input. A Wayland client can't take
                // focus itself, but the cursor extension runs
                // inside Mutter and can `Meta.Window.activate()`
                // us. (The window needs no resize — it's created
                // at chat height and the morph only uses drawing
                // space that was always there.)
                if state.is_ai_page() {
                    tasks.push(Task::perform(
                        oxidemx_window::cursor_helper::raise_overlay(APP_ID.to_string()),
                        |ok| {
                            if !ok {
                                warn!(
                                    "RaiseOverlay failed — chat input may not \
                                     receive keystrokes (is the oxidemx-cursor \
                                     extension enabled?)"
                                );
                            }
                            Message::Noop
                        },
                    ));
                }
                Task::batch(tasks)
            } else {
                Task::none()
            }
        }
        Message::ChatHeaderPressed => {
            debug!("chat header pressed — starting compositor move");
            if let Some(id) = state.window_id {
                iced::window::drag(id)
            } else {
                Task::none()
            }
        }
        Message::HandoffPointer { x, y } => {
            state.handoff_pointer(x, y);
            Task::none()
        }
        Message::HandoffClick { x, y } => {
            state.handoff_click(x, y);
            Task::none()
        }
        Message::ChatResizeStart { x, y } => {
            state.chat_resize_start(x, y);
            Task::none()
        }
        Message::ChatResizeMove { x, y } => {
            state.chat_resize_move(x, y);
            Task::none()
        }
        Message::ChatResizeEnd => {
            let Some((w, h)) = state.chat_resize_end() else {
                return Task::none();
            };
            info!(w, h, "chat resize committed");
            crate::config::save_chat_size(w.round() as u32, h.round() as u32);
            if let Some(id) = state.window_id {
                iced::window::resize(id, Size::new(w, h))
            } else {
                Task::none()
            }
        }
        Message::WindowResized(size) => {
            state.win_size = (size.width, size.height);
            Task::none()
        }
        Message::WindowUnfocused => {
            if state.ai_morph_progress() > 0.5 {
                // Chat shell is a persistent draggable window —
                // focus loss is routine (move grab, clicking another
                // app). Close it with ×, Escape, or the wheel.
                info!("window unfocused — chat shell active, staying open");
                Task::none()
            } else {
                debug!("window unfocused — dismissing disc");
                state.dismiss();
                Task::none()
            }
        }
        Message::WindowFocused => {
            info!("window gained keyboard focus");
            Task::none()
        }
        Message::FocusedClassResolved(class) => {
            debug!(?class, "Focused window class resolved");
            state.apply_focused_class(class);
            Task::none()
        }
        Message::ConfigReloaded(cfg) => {
            info!("config reloaded — refreshing theme + slices");
            state.reload_from(&cfg);
            Task::none()
        }
        Message::WindowOpened(id) => {
            info!("WindowOpened event received: {:?}", id);
            state.window_id = Some(id);
            Task::none()
        }
        Message::AiEditorAction(action) => {
            state.ai_editor.perform(action);
            Task::none()
        }
        Message::AiSubmitPrompt => {
            let prompt = state.ai_editor.text().trim().to_string();
            if state.ai_loading || prompt.is_empty() {
                return Task::none();
            }
            state.ai_editor = iced::widget::text_editor::Content::new();
            let now = crate::radial::now_secs();
            let chat = state.chat_mut();
            if chat.history.is_empty() {
                // First prompt names the thread for the history list.
                chat.title = prompt.chars().take(48).collect();
            }
            chat.history.push(ChatMessage::user(prompt.clone()));
            chat.updated_at = now;
            state.ai_loading = true;
            state.ai_activity = Some("Thinking…".to_string());
            // Server-side conversation state: the Interactions API
            // replays context from previous_interaction_id, so only
            // the new prompt travels (the local history is
            // display-only).
            let session_id = state.chat().session_id.clone();
            let mode = state.chat().mode;
            let model = state.chat().model.clone();
            let thread_idx = state.ai_active;
            let sink = crate::ai_client::StreamSink::for_thread(thread_idx);
            let (task, handle) = Task::perform(
                async move {
                    match crate::ai_client::load_api_key() {
                        Ok(key) => {
                            crate::ai_client::ask_ai(&key, mode, &model, &prompt, session_id, sink)
                                .await
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e.to_string()),
                    }
                },
                move |res| Message::AiResponseReceived(thread_idx, res),
            )
            .abortable();
            state.ai_abort = Some(handle);
            Task::batch([task, scroll_chat_to_end()])
        }
        Message::AiResponseReceived(thread_idx, res) => {
            state.ai_loading = false;
            state.ai_activity = None;
            state.ai_stream = None;
            state.ai_abort = None;
            let Some(chat) = state.ai_threads.get_mut(thread_idx) else {
                return Task::none();
            };
            match res {
                Ok((reply, next_session_id)) => {
                    chat.session_id = next_session_id;
                    chat.history.push(ChatMessage::assistant(reply));
                }
                Err(err) => {
                    chat.history
                        .push(ChatMessage::assistant(format!("Error: {}", err)));
                }
            }
            chat.updated_at = crate::radial::now_secs();
            crate::radial::save_chat_threads(&state.ai_threads);
            state.trigger_ripple();
            scroll_chat_to_end()
        }
        Message::AiStream((thread_idx, event)) => match event {
            crate::ai_client::StreamEvent::Activity(label) => {
                state.ai_activity = Some(label);
                Task::none()
            }
            crate::ai_client::StreamEvent::Card(card) => {
                // Cards land in the requesting thread immediately —
                // they describe something that already happened
                // (command ran, unit written), so they must survive
                // even if the turn is later stopped/errored.
                if let Some(chat) = state.ai_threads.get_mut(thread_idx) {
                    chat.history.push(ChatMessage::agent_card(card));
                    chat.updated_at = crate::radial::now_secs();
                }
                crate::radial::save_chat_threads(&state.ai_threads);
                scroll_chat_to_end()
            }
            crate::ai_client::StreamEvent::Delta(text) => {
                match &mut state.ai_stream {
                    Some((idx, buf)) if *idx == thread_idx => buf.push_str(&text),
                    _ => state.ai_stream = Some((thread_idx, text)),
                }
                scroll_chat_to_end()
            }
        },
        Message::AiStopRequest => {
            if let Some(handle) = state.ai_abort.take() {
                handle.abort();
            }
            state.ai_loading = false;
            state.ai_activity = None;
            if let Some((idx, partial)) = state.ai_stream.take() {
                if !partial.trim().is_empty() {
                    if let Some(chat) = state.ai_threads.get_mut(idx) {
                        chat.history
                            .push(ChatMessage::assistant(format!("{partial}\n\n*(stopped)*")));
                        chat.updated_at = crate::radial::now_secs();
                    }
                    crate::radial::save_chat_threads(&state.ai_threads);
                }
            }
            Task::none()
        }
        Message::AiLinkClicked(url) => {
            // Only open real web links — markdown can contain
            // arbitrary URIs.
            if url.starts_with("http://") || url.starts_with("https://") {
                if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    warn!(error = %e, url, "failed to open link");
                }
            }
            Task::none()
        }
        Message::AiCopyText(text) => iced::clipboard::write(text),
        Message::AiBubbleHover(idx) => {
            state.ai_hover_msg = idx;
            Task::none()
        }
        Message::AiModelToggled => {
            let chat = state.chat_mut();
            chat.model = if chat.model == crate::ai_client::PRO_MODEL {
                crate::ai_client::DEFAULT_MODEL.to_string()
            } else {
                crate::ai_client::PRO_MODEL.to_string()
            };
            crate::radial::save_chat_threads(&state.ai_threads);
            Task::none()
        }
        Message::AiRenameStart(idx) => {
            let draft = state
                .ai_threads
                .get(idx)
                .map(|t| t.title.clone())
                .unwrap_or_default();
            state.ai_renaming = Some((idx, draft));
            Task::none()
        }
        Message::AiRenameInput(text) => {
            if let Some((_, draft)) = &mut state.ai_renaming {
                *draft = text;
            }
            Task::none()
        }
        Message::AiRenameCommit => {
            if let Some((idx, draft)) = state.ai_renaming.take() {
                if let Some(thread) = state.ai_threads.get_mut(idx) {
                    let trimmed = draft.trim();
                    if !trimmed.is_empty() {
                        thread.title = trimmed.to_string();
                    }
                }
                crate::radial::save_chat_threads(&state.ai_threads);
            }
            Task::none()
        }
        Message::AiDeleteThread(idx) => {
            state.ai_delete_thread(idx);
            crate::radial::save_chat_threads(&state.ai_threads);
            Task::none()
        }
        Message::AiModeSelected(mode) => {
            let chat = state.chat_mut();
            if chat.mode != mode {
                chat.mode = mode;
                // New tool configuration → new server-side thread.
                chat.session_id = None;
                crate::radial::save_chat_threads(&state.ai_threads);
            }
            Task::none()
        }
        Message::AiNewChat => {
            state.ai_new_chat();
            Task::none()
        }
        Message::AiToggleThreads => {
            state.ai_show_threads = !state.ai_show_threads;
            Task::none()
        }
        Message::AiSelectThread(idx) => {
            state.ai_select_chat(idx);
            Task::none()
        }
        Message::AiChooseOption(choice) => {
            if let Some(pending) = state.ai_pending_question.take() {
                let tx = pending.response_tx;
                state
                    .chat_mut()
                    .history
                    .push(ChatMessage::user(choice.clone()));
                state.ai_loading = true;
                Task::perform(
                    async move {
                        let _ = tx.send(choice).await;
                    },
                    |_| Message::Noop,
                )
            } else {
                Task::none()
            }
        }
        Message::AiQuestionReceived(pending) => {
            state.ai_pending_question = Some(pending);
            state.ai_loading = false;
            state.trigger_ripple();
            Task::none()
        }
        Message::AiToggleMemories => {
            state.ai_show_memories = !state.ai_show_memories;
            state.ai_show_tasks = false;
            state.ai_show_threads = false;
            if state.ai_show_memories {
                state.ai_memories = crate::agent::memory::load_all();
                state.ai_memories_bytes = crate::agent::memory::store_size_bytes();
            }
            Task::none()
        }
        Message::AiMemorySearch(q) => {
            state.ai_memories_query = q;
            Task::none()
        }
        Message::AiMemoryDelete(id) => {
            crate::agent::memory::delete(&id);
            state.ai_memories = crate::agent::memory::load_all();
            state.ai_memories_bytes = crate::agent::memory::store_size_bytes();
            Task::none()
        }
        Message::AiMemoryPin(id, pinned) => {
            crate::agent::memory::set_pinned(&id, pinned);
            state.ai_memories = crate::agent::memory::load_all();
            Task::none()
        }
        Message::AiToggleTasks => {
            state.ai_show_tasks = !state.ai_show_tasks;
            state.ai_show_memories = false;
            state.ai_show_threads = false;
            if state.ai_show_tasks {
                refresh_tasks()
            } else {
                Task::none()
            }
        }
        Message::AiTasksLoaded(list) => {
            state.ai_tasks = list;
            Task::none()
        }
        Message::AiTaskToggle(unit, enabled) => Task::perform(
            async move {
                let u = unit.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    crate::agent::tasks::set_enabled(&u, enabled)
                })
                .await;
                tokio::task::spawn_blocking(crate::agent::tasks::list)
                    .await
                    .unwrap_or_default()
            },
            Message::AiTasksLoaded,
        ),
        Message::AiTaskRun(unit) => Task::perform(
            async move {
                let _ =
                    tokio::task::spawn_blocking(move || crate::agent::tasks::run_now(&unit)).await;
            },
            |_| Message::Noop,
        ),
        Message::AiTaskDelete(unit) => Task::perform(
            async move {
                let _ =
                    tokio::task::spawn_blocking(move || crate::agent::tasks::delete(&unit)).await;
                tokio::task::spawn_blocking(crate::agent::tasks::list)
                    .await
                    .unwrap_or_default()
            },
            Message::AiTasksLoaded,
        ),
        Message::AiTaskEdit(name) => {
            state.ai_editor = iced::widget::text_editor::Content::with_text(&format!(
                "Edit the scheduled task \"{name}\": "
            ));
            state.ai_show_tasks = false;
            state.ai_show_memories = false;
            Task::none()
        }
    }
}

/// Reload the scheduled-task list off the render path (systemctl
/// round trips).
fn refresh_tasks() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(crate::agent::tasks::list)
                .await
                .unwrap_or_default()
        },
        Message::AiTasksLoaded,
    )
}

/// Outcome of a dispatch attempt — drives which haptic event
/// to fire on Hide / ToggleClickSelect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchOutcome {
    /// No slice targeted — tap-to-toggle, click on empty space.
    NoTarget,
    /// Slice targeted with a non-empty command and a visible
    /// predicate — `confirm` haptic fires.
    Actionable,
    /// Slice targeted but its command is empty OR its visible_if
    /// predicate evaluates false — `invalid` haptic fires.
    Unactionable,
}

/// Inspect the current state to decide what would happen if we
/// dispatched right now. Mirrors the actionability checks in
/// `RadialState::dispatch_and_close` (sub-item beats parent;
/// visible_if false → unactionable; empty command → unactionable).
fn haptic_outcome_for(state: &RadialState) -> DispatchOutcome {
    // Sub-item dispatch wins over the parent slice when a submenu
    // sub-item is highlighted.
    if let Some(sub) = state.submenu.as_ref() {
        if let Some(child_idx) = sub.highlighted {
            if let Some(parent) = state.slices.get(sub.parent) {
                if let Some(child) = parent.submenu.get(child_idx) {
                    return classify(child);
                }
            }
        }
    }
    let idx = match state.target_slice() {
        Some(i) => i,
        None => return DispatchOutcome::NoTarget,
    };
    let slice = match state.slices.get(idx) {
        Some(s) => s,
        None => return DispatchOutcome::NoTarget,
    };
    classify(slice)
}

/// Slice slot index the dispatch burst should anchor at. Sub-item
/// dispatches anchor at the parent slice rather than the sub-item
/// itself — the sub-item lives outside the menu disc on an arc,
/// and a burst centred there would clip the window edge. Anchoring
/// at the parent keeps the flourish inside the visible area while
/// still pointing at the wedge the user pressed.
fn dispatch_origin_for(state: &RadialState) -> Option<usize> {
    if let Some(sub) = state.submenu.as_ref() {
        if sub.highlighted.is_some() {
            return Some(sub.parent);
        }
    }
    state.target_slice()
}

fn classify(slice: &oxidemx_shared::Slice) -> DispatchOutcome {
    let visible = slice.visible_if.as_ref().map(|c| c.eval()).unwrap_or(true);
    if !visible {
        return DispatchOutcome::Unactionable;
    }
    // Submenu slices are "actionable" in that hovering them is
    // useful, but pressing them with no sub-item highlighted
    // doesn't dispatch anything — treat as unactionable so the
    // user gets `invalid` feedback for that confused state.
    if matches!(slice.kind, oxidemx_shared::ActionKind::Submenu) && !slice.submenu.is_empty() {
        return DispatchOutcome::Unactionable;
    }
    if slice.command.trim().is_empty() {
        return DispatchOutcome::Unactionable;
    }
    DispatchOutcome::Actionable
}

/// Convert a DispatchOutcome into the corresponding haptic Task.
fn haptic_outcome_task(outcome: DispatchOutcome) -> Task<Message> {
    let event = match outcome {
        DispatchOutcome::Actionable => "confirm",
        DispatchOutcome::Unactionable => "invalid",
        DispatchOutcome::NoTarget => return Task::none(),
    };
    Task::perform(
        crate::haptic_client::trigger_haptic(event.to_string()),
        |_| Message::Noop,
    )
}

/// Fire a slice-change haptic when the cursor crosses into a new
/// slot. No-op when the user enters empty space (target → None)
/// — only positive transitions get a pulse, otherwise the daemon
/// would burn the motor on every drag-to-cancel.
///
/// Calls `TriggerHaptic("slice_change")` directly rather than
/// `NotifySliceHover(idx)` because the daemon's
/// `notify_slice_hover` handler only emits a SliceSelected D-Bus
/// signal — it doesn't touch the haptic motor. `trigger_haptic`
/// is the haptic-firing method.
fn haptic_on_target_change(before: Option<usize>, after: Option<usize>) -> Task<Message> {
    if before == after {
        return Task::none();
    }
    if after.is_none() {
        return Task::none();
    }
    Task::perform(
        crate::haptic_client::trigger_haptic("slice_change".to_string()),
        |_| Message::Noop,
    )
}

/// Snapshot the open submenu's parent slice index, or `None` when
/// no submenu is open. Used to detect submenu_open / submenu_close
/// transitions across a single user input event.
fn submenu_parent_of(state: &RadialState) -> Option<usize> {
    state.submenu.as_ref().map(|s| s.parent)
}

/// Fire submenu_open when the cursor newly enters a submenu (None
/// → Some, OR Some(a) → Some(b!=a)). Fire submenu_close only on
/// Some → None — going submenu-to-submenu already produces a
/// slice_change pulse on the outer-ring crossing, and stacking
/// open+close on top would feel busy.
fn haptic_on_submenu_change(before: Option<usize>, after: Option<usize>) -> Task<Message> {
    let event = match (before, after) {
        (None, Some(_)) => "submenu_open",
        (Some(a), Some(b)) if a != b => "submenu_open",
        (Some(_), None) => "submenu_close",
        _ => return Task::none(),
    };
    Task::perform(
        crate::haptic_client::trigger_haptic(event.to_string()),
        |_| Message::Noop,
    )
}

fn view(state: &RadialState) -> Element<'_, Message> {
    let canvas = Canvas::new(Painter::new(state))
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));

    // Aurora backdrop — only stacked when the user has it on
    // (intensity > 0) AND the menu is at least partly visible.
    // Closed/dismissed menus skip the shader entirely so the
    // overlay window costs zero GPU when idle.
    let intensity = state.visuals.aurora_intensity.clamp(0.0, 1.0);
    // Every disc layer (shaders + canvas) fades out at the start of
    // the AI-chat morph — the painted caps take over visually. By
    // multiplying here, none of the 12 shader programs need to know
    // a split is happening; they just see the menu going to alpha 0.
    let morph = state.ai_morph_progress();
    let menu_alpha = state.menu.current.clamp(0.0, 1.0) * crate::chat_shell::disc_alpha(morph);
    let palette = &state.theme.theme.colors;
    let accent_rgba = oxidemx_shared::theme::parse_hex_rgba(&palette.accent)
        .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
        .unwrap_or([0.5, 0.5, 1.0, 1.0]);
    let mut layers: Vec<iced::Element<Message>> = Vec::with_capacity(8);
    // Pre-computed once so the drop-shadow block (and any later
    // shader that needs to normalise pixel radii) can reach it
    // without redundant arithmetic.
    let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
    // Single source of truth for the 3D-framing shaders' virtual
    // light direction. drop_shadow / disc_bevel / slice_bevel /
    // center_dome all read this so highlights and shadows stay
    // consistent across the disc when the user rotates the knob
    // in settings.
    let light_angle = state.visuals.light_angle_rad;

    // Drop shadow — bottom-most layer. Paints outside the disc
    // boundary, offset away from the virtual light source, so
    // the menu reads as a floating physical object instead of
    // pixels painted onto the screen. Goes BEFORE the aurora so
    // the aurora sits on top (the aurora paints inside the disc
    // anyway; the shadow is purely for the area outside).
    let drop_shadow_intensity = state.visuals.drop_shadow_intensity.clamp(0.0, 1.0);
    if drop_shadow_intensity > 0.001 && menu_alpha > 0.001 {
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let shadow = iced::widget::Shader::new(crate::render::drop_shadow::DropShadowProgram {
            outer_r: outer_norm,
            intensity: drop_shadow_intensity * menu_alpha,
            // Spread the shadow ~25% past the disc edge.
            spread: 0.25,
            // Sharpish inner edge so the disc reads as
            // sitting clearly above its shadow.
            falloff: 0.55,
            // Read from the shared light direction (above).
            light_angle,
            // Offset the shadow centre 6% of half-extent
            // toward the lower-right so it reads as a cast
            // shadow, not a glow.
            offset_dist: 0.06,
            shadow_color: [0.0, 0.0, 0.0, 0.65],
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(shadow.into());
    }

    // Compute the menu's `ComposedTransform` once and lift to the
    // shader-side `MenuXformRaw` block so menu-tracking shaders
    // (aurora + sdf_ring today; ripple/hover_glow/dispatch_burst
    // pending) inverse-transform their UVs and visually follow
    // the canvas when custom translate/rotate/flip tracks are set
    // on the menu element. Identity unless tracks are active —
    // zero perf impact on the preset path.
    let menu_t = crate::anim::evaluate_composed(
        &state.menu,
        &state.anim_config.menu.enter,
        &state.anim_config.menu.exit,
    );
    let menu_xform_raw =
        crate::render::animation::MenuXformRaw::from_composed(&menu_t, half_extent);

    // Aurora backdrop — bottom layer when enabled + menu visible.
    if intensity > 0.001 && menu_alpha > 0.001 {
        let accent2 = oxidemx_shared::theme::parse_hex_rgba(&palette.accent2)
            .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
            .unwrap_or(accent_rgba);
        let accent_dim = oxidemx_shared::theme::parse_hex_rgba(&palette.accent_dim)
            .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
            .unwrap_or(accent_rgba);
        let effective = intensity * menu_alpha;
        let aurora = iced::widget::Shader::new(crate::render::aurora::AuroraProgram::new(
            state.show_time.unwrap_or_else(std::time::Instant::now),
            accent_rgba,
            accent2,
            accent_dim,
            effective,
            menu_xform_raw,
        ))
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(aurora.into());
    }

    // SDF wedge ring spike — sits between aurora and the canvas
    // so the canvas's icons + text paint on top. The canvas's
    // wedge fills fade via `wedge_fill_mul` (hooked in
    // radial.rs::draw) so users can A/B by sliding intensity 0
    // → 1 and watching the canvas wedges fade out as the SDF
    // wedges fade in. Hover highlights + stroke + wash all live
    // in the SDF too — see `sdf_ring.wgsl` for the layered
    // composition.
    let sdf_intensity = state.visuals.sdf_ring_intensity.clamp(0.0, 1.0);
    if sdf_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0) / half_extent;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32 - 6.0) / half_extent;
        // Icon centres sit at ICON_ZONE_RADIUS (100 px) from
        // the menu centre; the icon background disc is
        // ICON_BG_RADIUS (26 px). Both normalised to half-extent
        // for the shader.
        let icon_r_norm = crate::geometry::ICON_ZONE_RADIUS as f32 / half_extent;
        // ICON_BG_RADIUS lives in radial.rs as 26.0; replicate the
        // constant here rather than re-exporting it — adding a
        // pub constant exposes a tiny piece of the renderer's
        // internals that nothing else needs.
        let icon_bg_norm = 26.0_f32 / half_extent;
        let scale = menu_alpha;
        let palette = &state.theme.theme.colors;
        let (sr, sg, sb, _) = oxidemx_shared::theme::parse_hex_rgba(&palette.surface0)
            .unwrap_or((0.18, 0.18, 0.20, 1.0));
        let surface0 = [sr as f32, sg as f32, sb as f32, 1.0];
        let colors = [surface0; 8];
        let (s1r, s1g, s1b, _) = oxidemx_shared::theme::parse_hex_rgba(&palette.surface1)
            .unwrap_or((0.25, 0.25, 0.28, 1.0));
        let surface1_color = [s1r as f32, s1g as f32, s1b as f32, 1.0];
        let (s2r, s2g, s2b, _) = oxidemx_shared::theme::parse_hex_rgba(&palette.surface2)
            .unwrap_or((0.4, 0.4, 0.45, 1.0));
        let surface2_color = [s2r as f32, s2g as f32, s2b as f32, 1.0];
        // Canvas's stroke uses surface2 as its base colour, then
        // lerps to accent on hover. Same here.
        let stroke_color = surface2_color;
        let (ar, ag, ab, _) =
            oxidemx_shared::theme::parse_hex_rgba(&palette.accent).unwrap_or((0.5, 0.5, 1.0, 1.0));
        let accent_color = [ar as f32, ag as f32, ab as f32, 1.0];
        let highlights = state.highlights.map(|t| t.current);
        // Canvas wedges meet at exactly the slice-boundary angle —
        // no transparent gap, just the stroke band painting a thin
        // separator line. Match that here.
        let gap_rad: f32 = 0.0;
        let bg_op = state.visuals.menu_background_opacity.clamp(0.0, 1.0);
        let highlight_op = state.visuals.slice_highlight_opacity.clamp(0.0, 1.0);
        let sdf = iced::widget::Shader::new(crate::render::sdf_ring::SdfRingProgram {
            inner_r: inner_norm * scale,
            outer_r: outer_norm * scale,
            gap_rad,
            intensity: sdf_intensity * menu_alpha,
            slot_count: state.active_slot_count() as u32,
            base_alpha: bg_op,
            stroke_half_px: 1.2,
            hover_wash_peak: 0.275 * highlight_op,
            icon_r: icon_r_norm * scale,
            icon_bg_radius: icon_bg_norm * scale,
            colors,
            highlights,
            stroke_color,
            accent_color,
            surface1_color,
            surface2_color,
            menu_xform: menu_xform_raw,
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(sdf.into());
    }

    layers.push(canvas.into());

    // Disc bevel — paints a rim light along the outer ring +
    // carved inset shadow at the inner ring, framing the disc
    // in 3D regardless of hover state. Layered ABOVE the canvas
    // because the lighting needs to sit on top of slice fills,
    // but only paints in thin bands at the boundaries (the wedge
    // interiors stay transparent so the canvas slice colours
    // come through unaffected).
    let disc_bevel_intensity = state.visuals.disc_bevel_intensity.clamp(0.0, 1.0);
    if disc_bevel_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        // Multiply by menu_alpha so the bevel grows along with
        // the menu's open animation — same pattern the SDF ring
        // shader uses to stay in lockstep with the canvas.
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let bevel = iced::widget::Shader::new(crate::render::disc_bevel::DiscBevelProgram {
            inner_r: inner_norm,
            outer_r: outer_norm,
            intensity: disc_bevel_intensity * menu_alpha,
            rim_width: 0.045,
            inset_width: 0.035,
            light_angle,
            shadow_strength: 0.7,
            rim_color: [1.0, 1.0, 1.0, 0.85],
            shadow_color: [0.0, 0.0, 0.0, 0.65],
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(bevel.into());
    }

    // Specular sweep — animated narrow band of light rotating
    // around the disc rim. Sits on top of disc_bevel (which is
    // static) so the rotating gleam reads as additional motion
    // catching the rim, not as a new structural element. Lit-
    // side gated so the sweep fades on the shadow hemisphere.
    let sweep_intensity = state.visuals.specular_sweep_intensity.clamp(0.0, 1.0);
    if sweep_intensity > 0.001 && menu_alpha > 0.001 {
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let sweep =
            iced::widget::Shader::new(crate::render::specular_sweep::SpecularSweepProgram {
                start: state.show_time.unwrap_or_else(std::time::Instant::now),
                inner_r: inner_norm,
                outer_r: outer_norm,
                intensity: sweep_intensity * menu_alpha,
                period_s: state.visuals.specular_sweep_period_s.max(0.5),
                // ~25° wide sweep — wide enough to read as
                // "polished surface" and not "laser pointer".
                half_width_rad: std::f32::consts::PI / 7.0,
                light_angle,
                sweep_color: [1.0, 1.0, 1.0, 0.85],
            })
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(sweep.into());
    }

    // Slice bevel — paints the radial dividers between wedges
    // as carved grooves with directional rim lighting on the lit
    // side. Each slice ends up reading as its own raised 3D
    // button. Sits between disc_bevel (outer/inner ring) and
    // centre_dome (puck) in the visual stack.
    let slice_bevel_intensity = state.visuals.slice_bevel_intensity.clamp(0.0, 1.0);
    if slice_bevel_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let slot_count = state.active_slot_count().max(1) as u32;
        let bevel = iced::widget::Shader::new(crate::render::slice_bevel::SliceBevelProgram {
            inner_r: inner_norm,
            outer_r: outer_norm,
            intensity: slice_bevel_intensity * menu_alpha,
            slot_count,
            // Width relative to half-extent. ~1.5% reads as
            // a subtle bevel at typical menu sizes.
            groove_width: 0.018,
            light_angle,
            rim_brightness: 0.9,
            shadow_amount: 0.7,
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(bevel.into());
    }

    // Centre dome — Phong-shaded sphere over the centre puck.
    // Like the bevel: layered above the canvas because the
    // shader's lighting needs to sit on top of the puck's base
    // colour. Edge-faded so it doesn't form a hard ring.
    let center_dome_intensity = state.visuals.center_dome_intensity.clamp(0.0, 1.0);
    if center_dome_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        let radius_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let dome = iced::widget::Shader::new(crate::render::center_dome::CenterDomeProgram {
            radius: radius_norm,
            intensity: center_dome_intensity * menu_alpha,
            light_angle,
            shininess: 32.0,
            rim_brightness: 0.6,
            shadow_amount: 0.5,
            specular_color: [1.0, 1.0, 1.0, 0.95],
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(dome.into());
    }

    // Hover glow — overlaid above the canvas when a slice is
    // hovered AND the highlight tween is non-zero. Uses the
    // hovered slice's colour (or accent fallback) and scales
    // with the tween's progress so the glow eases in/out
    // synchronously with the canvas-side wedge highlight.
    let hover_glow_intensity = state.visuals.hover_glow_intensity.clamp(0.0, 1.0);
    if hover_glow_intensity > 0.001 && menu_alpha > 0.001 {
        if let Some(idx) = state.target_slice() {
            let progress = state.highlights[idx].current.clamp(0.0, 1.0);
            if progress > 0.001 {
                let n = state.active_slot_count().max(1) as f32;
                let slice_degrees = std::f32::consts::PI * 2.0 / n;
                let bisector = (idx as f32) * slice_degrees - std::f32::consts::FRAC_PI_2;
                let half_sweep = slice_degrees / 2.0;
                // Wedge radii in normalised half-extent units.
                // Geometry::default() gives MENU_RADIUS=150 and
                // CENTER_ZONE_RADIUS=45 in a 484-px window
                // (half-extent = 242). Hard-coded here because
                // the shader works in clip-space [-1,1] and the
                // values aren't going to drift between frames.
                let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
                let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0) / half_extent;
                let outer_norm = (crate::geometry::MENU_RADIUS as f32 - 6.0) / half_extent;
                let slice = state.slices.get(idx);
                let color_key = slice
                    .and_then(|s| {
                        let c = s.color.trim();
                        if c.is_empty() {
                            None
                        } else {
                            Some(c)
                        }
                    })
                    .unwrap_or("accent");
                let palette = &state.theme.theme.colors;
                let (cr, cg, cb, _) = palette.slice_color_rgba(color_key);
                let color = [cr as f32, cg as f32, cb as f32, 1.0];
                let glow = iced::widget::Shader::new(crate::render::hover_glow::HoverGlowProgram {
                    bisector_rad: bisector,
                    half_sweep,
                    inner_r: inner_norm,
                    outer_r: outer_norm,
                    progress,
                    intensity: hover_glow_intensity * menu_alpha,
                    color,
                })
                .width(Length::Fixed(WINDOW_SIZE as f32))
                .height(Length::Fixed(WINDOW_SIZE as f32));
                layers.push(glow.into());
            }
        }
    }

    // Hover-tilt — paints inside-the-wedge directional lighting
    // (specular spot near cursor + soft shadow opposite). Layered
    // ABOVE hover_glow so its in-wedge tinting reads on top of
    // the edge aura. Uses the per-slice colour for the lit side
    // and the active accent for the specular dot.
    let hover_tilt_intensity = state.visuals.hover_tilt_intensity.clamp(0.0, 1.0);
    if hover_tilt_intensity > 0.001 && menu_alpha > 0.001 {
        if let Some(idx) = state.target_slice() {
            let progress = state.highlights[idx].current.clamp(0.0, 1.0);
            if progress > 0.001 {
                let n = state.active_slot_count().max(1) as f32;
                let slice_degrees = std::f32::consts::PI * 2.0 / n;
                let bisector = (idx as f32) * slice_degrees - std::f32::consts::FRAC_PI_2;
                let half_sweep = slice_degrees / 2.0;
                let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
                let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0) / half_extent;
                let outer_norm = (crate::geometry::MENU_RADIUS as f32 - 6.0) / half_extent;
                // Cursor in clip-UV [-1, 1]. RadialState stores
                // it in canvas pixels relative to centre, so just
                // divide by half_extent. Clamp generously so an
                // out-of-bounds cursor doesn't push the highlight
                // off-shader (it's gated by the inside-wedge SDF
                // anyway, but a sane uniform value avoids float
                // weirdness in the falloff math).
                let cursor_uv = [
                    (state.pointer_dx / half_extent as f64).clamp(-2.0, 2.0) as f32,
                    (state.pointer_dy / half_extent as f64).clamp(-2.0, 2.0) as f32,
                ];
                // Slice colour for the side wash; accent for the
                // specular dot. White-ish highlight reads as
                // light-source agnostic and pops against any
                // theme.
                let slice = state.slices.get(idx);
                let color_key = slice
                    .and_then(|s| {
                        let c = s.color.trim();
                        if c.is_empty() {
                            None
                        } else {
                            Some(c)
                        }
                    })
                    .unwrap_or("accent");
                let palette = &state.theme.theme.colors;
                let (ar, ag, ab, _) = palette.slice_color_rgba(color_key);
                let highlight_color = [1.0, 1.0, 1.0, 0.85];
                let accent_color = [ar as f32, ag as f32, ab as f32, 1.0];
                let tilt = iced::widget::Shader::new(crate::render::hover_tilt::HoverTiltProgram {
                    bisector_rad: bisector,
                    half_sweep,
                    inner_r: inner_norm,
                    outer_r: outer_norm,
                    progress,
                    intensity: hover_tilt_intensity * menu_alpha,
                    shadow_amount: state.visuals.hover_tilt_shadow.clamp(0.0, 1.0),
                    sharpness: state.visuals.hover_tilt_sharpness.clamp(0.0, 1.0),
                    cursor_uv,
                    highlight_color,
                    accent_color,
                })
                .width(Length::Fixed(WINDOW_SIZE as f32))
                .height(Length::Fixed(WINDOW_SIZE as f32));
                layers.push(tilt.into());
            }
        }
    }

    // Ripple — top layer when an event has triggered one and the
    // user has the effect on. Skipped when ripple_started is
    // None (no event fired) or the duration has elapsed (the
    // advance loop clears it).
    let ripple_intensity = state.visuals.ripple_intensity.clamp(0.0, 1.0);
    if let Some(started) = state.ripple_started {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if ripple_intensity > 0.001
            && menu_alpha > 0.001
            && elapsed_ms < crate::radial::RIPPLE_DURATION_MS
        {
            let progress = elapsed_ms as f32 / crate::radial::RIPPLE_DURATION_MS as f32;
            let ripple = iced::widget::Shader::new(crate::render::ripple::RippleProgram::new(
                progress,
                ripple_intensity * menu_alpha,
                accent_rgba,
            ))
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
            layers.push(ripple.into());
        }
    }

    // Page-transition shader overlay — runs whenever the
    // resolved shader style is non-None and the page-cycle
    // tween is active. `effective_style` combines the legacy
    // `style: Dissolve|Plasma` (canvas-only configs) with the
    // new explicit `shader.style` (orthogonal to whatever
    // canvas style is doing) so users can either keep their
    // old config OR layer e.g. SpinCrossfade canvas + Plasma
    // shader.
    let pt_cfg = &state.anim_config.page_transition;
    let shader_style = pt_cfg.shader.effective_style(pt_cfg.style);
    if !matches!(
        shader_style,
        oxidemx_shared::PageTransitionShaderStyle::None
    ) && state.previous_slices.is_some()
    {
        let progress = state.page_transition.current.clamp(0.0, 1.0);
        if progress > 0.001 && progress < 0.999 {
            let palette = &state.theme.theme.colors;
            let color_a = oxidemx_shared::theme::parse_hex_rgba(&palette.accent)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or([0.5, 0.5, 1.0, 1.0]);
            let color_b = oxidemx_shared::theme::parse_hex_rgba(&palette.accent2)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or(color_a);
            let style = match shader_style {
                oxidemx_shared::PageTransitionShaderStyle::Dissolve => {
                    crate::render::page_fx::PageFxStyle::Dissolve
                }
                _ => crate::render::page_fx::PageFxStyle::Plasma,
            };
            let s = &pt_cfg.shader;
            let layer = iced::widget::Shader::new(crate::render::page_fx::PageFxProgram {
                progress,
                style,
                intensity: menu_alpha * s.intensity.clamp(0.0, 1.0),
                dissolve_noise_scale: s.dissolve_noise_scale,
                dissolve_band_softness: s.dissolve_band_softness,
                plasma_wave_scale: s.plasma_wave_scale,
                plasma_wave_speed: s.plasma_wave_speed,
                color_a,
                color_b,
            })
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
            layers.push(layer.into());
        }
    }

    // Dispatch burst — top layer when an action just fired.
    // Deliberately NOT gated on `menu_alpha` so the burst keeps
    // playing as the menu fades out (and even after it's fully
    // closed): the user's last visible feedback should be the
    // celebratory flourish, not the ring sliding away.
    let burst_intensity = state.visuals.dispatch_burst_intensity.clamp(0.0, 1.0);
    if let (Some(started), Some(origin_idx)) = (state.dispatch_started, state.dispatch_origin) {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if burst_intensity > 0.001 && elapsed_ms < crate::radial::BURST_DURATION_MS {
            let progress = elapsed_ms as f32 / crate::radial::BURST_DURATION_MS as f32;
            let slot_count = state.active_slot_count();
            let origin = crate::render::dispatch_burst::slice_origin(origin_idx, slot_count);
            let palette = &state.theme.theme.colors;
            let slice = state.slices.get(origin_idx);
            let color_key = slice
                .and_then(|s| {
                    let c = s.color.trim();
                    if c.is_empty() {
                        None
                    } else {
                        Some(c)
                    }
                })
                .unwrap_or("accent");
            let (cr, cg, cb, _) = palette.slice_color_rgba(color_key);
            let style = crate::render::dispatch_burst::DispatchBurstStyleGpu::from(
                state.visuals.dispatch_burst_style,
            );
            let burst = iced::widget::Shader::new(
                crate::render::dispatch_burst::DispatchBurstProgram::new(
                    progress,
                    burst_intensity,
                    style,
                    origin,
                    [cr as f32, cg as f32, cb as f32, 1.0],
                ),
            )
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
            layers.push(burst.into());
        }
    }

    let main_stack: Element<'_, Message> = if layers.len() == 1 {
        layers.into_iter().next().unwrap()
    } else {
        iced::widget::Stack::with_children(layers).into()
    };
    // Centre the 484 px disc stack horizontally so wider persisted
    // chat sizes keep the disc (and the morph origin) in the middle
    // of the window. Vertical anchor stays the top square.
    let main_stack: Element<'_, Message> = container(main_stack)
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .into();

    // Chat shell composition: while the disc ↔ chat morph is in
    // flight (or parked at the chat end), stack the caps painter
    // over the (fading) disc layers, and the chat widgets over the
    // caps once the arcs are nearly parked. Outside the morph this
    // branch costs nothing — the disc path is exactly as before.
    if morph > 0.001 {
        let caps = Canvas::new(crate::chat_shell::CapsPainter::new(state))
            .width(Length::Fill)
            .height(Length::Fill);
        let open_alpha = state.menu_open_alpha();
        let chat_a = crate::chat_shell::chat_alpha(morph) * open_alpha;
        let body_a = crate::chat_shell::cap_alpha(morph) * open_alpha;

        // Window body — the chat's full-window chrome per the
        // redesign: 24 px outer radius, near-opaque `base` fill,
        // hairline `surface1` border, accent-tinted shadow. Fades
        // in on the cap ramp so the chrome arrives as the disc
        // hands over.
        let base_c = to_iced_color(&palette.base, Color::from_rgba(0.07, 0.08, 0.09, 1.0));
        let surface1_c = to_iced_color(&palette.surface1, Color::from_rgba(0.14, 0.16, 0.20, 1.0));
        let accent_c = to_iced_color(&palette.accent, Color::from_rgb(0.5, 0.5, 1.0));
        let body = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(Color {
                    a: 0.94 * body_a,
                    ..base_c
                })),
                border: iced::border::Border {
                    color: Color {
                        a: body_a,
                        ..surface1_c
                    },
                    width: 1.0,
                    radius: 24.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color {
                        a: 0.08 * body_a,
                        ..accent_c
                    },
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 42.0,
                },
                ..Default::default()
            });

        let mut children: Vec<Element<'_, Message>> = vec![body.into()];

        // Aurora backdrop for the chat — the same theme-tinted
        // shader the disc uses, stretched over the whole window.
        // Its radial falloff discards past the inscribed ellipse,
        // so nothing spills into the transparent rounded corners.
        if intensity > 0.001 && body_a > 0.001 {
            let accent2 = oxidemx_shared::theme::parse_hex_rgba(&palette.accent2)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or(accent_rgba);
            let accent_dim = oxidemx_shared::theme::parse_hex_rgba(&palette.accent_dim)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or(accent_rgba);
            let chat_aurora = iced::widget::Shader::new(crate::render::aurora::AuroraProgram::new(
                state.show_time.unwrap_or_else(std::time::Instant::now),
                accent_rgba,
                accent2,
                accent_dim,
                // Quieter than the disc's pass — it's a backdrop
                // for reading text, not a hero element.
                intensity * 0.45 * body_a,
                crate::render::animation::MenuXformRaw::IDENTITY,
            ))
            .width(Length::Fill)
            .height(Length::Fill);
            children.push(chat_aurora.into());
        }

        children.push(main_stack);
        children.push(caps.into());
        if chat_a > 0.01 {
            children.push(crate::chat_ui::view(state, chat_a));
        }
        iced::widget::Stack::with_children(children)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        main_stack
    }
}

fn subscription(_state: &RadialState) -> Subscription<Message> {
    // Three streams merged into the same Message channel:
    //   * D-Bus listener — translates the daemon's three signal
    //     streams into OverlayEvent values.
    //   * Inotify config watcher — yields a fresh AppConfig each
    //     time `~/.config/oxidemx/config.json` is saved.
    //   * 60 Hz frame ticker — keeps animations smooth while a
    //     menu is visible. (Cheap when nothing animates because
    //     update() returns Task::none() immediately.)
    Subscription::batch([
        Subscription::run(crate::dbus::stream).map(Message::Overlay),
        Subscription::run(crate::config::watch_stream)
            .map(|cfg| Message::ConfigReloaded(Box::new(cfg))),
        Subscription::run(ai_question_stream),
        Subscription::run(ai_stream_stream),
        iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick),
        iced::window::events().map(|(id, event)| match event {
            iced::window::Event::Opened { .. } => Message::WindowOpened(id),
            // Routed through its own message (not ToggleDismiss)
            // so update() can ignore focus loss while the chat
            // shell is up — see Message::WindowUnfocused.
            iced::window::Event::Unfocused => Message::WindowUnfocused,
            iced::window::Event::Focused => Message::WindowFocused,
            iced::window::Event::Resized(size) => Message::WindowResized(size),
            _ => Message::Noop,
        }),
    ])
}

// =============================================================================
// AI ASSISTANT PANEL DRAWING & HELPERS
// =============================================================================

fn to_iced_color(hex: &str, default: Color) -> Color {
    oxidemx_shared::theme::parse_hex_rgba(hex)
        .map(|(r, g, b, a)| Color::from_rgba(r as f32, g as f32, b as f32, a as f32))
        .unwrap_or(default)
}

fn ai_question_stream() -> impl futures_util::stream::Stream<Item = Message> {
    let (tx, rx) = async_channel::unbounded();

    let (q_tx, mut q_rx) = tokio::sync::mpsc::channel(10);
    *crate::ai_client::QUESTION_TX.lock().unwrap() = Some(q_tx);

    tokio::task::spawn(async move {
        while let Some(pending) = q_rx.recv().await {
            let _ = tx.send(Message::AiQuestionReceived(pending)).await;
        }
    });

    rx
}

/// Registers the global stream-event channel and forwards
/// `(thread, StreamEvent)` pairs — text deltas + tool-activity
/// labels — into the iced message loop. Same pattern as
/// `ai_question_stream`.
fn ai_stream_stream() -> impl futures_util::stream::Stream<Item = Message> {
    let (tx, rx) = async_channel::unbounded();

    let (s_tx, mut s_rx) = tokio::sync::mpsc::channel(64);
    *crate::ai_client::STREAM_TX.lock().unwrap() = Some(s_tx);

    tokio::task::spawn(async move {
        while let Some(event) = s_rx.recv().await {
            let _ = tx.send(Message::AiStream(event)).await;
        }
    });

    rx
}

/// Snap the conversation scrollable to the newest message.
fn scroll_chat_to_end() -> Task<Message> {
    iced::widget::operation::snap_to_end(crate::chat_ui::body::CHAT_SCROLL_ID)
}

/// "2h ago"-style label for the thread list + memories view.
pub(crate) fn rel_time(ts: u64) -> String {
    if ts == 0 {
        return "earlier".to_string();
    }
    let now = crate::radial::now_secs();
    let delta = now.saturating_sub(ts);
    match delta {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", delta / 60),
        3600..=86_399 => format!("{}h ago", delta / 3600),
        _ => format!("{}d ago", delta / 86_400),
    }
}

#[allow(dead_code)]
fn _ensure_link(_e: &OverlayEvent) {
    error!("only here so the OverlayEvent path is referenced from app");
    info!("ditto");
}
