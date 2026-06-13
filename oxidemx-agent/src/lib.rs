//! OxideMX agent runtime on AutoAgents (P0 spike).
//!
//! Spec: docs/plans/agent-framework-integration-brainstorm.md (Part I
//! §3.2–3.3, Part II §12 P0). Plan: docs/superpowers/plans/
//! 2026-06-12-agent-framework-p0.md.

pub mod allowlist;
pub mod factory;
pub mod provider;
pub mod tools;

/// Embed texts with Gemini `text-embedding-004` (the provider seam
/// for semantic memory). Re-exported for callers that need
/// embeddings without building a full agent (e.g. memory recall).
pub use provider::embed::{embed_texts, EMBED_DIM, EMBED_MODEL};
