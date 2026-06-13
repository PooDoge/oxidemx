//! OxideMX agent runtime on AutoAgents (P0 spike).
//!
//! Spec: docs/plans/agent-framework-integration-brainstorm.md (Part I
//! §3.2–3.3, Part II §12 P0). Plan: docs/superpowers/plans/
//! 2026-06-12-agent-framework-p0.md.

pub mod allowlist;
pub mod claude_code;
pub mod embed;
pub mod factory;
pub mod keys;
pub mod tools;

/// Embed texts with Gemini `gemini-embedding-001` (the embedding seam
/// for semantic memory — independent of the chat provider). Re-
/// exported for callers that need embeddings without building an agent.
pub use embed::{embed_texts, EMBED_DIM, EMBED_MODEL};
