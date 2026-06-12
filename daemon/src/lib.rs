//! OxideMX Daemon Library
//!
//! Public API for testing and integration.

pub mod accessibility;
pub mod actions;
pub mod battery;
pub mod bundled_themes;
pub mod config;
pub mod cursor;
pub mod dbus;
pub mod evdev;
pub mod gamepad_haptics;
pub mod gaming;
pub mod hidpp;
pub mod hidraw;
pub mod macros;
pub mod overlay_spawner;
pub mod performance_monitor;
pub mod profiles;
pub mod theme;
pub mod theme_watcher;
pub mod thumb_wheel;
pub mod window_tracker;

/// Re-export commonly used types
pub use accessibility::{AccessibilitySettings, EffectiveAnimationTimings};
pub use actions::{Action, ActionType};
pub use battery::{
    new_shared_state as new_battery_state, start_battery_updater_shared,
    start_battery_updater_shared_with_dbus, BatteryState, SharedBatteryState,
};
pub use bundled_themes::{
    get_bundled_theme, get_default_theme, list_bundled_themes, DEFAULT_THEME_NAME,
};
pub use config::{load_shared_config, new_shared_config, Config, SharedConfig};
pub use cursor::{
    get_cursor_position, get_screen_bounds, CursorPosition, ScreenBounds, EDGE_MARGIN,
    MENU_DIAMETER, MENU_RADIUS,
};
pub use dbus::{
    init_dbus_service, init_dbus_service_with_device, OxideMXService, DBUS_INTERFACE, DBUS_NAME,
    DBUS_PATH,
};
pub use evdev::{
    DeviceInfo, EvdevError, EvdevHandler, GestureEvent, GENERIC_TRIGGER_BUTTON, LOGITECH_VENDOR_ID,
};
pub use gamepad_haptics::GamepadHapticsService;
pub use gaming::{new_shared_gaming_mode, GamingMode, SharedGamingMode};
pub use hidpp::{new_shared_haptic_manager, HapticEvent, HapticManager, SharedHapticManager};
pub use macros::{MacroEngine, MacroRecorder, SharedTriggerMap, TriggerMap};
pub use performance_monitor::{BlurMode, PerformanceMonitor};
pub use profiles::{Profile, ProfileManager};
pub use theme::{Theme, ThemeManager};
pub use theme_watcher::{ThemeEvent, ThemeHotReloader, ThemeWatcher};
pub use thumb_wheel::{
    new_shared_state as new_thumb_wheel_state, SharedThumbWheelState, ThumbWheelForwarder,
    ThumbWheelState,
};
pub use window_tracker::{WindowInfo, WindowTracker};
