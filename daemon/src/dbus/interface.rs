//! D-Bus interface implementation
//!
//! All methods, signals, and properties for org.oxidemx.Daemon.
//! This must be a single `#[interface]` impl block per zbus requirements.

use zbus::{interface, object_server::SignalEmitter, fdo};
use crate::config::{Config, PointerConfig, ScrollConfig};
use crate::hidpp::{HapticEvent, HapticManager, SharedHapticManager};
use crate::macros::events_to_actions;
use crate::thumb_wheel::{SharedThumbWheelState, ThumbWheelForwarder};
use super::service::OxideMXService;

/// True iff any field that actually drives the HID++ SmartShift
/// write differs between the two configs.
fn scroll_smartshift_changed(a: &ScrollConfig, b: &ScrollConfig) -> bool {
    a.mode != b.mode
        || a.smartshift != b.smartshift
        || a.smartshift_threshold != b.smartshift_threshold
}

/// True iff any field that needs a `gsettings` write differs.
/// Natural-scroll lives here (GNOME's
/// `org.gnome.desktop.peripherals.mouse natural-scroll`); smooth
/// scrolling is compositor-level and not exposed via gsettings.
fn scroll_gsettings_changed(a: &ScrollConfig, b: &ScrollConfig) -> bool {
    a.natural != b.natural
}

fn pointer_changed(a: &PointerConfig, b: &PointerConfig) -> bool {
    a.speed != b.speed || a.acceleration != b.acceleration
}

/// Map the daemon's internal `device_mode` string to the indicator's
/// documented connection-kind vocabulary:
///
/// | device_mode  | connection_kind |
/// |--------------|-----------------|
/// | "logitech"   | "unifying"      |  (conservative default; real sub-type
/// |              |                 |   not yet stored on OxideMXService)
/// | "bolt"       | "bolt"          |
/// | "bluetooth"  | "bluetooth"     |
/// | "usb"        | "usb"           |
/// | "generic"    | "usb"           |  (generic mouse — treat as wired USB)
/// | anything else| "off"           |
///
/// When `battery_available` is false the connection is always "off"
/// regardless of `device_mode`.
pub(crate) fn normalize_connection_kind(device_mode: &str, battery_available: bool) -> String {
    if !battery_available {
        return "off".to_string();
    }
    match device_mode {
        "logitech" => "unifying",
        "bolt" => "bolt",
        "bluetooth" => "bluetooth",
        "usb" => "usb",
        "generic" => "usb",
        _ => "off",
    }
    .to_string()
}

/// Apply pointer + scroll preferences to GNOME via `gsettings`.
/// No-op (logs a debug line) when gsettings isn't available — KDE
/// + COSMIC have their own paths and aren't wired yet.
fn apply_pointer_to_gnome(pointer: &PointerConfig) {
    // gsettings expects a double in [-1.0, 1.0] for `speed`.
    // Map our 1..20 dial linearly: 10 → 0, 1 → -1, 20 → 1.
    let speed = ((pointer.speed.clamp(1, 20) as f64) - 10.0) / 10.0;
    run_gsettings(
        "org.gnome.desktop.peripherals.mouse",
        "speed",
        &format!("{speed:.3}"),
    );
    let accel_profile = if pointer.acceleration { "default" } else { "flat" };
    run_gsettings(
        "org.gnome.desktop.peripherals.mouse",
        "accel-profile",
        &format!("'{accel_profile}'"),
    );
}

fn apply_scroll_to_gnome(scroll: &ScrollConfig) {
    run_gsettings(
        "org.gnome.desktop.peripherals.mouse",
        "natural-scroll",
        if scroll.natural { "true" } else { "false" },
    );
}

fn run_gsettings(schema: &str, key: &str, value: &str) {
    use std::process::{Command, Stdio};
    let result = Command::new("gsettings")
        .args(["set", schema, key, value])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    match result {
        Ok(child) => {
            // Don't block — fire and forget. Reaping happens via
            // the OS once the child exits.
            std::mem::drop(child);
            tracing::info!(schema, key, value, "Applied gsettings");
        }
        Err(e) => {
            // First failure is enough to know — debug-level so we
            // don't spam on non-GNOME systems.
            tracing::debug!(error = %e, schema, key, "gsettings unavailable");
        }
    }
}

/// Reconcile non-gesture button divert state with the new
/// buttons config. For each of the four non-gesture CIDs we
/// support (Middle / Back / Forward / SmartShift), compute the
/// desired state ("diverted" iff configured action != default
/// for that button) and call divert/undivert when the desired
/// state changed. Idempotent: if the assigned action is the
/// same as before, no HID++ traffic. Safe to call from the
/// reload-on-keystroke path.
fn sync_non_gesture_diverts(
    haptic_manager: &std::sync::Arc<std::sync::Mutex<HapticManager>>,
    prev: &crate::config::ButtonsConfig,
    next: &crate::config::ButtonsConfig,
) {
    use crate::config::ButtonAction;
    let pairs: &[(u16, ButtonAction, ButtonAction, ButtonAction)] = &[
        (
            crate::hidraw::button_cid::MIDDLE_BUTTON,
            prev.middle, next.middle, ButtonAction::MiddleClick,
        ),
        (
            crate::hidraw::button_cid::BACK_BUTTON,
            prev.back, next.back, ButtonAction::Back,
        ),
        (
            crate::hidraw::button_cid::FORWARD_BUTTON,
            prev.forward, next.forward, ButtonAction::Forward,
        ),
        (
            crate::hidraw::button_cid::SMART_SHIFT,
            prev.shift_wheel, next.shift_wheel, ButtonAction::Smartshift,
        ),
    ];
    let prev_diverted = |a: ButtonAction, default_for: ButtonAction| a != default_for;
    let next_diverted = |a: ButtonAction, default_for: ButtonAction| a != default_for;
    let mut work: Vec<(u16, bool)> = Vec::new();
    for (cid, prev_a, next_a, default_for) in pairs {
        let was = prev_diverted(*prev_a, *default_for);
        let now = next_diverted(*next_a, *default_for);
        if was != now {
            work.push((*cid, now));
        }
    }
    if work.is_empty() {
        return;
    }
    if let Ok(mut mgr) = haptic_manager.lock() {
        for (cid, want_diverted) in work {
            let res = if want_diverted {
                mgr.divert_single_button(cid)
            } else {
                mgr.undivert_single_button(cid)
            };
            match res {
                Ok(true) => tracing::info!(
                    cid = format!("0x{:04X}", cid),
                    diverted = want_diverted,
                    "Non-gesture button divert state synced"
                ),
                Ok(false) => tracing::debug!(
                    cid = format!("0x{:04X}", cid),
                    "Sync no-op (button not present or not divertable)"
                ),
                Err(e) => tracing::warn!(
                    cid = format!("0x{:04X}", cid),
                    error = %e,
                    "Failed to sync button divert state"
                ),
            }
        }
    }
}

/// Translate the user's `ScrollConfig` into a HID++ SmartShift call
/// and apply it to the device. Best-effort — silently logs and
/// continues if SmartShift isn't supported (older mouse, generic
/// device, or currently disconnected).
fn apply_scroll_to_device(manager: &mut HapticManager, scroll: &ScrollConfig) {
    if !manager.smartshift_supported() {
        tracing::debug!(
            "SmartShift not supported on this device — skipping scroll apply"
        );
        return;
    }

    // Match the Logitech-firmware-quirky mapping the legacy
    // Python settings used (proven working on MX Master 4):
    //   * freespin  → wheel_mode=1, auto_disengage=0
    //   * ratchet   → wheel_mode=2, auto_disengage=0
    //   * smartshift → wheel_mode=1, auto_disengage=(100−t)×2.55
    // The smartshift case puts the device into wheel_mode=1
    // (freespin) with a threshold byte that the firmware uses
    // to engage the detent at low speeds. Counter-intuitive but
    // correct per the legacy settings.
    let mode = scroll.mode.as_str();
    let smartshift_active =
        scroll.smartshift && mode != "ratchet" && mode != "freespin" && mode != "free";
    let (wheel_mode, auto_disengage): (u8, u8) = match mode {
        "freespin" | "free" => (1, 0),
        "ratchet" => (2, 0),
        // "smartshift" or any other slug: use Python's mapping.
        _ if smartshift_active => {
            let t = scroll.smartshift_threshold.clamp(1, 100) as i32;
            let scaled = ((100 - t) as f32 * 2.55).round().clamp(0.0, 255.0) as u8;
            (1, scaled)
        }
        // Smartshift toggle off but mode is "smartshift" → fall
        // back to plain ratchet so the toggle has meaning.
        _ => (2, 0),
    };

    // Don't touch the on-device default — pass 0 ("no change").
    match manager.set_smartshift(wheel_mode, auto_disengage, 0) {
        Ok(_) => {
            tracing::info!(
                wheel_mode,
                auto_disengage,
                requested_mode = %scroll.mode,
                smartshift = scroll.smartshift,
                "Applied SmartShift / wheel mode to device"
            );
        }
        Err(e) => {
            tracing::warn!(
                error = ?e,
                "SmartShift apply failed (device may be disconnected)"
            );
        }
    }
}

/// Apply the user's horizontal-scroll-invert toggle.
///
/// Pairs the on-device `(divert, invert)` write with the
/// uinput-forwarder lifecycle. Best-effort: logs and continues on
/// partial failures so a stale forwarder or a transient HID++ error
/// don't leave the system stuck. The boolean result is whether the
/// requested state was successfully reached end-to-end.
fn apply_thumb_wheel_invert(
    haptic_manager: &SharedHapticManager,
    thumb_wheel_state: &SharedThumbWheelState,
    invert: bool,
) -> Result<(), String> {
    // Firmware side. We always pair divert with whether the feature
    // is "active" — divert=true when invert is on, divert=false
    // when off (resuming kernel-managed REL_HWHEEL). We pass
    // invert=false to the firmware in both cases and do the actual
    // sign flip in our uinput forwarder; that keeps behaviour
    // deterministic regardless of how the MX4 firmware interprets
    // its own invert bit.
    let firmware_result = {
        let mut manager = haptic_manager
            .lock()
            .map_err(|e| format!("haptic_manager lock: {e}"))?;
        manager
            .set_thumb_wheel_reporting(invert, false)
            .map_err(|e| format!("set_thumb_wheel_reporting: {e}"))
    };
    if let Err(e) = firmware_result {
        tracing::warn!(error = %e, "ThumbWheel firmware write failed");
        return Err(e);
    }

    // Forwarder side.
    let mut state = thumb_wheel_state
        .lock()
        .map_err(|e| format!("thumb_wheel_state lock: {e}"))?;
    if invert {
        match state.forwarder.as_mut() {
            Some(fwd) => fwd.set_invert(true),
            None => match ThumbWheelForwarder::new(true) {
                Ok(fwd) => {
                    state.forwarder = Some(fwd);
                    tracing::info!("ThumbWheel forwarder enabled (invert=true)");
                }
                Err(e) => {
                    // Roll back the firmware divert if we can't
                    // forward — otherwise the user loses h-scroll
                    // entirely.
                    tracing::error!(error = %e, "ThumbWheel forwarder build failed — rolling back divert");
                    drop(state);
                    if let Ok(mut manager) = haptic_manager.lock() {
                        let _ = manager.set_thumb_wheel_reporting(false, false);
                    }
                    return Err(format!("forwarder build: {e}"));
                }
            },
        }
    } else if state.forwarder.take().is_some() {
        tracing::info!("ThumbWheel forwarder torn down (invert=false)");
    }

    Ok(())
}

#[interface(name = "org.oxidemx.Daemon")]
impl OxideMXService {
    // =========================================================================
    // MENU METHODS
    // =========================================================================

    /// Show the radial menu at the specified coordinates
    async fn show_menu(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        x: i32,
        y: i32,
    ) -> fdo::Result<()> {
        if let Ok(gm) = self.gaming_mode.read() {
            if gm.should_suppress_overlay() {
                tracing::debug!(x, y, "ShowMenu suppressed - gaming mode active");
                return Ok(());
            }
        }

        tracing::info!(x, y, "ShowMenu called - ensuring overlay is running");
        let _ = self.ensure_overlay_running().await;

        Self::menu_requested(&emitter, x, y).await?;
        Ok(())
    }

    /// Hide the radial menu
    async fn hide_menu(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        tracing::info!("HideMenu called - emitting HideMenu signal");
        Self::hide_menu_signal(&emitter).await?;
        Ok(())
    }

    /// Execute an action by its identifier
    async fn execute_action(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        action_id: String,
    ) -> fdo::Result<()> {
        tracing::info!(action_id = %action_id, "ExecuteAction called");
        Self::action_executed(&emitter, action_id).await?;
        Ok(())
    }

    // =========================================================================
    // MENU SIGNALS
    // =========================================================================

    #[zbus(signal)]
    async fn menu_requested(emitter: &SignalEmitter<'_>, x: i32, y: i32) -> zbus::Result<()>;

    #[zbus(signal, name = "HideMenu")]
    async fn hide_menu_signal(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn slice_selected(emitter: &SignalEmitter<'_>, index: u8) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_executed(emitter: &SignalEmitter<'_>, action_id: String) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn cursor_moved(emitter: &SignalEmitter<'_>, x: i32, y: i32) -> zbus::Result<()>;

    // =========================================================================
    // HAPTIC / PROFILE / CONFIG METHODS
    // =========================================================================

    /// Notify that a slice is being hovered
    async fn notify_slice_hover(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        index: u8,
    ) -> fdo::Result<()> {
        tracing::debug!(index, "Slice hover notification");
        Self::slice_selected(&emitter, index).await?;
        Ok(())
    }

    /// Trigger haptic feedback for a specific event
    async fn trigger_haptic(&self, event: &str) -> fdo::Result<()> {
        tracing::info!(event, "TriggerHaptic D-Bus method called");
        let haptic_event = match event {
            "menu_appear" => HapticEvent::MenuAppear,
            "slice_change" => HapticEvent::SliceChange,
            "confirm" => HapticEvent::SelectionConfirm,
            "invalid" => HapticEvent::InvalidAction,
            "page_change" => HapticEvent::PageChange,
            "submenu_open" => HapticEvent::SubmenuOpen,
            "submenu_close" => HapticEvent::SubmenuClose,
            _ => {
                tracing::warn!(event, "Unknown haptic event type");
                return Ok(());
            }
        };

        tracing::debug!("Attempting to lock haptic_manager");
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                tracing::debug!("Lock acquired, calling emit()");
                match manager.emit(haptic_event) {
                    Ok(()) => tracing::info!("Haptic emit succeeded"),
                    Err(e) => tracing::warn!(error = %e, "Haptic emit failed"),
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager");
            }
        }

        Ok(())
    }

    /// Set the active profile
    async fn set_profile(&self, name: &str) -> fdo::Result<()> {
        tracing::info!(name, "SetProfile called");
        Ok(())
    }

    /// Toggle haptic feedback globally. Mutates config.haptics.enabled,
    /// updates the live HapticManager so subsequent events are gated
    /// correctly, and persists the config to disk so the change
    /// survives daemon restarts.
    ///
    /// Called by the indicator popup's "Haptic Feedback" quick toggle.
    /// Returns Ok even if persist fails — the in-memory state is the
    /// authoritative one for the current session and the next
    /// settings-rs save would reconcile.
    async fn set_haptics_enabled(&self, enabled: bool) -> fdo::Result<()> {
        tracing::info!(enabled, "SetHapticsEnabled called");

        // Mutate in-memory config first so the haptic manager picks
        // up the new enabled flag on its next reload.
        let new_haptic_config = {
            match self.config.write() {
                Ok(mut config) => {
                    config.haptics.enabled = enabled;
                    config.haptics.clone()
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to lock config for SetHapticsEnabled");
                    return Err(fdo::Error::Failed(format!("Lock error: {}", e)));
                }
            }
        };

        // Push the live haptic manager so emit() calls respect the
        // new gate immediately (without waiting for the next
        // reload-from-disk cycle).
        if let Ok(mut manager) = self.haptic_manager.lock() {
            manager.update_from_config(&new_haptic_config);
        }

        // Best-effort disk persist. A failure here means settings-rs
        // (if running) would override our in-memory state on its
        // next save; logged but not surfaced as an Err.
        if let Ok(snapshot) = self.config.read() {
            if let Err(e) = snapshot.save() {
                tracing::warn!(error = %e, "SetHapticsEnabled disk persist failed; in-memory state still updated");
            }
        }

        Ok(())
    }

    /// Synthesize a keyboard shortcut into the focused window —
    /// xdotool first, ydotool fallback. Format mirrors xdotool's
    /// `key` argument: `"ctrl+c"`, `"ctrl+shift+z"`, `"super+e"`.
    /// Used by the radial overlay to dispatch slices configured
    /// with a shortcut binding (`ActionKind::Shortcut`).
    async fn execute_shortcut(&self, keys: &str) -> fdo::Result<()> {
        tracing::info!(keys, "ExecuteShortcut D-Bus method called");
        let action = crate::actions::Action {
            action_type: crate::actions::ActionType::Shortcut(keys.to_string()),
            label: None,
            icon: None,
        };
        match crate::actions::ActionExecutor::execute(&action).await {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::warn!(error = %e, "ExecuteShortcut failed");
                Err(fdo::Error::Failed(format!("Shortcut failed: {e}")))
            }
        }
    }

    /// Reload configuration from disk
    async fn reload_config(&self) -> fdo::Result<()> {
        tracing::info!("ReloadConfig called - reloading configuration from disk");

        match Config::load_default() {
            Ok(new_config) => {
                let haptic_config = new_config.haptics.clone();
                let new_scroll = new_config.scroll.clone();
                let new_pointer = new_config.pointer.clone();

                // Snapshot the previous scroll + pointer configs so
                // we only re-issue device / gsettings writes when
                // the user actually changed those fields. The
                // settings GUI autosaves on every keystroke;
                // without these gates we'd flood the mouse with
                // redundant HID++ commands (which froze the cursor
                // in a prior incident) and shell out to gsettings
                // dozens of times per minute.
                let (prev_scroll, prev_pointer, prev_buttons) = self
                    .config
                    .read()
                    .ok()
                    .map(|c| (c.scroll.clone(), c.pointer.clone(), c.buttons.clone()))
                    .unwrap_or_default();

                match self.config.write() {
                    Ok(mut config) => {
                        *config = new_config;
                        tracing::info!(
                            haptics_enabled = config.haptics.enabled,
                            default_pattern = %config.haptics.default_pattern,
                            theme = %config.theme,
                            scroll_mode = %config.scroll.mode,
                            smartshift = config.scroll.smartshift,
                            pointer_speed = config.pointer.speed,
                            "Configuration reloaded successfully"
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to acquire config write lock");
                        return Err(fdo::Error::Failed(format!("Lock error: {}", e)));
                    }
                }

                match self.haptic_manager.lock() {
                    Ok(mut manager) => {
                        manager.update_from_config(&haptic_config);
                        tracing::info!(
                            default_pattern = %haptic_config.default_pattern,
                            menu_appear = %haptic_config.per_event.menu_appear,
                            slice_change = %haptic_config.per_event.slice_change,
                            confirm = %haptic_config.per_event.confirm,
                            invalid = %haptic_config.per_event.invalid,
                            "Haptic manager updated with new patterns"
                        );

                        // Only apply if the SmartShift-relevant
                        // fields actually changed. Repeated
                        // identical HID++ writes wedged the device
                        // in a prior incident.
                        if scroll_smartshift_changed(&prev_scroll, &new_scroll) {
                            apply_scroll_to_device(&mut *manager, &new_scroll);
                        } else {
                            tracing::debug!(
                                "Scroll SmartShift config unchanged — skipping HID++ apply"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to lock haptic manager for update");
                        return Err(fdo::Error::Failed(format!("Haptic manager lock error: {}", e)));
                    }
                }

                // GNOME-side application via gsettings — pointer
                // speed/acceleration + natural-scroll. Same
                // change-gating logic so a settings-edit storm
                // doesn't fork-bomb gsettings.
                if pointer_changed(&prev_pointer, &new_pointer) {
                    apply_pointer_to_gnome(&new_pointer);
                }
                if scroll_gsettings_changed(&prev_scroll, &new_scroll) {
                    apply_scroll_to_gnome(&new_scroll);
                }

                // Sync non-gesture button divert state with the
                // new buttons config. For each non-gesture CID,
                // the desired state is "diverted iff configured
                // action != default for that button". Compare
                // against the prev state and call divert /
                // undivert as needed so toggling a button between
                // a custom action and the default takes effect
                // without a daemon restart.
                let new_buttons = self
                    .config
                    .read()
                    .ok()
                    .map(|c| c.buttons.clone())
                    .unwrap_or_default();
                sync_non_gesture_diverts(&self.haptic_manager, &prev_buttons, &new_buttons);

                // ThumbWheel side-scroll invert — apply only when
                // the field actually changed so we don't hammer
                // the device on every save. Best-effort: missing
                // ThumbWheel feature support silently skips. See
                // `apply_thumb_wheel_invert` for the divert + uinput
                // forwarder mechanics (the firmware invert bit alone
                // doesn't work on MX Master 4).
                if prev_scroll.horizontal_invert != new_scroll.horizontal_invert {
                    if let Err(e) = apply_thumb_wheel_invert(
                        &self.haptic_manager,
                        &self.thumb_wheel_state,
                        new_scroll.horizontal_invert,
                    ) {
                        tracing::warn!(error = %e, "ThumbWheel apply failed");
                    } else {
                        tracing::info!(
                            invert = new_scroll.horizontal_invert,
                            "ThumbWheel side-scroll inversion applied"
                        );
                    }
                }

                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to reload configuration");
                Err(fdo::Error::Failed(format!("Config reload failed: {}", e)))
            }
        }
    }

    // Helper kept inline as a free function (rather than a method
    // on impl) so it can run while we hold the haptic_manager lock
    // without the borrow checker confusing receiver lifetimes.

    /// Called by KWin script to report cursor position and show menu
    async fn show_menu_at_cursor(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        x: i32,
        y: i32,
    ) -> fdo::Result<()> {
        tracing::info!(x, y, "ShowMenuAtCursor called from KWin script - ensuring overlay is running");
        let _ = self.ensure_overlay_running().await;

        Self::menu_requested(&emitter, x, y).await?;
        Ok(())
    }

    /// Get battery status from the device
    async fn get_battery_status(&self) -> fdo::Result<(u8, bool)> {
        let state = self.battery_state.read().await;
        if state.available {
            Ok((state.percentage, state.charging))
        } else {
            Ok((0, false))
        }
    }

    // =========================================================================
    // INDICATOR METHODS
    // =========================================================================

    /// Returns the active device state in one shot.
    /// Tuple: (battery_percent, charging, connection_kind, device_name, device_id)
    /// connection_kind ∈ {"bluetooth", "unifying", "bolt", "usb", "off"}.
    ///
    /// battery + charging are sourced from the real SharedBatteryState.
    /// connection_kind is derived from self.device_mode via
    /// normalize_connection_kind(); "off" when battery is unavailable.
    /// device_name comes from self.device_name (set at startup by the
    /// HID++ probe or evdev fallback).
    /// device_id is empty — no hidraw-path source exists on
    /// OxideMXService yet.
    /// TODO: thread hidraw path through OxideMXService when
    ///       device-cache module lands.
    async fn get_active_device_state(
        &self,
    ) -> fdo::Result<(u8, bool, String, String, String)> {
        let state = self.battery_state.read().await;
        let (battery, charging) = if state.available {
            (state.percentage, state.charging)
        } else {
            (0u8, false)
        };
        let connection = normalize_connection_kind(&self.device_mode, state.available);
        let name = self.device_name.clone();
        // device_id: no hidraw-path source on OxideMXService yet.
        // TODO: thread hidraw path through OxideMXService when
        //       device-cache module lands.
        let id = String::new();
        Ok((battery, charging, connection, name, id))
    }

    /// Spawns the indicator popup as a one-shot subprocess, or kills it if already running (toggle behavior).
    /// panel_{x,y,w,h} are stage-absolute Mutter logical pixels of the
    /// indicator's panel rect so the popup can position its tip underneath.
    async fn show_popup(
        &self,
        _panel_x: i32,
        _panel_y: i32,
        _panel_w: i32,
        _panel_h: i32,
    ) -> fdo::Result<()> {
        tracing::info!("ShowPopup invoked on daemon, but the extension uses native GJS popover now. Skipping.");
        Ok(())
    }

    /// Idempotent overlay-process ensure. See `crate::overlay_spawner`
    /// for the contract and timing.
    async fn ensure_overlay_running(&self) -> fdo::Result<()> {
        // Need a session-bus connection. The daemon already has one via
        // its own service registration. zbus exposes the connection on
        // any SignalEmitter, but we don't want to require an emitter for
        // a method that doesn't fire signals — so we open a fresh
        // session connection (zbus pools internally, so this is cheap).
        let conn = zbus::Connection::session()
            .await
            .map_err(|e| fdo::Error::Failed(format!("session bus: {e}")))?;
        self.overlay_spawner
            .ensure_running(&conn)
            .await
            .map_err(fdo::Error::Failed)
    }

    /// Emitted after each battery poll when any of (battery %, charging,
    /// available) changes. Subscribed-to by the GNOME indicator extension
    /// for push updates.
    ///
    /// The device_name / connection / device_id fields carry empty strings
    /// in the battery-poll emit path (see battery.rs). Consumers that need
    /// those fields should follow up with a GetActiveDeviceState call.
    #[zbus(signal)]
    async fn device_state_changed(
        emitter: &SignalEmitter<'_>,
        battery: u8,
        charging: bool,
        connection: String,
        device_name: String,
        device_id: String,
    ) -> zbus::Result<()>;

    // =========================================================================
    // DPI METHODS
    // =========================================================================

    async fn get_dpi(&self) -> fdo::Result<u16> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => Ok(manager.get_dpi().unwrap_or(0)),
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for get_dpi");
                Ok(0)
            }
        }
    }

    async fn set_dpi(&self, dpi: u16) -> fdo::Result<()> {
        tracing::info!(dpi, "SetDpi called");

        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                match manager.set_dpi(dpi) {
                    Ok(()) => {
                        tracing::info!(dpi, "DPI set successfully");
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, dpi, "Failed to set DPI");
                        Err(fdo::Error::Failed(format!("Failed to set DPI: {}", e)))
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for set_dpi");
                Err(fdo::Error::Failed(format!("Lock error: {}", e)))
            }
        }
    }

    async fn dpi_supported(&self) -> fdo::Result<bool> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => Ok(manager.dpi_supported()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for dpi_supported");
                Ok(false)
            }
        }
    }

    // =========================================================================
    // SMARTSHIFT METHODS
    // =========================================================================

    async fn get_smart_shift(&self) -> fdo::Result<(bool, u8)> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                match manager.get_smartshift() {
                    Some((_wheel_mode, auto_disengage, _auto_disengage_default)) => {
                        let enabled = auto_disengage > 0;
                        let threshold = if enabled { auto_disengage } else { 30 };
                        Ok((enabled, threshold))
                    }
                    None => Ok((false, 0))
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for get_smart_shift");
                Ok((false, 0))
            }
        }
    }

    /// Read the current wheel mode from the device and return it as
    /// a `(slug, threshold)` pair the settings UI can drop straight
    /// into the picker. Slug is one of "freespin" / "ratchet" /
    /// "smartshift". Threshold is the user-visible 1..100 value
    /// (only meaningful for "smartshift"). Returns ("ratchet", 0)
    /// when the feature isn't supported, so the UI doesn't have to
    /// special-case Option.
    async fn get_wheel_mode(&self) -> fdo::Result<(String, u8)> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => match manager.get_wheel_mode() {
                Some((slug, threshold)) => Ok((slug, threshold)),
                None => Ok(("ratchet".to_string(), 0)),
            },
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock manager for get_wheel_mode");
                Ok(("ratchet".to_string(), 0))
            }
        }
    }

    async fn set_smart_shift(&self, enabled: bool, threshold: u8) -> fdo::Result<()> {
        tracing::info!(enabled, threshold, "SetSmartShift called");

        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                let wheel_mode = if enabled { 1u8 } else { 2u8 };
                let auto_disengage = if enabled { threshold } else { 255u8 };
                let auto_disengage_default = auto_disengage;

                match manager.set_smartshift(wheel_mode, auto_disengage, auto_disengage_default) {
                    Ok(()) => {
                        tracing::info!(enabled, threshold, "SmartShift set successfully");
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, enabled, threshold, "Failed to set SmartShift");
                        Err(fdo::Error::Failed(format!("Failed to set SmartShift: {}", e)))
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for set_smart_shift");
                Err(fdo::Error::Failed(format!("Lock error: {}", e)))
            }
        }
    }

    /// Set the wheel mode atomically. `mode` accepts:
    ///   * `"freespin"` (or `"free"` for back-compat) — wheel
    ///     spins continuously with no ratchet detents.
    ///     wheel_mode=1, auto_disengage=0.
    ///   * `"ratchet"` — clicky ratchet always engaged, never
    ///     auto-disengages. wheel_mode=2, auto_disengage=0.
    ///   * `"smartshift"` — clicky at low speed, auto-disengages
    ///     to free spin past the user's threshold.
    ///     wheel_mode=1 (Logitech firmware quirk: smartshift
    ///     uses the freespin mode flag with a threshold rather
    ///     than the ratchet mode flag — matches the legacy
    ///     Python settings binary which is the proven-working
    ///     reference).
    ///     auto_disengage = (100 − ui_threshold) × 2.55, so
    ///     ui_threshold=1 (low speed → easy auto-trigger) maps
    ///     to a high device value, ui_threshold=100 (high speed
    ///     needed) maps to a low device value.
    ///
    /// Settings-rs calls this directly on every wheel-mode picker
    /// change so the device reflects the new mode without waiting
    /// for the debounced config save + ReloadConfig round-trip.
    async fn set_wheel_mode(&self, mode: &str, threshold: u8) -> fdo::Result<()> {
        tracing::info!(mode, threshold, "SetWheelMode called");
        let (wheel_mode, auto_disengage) = match mode {
            // Python compat: legacy slug "free" maps to "freespin".
            "freespin" | "free" => (1u8, 0u8),
            "ratchet" => (2u8, 0u8),
            "smartshift" | _ => {
                let t = threshold.clamp(1, 100) as i32;
                let scaled =
                    ((100 - t) as f32 * 2.55).round().clamp(0.0, 255.0) as u8;
                (1u8, scaled)
            }
        };

        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                if !manager.smartshift_supported() {
                    tracing::warn!(
                        mode,
                        "SmartShift / wheel-mode not supported on this device"
                    );
                    return Err(fdo::Error::NotSupported(
                        "SmartShift feature not available on this device".into(),
                    ));
                }
                match manager.set_smartshift(wheel_mode, auto_disengage, 0) {
                    Ok(()) => {
                        tracing::info!(
                            wheel_mode,
                            auto_disengage,
                            mode,
                            "Wheel mode applied"
                        );
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            wheel_mode,
                            auto_disengage,
                            "Failed to set wheel mode"
                        );
                        Err(fdo::Error::Failed(format!("HID++ error: {}", e)))
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager");
                Err(fdo::Error::Failed(format!("Lock error: {}", e)))
            }
        }
    }

    /// Toggle horizontal-scroll inversion on the MX Master 4.
    ///
    /// On this device the firmware's 0x2150 invert bit is silently
    /// ignored unless the wheel is *diverted*. So enabling invert
    /// here actually:
    ///   1. Sets `(divert=true, invert=false)` on the device — the
    ///      firmware stops emitting REL_HWHEEL itself and starts
    ///      broadcasting HID++ notifications with signed displacement.
    ///   2. Creates a small uinput companion device that re-emits
    ///      `REL_HWHEEL` with the sign flipped (which is what gives
    ///      the user the reversed direction).
    ///
    /// Disabling does the inverse — tears down the forwarder and
    /// puts the wheel back in normal kernel-managed mode.
    ///
    /// See `daemon/src/thumb_wheel.rs` for the why.
    async fn set_thumb_wheel_invert(&self, invert: bool) -> fdo::Result<()> {
        tracing::info!(invert, "SetThumbWheelInvert called");
        if let Ok(manager) = self.haptic_manager.lock() {
            if !manager.thumb_wheel_supported() {
                return Err(fdo::Error::NotSupported(
                    "ThumbWheel not available on this device".into(),
                ));
            }
        } else {
            return Err(fdo::Error::Failed("Lock error".into()));
        }
        apply_thumb_wheel_invert(&self.haptic_manager, &self.thumb_wheel_state, invert)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Whether the device exposes ThumbWheel — settings UI gates
    /// the horizontal-scroll-reverse toggle on this.
    async fn thumb_wheel_supported(&self) -> fdo::Result<bool> {
        match self.haptic_manager.lock() {
            Ok(manager) => Ok(manager.thumb_wheel_supported()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock manager for thumb_wheel_supported");
                Ok(false)
            }
        }
    }

    /// Read live ThumbWheel state from the device. Returns
    /// `(divert, invert)`. Used by the settings UI to show the
    /// current device-side flag (so the UI doesn't drift from
    /// what the mouse actually has) and by us for diagnostics.
    async fn get_thumb_wheel_status(&self) -> fdo::Result<(bool, bool)> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => match manager.get_thumb_wheel_status() {
                Some((divert, invert)) => {
                    tracing::info!(divert, invert, "ThumbWheel status read");
                    Ok((divert, invert))
                }
                None => Err(fdo::Error::Failed(
                    "ThumbWheel read failed (unsupported or no device)".into(),
                )),
            },
            Err(e) => Err(fdo::Error::Failed(format!("Lock error: {}", e))),
        }
    }

    async fn smart_shift_supported(&self) -> fdo::Result<bool> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => Ok(manager.smartshift_supported()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for smart_shift_supported");
                Ok(false)
            }
        }
    }

    // =========================================================================
    // HIRESSCROLL METHODS
    // =========================================================================

    async fn get_hiresscroll_mode(&self) -> fdo::Result<(bool, bool, bool)> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                match manager.get_hiresscroll_mode() {
                    Some((hires, invert, target)) => Ok((hires, invert, target)),
                    None => Ok((true, false, false))
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for get_hiresscroll_mode");
                Ok((true, false, false))
            }
        }
    }

    async fn set_hiresscroll_mode(&self, hires: bool, invert: bool, target: bool) -> fdo::Result<()> {
        tracing::info!(hires, invert, target, "SetHiResScrollMode called");

        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                match manager.set_hiresscroll_mode(hires, invert, target) {
                    Ok(()) => {
                        tracing::info!(hires, invert, target, "HiResScroll mode set successfully");
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, hires, invert, target, "Failed to set HiResScroll mode");
                        Err(fdo::Error::Failed(format!("Failed to set HiResScroll mode: {}", e)))
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for set_hiresscroll_mode");
                Err(fdo::Error::Failed(format!("Lock error: {}", e)))
            }
        }
    }

    // =========================================================================
    // EASY-SWITCH METHODS
    // =========================================================================

    async fn get_host_names(&self) -> fdo::Result<Vec<String>> {
        // Cache hit: settings polls every 5s but the host names
        // change only on pair / unpair (rare). 30 s TTL keeps the
        // UI responsive without hammering the device + log.
        if let Ok(cache) = self.easy_switch_cache.read() {
            if let Some((names, at)) = &cache.host_names {
                if at.elapsed() < super::service::EASY_SWITCH_TTL {
                    return Ok(names.clone());
                }
            }
        }
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                let names = manager.get_host_names();
                tracing::debug!(host_names = ?names, "Easy-Switch host names retrieved");
                if let Ok(mut cache) = self.easy_switch_cache.write() {
                    cache.host_names = Some((names.clone(), std::time::Instant::now()));
                }
                Ok(names)
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for get_host_names");
                Ok(Vec::new())
            }
        }
    }

    async fn get_easy_switch_info(&self) -> fdo::Result<(u8, u8)> {
        // Same TTL cache — info only changes on pair/unpair/host
        // switch, settings poll otherwise.
        if let Ok(cache) = self.easy_switch_cache.read() {
            if let Some((info, at)) = &cache.info {
                if at.elapsed() < super::service::EASY_SWITCH_TTL {
                    return Ok(*info);
                }
            }
        }
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                match manager.get_easy_switch_info() {
                    Some((num, current)) => {
                        tracing::debug!(num_hosts = num, current_host = current, "Easy-Switch info retrieved");
                        if let Ok(mut cache) = self.easy_switch_cache.write() {
                            cache.info = Some(((num, current), std::time::Instant::now()));
                        }
                        Ok((num, current))
                    }
                    None => {
                        tracing::debug!("Easy-Switch not supported or unavailable");
                        Ok((0, 0))
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for get_easy_switch_info");
                Ok((0, 0))
            }
        }
    }

    async fn set_host(&self, host_index: u8) -> fdo::Result<bool> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                match manager.set_current_host(host_index) {
                    Ok(()) => {
                        tracing::info!(host_index, "Switched to Easy-Switch host");
                        Ok(true)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, host_index, "Failed to switch host");
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for set_host");
                Ok(false)
            }
        }
    }

    // =========================================================================
    // MACRO METHODS
    // =========================================================================

    async fn start_macro_recording(&self) -> fdo::Result<()> {
        tracing::info!("StartMacroRecording called");

        match self.macro_recorder.lock() {
            Ok(mut recorder) => {
                match recorder.start() {
                    Ok(()) => {
                        tracing::info!("Macro recording started");
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to start recording");
                        Err(fdo::Error::Failed(format!("Recording failed: {}", e)))
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock macro recorder");
                Err(fdo::Error::Failed(format!("Lock error: {}", e)))
            }
        }
    }

    async fn stop_macro_recording(&self) -> fdo::Result<String> {
        tracing::info!("StopMacroRecording called");

        match self.macro_recorder.lock() {
            Ok(mut recorder) => {
                let events = recorder.stop();
                let actions = events_to_actions(&events);

                tracing::info!(
                    event_count = events.len(),
                    action_count = actions.len(),
                    "Macro recording stopped"
                );

                let result = serde_json::json!({
                    "events": events,
                    "actions": actions,
                });

                serde_json::to_string(&result)
                    .map_err(|e| fdo::Error::Failed(format!("JSON serialization error: {}", e)))
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock macro recorder");
                Err(fdo::Error::Failed(format!("Lock error: {}", e)))
            }
        }
    }

    async fn execute_macro(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: String,
    ) -> fdo::Result<()> {
        tracing::info!(id = %id, "ExecuteMacro called");

        let config = crate::macros::storage::load_macro(&id)
            .map_err(|e| fdo::Error::Failed(format!("Failed to load macro: {}", e)))?;

        let macro_id = config.id.clone();
        {
            let mut engine = self.macro_engine.lock()
                .map_err(|e| fdo::Error::Failed(format!("Lock error: {}", e)))?;
            engine.execute(config);
        }

        Self::macro_playback_started(&emitter, macro_id).await?;
        Ok(())
    }

    async fn execute_macro_inline(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        json: String,
    ) -> fdo::Result<()> {
        tracing::info!("ExecuteMacroInline called");

        let config: crate::macros::MacroConfig = serde_json::from_str(&json)
            .map_err(|e| fdo::Error::Failed(format!("Invalid macro JSON: {}", e)))?;

        let macro_id = config.id.clone();
        {
            let mut engine = self.macro_engine.lock()
                .map_err(|e| fdo::Error::Failed(format!("Lock error: {}", e)))?;
            engine.execute(config);
        }

        Self::macro_playback_started(&emitter, macro_id).await?;
        Ok(())
    }

    async fn stop_macro(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        tracing::info!("StopMacro called");

        {
            let mut engine = self.macro_engine.lock()
                .map_err(|e| fdo::Error::Failed(format!("Lock error: {}", e)))?;
            engine.stop();
        }

        Self::macro_playback_stopped(&emitter, String::new()).await?;
        Ok(())
    }

    async fn save_macro(&self, json: String) -> fdo::Result<()> {
        tracing::info!("SaveMacro called");

        let config: crate::macros::MacroConfig = serde_json::from_str(&json)
            .map_err(|e| fdo::Error::Failed(format!("Invalid macro JSON: {}", e)))?;

        crate::macros::storage::save_macro(&config)
            .map_err(|e| fdo::Error::Failed(format!("Failed to save macro: {}", e)))?;

        tracing::info!(id = %config.id, name = %config.name, "Macro saved via D-Bus");
        Ok(())
    }

    async fn delete_macro(&self, id: String) -> fdo::Result<()> {
        tracing::info!(id = %id, "DeleteMacro called");

        crate::macros::storage::delete_macro(&id)
            .map_err(|e| fdo::Error::Failed(format!("Failed to delete macro: {}", e)))?;

        Ok(())
    }

    async fn list_macros(&self) -> fdo::Result<String> {
        let macros = crate::macros::storage::load_all_macros()
            .map_err(|e| fdo::Error::Failed(format!("Failed to load macros: {}", e)))?;

        let list: Vec<&crate::macros::MacroConfig> = macros.values().collect();
        serde_json::to_string(&list)
            .map_err(|e| fdo::Error::Failed(format!("JSON error: {}", e)))
    }

    async fn is_macro_running(&self) -> fdo::Result<bool> {
        match self.macro_engine.lock() {
            Ok(engine) => Ok(engine.is_running()),
            Err(_) => Ok(false),
        }
    }

    /// Reload macro trigger bindings from disk
    async fn reload_macro_triggers(&self) -> fdo::Result<()> {
        tracing::info!("ReloadMacroTriggers called");

        match self.trigger_map.write() {
            Ok(mut map) => {
                map.reload();
                tracing::info!("Macro trigger map reloaded");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock trigger map for reload");
                Err(fdo::Error::Failed(format!("Lock error: {}", e)))
            }
        }
    }

    // =========================================================================
    // MACRO SIGNALS
    // =========================================================================

    #[zbus(signal)]
    async fn macro_playback_started(emitter: &SignalEmitter<'_>, id: String) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn macro_playback_stopped(emitter: &SignalEmitter<'_>, id: String) -> zbus::Result<()>;

    // =========================================================================
    // GAMING MODE METHODS
    // =========================================================================

    async fn set_gaming_mode(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        enabled: bool,
    ) -> fdo::Result<()> {
        tracing::info!(enabled, "SetGamingMode called");

        // Snapshot the haptic-redirect settings out of the config
        // lock first, so we don't hold two locks at once when we
        // take the gaming-mode write lock below.
        let redirect_cfg = self
            .config
            .read()
            .map(|c| c.gaming.haptic_redirect.clone())
            .unwrap_or_default();

        {
            let mut gm = self.gaming_mode.write()
                .map_err(|e| fdo::Error::Failed(format!("Lock error: {}", e)))?;
            if enabled {
                gm.enable(&redirect_cfg);
            } else {
                gm.disable();
            }
        }

        Self::gaming_mode_changed(&emitter, enabled).await?;
        Ok(())
    }

    async fn get_gaming_mode(&self) -> fdo::Result<bool> {
        match self.gaming_mode.read() {
            Ok(gm) => Ok(gm.is_enabled()),
            Err(_) => Ok(false),
        }
    }

    async fn cycle_gaming_dpi(&self) -> fdo::Result<String> {
        match self.gaming_mode.write() {
            Ok(mut gm) => Ok(gm.cycle_dpi().unwrap_or_default()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock gaming mode for DPI cycle");
                Ok(String::new())
            }
        }
    }

    #[zbus(signal)]
    async fn gaming_mode_changed(emitter: &SignalEmitter<'_>, enabled: bool) -> zbus::Result<()>;

    /// Fire a one-shot test pulse through the gamepad-rumble →
    /// haptic translator. Lets the settings "Test haptic" button
    /// verify the chain without launching a game.
    async fn test_haptic_redirect(&self) -> fdo::Result<()> {
        tracing::info!("TestHapticRedirect called");
        let redirect_cfg = self
            .config
            .read()
            .map(|c| c.gaming.haptic_redirect.clone())
            .unwrap_or_default();
        crate::gamepad_haptics::fire_test_pulse(&redirect_cfg, &self.haptic_manager);
        Ok(())
    }

    /// Return a human-readable diagnostic report for the gamepad-
    /// rumble → haptic bridge (Steam state, virtual pads, connected
    /// controllers, and a recommendation). Backs the settings
    /// "Diagnose" button.
    async fn diagnose_haptic_redirect(&self) -> fdo::Result<String> {
        let redirect_cfg = self
            .config
            .read()
            .map(|c| c.gaming.haptic_redirect.clone())
            .unwrap_or_default();
        Ok(crate::gamepad_haptics::diagnostic_report(&redirect_cfg))
    }

    // =========================================================================
    // DEVICE MODE METHODS
    // =========================================================================

    async fn get_device_mode(&self) -> fdo::Result<String> {
        Ok(self.device_mode.clone())
    }

    async fn get_device_name(&self) -> fdo::Result<String> {
        Ok(self.device_name.clone())
    }

    // =========================================================================
    // PROPERTIES
    // =========================================================================

    #[zbus(property)]
    async fn current_profile(&self) -> &str {
        &self.current_profile
    }

    #[zbus(property)]
    async fn haptics_enabled(&self) -> bool {
        self.config
            .read()
            .map(|c| c.haptics.enabled)
            .unwrap_or(true)
    }

    #[zbus(property)]
    async fn daemon_version(&self) -> &str {
        &self.version
    }

    #[zbus(property)]
    async fn device_mode(&self) -> &str {
        &self.device_mode
    }

    #[zbus(property)]
    async fn device_name(&self) -> &str {
        &self.device_name
    }

    #[zbus(property)]
    async fn gaming_mode_enabled(&self) -> bool {
        self.gaming_mode
            .read()
            .map(|gm| gm.is_enabled())
            .unwrap_or(false)
    }
}
