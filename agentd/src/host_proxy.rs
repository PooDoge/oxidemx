#![forbid(unsafe_code)]
//! Production [`HostCapability`] that proxies calls to the overlay's
//! `org.oxidemx.AgentHost` D-Bus interface.
//!
//! [`HostCapabilityProxy`] creates a fresh session-bus connection on each
//! invocation (calls are infrequent). When the overlay name is not on the bus
//! it returns [`AgentdError::NotFound`] so callers can degrade gracefully
//! rather than receive an opaque D-Bus error.

use async_trait::async_trait;
use serde_json::Value;
use zbus::proxy;

use crate::error::AgentdError;
use crate::seams::HostCapability;

// ── D-Bus proxy ───────────────────────────────────────────────────────────────

#[proxy(
    interface = "org.oxidemx.AgentHost",
    default_service = "org.oxidemx.overlay",
    default_path = "/org/oxidemx/AgentHost"
)]
trait AgentHost {
    async fn screenshot(&self, args_json: String) -> zbus::Result<String>;
    async fn vision(&self, args_json: String) -> zbus::Result<String>;
    async fn clipboard(&self, args_json: String) -> zbus::Result<String>;
    async fn current_window(&self, args_json: String) -> zbus::Result<String>;
    async fn ask_multiple_choice_question(&self, args_json: String) -> zbus::Result<String>;
    async fn apply_menu_config(&self, args_json: String) -> zbus::Result<String>;
}

// ── HostCapabilityProxy ───────────────────────────────────────────────────────

/// Production [`HostCapability`] backed by `org.oxidemx.AgentHost`.
pub struct HostCapabilityProxy;

/// Convert a zbus `Result<String>` into a `Result<Value, AgentdError>`.
///
/// On `ServiceUnknown` / `NameHasNoOwner` (overlay not running) → `NotFound`.
/// Any other D-Bus error → `AgentdError::Io`.
/// On success the JSON response from the overlay is parsed back into a
/// `serde_json::Value`.
fn map_zbus(result: zbus::Result<String>) -> Result<Value, AgentdError> {
    match result {
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| AgentdError::Io(format!("overlay returned invalid JSON: {e}"))),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("ServiceUnknown")
                || msg.contains("NameHasNoOwner")
                || msg.contains("org.freedesktop.DBus.Error.ServiceUnknown")
                || msg.contains("org.freedesktop.DBus.Error.NameHasNoOwner")
            {
                Err(AgentdError::NotFound("host unavailable".into()))
            } else {
                Err(AgentdError::Io(msg))
            }
        }
    }
}

#[async_trait]
impl HostCapability for HostCapabilityProxy {
    async fn invoke(&self, cap: &str, args: Value) -> Result<Value, AgentdError> {
        // Serialize args to JSON for the D-Bus call.
        let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());

        // Build a fresh session-bus connection each call (calls are infrequent).
        let conn = zbus::connection::Builder::session()
            .map_err(|e| AgentdError::Dbus(e.to_string()))?
            .build()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("ServiceUnknown") || msg.contains("NameHasNoOwner") {
                    AgentdError::NotFound("host unavailable".into())
                } else {
                    AgentdError::Dbus(msg)
                }
            })?;

        let proxy = AgentHostProxy::new(&conn).await.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("ServiceUnknown") || msg.contains("NameHasNoOwner") {
                AgentdError::NotFound("host unavailable".into())
            } else {
                AgentdError::Dbus(msg)
            }
        })?;

        let result = match cap {
            "screenshot" => proxy.screenshot(args_json).await,
            "vision" => proxy.vision(args_json).await,
            "clipboard" => proxy.clipboard(args_json).await,
            "current_window" => proxy.current_window(args_json).await,
            "ask_multiple_choice_question" => {
                proxy.ask_multiple_choice_question(args_json).await
            }
            "apply_menu_config" => proxy.apply_menu_config(args_json).await,
            other => {
                return Err(AgentdError::NotFound(format!(
                    "unknown host cap: {other}"
                )))
            }
        };

        map_zbus(result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Integration test: verifies `HostCapabilityProxy` returns "host unavailable"
    /// when no overlay owns `org.oxidemx.overlay` on the session bus.
    ///
    /// Requires a live D-Bus session bus. Mark `#[ignore]` for CI (run with
    /// `cargo test -p agentd -- --ignored host_proxy_returns_unavailable`).
    #[tokio::test]
    #[ignore]
    async fn host_proxy_returns_unavailable_when_no_overlay() {
        let proxy = HostCapabilityProxy;
        let result = proxy
            .invoke("screenshot", serde_json::json!({}))
            .await;
        match result {
            Err(AgentdError::NotFound(msg)) => {
                assert!(
                    msg.contains("unavailable") || msg.contains("host"),
                    "expected 'unavailable' or 'host' in error: {msg}"
                );
            }
            other => panic!("expected NotFound(host unavailable), got {other:?}"),
        }
    }
}
