//! Semantic recall for the memory store: query↔memory cosine
//! similarity over Gemini embeddings, blended into the lexical
//! ranking by `memory::injection_block_for_async`.
//!
//! Embeddings are cached in a sidecar `memory-embeddings.json`
//! (keyed by the memory TEXT, so identical texts dedup and a changed
//! text auto-invalidates), so each distinct fact is embedded once.
//! The whole path is best-effort: any failure (no key, offline,
//! timeout) leaves the caller on the existing lexical ranking.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

/// Don't embed an unbounded store on a cold cache. Desktop stores are
/// small; this only bounds a pathological case. Most-recent entries
/// win (the caller passes texts newest-last is not guaranteed, so we
/// just cap the slice length).
const MAX_EMBED_PER_RECALL: usize = 256;

/// Cosine similarity. 0 for a zero vector or a length mismatch (a
/// stale cache entry from a different dimensionality).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

fn cache_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    Path::new(&home).join(".local/share/oxidemx/memory-embeddings.json")
}

fn load_cache(path: &Path) -> HashMap<String, Vec<f32>> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cache(path: &Path, cache: &HashMap<String, Vec<f32>>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, json);
    }
}

/// Ensure every `text` has a cached embedding, embedding the misses
/// via the provider seam. Prunes the cache to the live texts so it
/// can't grow without bound. Returns the text→vector map for the
/// requested set (missing entries that failed to embed are simply
/// absent — the caller treats them as lexical-only).
async fn ensure_embeddings(texts: &[String]) -> HashMap<String, Vec<f32>> {
    let path = cache_path();
    let mut cache = load_cache(&path);

    let missing: Vec<String> = texts
        .iter()
        .filter(|t| !cache.contains_key(*t))
        .take(MAX_EMBED_PER_RECALL)
        .cloned()
        .collect();

    if !missing.is_empty() {
        if let Ok(key) = crate::ai_client::load_api_key() {
            let client = reqwest::Client::new();
            let cancel = CancellationToken::new();
            if let Ok(vecs) = oxidemx_agent::embed_texts(&client, &key, &missing, &cancel).await {
                for (t, v) in missing.iter().zip(vecs) {
                    cache.insert(t.clone(), v);
                }
            }
        }
    }

    // Prune to live texts + persist (keeps the sidecar small as
    // memories are consolidated/deleted).
    let live: std::collections::HashSet<&String> = texts.iter().collect();
    cache.retain(|k, _| live.contains(k));
    save_cache(&path, &cache);

    cache
}

/// Cosine of `query` against each `(id, text)`. `None` when the query
/// itself can't be embedded (no key / offline) — the signal to fall
/// back to pure lexical recall. Entries whose embedding is missing
/// are omitted (scored lexical-only upstream).
pub async fn semantic_scores(
    query: &str,
    entries: &[(String, String)],
) -> Option<HashMap<String, f32>> {
    let key = crate::ai_client::load_api_key().ok()?;
    let client = reqwest::Client::new();
    let cancel = CancellationToken::new();
    let qv = oxidemx_agent::embed_texts(&client, &key, &[query.to_string()], &cancel)
        .await
        .ok()?
        .into_iter()
        .next()?;

    let texts: Vec<String> = entries.iter().map(|(_, t)| t.clone()).collect();
    let emb = ensure_embeddings(&texts).await;

    let mut scores = HashMap::new();
    for (id, text) in entries {
        if let Some(v) = emb.get(text) {
            scores.insert(id.clone(), cosine(&qv, v));
        }
    }
    Some(scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        // mismatched length / empty → 0, not a panic
        assert_eq!(cosine(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }

    #[test]
    fn cache_round_trips() {
        let dir = std::env::temp_dir().join(format!("oxmx-emb-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("cache.json");
        let mut c = HashMap::new();
        c.insert("hello".to_string(), vec![0.1, 0.2, 0.3]);
        save_cache(&p, &c);
        let back = load_cache(&p);
        assert_eq!(back.get("hello"), Some(&vec![0.1, 0.2, 0.3]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
