//! Connector layer: capability-scoped transports over the connector-agnostic core.
#![forbid(unsafe_code)]

pub mod auth;
pub mod caps;
pub mod event_hub;
pub mod http;
pub mod tailnet;
