#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Verification harness for the oxidemx autonomous coding system.
//!
//! Provides verification commands (e.g., `cargo check`, `cargo test`) and turns
//! a PASS into a `CompletionPromise` (ground-truth token) or a FAIL into a critique
//! string for the reflection loop.

pub mod verify;

pub use verify::{CommandResult, CommandRunner, Verifier, VerifyFailure};
