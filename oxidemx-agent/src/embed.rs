//! Gemini text embeddings (`gemini-embedding-001`).
//!
//! The provider seam for semantic memory: the chat transport
//! (Interactions) has no embedding endpoint, so embeddings ride the
//! `:embedContent` API on the same key.
//!
//! Model note (verified live 2026-06-13): the classic
//! `text-embedding-004` / `embedding-001` names 404 on this key — the
//! available models are `gemini-embedding-001` (stable) and
//! `gemini-embedding-2*`, consistent with this project's post-cutoff
//! Interactions API surface. `gemini-embedding-001` defaults to
//! 3072-dim; we request `outputDimensionality: 768` to keep the cache
//! compact. Wire shape: request `{model, content:{parts:[{text}]},
//! outputDimensionality}`, response `{embedding:{values:[f32; 768]}}`.

use autoagents::llm::error::LLMError;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Embedding model + the output dimensionality we request.
pub const EMBED_MODEL: &str = "gemini-embedding-001";
pub const EMBED_DIM: usize = 768;

const EMBED_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent";

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    content: EmbedContent<'a>,
    #[serde(rename = "outputDimensionality")]
    output_dimensionality: usize,
}

#[derive(Serialize)]
struct EmbedContent<'a> {
    parts: Vec<EmbedPart<'a>>,
}

#[derive(Serialize)]
struct EmbedPart<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: EmbedValues,
}

#[derive(Deserialize)]
struct EmbedValues {
    values: Vec<f32>,
}

/// Embed each text, in order. The API takes one document per call, so
/// this loops; callers should batch only what they need (memory
/// recall pre-filters to a bounded candidate set). Honors `cancel`.
/// Empty input short-circuits to an empty vec.
pub async fn embed_texts(
    client: &reqwest::Client,
    api_key: &str,
    texts: &[String],
    cancel: &CancellationToken,
) -> Result<Vec<Vec<f32>>, LLMError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(texts.len());
    for text in texts {
        let body = EmbedRequest {
            model: "models/gemini-embedding-001",
            content: EmbedContent {
                parts: vec![EmbedPart { text }],
            },
            output_dimensionality: EMBED_DIM,
        };
        let request = client
            .post(EMBED_URL)
            .header("x-goog-api-key", api_key)
            .json(&body)
            .send();
        let res = tokio::select! {
            r = request => r.map_err(|e| LLMError::HttpError(e.to_string()))?,
            _ = cancel.cancelled() => return Err(LLMError::Generic("cancelled".into())),
        };
        let status = res.status();
        if !status.is_success() {
            let detail = res.text().await.unwrap_or_default();
            return Err(LLMError::ProviderError(format!(
                "embed error ({status}): {detail}"
            )));
        }
        let parsed: EmbedResponse = res.json().await.map_err(|e| LLMError::ResponseFormatError {
            message: e.to_string(),
            raw_response: String::new(),
        })?;
        out.push(parsed.embedding.values);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_input_short_circuits() {
        let client = reqwest::Client::new();
        let out = embed_texts(&client, "k", &[], &CancellationToken::new())
            .await
            .unwrap();
        assert!(out.is_empty());
    }

    // Live check: run with a real key to confirm dimensionality and
    // that semantically related text scores higher than unrelated.
    //   GEMINI_API_KEY=… cargo test -p oxidemx-agent embed_live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn embed_live_semantic_ordering() {
        let key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY");
        let client = reqwest::Client::new();
        let texts: Vec<String> = ["I prefer a dark color theme", "switch to night mode", "the price of tea in China"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let v = embed_texts(&client, &key, &texts, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].len(), EMBED_DIM);
        let cos = |a: &[f32], b: &[f32]| {
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            dot / (na * nb)
        };
        let related = cos(&v[0], &v[1]);
        let unrelated = cos(&v[0], &v[2]);
        println!("related={related:.3} unrelated={unrelated:.3}");
        assert!(related > unrelated, "related {related} !> unrelated {unrelated}");
    }
}
