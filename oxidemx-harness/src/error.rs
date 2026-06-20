//! Error types for the oxidemx harness.

use thiserror::Error;

/// Errors produced by the harness layer.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum HarnessError {
    /// The worker back-end returned an error.
    #[error("worker error: {0}")]
    Worker(String),

    /// A ledger operation failed.
    #[error("ledger error: {0}")]
    Ledger(String),

    /// Verification command failed.
    #[error("verify error: {0}")]
    Verify(String),
}

impl From<oxidemx_ledger::LedgerError> for HarnessError {
    fn from(e: oxidemx_ledger::LedgerError) -> Self {
        Self::Ledger(e.to_string())
    }
}
