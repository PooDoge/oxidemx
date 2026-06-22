//! Capability contract: every transport declares what it may do; the HTTP edge enforces it.
#![forbid(unsafe_code)]

/// Ordered capability tier. `Control` implies `Messaging`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeTier {
    /// Conversation/messaging surface: safe to expose remotely.
    Messaging,
    /// Messaging + the high-security control plane (AI settings, skill/flow
    /// management + generation, conductor/orchestrator control + debug). Same-device only.
    Control,
}

/// What a given connector/connection is allowed to do.
#[derive(Debug, Clone, Copy)]
pub struct ConnectorCaps {
    pub scope: ScopeTier,
    pub can_stream: bool,
}

impl ConnectorCaps {
    /// Local Unix-socket connection: full control plane.
    pub const UDS_LOCAL: ConnectorCaps =
        ConnectorCaps { scope: ScopeTier::Control, can_stream: true };
    /// Remote tailnet connection: messaging only.
    pub const TAILNET_REMOTE: ConnectorCaps =
        ConnectorCaps { scope: ScopeTier::Messaging, can_stream: true };

    /// True if this connection may invoke a route requiring `required`.
    pub fn allows(&self, required: ScopeTier) -> bool {
        self.scope >= required
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_outranks_messaging() {
        assert!(ScopeTier::Control > ScopeTier::Messaging);
    }

    #[test]
    fn messaging_caps_deny_control_routes() {
        let caps = ConnectorCaps::TAILNET_REMOTE;
        assert_eq!(caps.scope, ScopeTier::Messaging);
        assert!(caps.allows(ScopeTier::Messaging));
        assert!(!caps.allows(ScopeTier::Control));
    }

    #[test]
    fn control_caps_allow_both_tiers() {
        let caps = ConnectorCaps::UDS_LOCAL;
        assert_eq!(caps.scope, ScopeTier::Control);
        assert!(caps.allows(ScopeTier::Messaging));
        assert!(caps.allows(ScopeTier::Control));
    }
}
