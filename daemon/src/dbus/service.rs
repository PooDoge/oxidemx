//! OxideMX D-Bus service struct and constructors

use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use crate::battery::SharedBatteryState;
use crate::config::SharedConfig;
use crate::gaming::SharedGamingMode;
use crate::hidpp::SharedHapticManager;
use crate::macros::{MacroEngine, MacroRecorder, SharedTriggerMap, TriggerMap};
use crate::overlay_spawner::OverlaySpawner;
use crate::thumb_wheel::{new_shared_state as new_thumb_wheel_state, SharedThumbWheelState};

/// TTL cache for HID++ Easy-Switch queries. Settings polls the
/// daemon every 5s; without caching, each poll re-issues the
/// HOSTS_INFO HID++ chain (one getHostDescriptor + N
/// getHostFriendlyName chunks per host slot). That hammered
/// the device with redundant traffic AND spammed the daemon log
/// every poll on devices that error on optional sub-functions
/// for unpaired slots. Cache invalidates after 30s — host names
/// + count change rarely (only on pair / unpair / host switch).
#[derive(Debug, Clone, Default)]
pub(crate) struct EasySwitchCache {
    pub host_names: Option<(Vec<String>, Instant)>,
    pub info: Option<((u8, u8), Instant)>,
}

pub(crate) const EASY_SWITCH_TTL: std::time::Duration = std::time::Duration::from_secs(30);

/// OxideMX D-Bus service
///
/// Implements the D-Bus interface for IPC between daemon, KWin overlay, and Plasma widget.
pub struct OxideMXService {
    /// Current profile name
    pub(crate) current_profile: String,
    /// Daemon version
    pub(crate) version: String,
    /// Shared battery state
    pub(crate) battery_state: SharedBatteryState,
    /// Shared configuration for hot-reload
    pub(crate) config: SharedConfig,
    /// Shared haptic manager for triggering haptic feedback
    pub(crate) haptic_manager: SharedHapticManager,
    /// Device mode: "logitech" or "generic"
    pub(crate) device_mode: String,
    /// Detected device name (e.g., "MX Master 4" or "SteelSeries Rival 3")
    pub(crate) device_name: String,
    /// Gaming mode state
    pub(crate) gaming_mode: SharedGamingMode,
    /// Macro playback engine
    pub(crate) macro_engine: Arc<Mutex<MacroEngine>>,
    /// Macro event recorder
    pub(crate) macro_recorder: Arc<Mutex<MacroRecorder>>,
    /// Macro trigger map (evdev button code -> macro ID)
    pub(crate) trigger_map: SharedTriggerMap,
    /// Overlay process spawner — backs EnsureOverlayRunning() D-Bus handler.
    pub(crate) overlay_spawner: Arc<OverlaySpawner>,
    /// TTL cache for Easy-Switch HID++ queries — see
    /// `EasySwitchCache`. Wrapped in RwLock so the read-heavy
    /// path (settings poll) doesn't block on a single cache
    /// look-up.
    pub(crate) easy_switch_cache: Arc<RwLock<EasySwitchCache>>,
    /// Shared thumb-wheel state. Owns the uinput forwarder when
    /// horizontal-scroll inversion is active and carries the
    /// HID++ feature index so the hidraw loop can route diverted
    /// notifications back into the forwarder. Cloned into the
    /// hidraw loop at startup; the D-Bus side mutates it when the
    /// user toggles the invert setting.
    pub(crate) thumb_wheel_state: SharedThumbWheelState,
    /// Active popup child process handle for single-instance tracking.
    /// Held (not read) so the child handle isn't dropped and zombied.
    #[allow(dead_code)]
    pub(crate) popup_child: Mutex<Option<std::process::Child>>,
}

impl OxideMXService {
    /// Create a new D-Bus service instance with battery state, config, and haptic manager
    pub fn new(
        battery_state: SharedBatteryState,
        config: SharedConfig,
        haptic_manager: SharedHapticManager,
    ) -> Self {
        let gaming_mode = crate::gaming::new_shared_gaming_mode(haptic_manager.clone());
        Self {
            current_profile: "default".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            battery_state,
            config,
            haptic_manager,
            device_mode: "logitech".to_string(),
            device_name: "Unknown".to_string(),
            gaming_mode,
            macro_engine: Arc::new(Mutex::new(MacroEngine::new())),
            macro_recorder: Arc::new(Mutex::new(MacroRecorder::new())),
            trigger_map: Arc::new(std::sync::RwLock::new(TriggerMap::default())),
            overlay_spawner: Arc::new(OverlaySpawner::new()),
            easy_switch_cache: Arc::new(RwLock::new(EasySwitchCache::default())),
            thumb_wheel_state: new_thumb_wheel_state(),
            popup_child: Mutex::new(None),
        }
    }

    /// Create a new D-Bus service instance with device mode info.
    ///
    /// `thumb_wheel_state` must be the same `Arc` clone handed to the
    /// hidraw read loop, so the D-Bus side (which manages activation)
    /// and the read side (which dispatches notifications) see the
    /// same forwarder + feature index.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_device(
        battery_state: SharedBatteryState,
        config: SharedConfig,
        haptic_manager: SharedHapticManager,
        device_mode: String,
        device_name: String,
        gaming_mode: SharedGamingMode,
        macro_engine: Arc<Mutex<MacroEngine>>,
        macro_recorder: Arc<Mutex<MacroRecorder>>,
        trigger_map: SharedTriggerMap,
        overlay_spawner: Arc<OverlaySpawner>,
        thumb_wheel_state: SharedThumbWheelState,
    ) -> Self {
        Self {
            current_profile: "default".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            battery_state,
            config,
            haptic_manager,
            device_mode,
            device_name,
            gaming_mode,
            macro_engine,
            macro_recorder,
            trigger_map,
            overlay_spawner,
            easy_switch_cache: Arc::new(RwLock::new(EasySwitchCache::default())),
            thumb_wheel_state,
            popup_child: Mutex::new(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battery::new_shared_state;
    use crate::config::new_shared_config;
    use crate::hidpp::new_shared_haptic_manager;
    use std::sync::Arc;

    #[test]
    fn test_service_creation() {
        let battery_state = new_shared_state();
        let config = new_shared_config();
        let haptic_config = config.read().unwrap().haptics.clone();
        let haptic_manager = new_shared_haptic_manager(&haptic_config);
        let service = OxideMXService::new(battery_state, config, haptic_manager);
        assert_eq!(service.current_profile, "default");
        assert_eq!(service.device_mode, "logitech");
        assert_eq!(service.device_name, "Unknown");
        let haptics = service.config.read().unwrap().haptics.enabled;
        assert!(haptics);
        assert!(!service.version.is_empty());
    }

    #[test]
    fn test_service_creation_with_device() {
        let battery_state = new_shared_state();
        let config = new_shared_config();
        let haptic_config = config.read().unwrap().haptics.clone();
        let haptic_manager = new_shared_haptic_manager(&haptic_config);
        let gaming_mode = crate::gaming::new_shared_gaming_mode(haptic_manager.clone());
        let macro_engine = Arc::new(Mutex::new(MacroEngine::new()));
        let macro_recorder = Arc::new(Mutex::new(MacroRecorder::new()));
        let trigger_map = Arc::new(std::sync::RwLock::new(TriggerMap::default()));
        let overlay_spawner = Arc::new(crate::overlay_spawner::OverlaySpawner::new());
        let service = OxideMXService::new_with_device(
            battery_state,
            config,
            haptic_manager,
            "generic".to_string(),
            "SteelSeries Rival 3".to_string(),
            gaming_mode,
            macro_engine,
            macro_recorder,
            trigger_map,
            overlay_spawner,
            new_thumb_wheel_state(),
        );
        assert_eq!(service.device_mode, "generic");
        assert_eq!(service.device_name, "SteelSeries Rival 3");
    }
}
