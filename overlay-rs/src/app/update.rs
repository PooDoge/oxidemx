//! `update` — the iced message handler, plus the dispatch-outcome
//! classification and haptic-trigger helpers it drives.

use iced::{Size, Task};
use tracing::{debug, error, info, warn};

use super::subscriptions::scroll_chat_to_end;
use super::{Message, APP_ID};
use crate::dbus::OverlayEvent;
use crate::geometry::WINDOW_SIZE;
use crate::radial::{ChatMessage, RadialState};

pub(super) fn update(state: &mut RadialState, message: Message) -> Task<Message> {
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
            // Debounced persist of the chat size after a native grip
            // resize: the compositor streams configure events while
            // the user drags; once it's been quiet, query the
            // monitor so the saved size can be clamped below it —
            // a window at (or clamped to) full output size makes
            // Mutter composite the surface opaque, turning every
            // transparent region solid black on the next launch.
            if let Some((at, pending)) = state.chat_size_pending_save {
                if at.elapsed() >= std::time::Duration::from_millis(800) {
                    state.chat_size_pending_save = None;
                    if let Some(id) = state.window_id {
                        return iced::window::monitor_size(id)
                            .map(move |m| Message::ChatSizePersist(pending, m));
                    }
                    crate::config::save_chat_size(
                        pending.0.round() as u32,
                        pending.1.round() as u32,
                    );
                }
            }
            // Vision-loop dev hook: optional programmatic resize at
            // half-delay (OXIDEMX_VISION_RESIZE="WxH") to exercise
            // the grip-commit path headlessly, then one screenshot.
            if !state.vision_shot_taken {
                static VISION_RESIZED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if let (Ok(spec), Some(t0), Some(id)) = (
                    std::env::var("OXIDEMX_VISION_RESIZE"),
                    state.show_time,
                    state.window_id,
                ) {
                    let delay_ms: u64 = std::env::var("OXIDEMX_VISION_DELAY_MS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1500);
                    if t0.elapsed() >= std::time::Duration::from_millis(delay_ms / 2)
                        && !VISION_RESIZED.swap(true, std::sync::atomic::Ordering::SeqCst)
                    {
                        if let Some((w, h)) = spec
                            .split_once('x')
                            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                        {
                            state.win_size = (w, h);
                            return iced::window::resize(id, Size::new(w, h));
                        }
                    }
                }
            }
            if !state.vision_shot_taken {
                if let (Ok(_), Some(t0), Some(id)) = (
                    std::env::var("OXIDEMX_VISION_SHOT"),
                    state.show_time,
                    state.window_id,
                ) {
                    let delay_ms: u64 = std::env::var("OXIDEMX_VISION_DELAY_MS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1500);
                    if t0.elapsed() >= std::time::Duration::from_millis(delay_ms) {
                        state.vision_shot_taken = true;
                        return iced::window::screenshot(id).map(Message::VisionShot);
                    }
                }
            }
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
            // disc is anchored in the window's top-left 484 px
            // square, so the offset is the disc's own half-size
            // regardless of how large the chat-sized window is.
            let half_x = (WINDOW_SIZE / 2.0) as i32;
            let half_y = (WINDOW_SIZE / 2.0) as i32;
            // Three parallel D-Bus round-trips: window position,
            // focus query, and the menu_appear haptic pulse.
            // Batching keeps perceived open latency bounded by
            // the slowest of the three.
            // menu_appear haptic always fires on Show → kick the
            // ripple too so visual feedback synchronises with the
            // motor pulse.
            if !std::env::var("OXIDEMX_SHOW_SKIP")
                .unwrap_or_default()
                .contains("ripple")
            {
                state.trigger_ripple();
            }

            // OXIDEMX_SHOW_SKIP bisect gate (comma list:
            // move_to,minimize,focus,ext) — lets us isolate which
            // Show step flips the Wayland surface opaque.
            let skip = std::env::var("OXIDEMX_SHOW_SKIP").unwrap_or_default();
            let window_tasks = if let Some(id) = state.window_id {
                let mut v: Vec<Task<Message>> = Vec::new();
                if !skip.contains("move_to") {
                    v.push(iced::window::move_to(
                        id,
                        iced::Point::new((x - half_x) as f32, (y - half_y) as f32),
                    ));
                }
                if !skip.contains("minimize") {
                    v.push(iced::window::minimize(id, false));
                }
                if !skip.contains("focus") {
                    v.push(iced::window::gain_focus(id));
                }
                Task::batch(v)
            } else {
                Task::none()
            };

            Task::batch(vec![
                window_tasks,
                if std::env::var("OXIDEMX_SHOW_SKIP")
                    .unwrap_or_default()
                    .contains("ext")
                {
                    Task::none()
                } else {
                    Task::perform(
                        oxidemx_window::cursor_helper::move_overlay(
                            APP_ID.to_string(),
                            x - half_x,
                            y - half_y,
                            -1,
                        ),
                        Message::Positioned,
                    )
                },
                if std::env::var("OXIDEMX_SHOW_SKIP")
                    .unwrap_or_default()
                    .contains("class")
                {
                    Task::none()
                } else {
                    Task::perform(
                        oxidemx_window::cursor_helper::get_focused_window_class(APP_ID.to_string()),
                        Message::FocusedClassResolved,
                    )
                },
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
        Message::ChatResizeStart => {
            // Hand the whole gesture to the compositor: a native
            // xdg_toplevel.resize tracks the pointer, streams real
            // configure events (so the layout truly re-flows live),
            // and ends itself on release. Client-side ghost dragging
            // and one-shot `window::resize` both proved unreliable
            // on Mutter — the surface kept its old buffer and got
            // stretched.
            state.activate_chat();
            if let Some(id) = state.window_id {
                iced::window::drag_resize(id, iced::window::Direction::SouthEast)
            } else {
                Task::none()
            }
        }
        Message::WindowResized(size) => {
            state.win_size = (size.width, size.height);
            // During a chat-shell grip resize, remember the size for
            // a debounced persist (flushed by Tick once the stream
            // of configure events goes quiet).
            if state.ai_morph_progress() > 0.9 {
                state.chat_size_pending_save =
                    Some((std::time::Instant::now(), (size.width, size.height)));
            }
            Task::none()
        }
        Message::ChatSizePersist((w, h), monitor) => {
            let (mut w, mut h) = (w, h);
            if let Some(m) = monitor {
                // 90% of the output: keeps the surface comfortably
                // off Mutter's "this is basically fullscreen" paths.
                w = w.min(m.width * 0.9);
                h = h.min(m.height * 0.9);
            }
            info!(w, h, "chat size persisted (monitor-clamped)");
            crate::config::save_chat_size(w.round() as u32, h.round() as u32);
            Task::none()
        }
        Message::WidgetSample(snap) => {
            state.widgets.apply(snap);
            Task::none()
        }
        Message::VisionShot(shot) => {
            // Dev-only: encode + save, then exit — each vision run
            // captures exactly one surface state.
            let path = std::env::var("OXIDEMX_VISION_SHOT")
                .unwrap_or_else(|_| "/tmp/oxidemx-vision.png".into());
            let (w, h) = (shot.size.width, shot.size.height);
            match image::RgbaImage::from_raw(w, h, shot.rgba.to_vec()) {
                Some(img) => match img.save(&path) {
                    Ok(()) => info!(path, "vision screenshot saved"),
                    Err(e) => error!(%e, "vision screenshot save failed"),
                },
                None => error!("vision screenshot: byte size mismatch"),
            }
            std::process::exit(0);
        }
        Message::DialAdjust { idx, direction } => {
            if let Some(kind) = state.slices.get(idx).and_then(|s| s.dial) {
                crate::actions::adjust_dial(kind, direction);
                // Optimistic local bump so the wedge's % readout
                // tracks the wheel instantly; the sampler corrects
                // it on its next pass.
                let step = 5i16 * direction as i16;
                let bump = |v: &mut Option<u8>| {
                    if let Some(cur) = v {
                        *v = Some((*cur as i16 + step).clamp(0, 100) as u8);
                    }
                };
                match kind {
                    oxidemx_shared::DialKind::Brightness => {
                        bump(&mut state.widgets.snap.brightness_percent)
                    }
                    oxidemx_shared::DialKind::Volume => {
                        bump(&mut state.widgets.snap.volume_percent)
                    }
                }
            }
            Task::none()
        }
        Message::WindowUnfocused => {
            // Vision-loop instances must survive focus churn from
            // the screenshot portal — never dismiss them.
            if std::env::var_os("OXIDEMX_VISION_SHOT").is_some() {
                return Task::none();
            }
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
            // Vision-loop dev hook (OXIDEMX_VISION_SHOT=<png path>):
            // self-show without any daemon signal, optionally jump
            // to OXIDEMX_START_PAGE, and let the Tick handler take
            // a window screenshot after OXIDEMX_VISION_DELAY_MS.
            // Used by the design-comparison loop; inert in normal
            // runs.
            if std::env::var("OXIDEMX_VISION_SHOT").is_ok()
                && std::env::var_os("OXIDEMX_VISION_NOSHOW").is_none()
            {
                state.show();
                if let Some(page) = std::env::var("OXIDEMX_START_PAGE")
                    .ok()
                    .and_then(|p| p.parse::<usize>().ok())
                {
                    state.set_active_page(page);
                }
                if let Some(slot) = std::env::var("OXIDEMX_VISION_HOVER")
                    .ok()
                    .and_then(|p| p.parse::<usize>().ok())
                {
                    state.vision_force_hover(slot);
                }
            }
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
