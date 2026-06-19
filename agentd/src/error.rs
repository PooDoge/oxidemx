//! Crate-wide error type for agentd.

/// Top-level error type for agentd operations.
///
/// `#[non_exhaustive]` lets downstream crates match on known variants
/// while allowing new variants to be added without a breaking change.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AgentdError {
    /// Wraps an I/O failure; the inner `String` is the `Display` of the
    /// original `std::io::Error` so `AgentdError` stays `Send + Sync`.
    #[error("I/O error: {0}")]
    Io(String),

    /// A required resource (file, directory, key) was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// A project-model invariant was violated (e.g. bad key format).
    #[error("project error: {0}")]
    Project(String),

    /// A D-Bus operation failed.
    #[error("D-Bus error: {0}")]
    Dbus(String),
}

impl From<std::io::Error> for AgentdError {
    fn from(e: std::io::Error) -> Self {
        AgentdError::Io(e.to_string())
    }
}
