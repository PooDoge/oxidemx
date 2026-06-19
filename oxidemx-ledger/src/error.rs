//! Error types for the ledger persistence layer.

use thiserror::Error;

/// Errors that can occur during ledger operations.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum LedgerError {
    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(String),

    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid status transition.
    #[error("Invalid status transition from {from:?} to {to:?}")]
    BadTransition {
        /// Previous step status.
        from: crate::model::StepStatus,
        /// Attempted new status.
        to: crate::model::StepStatus,
    },

    /// Missing completion promise for a step.
    #[error("Missing promise: {0}")]
    MissingPromise(String),

    /// Data corruption or integrity violation.
    #[error("Corrupt data: {0}")]
    Corrupt(String),
}

impl From<std::io::Error> for LedgerError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_json::Error> for LedgerError {
    fn from(e: serde_json::Error) -> Self {
        Self::Corrupt(e.to_string())
    }
}
