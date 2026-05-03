//! D-Bus interface implementation
//!
//! All methods, signals, and properties for org.kde.juhradialmx.Daemon.
//! This must be a single `#[interface]` impl block per zbus requirements.

use zbus::{interface, object_server::SignalEmitter, fdo};
use crate::config::{Config, PointerConfig, ScrollConfig};
use crate::hidpp::{HapticEvent, HapticManager};
use crate::macros::events_to_actions;
use super::service::JuhRadialService;

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

    // wheel_mode: 1 = Freespin, 2 = Ratchet
    let wheel_mode: u8 = match scroll.mode.as_str() {
        "free" => 1,
        // "ratchet" + "smartshift" both use the device's Ratchet
        // mode; the difference is whether auto-disengage is on,
        // which is what the threshold byte controls.
        _ => 2,
    };

    // auto_disengage: 1..254 = N/4 turns/sec, 255 = always engaged.
    // When SmartShift is OFF (or mode == "ratchet"), pin to 255 so
    // the wheel never auto-disengages. Otherwise scale the user's
    // 1..100 threshold linearly into the 1..254 device range.
    let auto_disengage: u8 = if !scroll.smartshift || scroll.mode == "ratchet" {
        255
    } else {
        let t = scroll.smartshift_threshold.clamp(1, 100);
        let scaled = ((t as f32 / 100.0) * 254.0).round().clamp(1.0, 254.0);
        scaled as u8
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

#[interface(name = "org.kde.juhradialmx.Daemon")]
impl JuhRadialService {
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

        tracing::info!(x, y, "ShowMenu called - emitting MenuRequested signal");
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
                let (prev_scroll, prev_pointer) = self
                    .config
                    .read()
                    .ok()
                    .map(|c| (c.scroll.clone(), c.pointer.clone()))
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
        tracing::info!(x, y, "ShowMenuAtCursor called from KWin script");
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
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                let names = manager.get_host_names();
                tracing::info!(host_names = ?names, "Easy-Switch host names retrieved");
                Ok(names)
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to lock haptic manager for get_host_names");
                Ok(Vec::new())
            }
        }
    }

    async fn get_easy_switch_info(&self) -> fdo::Result<(u8, u8)> {
        match self.haptic_manager.lock() {
            Ok(mut manager) => {
                match manager.get_easy_switch_info() {
                    Some((num, current)) => {
                        tracing::info!(num_hosts = num, current_host = current, "Easy-Switch info retrieved");
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

        {
            let mut gm = self.gaming_mode.write()
                .map_err(|e| fdo::Error::Failed(format!("Lock error: {}", e)))?;
            if enabled {
                gm.enable();
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
