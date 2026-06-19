#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Schema-gated plan generation for the OxideMX autonomous coding harness.
//!
//! The central type is [`Planner`], which turns a free-form goal string into a
//! validated [`StepGraph`] by:
//!
//! 1. Deriving a JSON Schema for [`PlanOutput`] via `schemars`.
//! 2. Calling a [`PlannerModel`] with that schema as a generate-time constraint.
//! 3. Validating the raw reply against the schema with `jsonschema` at
//!    *validate time*.
//! 4. Mapping the deserialized output to an [`oxidemx_ledger::StepGraph`] and
//!    calling its cycle/orphan/dup validator.
//! 5. Retrying with corrective notes on failure, escalating to the cloud model
//!    after `max_local_retries` exhausted, and returning
//!    [`PlannerError::Unresolved`] if everything fails.

pub mod error;
pub mod model;
pub mod planner;

pub use error::PlannerError;
pub use model::{PlanRequest, PlannerModel};
pub use planner::{PlanOutput, PlanStep, Planner};
