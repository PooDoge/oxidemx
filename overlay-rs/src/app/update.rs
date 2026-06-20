//! `update` — the iced message handler, plus the dispatch-outcome
//! classification and haptic-trigger helpers it drives.

use iced::{Size, Task};
use tracing::{debug, error, info, warn};

use super::subscriptions::{scroll_chat_to, scroll_chat_to_end};
use super::{Message, APP_ID};
use crate::dbus::OverlayEvent;
use crate::geometry::WINDOW_SIZE;
use crate::radial::{ChatMessage, RadialState};

pub(super) fn update(state: &mut RadialState, message: Message) -> Task<Message> {
    // The chat WINDOW reuses this update fn but is not the radial overlay:
    // ignore the daemon's radial show/hide and the puck/handoff geometry.
    if state.chat_window_mode {
        match &message {
            // Daemon radial show/hide + puck/handoff geometry + chat-shell drag.
            Message::Overlay(_)
            | Message::HandoffPointer { .. }
            | Message::HandoffClick { .. }
            | Message::ChatHeaderPressed
            | Message::ChatResizeStart
            // Radial-only triggers (daemon/keyboard driven) — inert here.
            | Message::CyclePage(_)
            | Message::ToggleCursor { .. }
            | Message::ToggleClickSelect
            | Message::ToggleDismiss
            // CRITICAL: a normal window loses focus routinely; the overlay's
            // arm calls state.dismiss() (morph<0.5 here), which is precisely the
            // dismiss-on-unfocus behavior this window exists to avoid.
            | Message::WindowUnfocused => return Task::none(),
            _ => {}
        }
    }
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
                | Message::AiOpenCommandCenter
                | Message::AiOpenAgentsConfig
                | Message::AiOpenMcpConfig
                | Message::AiNewChat
                | Message::AiToggleThreads
                | Message::AiSelectThread(_)
                | Message::AiChooseOption(_)
                | Message::AiStopRequest
                | Message::AiLinkClicked(_)
                | Message::AiCopyText(_)
                | Message::AiBubbleMenu(_)
                | Message::AiBubbleSelect(_)
                | Message::AiSelectAction(_)
                | Message::AiSelectExit
                | Message::AiPasteToInput
                | Message::AiScrollToBottom
                | Message::AiToggleSkills
                | Message::AiShowView(_)
                | Message::AiCardToggle(_)
                | Message::AiLightboxOpen(_)
                | Message::AiLightboxClose
                | Message::AiSkillEnable(_, _)
                | Message::AiSkillsSearch(_)
                | Message::AiPaletteSelect(_)
                | Message::AiPaletteRun
                | Message::AiRetryLast
                | Message::AiThreadsSearch(_)
                | Message::AiExportThread(_)
                | Message::AiAttachPick
                | Message::AiAttachClear
                | Message::AiToggleMemories
                | Message::AiMemorySearch(_)
                | Message::AiMemoryDelete(_)
                | Message::AiMemoryPin(_, _)
                | Message::AiWatchFlow(_)
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
            // Drive the eased "↓ Latest" auto-scroll. advance_animations
            // already stepped the tween this frame; emit a snap_to to the
            // eased relative offset. We gate on the `active` flag rather
            // than is_idle() so the settle frame — where `current` has
            // just reached 1.0 and the tween became idle — still emits
            // its final exact snap before we stop.
            if state.ai_scroll_active {
                let y = state.ai_scroll_tween.current;
                if state.ai_scroll_tween.is_idle() {
                    state.ai_scroll_active = false;
                    state.ai_chat_at_bottom = true;
                }
                return scroll_chat_to(y);
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
            // The widget-host worker re-derives its desired instance
            // set (load/drop/SettingsChanged) from the same config
            // the UI just applied.
            crate::widget_host::send(oxidemx_widget_host::HostCtl::ConfigChanged(
                std::sync::Arc::new((*cfg).clone()),
            ));
            state.reload_from(&cfg);
            // Prune widget_scenes and widget_failed for instances that
            // no longer correspond to any Custom-widget slice in the new
            // config. Derives expected InstanceIds the same way the
            // worker's desired_instances does (explicit instance_key wins,
            // otherwise the shared <page-slug>.slot<N> helper).
            let expected: std::collections::HashSet<oxidemx_widget_host::InstanceId> = {
                let legacy_page;
                let pages: Vec<(&str, &[oxidemx_shared::config::Slice])> =
                    if cfg.radial_menu.pages.is_empty() {
                        legacy_page = ("Default", cfg.radial_menu.slices.as_slice());
                        vec![legacy_page]
                    } else {
                        cfg.radial_menu
                            .pages
                            .iter()
                            .map(|p| (p.name.as_str(), p.slices.as_slice()))
                            .collect()
                    };
                let mut set = std::collections::HashSet::new();
                for (page_name, slices) in pages {
                    for (slot, slice) in slices.iter().enumerate() {
                        let Some(w) = &slice.widget else { continue };
                        let oxidemx_shared::WidgetSource::Custom(widget_id) = &w.source else {
                            continue;
                        };
                        let instance_key = w.instance_key.clone().unwrap_or_else(|| {
                            oxidemx_shared::widgets::instance_key(page_name, slot)
                        });
                        set.insert(oxidemx_widget_host::InstanceId {
                            instance_key,
                            widget_id: widget_id.clone(),
                        });
                    }
                }
                set
            };
            state.widget_scenes.retain(|id, _| expected.contains(id));
            state.widget_failed.retain(|id, _| expected.contains(id));
            Task::none()
        }
        Message::WidgetHost(ev) => {
            match ev {
                oxidemx_widget_host::HostEvent::Scene {
                    instance,
                    scene,
                    revision,
                } => {
                    // A live scene clears any earlier failure (the
                    // worker reloaded the instance after a rescan or
                    // config edit). Storing it is all a redraw needs:
                    // the painter rebuilds its Frame every draw and
                    // the cache_epsilon buster (do NOT remove it)
                    // keeps iced's layer cache from resurrecting
                    // stale frames — same path the sampler's
                    // WidgetSample updates ride.
                    state.widget_failed.remove(&instance);
                    state.widget_scenes.insert(instance, (scene, revision));
                }
                oxidemx_widget_host::HostEvent::InstanceFailed { instance, error } => {
                    warn!(
                        widget = %instance.widget_id,
                        key = %instance.instance_key,
                        error,
                        "widget instance failed — rendering fallback wedge"
                    );
                    state.widget_scenes.remove(&instance);
                    state.widget_failed.insert(instance, error);
                }
                oxidemx_widget_host::HostEvent::RegistryChanged(list) => {
                    debug!(count = list.len(), "widget registry updated");
                    // Prune scenes/failures for any widget_id no longer
                    // present in the registry (uninstalled widget).
                    let installed_ids: std::collections::HashSet<&str> =
                        list.iter().map(|s| s.id.as_str()).collect();
                    state
                        .widget_scenes
                        .retain(|id, _| installed_ids.contains(id.widget_id.as_str()));
                    state
                        .widget_failed
                        .retain(|id, _| installed_ids.contains(id.widget_id.as_str()));
                    state.widget_registry = list.into_iter().map(|s| (s.id.clone(), s)).collect();
                }
            }
            Task::none()
        }
        Message::WidgetScroll { idx, delta } => {
            state.send_widget_slice_event(idx, oxidemx_widget_host::SliceEvent::Scroll(delta));
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
            // One-shot boot hook: consolidate the agent memory store
            // when it's due (size/age trigger). App start is the
            // overlay's idle time — never collides with a live
            // conversation. The rails in agent::memory make a bad
            // pass a no-op rather than data loss.
            static CONSOLIDATE_ONCE: std::sync::Once = std::sync::Once::new();
            let mut task = Task::none();
            CONSOLIDATE_ONCE.call_once(|| {
                task = Task::future(async {
                    crate::ai_client::tools::auto_consolidate_if_due().await;
                })
                .discard();
            });
            task
        }
        Message::AiEditorAction(action) => {
            state.ai_editor.perform(action);
            refresh_palette(state);
            Task::none()
        }
        Message::AiSubmitPrompt => {
            let typed = state.ai_editor.text().trim().to_string();
            let attachment = state.ai_attachment.take();
            if state.ai_loading || (typed.is_empty() && attachment.is_none()) {
                return Task::none();
            }
            // An image attachment is sent to the model directly (vision);
            // any other file is read by the agent via its read_file /
            // parse_document tools. `image` carries (mime, bytes).
            let image: Option<(String, Vec<u8>)> = attachment.as_ref().and_then(|p| {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase());
                let mime = match ext.as_deref() {
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("gif") => "image/gif",
                    Some("webp") => "image/webp",
                    _ => return None,
                };
                let bytes = std::fs::read(p).ok()?;
                Some((mime.to_string(), bytes))
            });

            // The bubble shows the user's text + a 📎 chip.
            let (bubble_text, prompt) = match &attachment {
                Some(p) => {
                    let fname = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.display().to_string());
                    let bubble = if typed.is_empty() {
                        format!("📎 {fname}")
                    } else {
                        format!("{typed}\n\n📎 {fname}")
                    };
                    let sent = if image.is_some() {
                        // The model sees the image; no read_file needed.
                        if typed.is_empty() {
                            "What's in this image?".to_string()
                        } else {
                            typed.clone()
                        }
                    } else {
                        format!(
                            "{typed}\n\n[The user attached a file — read it with read_file \
                             (or parse_document for PDF/DOCX/XLSX/etc.) and use its contents: {}]",
                            p.display()
                        )
                    };
                    (bubble, sent)
                }
                None => (typed.clone(), typed.clone()),
            };
            // Passive memory capture from explicit user cues
            // ("remember …", "my name is …") — zero-cost, high-precision.
            if !typed.is_empty() {
                crate::agent::memory::capture_from_user(&typed);
            }
            // Keep the image path for the bubble thumbnail.
            let thumb_path: Option<String> = if image.is_some() {
                attachment.as_ref().map(|p| p.display().to_string())
            } else {
                None
            };
            state.ai_editor = iced::widget::text_editor::Content::new();
            let now = crate::radial::now_secs();
            let chat = state.chat_mut();
            if chat.history.is_empty() {
                // First prompt names the thread for the history list.
                chat.title = bubble_text.chars().take(48).collect();
            }
            let mut user_msg = ChatMessage::user(bubble_text);
            user_msg.image_path = thumb_path;
            chat.history.push(user_msg);
            chat.updated_at = now;
            state.ai_loading = true;
            state.ai_turn_tokens = (0, 0);
            state.ai_activity = Some("Thinking…".to_string());
            let mode = state.chat().mode;
            let model = state.chat().model.clone();
            let thread_idx = state.ai_active;
            // Each thread carries a stable session id (survives the thread
            // list's index-shifting deletes). Mint one on first turn; the
            // session manager keys the thread's reused provider on it.
            let session_id = {
                let chat = state.chat_mut();
                if chat.session_id.is_none() {
                    chat.session_id = Some(crate::agent_runtime::new_session_id());
                }
                chat.session_id.clone().unwrap()
            };
            let sink = crate::ai_client::stream_sink_for_thread(thread_idx);
            // Prior turns shipped as context (providers are stateless;
            // history lives client-side). For long threads, ship the
            // rolling summary + only the messages after summary_upto, so
            // context stays bounded without losing earlier facts.
            let history: Vec<(bool, String)> = {
                let chat = state.chat();
                let h = &chat.history;
                let end = h.len().saturating_sub(1); // exclude the just-pushed user msg
                let start = chat.summary_upto.min(end);
                let mut v: Vec<(bool, String)> = Vec::new();
                if !chat.summary.is_empty() {
                    v.push((
                        false,
                        format!("[Summary of earlier conversation]\n{}", chat.summary),
                    ));
                }
                v.extend(h[start..end].iter().map(|m| (m.is_user, m.text.clone())));
                v
            };
            let (task, handle) = if state.use_agentd {
                // ── agentd path ──────────────────────────────────────────────
                // send_message returns the turn-id; the reply arrives via the
                // `event` D-Bus signal handled in `Message::AgentdEvent` below.
                // We keep ai_loading = true until `AgentdFinal` arrives.
                let session_id_remote = session_id.clone();
                Task::perform(
                    async move {
                        crate::ai_client::ask_ai_remote(&session_id_remote, &prompt, &model)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |res| match res {
                        Ok(_turn_id) => Message::Noop,
                        Err(e) => Message::AiResponseReceived(
                            thread_idx,
                            Err(format!("agentd send failed: {e}")),
                        ),
                    },
                )
                .abortable()
            } else {
                // ── in-proc path (default) ───────────────────────────────────
                Task::perform(
                    async move {
                        crate::ai_client::ask_ai(
                            mode, &model, &prompt, sink, &history, image, &session_id,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    },
                    move |res| Message::AiResponseReceived(thread_idx, res),
                )
                .abortable()
            };
            state.ai_abort = Some(handle);
            Task::batch([task, scroll_chat_to_end()])
        }
        Message::AiResponseReceived(thread_idx, res) => {
            state.ai_loading = false;
            state.ai_activity = None;
            state.ai_stream = None;
            state.ai_stream_md = Vec::new();
            state.ai_abort = None;
            let Some(chat) = state.ai_threads.get_mut(thread_idx) else {
                return Task::none();
            };
            match res {
                Ok((reply, next_session_id)) => {
                    chat.session_id = next_session_id;
                    let mut m = ChatMessage::assistant(reply);
                    m.tokens = state.ai_turn_tokens;
                    chat.history.push(m);
                }
                Err(err) => {
                    chat.history.push(ChatMessage::error(err));
                }
            }
            chat.updated_at = crate::radial::now_secs();
            crate::radial::save_chat_threads(&state.ai_threads);
            state.trigger_ripple();

            // Roll up older turns into the thread summary once the
            // unsummarized tail grows large, so future turns stay within
            // a bounded context. Keep the most recent RECENT_KEEP raw.
            const SUMMARIZE_THRESHOLD: usize = 24;
            const RECENT_KEEP: usize = 8;
            let chat = &state.ai_threads[thread_idx];
            let unsummarized = chat.history.len().saturating_sub(chat.summary_upto);
            let summary_task = if unsummarized > SUMMARIZE_THRESHOLD {
                let cutoff = chat.history.len() - RECENT_KEEP;
                let prior = chat.summary.clone();
                let model = chat.model.clone();
                let msgs: Vec<(bool, String)> = chat.history[chat.summary_upto..cutoff]
                    .iter()
                    .filter(|m| !m.is_error)
                    .map(|m| (m.is_user, m.text.clone()))
                    .collect();
                Some(Task::perform(
                    async move { crate::agent_runtime::summarize(&model, &prior, &msgs).await },
                    move |sum| Message::AiSummaryUpdated(thread_idx, sum, cutoff),
                ))
            } else {
                None
            };
            let mut tasks: Vec<Task<Message>> = vec![scroll_chat_to_end()];
            if let Some(t) = summary_task {
                tasks.push(t);
            }
            // Fetch any remote images in the new AI reply for inline display.
            let urls: Vec<String> = state
                .ai_threads
                .get(thread_idx)
                .and_then(|c| c.history.last())
                .filter(|m| !m.is_user)
                .map(|m| collect_remote_image_urls(&m.md))
                .unwrap_or_default();
            for url in urls {
                if !state.ai_image_cache.contains_key(&url) {
                    state
                        .ai_image_cache
                        .insert(url.clone(), crate::radial::ImgState::Loading);
                    tasks.push(fetch_image_task(url));
                }
            }
            Task::batch(tasks)
        }
        Message::AiImageFetched(url, bytes) => {
            let entry = match bytes {
                Some(b) => {
                    crate::radial::ImgState::Ready(iced::widget::image::Handle::from_bytes(b))
                }
                None => crate::radial::ImgState::Failed,
            };
            state.ai_image_cache.insert(url, entry);
            Task::none()
        }
        Message::AiSummaryUpdated(thread_idx, summary, upto) => {
            if let (Some(chat), Some(summary)) = (state.ai_threads.get_mut(thread_idx), summary) {
                let summary = summary.trim().to_string();
                if !summary.is_empty() {
                    chat.summary = summary;
                    chat.summary_upto = upto.min(chat.history.len());
                    crate::radial::save_chat_threads(&state.ai_threads);
                }
            }
            Task::none()
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
            crate::ai_client::StreamEvent::Usage { prompt, completion } => {
                if let Some(chat) = state.ai_threads.get_mut(thread_idx) {
                    chat.tokens_prompt += prompt as u64;
                    chat.tokens_completion += completion as u64;
                }
                state.ai_turn_tokens.0 += prompt;
                state.ai_turn_tokens.1 += completion;
                Task::none()
            }
            crate::ai_client::StreamEvent::Delta(text) => {
                match &mut state.ai_stream {
                    Some((idx, buf)) if *idx == thread_idx => buf.push_str(&text),
                    _ => state.ai_stream = Some((thread_idx, text)),
                }
                // Re-parse the in-flight text as markdown each delta so
                // the streaming bubble is formatted live. A trailing
                // cursor is appended to the source so it rides the last
                // text run. pulldown-cmark parses incomplete markdown
                // leniently, so half-written **bold**/```fences degrade
                // gracefully until the next delta closes them.
                if let Some((_, buf)) = &state.ai_stream {
                    state.ai_stream_md =
                        iced::widget::markdown::parse(&format!("{buf}\u{258c}")).collect();
                }
                // Follow the stream only while the user is already at the
                // bottom; if they scrolled up to read, don't yank them.
                if state.ai_chat_at_bottom {
                    scroll_chat_to_end()
                } else {
                    Task::none()
                }
            }
        },
        Message::AiPromptOptimized(res) => {
            state.ai_activity = None;
            match res {
                Ok(text) if !text.trim().is_empty() => {
                    // Drop the rewrite into the input for the user to review,
                    // edit, and submit (we don't auto-send).
                    state.ai_editor =
                        iced::widget::text_editor::Content::with_text(text.trim());
                }
                Ok(_) => state.ai_activity = Some("Optimizer returned nothing".to_string()),
                Err(e) => state.ai_activity = Some(format!("Optimize failed: {e}")),
            }
            Task::none()
        }
        Message::AiStopRequest => {
            if let Some(handle) = state.ai_abort.take() {
                handle.abort();
            }
            if state.use_agentd {
                // agentd cancel: cancel_turn is not yet wired on the daemon side
                // (Task 6 placeholder only). Attempt it and surface a graceful
                // "not available yet" label rather than crashing.
                let session_id = state.chat().session_id.clone().unwrap_or_default();
                let task = Task::perform(
                    async move { crate::ai_client::cancel_agentd_turn(&session_id).await },
                    |result| match result {
                        Ok(()) => Message::Noop,
                        Err(_) => {
                            // Cancellation not yet wired — silently ignore.
                            Message::Noop
                        }
                    },
                );
                state.ai_loading = false;
                state.ai_activity = Some("Stop requested (cancellation not yet available)".to_string());
                state.ai_stream_md = Vec::new();
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
                task
            } else {
                // Canonical cancel path: fire the session's token so a turn parked
                // on an approval await (which dropping the Task alone may not
                // unblock) stops too.
                if let Some(id) = state.chat().session_id.clone() {
                    use oxidemx_agent::session::SessionStore;
                    crate::agent_runtime::SESSIONS.cancel(&id);
                }
                state.ai_loading = false;
                state.ai_activity = None;
                state.ai_stream_md = Vec::new();
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
        Message::AiCopyText(text) => {
            state.ai_toast = Some(("✓ Copied".to_string(), std::time::Instant::now()));
            Task::batch([
                iced::clipboard::write(text),
                Task::perform(
                    tokio::time::sleep(std::time::Duration::from_millis(1600)),
                    |_| Message::AiToastExpire,
                ),
            ])
        }
        Message::AiToastExpire => {
            // Only clear if no newer toast replaced this one (the newer
            // one resets the instant and ships its own timer).
            if let Some((_, at)) = &state.ai_toast {
                if at.elapsed() >= std::time::Duration::from_millis(1500) {
                    state.ai_toast = None;
                }
            }
            Task::none()
        }
        Message::AiToggleSkills => {
            state.ai_show_skills = !state.ai_show_skills;
            state.ai_show_memories = false;
            state.ai_show_tasks = false;
            state.ai_show_threads = false;
            if state.ai_show_skills {
                state.ai_skills = crate::agent::skills::discover();
                state.ai_skills_enabled = crate::agent::skills::enabled_set();
            }
            Task::none()
        }
        Message::AiCardToggle(i) => {
            if !state.ai_card_expanded.remove(&i) {
                state.ai_card_expanded.insert(i);
            }
            Task::none()
        }
        Message::AiLightboxOpen(url) => {
            state.ai_lightbox = Some(url);
            Task::none()
        }
        Message::AiLightboxClose => {
            state.ai_lightbox = None;
            Task::none()
        }
        Message::AiShowView(view) => {
            use crate::app::ChatView;
            state.ai_show_skills = view == ChatView::Skills;
            state.ai_show_memories = view == ChatView::Memory;
            state.ai_show_tasks = view == ChatView::Tasks;
            state.ai_show_threads = false;
            match view {
                ChatView::Skills => {
                    state.ai_skills = crate::agent::skills::discover();
                    state.ai_skills_enabled = crate::agent::skills::enabled_set();
                    Task::none()
                }
                ChatView::Memory => {
                    state.ai_memories = crate::agent::memory::load_all();
                    state.ai_memories_bytes = crate::agent::memory::store_size_bytes();
                    Task::none()
                }
                ChatView::Tasks => refresh_tasks(),
                ChatView::Conversation => Task::none(),
            }
        }
        Message::AiSkillEnable(name, on) => {
            crate::agent::skills::set_enabled(&name, on);
            state.ai_skills_enabled = crate::agent::skills::enabled_set();
            state.ai_toast = Some((
                format!("{} {}", if on { "✓ Enabled" } else { "○ Disabled" }, name),
                std::time::Instant::now(),
            ));
            Task::perform(
                tokio::time::sleep(std::time::Duration::from_millis(1600)),
                |_| Message::AiToastExpire,
            )
        }
        Message::AiSkillsSearch(q) => {
            state.ai_skills_query = q;
            Task::none()
        }
        Message::AiPaletteSelect(i) => {
            if let Some((sel, _)) = &mut state.ai_palette {
                *sel = i;
            }
            run_palette(state)
        }
        Message::AiPaletteNav(delta) => {
            if let Some((sel, items)) = &mut state.ai_palette {
                if !items.is_empty() {
                    let n = items.len() as i32;
                    *sel = (((*sel as i32 + delta) % n + n) % n) as usize;
                }
            }
            Task::none()
        }
        Message::AiPaletteRun => run_palette(state),
        Message::AiPaletteClose => {
            state.ai_palette = None;
            Task::none()
        }
        Message::AiRetryLast => {
            if state.ai_loading {
                return Task::none();
            }
            // Drop trailing non-user messages (the error + any cards from
            // the failed turn) and the last user message, then resubmit
            // it through the normal submit path.
            let prompt = {
                let chat = state.chat_mut();
                while chat.history.last().is_some_and(|m| !m.is_user) {
                    chat.history.pop();
                }
                match chat.history.pop() {
                    Some(u) if u.is_user => Some(u.text),
                    other => {
                        // Put it back if it wasn't a user message.
                        if let Some(m) = other {
                            chat.history.push(m);
                        }
                        None
                    }
                }
            };
            if let Some(text) = prompt {
                state.ai_editor = iced::widget::text_editor::Content::with_text(&text);
                Task::done(Message::AiSubmitPrompt)
            } else {
                Task::none()
            }
        }
        Message::AiThreadsSearch(q) => {
            state.ai_threads_query = q;
            Task::none()
        }
        Message::AiExportThread(idx) => {
            let toast = match state.ai_threads.get(idx) {
                Some(t) => match export_thread_markdown(t) {
                    Ok(path) => format!("✓ Exported to {path}"),
                    Err(e) => format!("⚠ Export failed: {e}"),
                },
                None => "⚠ No such thread".to_string(),
            };
            state.ai_toast = Some((toast, std::time::Instant::now()));
            Task::perform(
                tokio::time::sleep(std::time::Duration::from_millis(2200)),
                |_| Message::AiToastExpire,
            )
        }
        Message::AiFileDropped(path) => {
            // Only stage drops while the overlay is on screen (drops can
            // only reach our window when it's visible anyway).
            if state.is_drawable() {
                stage_attachment(state, path)
            } else {
                Task::none()
            }
        }
        Message::AiAttachPick => Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Attach a file")
                    .pick_file()
                    .await
                    .map(|h| h.path().to_path_buf())
            },
            Message::AiAttachReceived,
        ),
        Message::AiAttachReceived(opt) => match opt {
            Some(path) => stage_attachment(state, path),
            None => Task::none(),
        },
        Message::AiAttachClear => {
            state.ai_attachment = None;
            Task::none()
        }
        Message::AiBubbleHover(idx) => {
            state.ai_hover_msg = idx;
            Task::none()
        }
        Message::AiBubbleMenu(opt) => {
            // Right-click opens (Some(i)); left-click / repeat-right
            // toggles closed.
            state.ai_context_menu = if state.ai_context_menu == opt {
                None
            } else {
                opt
            };
            Task::none()
        }
        Message::AiBubbleSelect(i) => {
            state.ai_context_menu = None;
            if let Some(msg) = state.chat().history.get(i) {
                state.ai_select =
                    Some((i, iced::widget::text_editor::Content::with_text(&msg.text)));
            }
            Task::none()
        }
        Message::AiSelectAction(action) => {
            // Read-only: apply selection/cursor/scroll actions, drop edits.
            if !action.is_edit() {
                if let Some((_, content)) = &mut state.ai_select {
                    content.perform(action);
                }
            }
            Task::none()
        }
        Message::AiSelectExit => {
            state.ai_select = None;
            state.ai_context_menu = None;
            Task::none()
        }
        Message::AiPasteToInput => {
            state.ai_context_menu = None;
            iced::clipboard::read().map(Message::AiPasteReceived)
        }
        Message::AiPasteReceived(opt) => {
            if let Some(s) = opt.filter(|s| !s.is_empty()) {
                state
                    .ai_editor
                    .perform(iced::widget::text_editor::Action::Edit(
                        iced::widget::text_editor::Edit::Paste(std::sync::Arc::new(s)),
                    ));
            }
            Task::none()
        }
        Message::AiScrollToBottom => {
            // Animate (ease-out) to the bottom instead of jumping. The
            // tween's `current` mirrors the live offset (kept in sync by
            // AiChatScrolled), so it eases from where the user actually
            // is. The Tick loop drives the per-frame snap_to; on settle
            // it flips `ai_chat_at_bottom` true. `kind` only needs to be
            // non-None for the tween to interpolate — we read the raw
            // `current` scalar, so the specific kind is irrelevant.
            state.ai_scroll_tween.set_target(
                1.0,
                &oxidemx_shared::TransitionConfig {
                    kind: oxidemx_shared::TransitionKind::Fade,
                    duration_ms: 300,
                    easing: oxidemx_shared::Easing::EaseOut,
                    ..Default::default()
                },
            );
            state.ai_scroll_active = true;
            Task::none()
        }
        Message::AiChatScrolled(viewport) => {
            let y = viewport.relative_offset().y;
            // y == 1.0 is the bottom; treat the last sliver as "at
            // bottom" so follow-along auto-scroll stays on.
            state.ai_chat_at_bottom = y >= 0.985;
            // This fires only on genuine user scrolling — a programmatic
            // snap_to mutates the offset directly and does NOT re-fire
            // on_scroll. So reaching here means the user took control:
            // cancel any in-flight "↓ Latest" animation and resync the
            // tween to the real offset so the next press eases from here.
            state.ai_scroll_active = false;
            state.ai_scroll_tween = crate::anim::Tween::at(y);
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
            // Drop the deleted thread's session (cancels any in-flight turn
            // and frees its cached provider) before the indices shift.
            if let Some(id) = state
                .ai_threads
                .get(idx)
                .and_then(|c| c.session_id.clone())
            {
                use oxidemx_agent::session::SessionStore;
                crate::agent_runtime::SESSIONS.end(&id);
            }
            state.ai_delete_thread(idx);
            crate::radial::save_chat_threads(&state.ai_threads);
            Task::none()
        }
        Message::AiOpenCommandCenter => {
            let _ = std::process::Command::new("oxidemx-mission-control").spawn();
            Task::none()
        }
        Message::AiOpenAgentsConfig | Message::AiOpenMcpConfig => {
            // Open Settings on the Agents tab (roster / tools / MCP).
            let _ = std::process::Command::new("oxidemx-settings")
                .env("OXIDEMX_SETTINGS_TAB", "agents")
                .spawn();
            Task::none()
        }
        Message::AiNewChat => {
            state.ai_new_chat();
            if state.use_agentd {
                let thread_idx = state.ai_active;
                Task::batch([Task::none(), load_agentd_history(state, thread_idx)])
            } else {
                Task::none()
            }
        }
        Message::AiToggleThreads => {
            state.ai_show_threads = !state.ai_show_threads;
            Task::none()
        }
        Message::AiSelectThread(idx) => {
            state.ai_select_chat(idx);
            if state.use_agentd {
                load_agentd_history(state, idx)
            } else {
                Task::none()
            }
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
            state.ai_show_skills = false;
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
        Message::AiWatchFlow(flow_id) => {
            // Open Mission Control on this flow (fire-and-forget; a
            // missing binary is harmless — the card still stands).
            let _ = std::process::Command::new("oxidemx-mission-control")
                .env("OXIDEMX_MC_FLOW", &flow_id)
                .spawn();
            Task::none()
        }
        Message::AiToggleTasks => {
            state.ai_show_tasks = !state.ai_show_tasks;
            state.ai_show_memories = false;
            state.ai_show_threads = false;
            state.ai_show_skills = false;
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

        // ── agentd event path (use_agentd = true) ────────────────────────────

        // Raw D-Bus event from agentd: resolve the session_id to a thread index
        // and fan out to the appropriate iced StreamEvent or Final.
        Message::AgentdEvent { session_id, inner } => {
            use crate::app::agent_events::{session_to_thread_idx, Inner};
            let thread_idx = session_to_thread_idx(&state.ai_threads, &session_id)
                .unwrap_or(state.ai_active);

            match inner {
                Inner::Delta(text) => {
                    // Route through the same AiStream handler.
                    update(
                        state,
                        Message::AiStream((
                            thread_idx,
                            crate::ai_client::StreamEvent::Delta(text),
                        )),
                    )
                }
                Inner::Activity(text) => update(
                    state,
                    Message::AiStream((
                        thread_idx,
                        crate::ai_client::StreamEvent::Activity(text),
                    )),
                ),
                Inner::Card(Some(card)) => update(
                    state,
                    Message::AiStream((thread_idx, crate::ai_client::StreamEvent::Card(card))),
                ),
                Inner::Card(None) => {
                    // Card deserialization failed — surface as a generic activity note.
                    update(
                        state,
                        Message::AiStream((
                            thread_idx,
                            crate::ai_client::StreamEvent::Activity(
                                "Agent tool completed".to_string(),
                            ),
                        )),
                    )
                }
                Inner::Final(text) => update(
                    state,
                    Message::AgentdFinal { thread_idx, text },
                ),
            }
        }

        // agentd turn complete — commit the final reply text.
        Message::AgentdFinal { thread_idx, text } => {
            state.ai_loading = false;
            state.ai_activity = None;
            state.ai_stream = None;
            state.ai_stream_md = Vec::new();
            state.ai_abort = None;
            let Some(chat) = state.ai_threads.get_mut(thread_idx) else {
                return Task::none();
            };
            chat.history.push(ChatMessage::assistant(text));
            chat.updated_at = crate::radial::now_secs();
            crate::radial::save_chat_threads(&state.ai_threads);
            state.trigger_ripple();
            scroll_chat_to_end()
        }

        // agentd is waiting for approval — store the card and surface the UI.
        Message::AgentdApprovalRequested {
            thread,
            request_id,
            card_json,
        } => {
            state.ai_agentd_approval = Some((request_id, card_json));
            // Mark as not loading so the user can see the approval card.
            state.ai_loading = false;
            state.ai_activity = Some(format!("Waiting for approval (thread {thread})"));
            Task::none()
        }

        // User responded to an agentd approval card.
        Message::AgentdRespondApproval { request_id, allow } => {
            state.ai_agentd_approval = None;
            state.ai_loading = true;
            state.ai_activity = Some("Continuing…".to_string());
            let project = crate::ai_client::agentd_project();
            Task::perform(
                async move {
                    use oxidemx_agent_proxy::AgentProxy;
                    let conn = zbus::connection::Builder::session()?.build().await?;
                    let proxy = AgentProxy::new(&conn).await?;
                    proxy
                        .respond_approval(&project, &request_id, allow, "")
                        .await?;
                    Ok::<(), zbus::Error>(())
                },
                |res| match res {
                    Ok(()) => Message::Noop,
                    Err(e) => Message::AiResponseReceived(
                        0,
                        Err(format!("respond_approval failed: {e}")),
                    ),
                },
            )
        }

        // agentd model lifecycle change — update the status label.
        Message::AgentdModelStatus(alias, status_json) => {
            // Parse the status string for a simple "loading" / "ready" / "error" label.
            let label = serde_json::from_str::<serde_json::Value>(&status_json)
                .ok()
                .and_then(|v| v.get("state").and_then(|s| s.as_str()).map(String::from))
                .unwrap_or_else(|| status_json.clone());
            info!("agentd model status: {alias} → {label}");
            // Only surface non-ready states as activity text (keep idle UI quiet).
            if label != "ready" && label != "loaded" {
                state.ai_activity = Some(format!("Model {alias}: {label}"));
            }
            Task::none()
        }

        // History loaded from agentd on chat open / thread switch.
        Message::AgentdHistoryLoaded { thread_idx, turns } => {
            let Some(chat) = state.ai_threads.get_mut(thread_idx) else {
                return Task::none();
            };
            // Only populate if the thread's local history is empty — avoid
            // double-appending if the user already typed in the thread.
            if chat.history.is_empty() {
                for (is_user, text) in turns {
                    let msg = if is_user {
                        ChatMessage::user(text)
                    } else {
                        ChatMessage::assistant(text)
                    };
                    chat.history.push(msg);
                }
                crate::radial::save_chat_threads(&state.ai_threads);
            }
            scroll_chat_to_end()
        }
    }
}

/// Kick off a history load from agentd for the given thread index.
///
/// Only fires if the thread has a session_id (i.e. it has been used at least
/// once and agentd can look it up).  If the thread is brand-new (no session_id)
/// there's nothing to load — the send will mint the id on first turn.
fn load_agentd_history(state: &RadialState, thread_idx: usize) -> Task<Message> {
    let session_id = state
        .ai_threads
        .get(thread_idx)
        .and_then(|t| t.session_id.clone());
    let Some(session_id) = session_id else {
        return Task::none();
    };
    Task::perform(
        async move { crate::ai_client::fetch_agentd_transcript(&session_id).await },
        move |turns| Message::AgentdHistoryLoaded { thread_idx, turns },
    )
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

/// Write a thread to a Markdown file under
/// `~/.local/share/oxidemx/exports/` and return the path (as a string).
fn export_thread_markdown(t: &crate::radial::ChatThread) -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|_| "no HOME".to_string())?;
    let dir = std::path::Path::new(&home).join(".local/share/oxidemx/exports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let slug: String = t
        .title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(40)
        .collect();
    let slug = if slug.is_empty() {
        "chat".to_string()
    } else {
        slug
    };
    let file = dir.join(format!("{slug}-{}.md", t.updated_at));

    let mut md = String::new();
    md.push_str(&format!(
        "# {}\n\n",
        if t.title.is_empty() {
            "Untitled chat"
        } else {
            &t.title
        }
    ));
    md.push_str(&format!(
        "_Model: {} · {} messages · exported from OxideMX_\n\n---\n\n",
        t.model,
        t.history.len()
    ));
    for m in &t.history {
        if m.is_error {
            md.push_str(&format!("> ⚠ **Error:** {}\n\n", m.text));
        } else if let Some(card) = &m.card {
            let _ = card;
            md.push_str(&format!("> 🔧 {}\n\n", m.text));
        } else {
            md.push_str(&format!(
                "**{}**\n\n{}\n\n",
                if m.is_user { "You" } else { "Oxide" },
                m.text
            ));
        }
    }
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    Ok(file.display().to_string())
}

/// Stage a file as the next prompt's attachment, enforcing the 20 MB
/// cap (oversize is rejected with a toast). Either way a toast shows.
fn stage_attachment(state: &mut RadialState, path: std::path::PathBuf) -> Task<Message> {
    const CAP: u64 = 20 * 1024 * 1024;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let toast = if size > CAP {
        format!("⚠ {name} is over the 20 MB limit")
    } else {
        state.ai_attachment = Some(path);
        format!("📎 Attached {name}")
    };
    state.ai_toast = Some((toast, std::time::Instant::now()));
    Task::perform(
        tokio::time::sleep(std::time::Duration::from_millis(1800)),
        |_| Message::AiToastExpire,
    )
}

/// Collect remote (`http`/`https`) image URLs referenced in an AI
/// reply's markdown (top level + inside quotes), for inline fetch.
fn collect_remote_image_urls(items: &[iced::widget::markdown::Item]) -> Vec<String> {
    use iced::widget::markdown::Item;
    let mut out = Vec::new();
    for it in items {
        match it {
            Item::Image { url, .. } => {
                let u = url.to_string();
                if u.starts_with("http://") || u.starts_with("https://") {
                    out.push(u);
                }
            }
            Item::Quote(inner) => out.extend(collect_remote_image_urls(inner)),
            _ => {}
        }
    }
    out
}

/// Fetch one remote image's bytes for inline display.
fn fetch_image_task(url: String) -> Task<Message> {
    let fetch_url = url.clone();
    Task::perform(
        async move {
            let resp = reqwest::get(&fetch_url).await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.bytes().await.ok().map(|b| b.to_vec())
        },
        move |opt| Message::AiImageFetched(url.clone(), opt),
    )
}

/// Recompute the slash palette from the current input. Opens it when
/// the input is a single line starting with `/`; closes it otherwise.
fn refresh_palette(state: &mut RadialState) {
    let raw = state.ai_editor.text();
    let t = raw.trim_end_matches('\n');
    let Some(query) = t.strip_prefix('/') else {
        state.ai_palette = None;
        return;
    };
    if t.contains('\n') {
        // Multi-line — not a command (e.g. pasted text starting with /).
        state.ai_palette = None;
        return;
    }
    if state.ai_flow_cache.is_empty() {
        state.ai_flow_cache = crate::ai_client::list_flows();
    }
    let mut enabled: Vec<String> = crate::agent::skills::enabled_set().into_iter().collect();
    enabled.sort();
    let commands: Vec<(String, String, std::path::PathBuf)> =
        crate::agent::skills::discover_commands()
            .into_iter()
            .map(|c| (c.name, c.description, c.path))
            .collect();
    let items = crate::chat_ui::palette::build(query, &state.ai_flow_cache, &enabled, &commands);
    let sel = state
        .ai_palette
        .as_ref()
        .map(|(s, _)| (*s).min(items.len().saturating_sub(1)))
        .unwrap_or(0);
    state.ai_palette = Some((sel, items));
}

/// Run the selected palette row, then close the palette and clear the
/// `/...` input. Non-flow actions re-emit their existing Message via
/// `Task::done`; a flow prefills + submits a run prompt; a skill toggles
/// its enabled state.
fn run_palette(state: &mut RadialState) -> Task<Message> {
    use crate::chat_ui::palette::PaletteKind;
    let Some((sel, items)) = state.ai_palette.take() else {
        return Task::none();
    };
    let Some(item) = items.into_iter().nth(sel) else {
        return Task::none();
    };
    // Clear the `/...` query from the input.
    state.ai_editor = iced::widget::text_editor::Content::new();

    match item.kind {
        PaletteKind::NewChat => Task::done(Message::AiNewChat),
        PaletteKind::CommandCenter => Task::done(Message::AiOpenCommandCenter),
        PaletteKind::Agents => Task::done(Message::AiOpenAgentsConfig),
        PaletteKind::Mcp => Task::done(Message::AiOpenMcpConfig),
        PaletteKind::Memories => Task::done(Message::AiToggleMemories),
        PaletteKind::Tasks => Task::done(Message::AiToggleTasks),
        PaletteKind::Skills => Task::done(Message::AiToggleSkills),
        PaletteKind::ModelToggle => Task::done(Message::AiModelToggled),
        PaletteKind::OptimizePrompt(draft) => {
            let draft = draft.trim().to_string();
            if draft.is_empty() {
                state.ai_activity = Some("Type the draft after /optimize".to_string());
                return Task::none();
            }
            // Lean, no-tools rewrite via the configured model (ideal for the
            // local SLM). Result lands in the input for review, not submitted.
            let model = state.chat().model.clone();
            state.ai_activity = Some("Optimizing prompt…".to_string());
            Task::perform(
                async move {
                    crate::agent_runtime::optimize_prompt(&model, &draft)
                        .await
                        .map_err(|e| e.to_string())
                },
                Message::AiPromptOptimized,
            )
        }
        PaletteKind::Flow(name) => {
            state.ai_editor =
                iced::widget::text_editor::Content::with_text(&format!("Run the {name} flow."));
            Task::done(Message::AiSubmitPrompt)
        }
        PaletteKind::Skill(name) => {
            let on = !state.ai_skills_enabled.contains(&name);
            Task::done(Message::AiSkillEnable(name, on))
        }
        PaletteKind::Command(path, args) => {
            match crate::agent::skills::render_command(&path, &args) {
                Some(prompt) if !prompt.is_empty() => {
                    state.ai_editor = iced::widget::text_editor::Content::with_text(&prompt);
                    Task::done(Message::AiSubmitPrompt)
                }
                _ => Task::none(),
            }
        }
    }
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
    // Custom-widget wedges dispatch a real Event::Click to the
    // plugin even though their `command` is empty — confirm, not
    // invalid.
    if matches!(
        slice.widget.as_ref().map(|w| &w.source),
        Some(oxidemx_shared::WidgetSource::Custom(_))
    ) {
        return DispatchOutcome::Actionable;
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

#[cfg(test)]
mod img_tests {
    #[test]
    fn extracts_remote_image_urls() {
        let md: Vec<_> = iced::widget::markdown::parse(
            "Here is a pic:\n\n![cat](https://example.com/cat.png)\n\nand a local ![x](/tmp/y.png)",
        )
        .collect();
        let urls = super::collect_remote_image_urls(&md);
        assert_eq!(urls, vec!["https://example.com/cat.png"], "got {urls:?}");
    }
}
