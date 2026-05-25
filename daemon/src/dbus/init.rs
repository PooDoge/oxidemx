//! D-Bus service initialization

use std::sync::{Arc, Mutex};

use crate::battery::SharedBatteryState;
use crate::config::SharedConfig;
use crate::gaming::SharedGamingMode;
use crate::hidpp::SharedHapticManager;
use crate::macros::{MacroEngine, MacroRecorder, SharedTriggerMap, TriggerMap};
use crate::overlay_spawner::OverlaySpawner;
use crate::thumb_wheel::{new_shared_state as new_thumb_wheel_state, SharedThumbWheelState};

use super::service::JuhRadialService;
use super::{DBUS_NAME, DBUS_PATH};

/// Initialize and run the D-Bus service
///
/// Connects to the session bus, registers the service name, and exports
/// the interface at the specified object path.
pub async fn init_dbus_service(
    battery_state: SharedBatteryState,
    config: SharedConfig,
    haptic_manager: SharedHapticManager,
) -> zbus::Result<zbus::Connection> {
    let gaming_mode = crate::gaming::new_shared_gaming_mode(haptic_manager.clone());
    let macro_engine = Arc::new(Mutex::new(MacroEngine::new()));
    let macro_recorder = Arc::new(Mutex::new(MacroRecorder::new()));
    let trigger_map = Arc::new(std::sync::RwLock::new(TriggerMap::default()));
    let overlay_spawner = Arc::new(OverlaySpawner::new());
    init_dbus_service_with_device(
        battery_state,
        config,
        haptic_manager,
        "logitech".to_string(),
        "Unknown".to_string(),
        gaming_mode,
        macro_engine,
        macro_recorder,
        trigger_map,
        overlay_spawner,
        new_thumb_wheel_state(),
    )
    .await
}

/// Initialize and run the D-Bus service with device mode information
#[allow(clippy::too_many_arguments)]
pub async fn init_dbus_service_with_device(
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
) -> zbus::Result<zbus::Connection> {
    let service = JuhRadialService::new_with_device(
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
        thumb_wheel_state,
    );

    let connection = zbus::connection::Builder::session()?
        .name(DBUS_NAME)?
        .serve_at(DBUS_PATH, service)?
        .build()
        .await?;

    tracing::info!(
        name = DBUS_NAME,
        path = DBUS_PATH,
        "D-Bus service registered"
    );

    Ok(connection)
}
