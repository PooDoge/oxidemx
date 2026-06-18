//! [`LocalError`] — the single error type for the local-LLM service.

use oxidemx_shared::config::Capabilities;
use thiserror::Error;

/// All failure modes of the local-LLM service.
///
/// `#[non_exhaustive]` lets us add variants in patch releases without
/// breaking downstream match arms.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LocalError {
    /// No registered model with the given alias.
    #[error("model not found: {0}")]
    ModelNotFound(String),

    /// The requested [`crate::mode::Mode`] requires capabilities the chosen
    /// model does not advertise.
    #[error("capability unmet: needs {needs:?}, model has {have:?}")]
    CapabilityUnmet {
        /// Capabilities required by the requested mode.
        needs: Capabilities,
        /// Capabilities the selected model advertises.
        have: Capabilities,
    },

    /// Weights found but the backend could not initialise the model.
    #[error("failed to load model '{alias}': {reason}")]
    LoadFailed {
        /// Short human-readable alias of the model that failed to load.
        alias: String,
        /// Underlying error description from the backend.
        reason: String,
    },

    /// The inference call itself returned an error.
    #[error("inference error: {0}")]
    Inference(String),

    /// The [`crate::guard`] module rejected the model output.
    #[error("guard rejected response: {reasons:?}")]
    GuardRejected {
        /// Human-readable descriptions of each check that failed.
        reasons: Vec<String>,
    },
}
