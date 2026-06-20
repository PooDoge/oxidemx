#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Verification harness for the oxidemx autonomous coding system.
//!
//! Provides:
//! - Verification commands (`cargo check`, `cargo test`) that turn a PASS into
//!   a [`CompletionPromise`] or a FAIL into a critique string.
//! - The [`Worker`] trait seam for step execution.
//! - The [`Executor`] that drives a [`TaskManifest`] to completion.

pub mod edge;
pub mod error;
pub mod executor;
pub mod verify;
pub mod worker;
#[cfg(feature = "process")]
pub mod run;

pub use edge::validate_edge;
pub use error::HarnessError;
pub use executor::{Caps, Executor, RunReport};
pub use verify::{CommandResult, CommandRunner, Verifier, VerifyFailure};
pub use worker::{StepOutput, ToolInvocation, Worker, WorkerBrief};

// Re-export approval types so callers don't need to depend on oxidemx-approval directly.
pub use oxidemx_approval::{ApprovalClassifier, ClassifierConfig, Decision, Tier};

// Re-export CompletionPromise for callers that build trivial promises.
pub use oxidemx_ledger::CompletionPromise;
