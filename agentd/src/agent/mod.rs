//! Agent-level integration adapters for agentd.
//!
//! - [`approver_prompt`] — bridges the SP1b [`crate::seams::Approver`] to the
//!   [`crate::tools::gated::ApprovalPrompt`] trait so `GatedToolExecutor` can
//!   surface Attended-mode approvals through the D-Bus host UI.
#![forbid(unsafe_code)]

pub mod approver_prompt;
