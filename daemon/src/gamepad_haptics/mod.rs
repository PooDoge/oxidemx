//! Gamepad-rumble → MX Master 4 haptic bridge.
//!
//! When Game Mode is enabled with `gaming.haptic_redirect.enabled`,
//! this module stands up a virtual evdev gamepad that games send
//! their rumble to. The captured `FF_RUMBLE` effects are re-rendered
//! as haptic patterns on the MX Master 4's piezo actuator.
//!
//! Full design — including the rumble-capture surface, Steam Input
//! interaction, and the implementation phases — lives in
//! `oxidemx/HAPTIC_GAMEPAD_BRIDGE_DESIGN.md`.
//!
//! ## Implementation status
//!
//! * Phase 3 created the virtual gamepad and kept it alive for the
//!   lifetime of Game Mode.
//! * Phase 4 serviced the `UI_FF_UPLOAD` / `UI_FF_ERASE` handshake.
//! * Phase 5 translated captured rumble to MX-4 haptic patterns.
//! * Phase 6 added `Proxy` mode — wrapping a real controller.
//! * **Phase 7 (this commit):** diagnostics + the "Test haptic"
//!   one-shot, backing the settings tab's two buttons. See
//!   [`diagnostics`] and [`fire_test_pulse`].

mod diagnostics;
mod ff_protocol;
mod proxy;
mod translator;
mod virtual_pad;

pub use diagnostics::diagnostic_report;
pub use translator::fire_test_pulse;

use oxidemx_shared::{HapticRedirectConfig, HapticRedirectMode};
use tokio::sync::oneshot;

use crate::hidpp::SharedHapticManager;

/// A running instance of the haptic-redirect bridge.
///
/// Created by [`GamepadHapticsService::start`] when Game Mode turns
/// on with `haptic_redirect.enabled = true`. Dropping the service
/// signals its background task to tear down the virtual gamepad, so
/// `GamingMode::disable()` only has to drop its `Option<_>`.
pub struct GamepadHapticsService {
    /// Held only so its `Drop` resolves the task's shutdown future.
    /// A `oneshot::Receiver` yields `Err(RecvError)` once every
    /// `Sender` is gone — that `Err` is our shutdown signal.
    _shutdown: oneshot::Sender<()>,
}

impl GamepadHapticsService {
    /// Spawn the bridge. Returns immediately; the virtual gamepad is
    /// created on a background task. Device-creation failures are
    /// logged via `tracing` rather than propagated — matching the
    /// rest of `GamingMode`'s best-effort style (a missing `uinput`
    /// module shouldn't abort Game Mode's DPI + overlay changes).
    ///
    /// `haptics` is the daemon's shared [`HapticManager`] — the
    /// translator dispatches the MX-4 pulses through it.
    ///
    /// Must be called from within a Tokio runtime (it is — the only
    /// caller is the async `SetGamingMode` D-Bus handler).
    pub fn start(
        config: HapticRedirectConfig,
        haptics: SharedHapticManager,
        handle: tokio::runtime::Handle,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        handle.spawn(run(config, haptics, shutdown_rx));
        Self {
            _shutdown: shutdown_tx,
        }
    }
}

/// Background task: create the virtual pad, then service its
/// force-feedback traffic until shutdown.
///
/// The `UI_FF_UPLOAD` servicing loop lives in [`ff_protocol::serve`],
/// which takes ownership of the device and drops it (destroying the
/// kernel node) when the `shutdown` future resolves.
async fn run(
    config: HapticRedirectConfig,
    haptics: SharedHapticManager,
    shutdown: oneshot::Receiver<()>,
) {
    // In Proxy mode, find and grab a real controller to wrap. A
    // failure to find one degrades gracefully to Standalone.
    let real = if matches!(config.mode, HapticRedirectMode::Proxy) {
        if config.passthrough_to_pad {
            tracing::info!(
                "haptic redirect: 'passthrough to pad' is not implemented yet — \
                 rumble goes to the mouse only"
            );
        }
        if config.hard_hide_real_controller {
            tracing::info!(
                "haptic redirect: 'hard-hide real controller' is not implemented \
                 yet — the controller is grabbed (EVIOCGRAB) but still enumerable"
            );
        }
        let controller = proxy::discover_controller(&config);
        if controller.is_none() {
            tracing::info!(
                "haptic redirect: proxy mode requested but no controller found — \
                 running standalone"
            );
        }
        controller
    } else {
        None
    };

    let mut device = match virtual_pad::build_virtual_pad() {
        Ok(device) => device,
        Err(e) => {
            tracing::error!(
                error = %e,
                "haptic redirect: failed to create the virtual gamepad — is the \
                 'uinput' kernel module loaded and the udev rule installed?"
            );
            return;
        }
    };

    match device.enumerate_dev_nodes_blocking() {
        Ok(nodes) => {
            for node in nodes.flatten() {
                tracing::info!(
                    node = %node.display(),
                    "haptic redirect: virtual gamepad ready"
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "haptic redirect: virtual gamepad created but dev-node enumeration failed"
            );
        }
    }

    // Hand the device + proxied controller to the FF servicing
    // loop. It returns — and the kernel uinput device is destroyed,
    // the proxied controller ungrabbed — once `shutdown` resolves,
    // which happens when the GamepadHapticsService is dropped.
    ff_protocol::serve(device, real, config, haptics, shutdown).await;

    tracing::info!("haptic redirect: virtual gamepad torn down");
}
