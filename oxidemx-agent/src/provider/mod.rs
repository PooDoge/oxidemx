//! Gemini Interactions provider (P0 spike).
pub mod sse;
pub mod wire;

/// The Interactions endpoint (same as the production overlay client).
pub const INTERACTIONS_URL: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
