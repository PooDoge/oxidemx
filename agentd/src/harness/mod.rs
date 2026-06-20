//! Harness integration for agentd.
//!
//! Provides [`CoreWorker`] — the production [`oxidemx_harness::Worker`]
//! implementation that runs a single step as a gated agent turn via
//! `oxidemx_agent_core::runtime::route_turn`.
#![forbid(unsafe_code)]

pub mod worker;
