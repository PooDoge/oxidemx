//! Backends for the chat agent's local "do things on this machine"
//! tools. Each submodule is the pure capability — running an
//! allowlisted shell command, managing systemd user timers, keeping
//! a small persistent memory store — and knows nothing about the
//! Interactions API. The tool-call plumbing (declarations, dispatch,
//! stream events, confirmation chips) lives in `ai_client.rs`, so
//! the UI layer can also call straight into these modules (e.g. a
//! settings page listing scheduled tasks or saved memories) without
//! going through the model.

pub mod commands;
pub mod memory;
pub mod persona;
pub mod tasks;
