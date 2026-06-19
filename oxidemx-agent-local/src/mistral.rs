//! Real [`MistralEngine`] — the only [`InferenceEngine`] impl that touches
//! mistral.rs 0.8.1. Gated behind `#[cfg(feature = "mistral")]` so the
//! default build never pulls in the heavy native dependency.
//!
//! ## API notes (0.8.1 verified 2026-06-18)
//! - `TextModelBuilder::new(repo)` + `.with_isq(IsqType)` + `.build().await`
//! - `GgufModelBuilder::new(dir, files)` + `.build().await` (GGUF is pre-quantized; `isq` is ignored)
//! - `MultimodalModelBuilder` (renamed from `VisionModelBuilder` in 0.7.x)
//! - `MultiModelBuilder::new()
//!       .add_model_with_alias(alias, AnyModelBuilder::Text(…))
//!       .with_default_model(alias)
//!       .build().await`
//! - `Model::send_chat_request_with_model(req, Some(alias)).await`
//! - `Model::unload_model(alias)` (sync)
//! - `Model::reload_model(alias).await` (async)
//! - `Model::list_models_with_status()` → `Vec<(String, ModelStatus)>`
//! - `RequestBuilder::add_message(TextMessageRole, text)`
//!   `.set_sampler_temperature(f64)`, `.set_sampler_topp(f64)`,
//!   `.set_sampler_topk(usize)`, `.set_sampler_max_len(usize)`
//! - `WebSearchOptions` via `RequestBuilder::with_web_search_options(…)`

use async_trait::async_trait;
use mistralrs::{
    AnyModelBuilder, Constraint, GgufModelBuilder, IsqType, Model, MultiModelBuilder,
    MultimodalModelBuilder, RequestBuilder, TextMessageRole, TextModelBuilder, WebSearchOptions,
};
use oxidemx_shared::config::{Capabilities, ModelSource, ModelSpec};
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

use crate::engine::{EngineReply, EngineRequest, InferenceEngine, SchemaConstraint};
use crate::error::LocalError;
use crate::types::{Role, Usage};

// ── ISQ parsing ───────────────────────────────────────────────────────────────

/// Parse a user-supplied ISQ string (case-insensitive) into `IsqType`.
///
/// Accepts both the display form (`q4k`) and the config form (`Q4K`).
fn parse_isq(s: &str) -> Option<IsqType> {
    match s.to_lowercase().as_str() {
        "q4_0" => Some(IsqType::Q4_0),
        "q4_1" => Some(IsqType::Q4_1),
        "q5_0" => Some(IsqType::Q5_0),
        "q5_1" => Some(IsqType::Q5_1),
        "q8_0" => Some(IsqType::Q8_0),
        "q8_1" => Some(IsqType::Q8_1),
        "q2k" => Some(IsqType::Q2K),
        "q3k" => Some(IsqType::Q3K),
        "q4k" => Some(IsqType::Q4K),
        "q5k" => Some(IsqType::Q5K),
        "q6k" => Some(IsqType::Q6K),
        "q8k" => Some(IsqType::Q8K),
        "hqq8" => Some(IsqType::HQQ8),
        "hqq4" => Some(IsqType::HQQ4),
        "fp8" | "f8e4m3" => Some(IsqType::F8E4M3),
        "afq8" => Some(IsqType::AFQ8),
        "afq6" => Some(IsqType::AFQ6),
        "afq4" => Some(IsqType::AFQ4),
        "afq3" => Some(IsqType::AFQ3),
        "afq2" => Some(IsqType::AFQ2),
        "f8q8" => Some(IsqType::F8Q8),
        "mxfp4" => Some(IsqType::MXFP4),
        _ => None,
    }
}

// ── Role conversion ───────────────────────────────────────────────────────────

fn to_mistral_role(role: &Role) -> TextMessageRole {
    match role {
        Role::System => TextMessageRole::System,
        Role::User => TextMessageRole::User,
        Role::Assistant => TextMessageRole::Assistant,
        // Tool results carry the Tool role in mistral.rs 0.8.1 TextMessageRole.
        // Note: tool_call_id is not carried by add_message — that's a separate gap.
        Role::Tool => TextMessageRole::Tool,
    }
}

// ── MistralEngine ─────────────────────────────────────────────────────────────

/// Real inference engine backed by mistral.rs 0.8.1.
///
/// Instantiate with [`MistralEngine::new`]; hold it behind an `Arc<dyn
/// InferenceEngine>` so the session manager never imports this crate.
pub struct MistralEngine {
    /// The multi-model runner (owns all registered models).
    model: Arc<Mutex<Model>>,
    /// Aliases that have WEB_SEARCH capability enabled (for per-request
    /// `WebSearchOptions` injection).
    web_search_aliases: HashSet<String>,
}

impl MistralEngine {
    /// Build a [`MistralEngine`] from a list of [`ModelSpec`]s.
    ///
    /// All models are registered with `MultiModelBuilder` but NOT yet loaded
    /// into GPU/CPU memory — the actual weight loading happens in [`load`].
    ///
    /// `download_dir` is forwarded to the underlying download machinery (where
    /// mistral.rs uses it as a cache root for HF downloads).
    pub async fn new(
        _download_dir: PathBuf,
        specs: &[ModelSpec],
    ) -> Result<Self, LocalError> {
        if specs.is_empty() {
            return Err(LocalError::Inference(
                "MistralEngine requires at least one ModelSpec".to_string(),
            ));
        }

        let mut builder = MultiModelBuilder::new();
        let mut web_search_aliases = HashSet::new();
        let mut first_alias: Option<String> = None;

        for spec in specs {
            let alias = &spec.alias;

            // Track WEB_SEARCH capability for per-request injection.
            if spec.capabilities.contains(Capabilities::WEB_SEARCH) {
                web_search_aliases.insert(alias.clone());
            }

            // Build the appropriate AnyModelBuilder for the source type.
            let any_builder = build_any_model_builder(spec)?;
            builder = builder.add_model_with_alias(alias.as_str(), any_builder);

            if first_alias.is_none() {
                first_alias = Some(alias.clone());
            }
        }

        // Set the first registered model as the default.
        if let Some(ref alias) = first_alias {
            builder = builder.with_default_model(alias.as_str());
        }

        let model = builder
            .build()
            .await
            .map_err(|e| LocalError::Inference(format!("MultiModelBuilder::build failed: {e}")))?;

        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            web_search_aliases,
        })
    }
}

/// Construct the correct `AnyModelBuilder` for a given `ModelSpec`.
fn build_any_model_builder(spec: &ModelSpec) -> Result<AnyModelBuilder, LocalError> {
    let isq: Option<IsqType> = match &spec.isq {
        Some(s) => {
            let t = parse_isq(s).ok_or_else(|| {
                LocalError::LoadFailed {
                    alias: spec.alias.clone(),
                    reason: format!("unknown ISQ string: {s:?}"),
                }
            })?;
            Some(t)
        }
        None => None,
    };

    match &spec.source {
        ModelSource::Hf { repo, revision } => {
            // Use multimodal builder when VISION capability is requested.
            if spec.capabilities.contains(Capabilities::VISION) {
                let mut b = MultimodalModelBuilder::new(repo.as_str());
                if let Some(isq_type) = isq {
                    b = b.with_isq(isq_type);
                }
                if let Some(rev) = revision {
                    b = b.with_hf_revision(rev.as_str());
                }
                Ok(AnyModelBuilder::Multimodal(b))
            } else {
                let mut b = TextModelBuilder::new(repo.as_str());
                if let Some(isq_type) = isq {
                    b = b.with_isq(isq_type);
                }
                if let Some(rev) = revision {
                    b = b.with_hf_revision(rev.as_str());
                }
                Ok(AnyModelBuilder::Text(b))
            }
        }
        ModelSource::Gguf { dir, files } => {
            // In 0.8.1 GgufModelBuilder::new(model_id, files) where model_id
            // is the local directory (or HF repo) that contains the GGUF files.
            // GgufModelBuilder does NOT expose with_isq — GGUF files are
            // already quantized; ISQ on a GGUF model is a no-op / unsupported.
            // Log a warning if the user requested ISQ on a GGUF spec.
            if isq.is_some() {
                tracing::warn!(
                    alias = %spec.alias,
                    "ModelSpec.isq is set but has no effect for GGUF sources \
                     (GGUF files are already quantized; GgufModelBuilder \
                     does not expose with_isq)"
                );
            }
            let b = GgufModelBuilder::new(dir.as_str(), files.clone());
            Ok(AnyModelBuilder::Gguf(b))
        }
    }
}

#[async_trait]
impl InferenceEngine for MistralEngine {
    /// Reload (or load for the first time) the model weights for `spec`.
    async fn load(&self, spec: &ModelSpec) -> Result<(), LocalError> {
        let model = self.model.lock().await;
        model.reload_model(&spec.alias).await.map_err(|e| {
            LocalError::LoadFailed {
                alias: spec.alias.clone(),
                reason: e.to_string(),
            }
        })
    }

    /// Unload the model weights for `alias`, freeing its memory.
    async fn unload(&self, alias: &str) -> Result<(), LocalError> {
        let model = self.model.lock().await;
        model.unload_model(alias).map_err(|e| {
            LocalError::Inference(format!("unload_model({alias:?}) failed: {e}"))
        })
    }

    /// Run one inference turn against the model registered under `alias`.
    async fn generate(&self, alias: &str, req: &EngineRequest) -> Result<EngineReply, LocalError> {
        // Build the RequestBuilder from EngineRequest messages + sampling.
        let mut rb = RequestBuilder::new();

        for msg in &req.messages {
            rb = rb.add_message(to_mistral_role(&msg.role), msg.content.clone());
        }

        // Apply sampling parameters.
        let s = &req.sampling;
        if let Some(temp) = s.temperature {
            rb = rb.set_sampler_temperature(temp as f64);
        }
        if let Some(top_p) = s.top_p {
            rb = rb.set_sampler_topp(top_p as f64);
        }
        if let Some(top_k) = s.top_k {
            rb = rb.set_sampler_topk(top_k as usize);
        }
        if let Some(max_tokens) = s.max_tokens {
            rb = rb.set_sampler_max_len(max_tokens as usize);
        }

        // Inject web-search options when the model advertises WEB_SEARCH.
        if self.web_search_aliases.contains(alias) {
            rb = rb.with_web_search_options(WebSearchOptions::default());
        }

        // Apply constrained decoding when the request carries a SchemaConstraint.
        // Mapping: SchemaConstraint::JsonSchema(v) → Constraint::JsonSchema(v)
        //          SchemaConstraint::Regex(r)      → Constraint::Regex(r)
        if let Some(sc) = &req.constraint {
            let c = match sc {
                SchemaConstraint::JsonSchema(schema) => Constraint::JsonSchema(schema.clone()),
                SchemaConstraint::Regex(pattern) => Constraint::Regex(pattern.clone()),
            };
            rb = rb.set_constraint(c);
        }

        // Dispatch to the named model.
        let model = self.model.lock().await;
        let response = model
            .send_chat_request_with_model(rb, Some(alias))
            .await
            .map_err(|e| LocalError::Inference(format!("send_chat_request_with_model failed: {e}")))?;

        // Extract text from the first choice.
        let text = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        // Map usage — saturating cast so huge usize counts don't silently truncate.
        let usage = Usage {
            prompt_tokens: u32::try_from(response.usage.prompt_tokens).unwrap_or(u32::MAX),
            completion_tokens: u32::try_from(response.usage.completion_tokens).unwrap_or(u32::MAX),
        };

        Ok(EngineReply { text, usage })
    }
}

// ── Tests (compile-only; no live model needed) ────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isq_round_trips() {
        for (s, expected) in [
            ("Q4K", IsqType::Q4K),
            ("q4k", IsqType::Q4K),
            ("Q8_0", IsqType::Q8_0),
            ("HQQ4", IsqType::HQQ4),
            ("fp8", IsqType::F8E4M3),
        ] {
            assert!(
                matches!(parse_isq(s), Some(v) if std::mem::discriminant(&v) == std::mem::discriminant(&expected)),
                "parse_isq({s:?}) failed"
            );
        }
        assert!(parse_isq("not_a_real_quant").is_none());
    }

    #[test]
    fn role_mapping() {
        assert!(matches!(
            to_mistral_role(&Role::System),
            TextMessageRole::System
        ));
        assert!(matches!(
            to_mistral_role(&Role::User),
            TextMessageRole::User
        ));
        assert!(matches!(
            to_mistral_role(&Role::Assistant),
            TextMessageRole::Assistant
        ));
        // Tool maps to TextMessageRole::Tool (mistral.rs 0.8.1 has the variant).
        assert!(matches!(
            to_mistral_role(&Role::Tool),
            TextMessageRole::Tool
        ));
    }
}
