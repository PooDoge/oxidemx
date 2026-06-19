#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Persistence foundation for the oxidemx autonomous coding harness.
//!
//! This crate provides the task and step model, with stable IDs and
//! deterministic serialization suitable for ledger-based workflow tracking.

pub mod error;
pub mod event;
pub mod model;
pub mod store;

pub use error::LedgerError;
pub use event::LedgerEvent;
pub use model::{CompletionPromise, Step, StepStatus, TaskId, TaskManifest};
pub use store::TaskLedger;
