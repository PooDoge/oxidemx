#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Verification harness for the oxidemx autonomous coding system.
//!
//! Provides:
//! - Verification commands (`cargo check`, `cargo test`) that turn a PASS into
//!   a [`CompletionPromise`] or a FAIL into a critique string.
//! - The [`Worker`] trait seam for step execution.
//! - The [`Executor`] that drives a [`TaskManifest`] to completion.

pub mod error;
pub mod executor;
pub mod verify;
pub mod worker;

pub use error::HarnessError;
pub use executor::{ApprovalClassifier, Caps, Executor, RunReport};
pub use verify::{CommandResult, CommandRunner, Verifier, VerifyFailure};
pub use worker::{StepOutput, ToolInvocation, Worker, WorkerBrief};

// Re-export CompletionPromise for callers that build trivial promises.
pub use oxidemx_ledger::CompletionPromise;
