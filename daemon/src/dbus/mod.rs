//! D-Bus IPC server for OxideMX
//!
//! Implements the org.oxidemx.Daemon interface for communication
//! with the overlay, settings UI, and Plasma widget.
//!
//! ## Module Layout
//!
//! - `service` - OxideMXService struct and constructors
//! - `interface` - #[interface] impl with all D-Bus methods/signals/properties
//! - `init` - Service initialization and bus registration

mod init;
mod interface;
mod service;

/// D-Bus interface name
pub const DBUS_INTERFACE: &str = "org.oxidemx.Daemon";

/// D-Bus object path
pub const DBUS_PATH: &str = "/org/oxidemx/Daemon";

/// D-Bus bus name
pub const DBUS_NAME: &str = "org.oxidemx.Daemon";

// Re-export public API
pub use init::{init_dbus_service, init_dbus_service_with_device};
pub use service::OxideMXService;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dbus_constants() {
        assert_eq!(DBUS_INTERFACE, "org.oxidemx.Daemon");
        assert_eq!(DBUS_PATH, "/org/oxidemx/Daemon");
        assert_eq!(DBUS_NAME, "org.oxidemx.Daemon");
    }
}
