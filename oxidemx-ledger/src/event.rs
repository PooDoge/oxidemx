//! Append-only event log for task execution.
//!
//! Events are serialized to `events.jsonl` (one JSON object per line)
//! and serve as the ground truth for step transitions.

use serde::{Deserialize, Serialize};

/// An event in the task execution ledger.
///
/// Events are serialized to JSON with a `kind` discriminant for identification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum LedgerEvent {
    /// Task was created.
    #[serde(rename = "TaskCreated")]
    TaskCreated {
        /// Task ID.
        task_id: String,
        /// Goal description.
        goal: String,
        /// Timestamp.
        ts: u64,
    },

    /// Step started execution.
    #[serde(rename = "StepStarted")]
    StepStarted {
        /// Step ID.
        step: String,
        /// Timestamp.
        ts: u64,
    },

    /// Step completed successfully.
    #[serde(rename = "StepDone")]
    StepDone {
        /// Step ID.
        step: String,
        /// Verifier token proving completion.
        token: String,
        /// Timestamp.
        ts: u64,
    },

    /// Step failed.
    #[serde(rename = "StepFailed")]
    StepFailed {
        /// Step ID.
        step: String,
        /// Error message.
        error: String,
        /// Timestamp.
        ts: u64,
    },

    /// Step was blocked waiting for a dependency.
    #[serde(rename = "StepBlocked")]
    StepBlocked {
        /// Step ID.
        step: String,
        /// Reason for blocking.
        reason: String,
        /// Timestamp.
        ts: u64,
    },

    /// Step was skipped.
    #[serde(rename = "StepSkipped")]
    StepSkipped {
        /// Step ID.
        step: String,
        /// Reason for skipping.
        reason: String,
        /// Timestamp.
        ts: u64,
    },

    /// Tool call during step execution.
    #[serde(rename = "ToolCall")]
    ToolCall {
        /// Step ID.
        step: String,
        /// Tool name.
        name: String,
        /// Whether the call succeeded.
        ok: bool,
        /// Timestamp.
        ts: u64,
    },

    /// Annotation or note.
    #[serde(rename = "Note")]
    Note {
        /// Optional step ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<String>,
        /// Note text.
        text: String,
        /// Timestamp.
        ts: u64,
    },
}
