//! Error types for the planner crate.

use thiserror::Error;

/// Errors that can occur during planning.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum PlannerError {
    /// The underlying model returned an error.
    #[error("Model error: {0}")]
    Model(String),

    /// The model's response was not valid JSON or violated the plan schema.
    #[error("Schema invalid: {0}")]
    SchemaInvalid(String),

    /// The plan produced a graph that failed DAG validation (cycle, orphan, dup).
    #[error("Graph invalid: {0}")]
    GraphInvalid(String),

    /// All local retries and the escalation call were exhausted.
    #[error("Planning unresolved after all retries. Reasons: {}", reasons.join("; "))]
    Unresolved {
        /// Accumulated reasons (one per failed attempt).
        reasons: Vec<String>,
    },
}
