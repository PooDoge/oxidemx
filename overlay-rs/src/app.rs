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
use iced::widget::{column, row, container, scrollable, text, text_input, button, Space};
use iced::{Color, Element, Length, Size, Subscription, Task, Alignment};
use tracing::{debug, error, info, warn};

use oxidemx_shared::AppConfig;

use crate::dbus::OverlayEvent;
use crate::geometry::WINDOW_SIZE;
use crate::radial::{RadialState, Painter, ChatMessage};

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
    ToggleCursor { x: f64, y: f64 },
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
    ConfigReloaded(oxidemx_shared::AppConfig),
    WindowOpened(iced::window::Id),
    /// Result of querying the monitor size for centering fallback.
    CenterOverlay(Option<Size>),
    
    // AI Assistant Messages
    AiInputChanged(String),
    AiSubmitPrompt,
    AiResponseReceived(Result<(String, Option<String>), String>),
    AiChooseOption(String),
    AiQuestionReceived(crate::ai_client::PendingQuestion),
}

pub fn run() -> iced::Result {
    let window = oxidemx_window::frameless_topmost(
        APP_ID,
        Size::new(WINDOW_SIZE as f32, WINDOW_SIZE as f32),
    );

    iced::application(boot, update, view)
        .title("OxideMX MX")
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
    match message {
        Message::Tick => {
            state.advance_animations();
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
            let half = (WINDOW_SIZE / 2.0) as i32;
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
                    iced::window::move_to(id, iced::Point::new((x - half) as f32, (y - half) as f32)),
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
                        x - half,
                        y - half,
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
            if (before != after && after.is_some())
                || (before_sub != after_sub)
            {
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
                let x = (size.width - WINDOW_SIZE as f32) / 2.0;
                let y = (size.height - WINDOW_SIZE as f32) / 2.0;
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
            if (before != after && after.is_some())
                || (before_sub != after_sub)
            {
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
                Task::perform(
                    crate::haptic_client::trigger_haptic("page_change".to_string()),
                    |_| Message::Noop,
                )
            } else {
                Task::none()
            }
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
        Message::AiInputChanged(val) => {
            state.ai_input = val;
            Task::none()
        }
        Message::AiSubmitPrompt => {
            let prompt = state.ai_input.trim().to_string();
            if state.ai_loading || prompt.is_empty() {
                return Task::none();
            }
            state.ai_input.clear();
            state.ai_history.push(ChatMessage {
                is_user: true,
                text: prompt.clone(),
            });
            state.ai_loading = true;
            let session_id = state.ai_session_id.clone();
            Task::perform(
                async move {
                    match crate::ai_client::load_api_key() {
                        Ok(key) => {
                            let mode = crate::ai_client::AgentMode::SettingsCustomizer;
                            crate::ai_client::ask_ai(&key, mode, &prompt, session_id).await
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e.to_string()),
                    }
                },
                Message::AiResponseReceived,
            )
        }
        Message::AiResponseReceived(res) => {
            state.ai_loading = false;
            match res {
                Ok((reply, next_session_id)) => {
                    state.ai_session_id = next_session_id;
                    state.ai_history.push(ChatMessage {
                        is_user: false,
                        text: reply,
                    });
                }
                Err(err) => {
                    state.ai_history.push(ChatMessage {
                        is_user: false,
                        text: format!("Error: {}", err),
                    });
                }
            }
            state.trigger_ripple();
            Task::none()
        }
        Message::AiChooseOption(choice) => {
            if let Some(pending) = state.ai_pending_question.take() {
                let tx = pending.response_tx;
                state.ai_history.push(ChatMessage {
                    is_user: true,
                    text: choice.clone(),
                });
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
    let visible = slice
        .visible_if
        .as_ref()
        .map(|c| c.eval())
        .unwrap_or(true);
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
    let menu_alpha = state.menu.current.clamp(0.0, 1.0);
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
        let outer_norm =
            (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let shadow = iced::widget::Shader::new(
            crate::render::drop_shadow::DropShadowProgram {
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
            },
        )
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
        let (sr, sg, sb, _) =
            oxidemx_shared::theme::parse_hex_rgba(&palette.surface0)
                .unwrap_or((0.18, 0.18, 0.20, 1.0));
        let surface0 = [sr as f32, sg as f32, sb as f32, 1.0];
        let colors = [surface0; 8];
        let (s1r, s1g, s1b, _) =
            oxidemx_shared::theme::parse_hex_rgba(&palette.surface1)
                .unwrap_or((0.25, 0.25, 0.28, 1.0));
        let surface1_color = [s1r as f32, s1g as f32, s1b as f32, 1.0];
        let (s2r, s2g, s2b, _) =
            oxidemx_shared::theme::parse_hex_rgba(&palette.surface2)
                .unwrap_or((0.4, 0.4, 0.45, 1.0));
        let surface2_color = [s2r as f32, s2g as f32, s2b as f32, 1.0];
        // Canvas's stroke uses surface2 as its base colour, then
        // lerps to accent on hover. Same here.
        let stroke_color = surface2_color;
        let (ar, ag, ab, _) =
            oxidemx_shared::theme::parse_hex_rgba(&palette.accent)
                .unwrap_or((0.5, 0.5, 1.0, 1.0));
        let accent_color = [ar as f32, ag as f32, ab as f32, 1.0];
        let highlights = state.highlights.map(|t| t.current);
        // Canvas wedges meet at exactly the slice-boundary angle —
        // no transparent gap, just the stroke band painting a thin
        // separator line. Match that here.
        let gap_rad: f32 = 0.0;
        let bg_op =
            state.visuals.menu_background_opacity.clamp(0.0, 1.0);
        let highlight_op =
            state.visuals.slice_highlight_opacity.clamp(0.0, 1.0);
        let sdf = iced::widget::Shader::new(
            crate::render::sdf_ring::SdfRingProgram {
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
            },
        )
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
        let inner_norm =
            (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm =
            (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let bevel = iced::widget::Shader::new(
            crate::render::disc_bevel::DiscBevelProgram {
                inner_r: inner_norm,
                outer_r: outer_norm,
                intensity: disc_bevel_intensity * menu_alpha,
                rim_width: 0.045,
                inset_width: 0.035,
                light_angle,
                shadow_strength: 0.7,
                rim_color: [1.0, 1.0, 1.0, 0.85],
                shadow_color: [0.0, 0.0, 0.0, 0.65],
            },
        )
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
        let inner_norm =
            (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm =
            (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let sweep = iced::widget::Shader::new(
            crate::render::specular_sweep::SpecularSweepProgram {
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
            },
        )
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
        let inner_norm =
            (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm =
            (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let slot_count = state.active_slot_count().max(1) as u32;
        let bevel = iced::widget::Shader::new(
            crate::render::slice_bevel::SliceBevelProgram {
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
            },
        )
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
        let radius_norm =
            (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let dome = iced::widget::Shader::new(
            crate::render::center_dome::CenterDomeProgram {
                radius: radius_norm,
                intensity: center_dome_intensity * menu_alpha,
                light_angle,
                shininess: 32.0,
                rim_brightness: 0.6,
                shadow_amount: 0.5,
                specular_color: [1.0, 1.0, 1.0, 0.95],
            },
        )
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
                        if c.is_empty() { None } else { Some(c) }
                    })
                    .unwrap_or("accent");
                let palette = &state.theme.theme.colors;
                let (cr, cg, cb, _) = palette.slice_color_rgba(color_key);
                let color = [cr as f32, cg as f32, cb as f32, 1.0];
                let glow = iced::widget::Shader::new(
                    crate::render::hover_glow::HoverGlowProgram {
                        bisector_rad: bisector,
                        half_sweep,
                        inner_r: inner_norm,
                        outer_r: outer_norm,
                        progress,
                        intensity: hover_glow_intensity * menu_alpha,
                        color,
                    },
                )
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
                let inner_norm =
                    (crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0) / half_extent;
                let outer_norm =
                    (crate::geometry::MENU_RADIUS as f32 - 6.0) / half_extent;
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
                        if c.is_empty() { None } else { Some(c) }
                    })
                    .unwrap_or("accent");
                let palette = &state.theme.theme.colors;
                let (ar, ag, ab, _) = palette.slice_color_rgba(color_key);
                let highlight_color = [1.0, 1.0, 1.0, 0.85];
                let accent_color = [ar as f32, ag as f32, ab as f32, 1.0];
                let tilt = iced::widget::Shader::new(
                    crate::render::hover_tilt::HoverTiltProgram {
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
                    },
                )
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
            let progress =
                elapsed_ms as f32 / crate::radial::RIPPLE_DURATION_MS as f32;
            let ripple = iced::widget::Shader::new(
                crate::render::ripple::RippleProgram::new(
                    progress,
                    ripple_intensity * menu_alpha,
                    accent_rgba,
                ),
            )
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
            let layer = iced::widget::Shader::new(
                crate::render::page_fx::PageFxProgram {
                    progress,
                    style,
                    intensity: menu_alpha * s.intensity.clamp(0.0, 1.0),
                    dissolve_noise_scale: s.dissolve_noise_scale,
                    dissolve_band_softness: s.dissolve_band_softness,
                    plasma_wave_scale: s.plasma_wave_scale,
                    plasma_wave_speed: s.plasma_wave_speed,
                    color_a,
                    color_b,
                },
            )
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
    if let (Some(started), Some(origin_idx)) =
        (state.dispatch_started, state.dispatch_origin)
    {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if burst_intensity > 0.001
            && elapsed_ms < crate::radial::BURST_DURATION_MS
        {
            let progress =
                elapsed_ms as f32 / crate::radial::BURST_DURATION_MS as f32;
            let slot_count = state.active_slot_count();
            let origin = crate::render::dispatch_burst::slice_origin(
                origin_idx, slot_count,
            );
            let palette = &state.theme.theme.colors;
            let slice = state.slices.get(origin_idx);
            let color_key = slice
                .and_then(|s| {
                    let c = s.color.trim();
                    if c.is_empty() { None } else { Some(c) }
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

    let main_stack = if layers.len() == 1 {
        layers.into_iter().next().unwrap()
    } else {
        iced::widget::Stack::with_children(layers).into()
    };

    let is_ai_page = state.pages.get(state.active_page)
        .map(|p| p.name == "AI Assistant")
        .unwrap_or(false);

    if is_ai_page {
        let ai_panel = build_ai_panel(state);
        iced::widget::Stack::with_children(vec![
            main_stack,
            ai_panel,
        ]).into()
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
        Subscription::run(crate::config::watch_stream).map(Message::ConfigReloaded),
        Subscription::run(ai_question_stream),
        iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick),
        iced::window::events().map(|(id, event)| match event {
            iced::window::Event::Opened { .. } => Message::WindowOpened(id),
            iced::window::Event::Unfocused => Message::ToggleDismiss,
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

fn build_ai_panel(state: &RadialState) -> Element<'_, Message> {
    let palette = &state.theme.theme.colors;
    
    let base_color = to_iced_color(&palette.base, Color::from_rgba(0.08, 0.08, 0.1, 0.95));
    let surface_color = to_iced_color(&palette.surface0, Color::from_rgba(0.12, 0.12, 0.15, 0.9));
    let text_color = to_iced_color(&palette.text, Color::WHITE);
    let accent_color = to_iced_color(&palette.accent, Color::from_rgb(0.5, 0.5, 1.0));
    
    let sidebar_bg = Color::from_rgba(base_color.r, base_color.g, base_color.b, 0.85);
    let border_color = Color::from_rgba(accent_color.r, accent_color.g, accent_color.b, 0.2);

    let header = column![
        text("OxideMX AI")
            .size(20)
            .font(iced::Font { weight: iced::font::Weight::Bold, ..Default::default() })
            .color(text_color),
        text("Settings & Chat Assistant")
            .size(11)
            .color(Color::from_rgba(text_color.r, text_color.g, text_color.b, 0.6)),
        Space::new().height(Length::Fixed(8.0)),
        container(Space::new().width(Length::Fill))
            .height(1)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(text_color.r, text_color.g, text_color.b, 0.1))),
                ..Default::default()
            })
    ];

    let mut chat_list = column![].spacing(8);
    
    if state.ai_history.is_empty() {
        chat_list = chat_list.push(
            text("Try asking:\n• 'Change slice colors to green'\n• 'Set animation speed to fast'\n• 'Switch to Dracula theme'\n• 'Show installed apps'")
                .size(12)
                .color(Color::from_rgba(text_color.r, text_color.g, text_color.b, 0.5))
        );
    } else {
        for msg in &state.ai_history {
            let bubble = if msg.is_user {
                container(
                    text(&msg.text)
                        .size(12)
                        .color(text_color)
                )
                .padding(8)
                .max_width(200.0)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(accent_color.r, accent_color.g, accent_color.b, 0.25))),
                    border: iced::border::Border {
                        color: Color::from_rgba(accent_color.r, accent_color.g, accent_color.b, 0.4),
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
            } else {
                container(
                    text(&msg.text)
                        .size(12)
                        .color(text_color)
                )
                .padding(8)
                .max_width(200.0)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(surface_color.r, surface_color.g, surface_color.b, 0.4))),
                    border: iced::border::Border {
                        color: Color::from_rgba(text_color.r, text_color.g, text_color.b, 0.1),
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
            };

            let align_row = row![
                if msg.is_user { Element::from(Space::new().width(Length::Fill)) } else { Element::from(text("")) },
                bubble,
                if msg.is_user { Element::from(text("")) } else { Element::from(Space::new().width(Length::Fill)) },
            ];
            chat_list = chat_list.push(align_row);
        }
    }

    let history_scroll = scrollable(chat_list)
        .height(Length::Fill);

    let mut footer = column![].spacing(6);

    if let Some(pending) = &state.ai_pending_question {
        let mut question_col = column![
            text(&pending.question)
                .size(12)
                .font(iced::Font { weight: iced::font::Weight::Bold, ..Default::default() })
                .color(accent_color),
            Space::new().height(Length::Fixed(4.0)),
        ].spacing(4);

        for opt in &pending.options {
            let opt_clone = opt.clone();
            question_col = question_col.push(
                button(
                    text(opt)
                        .size(11)
                        .color(text_color)
                        .align_x(iced::alignment::Horizontal::Center)
                )
                .width(Length::Fill)
                .padding(6)
                .style(move |theme, status| {
                    let mut s = button::primary(theme, status);
                    s.background = Some(iced::Background::Color(Color::from_rgba(accent_color.r, accent_color.g, accent_color.b, 0.3)));
                    s.border.color = accent_color;
                    s.border.width = 1.0;
                    s.border.radius = 6.0.into();
                    s
                })
                .on_press(Message::AiChooseOption(opt_clone))
            );
        }

        footer = footer.push(
            container(question_col)
                .padding(8)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(surface_color.r, surface_color.g, surface_color.b, 0.5))),
                    border: iced::border::Border {
                        color: accent_color,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
        );
    }

    if state.ai_loading {
        footer = footer.push(
            row![
                text("Agent thinking...")
                    .size(12)
                    .font(iced::Font { style: iced::font::Style::Italic, ..Default::default() })
                    .color(accent_color)
            ]
            .align_y(Alignment::Center)
        );
    } else {
        let input_box = text_input("Ask AI...", &state.ai_input)
            .size(12)
            .on_input(Message::AiInputChanged)
            .on_submit(Message::AiSubmitPrompt)
            .style(move |theme, status| {
                let mut s = text_input::default(theme, status);
                s.background = iced::Background::Color(Color::from_rgba(surface_color.r, surface_color.g, surface_color.b, 0.6));
                s.value = text_color;
                s.placeholder = Color::from_rgba(text_color.r, text_color.g, text_color.b, 0.4);
                s.border.color = Color::from_rgba(text_color.r, text_color.g, text_color.b, 0.15);
                s.border.radius = 6.0.into();
                s
            });

        let send_btn = button(
            text("Send")
                .size(11)
                .font(iced::Font { weight: iced::font::Weight::Bold, ..Default::default() })
                .color(text_color)
        )
        .padding(6)
        .style(move |theme, status| {
            let mut s = button::primary(theme, status);
            s.background = Some(iced::Background::Color(accent_color));
            s.border.radius = 6.0.into();
            s
        })
        .on_press(Message::AiSubmitPrompt);

        footer = footer.push(
            row![input_box, send_btn].spacing(6).align_y(Alignment::Center)
        );
    }

    let content_col = column![
        header,
        Space::new().height(Length::Fixed(8.0)),
        history_scroll,
        Space::new().height(Length::Fixed(8.0)),
        footer
    ]
    .padding(12)
    .spacing(4)
    .height(Length::Fill);

    row![
        container(content_col)
            .width(Length::Fixed(260.0))
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(sidebar_bg)),
                border: iced::border::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }),
        Space::new().width(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[allow(dead_code)]
fn _ensure_link(_e: &OverlayEvent) {
    error!("only here so the OverlayEvent path is referenced from app");
    info!("ditto");
}
