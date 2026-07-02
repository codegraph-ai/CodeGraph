// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Vector embedding engine
//!
//! High-level API for generating and caching embeddings.

use super::fastembed_embed::{CodeGraphEmbeddingModel, FastembedEmbedding};
use super::{Embedder, EmbeddingBackend};
use crate::error::Result;
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Vector embedding engine with caching
///
/// Wraps fastembed with a configurable model and DashMap cache for efficient repeated lookups.
pub struct VectorEngine {
    model: Arc<dyn Embedder>,
    cache: DashMap<String, Vec<f32>>,
    dimension: usize,
}

impl VectorEngine {
    /// Create VectorEngine with the default model (Jina Code V2)
    pub fn new(_extension_path: Option<&Path>) -> Result<Self> {
        Self::with_model(default_cache_dir(), CodeGraphEmbeddingModel::default())
    }

    /// Create VectorEngine with a specific embedding model
    pub fn with_model(cache_dir: PathBuf, model_type: CodeGraphEmbeddingModel) -> Result<Self> {
        let model = FastembedEmbedding::new(cache_dir, model_type)?;
        let dimension = model.dimension();

        log::info!(
            "VectorEngine ready ({}, {}d)",
            model_type.display_name(),
            dimension
        );

        Ok(Self {
            model: Arc::new(model),
            cache: DashMap::new(),
            dimension,
        })
    }

    /// Create a VectorEngine backed by a static (lookup-table) model loaded from
    /// a model2vec-format directory (`config.json` + `tokenizer.json` +
    /// `model.safetensors`). No ONNX — the fast indexing path.
    pub fn with_static_model(model_dir: &Path) -> Result<Self> {
        let model = super::static_embed::StaticEmbedding::from_pretrained(model_dir)?;
        let dimension = model.dimension();
        log::info!(
            "VectorEngine ready ({}, {}d, static)",
            model.model_name(),
            dimension
        );
        Ok(Self {
            model: Arc::new(model),
            cache: DashMap::new(),
            dimension,
        })
    }

    /// Build a VectorEngine from an `EmbeddingBackend` selection — the ONNX
    /// fastembed path or the static model2vec path.
    pub fn from_backend(cache_dir: PathBuf, backend: &EmbeddingBackend) -> Result<Self> {
        match backend {
            EmbeddingBackend::Fastembed(model) => Self::with_model(cache_dir, *model),
            EmbeddingBackend::Static(dir) => Self::with_static_model(dir),
        }
    }

    /// Generate embedding with caching
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Check cache first
        if let Some(cached) = self.cache.get(text) {
            return Ok(cached.clone());
        }

        // Generate and cache
        let embedding = self.model.embed(text)?;
        self.cache.insert(text.to_string(), embedding.clone());
        Ok(embedding)
    }

    /// Batch embed with caching
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        // Check cache for all texts
        let mut results: Vec<Option<Vec<f32>>> = texts
            .iter()
            .map(|text| self.cache.get(*text).map(|v| v.clone()))
            .collect();

        // Find uncached texts
        let uncached: Vec<(usize, &str)> = results
            .iter()
            .enumerate()
            .filter(|(_, cached)| cached.is_none())
            .map(|(i, _)| (i, texts[i]))
            .collect();

        if uncached.is_empty() {
            return Ok(results.into_iter().flatten().collect());
        }

        // Batch embed uncached texts
        let uncached_texts: Vec<&str> = uncached.iter().map(|(_, t)| *t).collect();
        let new_embeddings = self.model.embed_batch(&uncached_texts)?;

        // Update cache and results
        for ((idx, text), emb) in uncached.iter().zip(new_embeddings.into_iter()) {
            self.cache.insert(text.to_string(), emb.clone());
            results[*idx] = Some(emb);
        }

        Ok(results.into_iter().flatten().collect())
    }

    /// Cosine similarity between two embeddings
    pub fn similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    /// Get embedding dimension
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Get model display name (e.g. "Jina Code V2 (768d)")
    pub fn model_name(&self) -> &str {
        self.model.model_name()
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Clear the cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }
}

/// Default cache directory for fastembed models
fn default_cache_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".codegraph")
        .join("fastembed_cache")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// In-memory `Embedder` that fabricates deterministic vectors and counts
    /// how many times each entry point is hit, so the `VectorEngine` caching
    /// logic can be exercised without downloading an ONNX/static model.
    struct MockEmbedder {
        dim: usize,
        embed_calls: AtomicUsize,
        batch_calls: AtomicUsize,
    }

    impl MockEmbedder {
        fn new(dim: usize) -> Self {
            Self {
                dim,
                embed_calls: AtomicUsize::new(0),
                batch_calls: AtomicUsize::new(0),
            }
        }

        /// A vector uniquely determined by the text length: index 0 holds the
        /// length, the rest are zero-padded to `dim`.
        fn vector_for(&self, text: &str) -> Vec<f32> {
            let mut v = vec![0.0_f32; self.dim];
            v[0] = text.len() as f32;
            v
        }
    }

    impl Embedder for MockEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            self.embed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.vector_for(text))
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            self.batch_calls.fetch_add(1, Ordering::SeqCst);
            Ok(texts.iter().map(|t| self.vector_for(t)).collect())
        }

        fn dimension(&self) -> usize {
            self.dim
        }

        fn model_name(&self) -> &str {
            "mock-embedder"
        }
    }

    fn engine_with(dim: usize) -> (VectorEngine, Arc<MockEmbedder>) {
        let mock = Arc::new(MockEmbedder::new(dim));
        let engine = VectorEngine {
            model: mock.clone(),
            cache: DashMap::new(),
            dimension: dim,
        };
        (engine, mock)
    }

    #[test]
    fn embed_caches_after_first_call() {
        let (engine, mock) = engine_with(4);
        assert_eq!(engine.cache_size(), 0);

        let first = engine.embed("hello").unwrap();
        assert_eq!(first, vec![5.0, 0.0, 0.0, 0.0]);
        assert_eq!(engine.cache_size(), 1);
        assert_eq!(mock.embed_calls.load(Ordering::SeqCst), 1);

        // Second call for the same text is served from the cache.
        let second = engine.embed("hello").unwrap();
        assert_eq!(second, first);
        assert_eq!(mock.embed_calls.load(Ordering::SeqCst), 1);
        assert_eq!(engine.cache_size(), 1);
    }

    #[test]
    fn embed_batch_only_embeds_uncached_and_preserves_order() {
        let (engine, mock) = engine_with(4);
        // Warm the cache with one entry via the single-embed path.
        engine.embed("bb").unwrap();
        assert_eq!(mock.batch_calls.load(Ordering::SeqCst), 0);

        let out = engine.embed_batch(&["bb", "cccc", "d"]).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0][0], 2.0); // cached "bb"
        assert_eq!(out[1][0], 4.0); // new "cccc"
        assert_eq!(out[2][0], 1.0); // new "d"

        // Exactly one batch call, covering only the two uncached texts.
        assert_eq!(mock.batch_calls.load(Ordering::SeqCst), 1);
        assert_eq!(engine.cache_size(), 3);
    }

    #[test]
    fn embed_batch_all_cached_skips_model() {
        let (engine, mock) = engine_with(4);
        engine.embed("aa").unwrap();
        engine.embed("bbb").unwrap();

        let out = engine.embed_batch(&["aa", "bbb"]).unwrap();
        assert_eq!(out[0][0], 2.0);
        assert_eq!(out[1][0], 3.0);
        // No batch call is issued when everything is cached.
        assert_eq!(mock.batch_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn similarity_identical_orthogonal_and_edge_cases() {
        let (engine, _) = engine_with(3);
        let a = [1.0, 0.0, 0.0];
        assert!((engine.similarity(&a, &a) - 1.0).abs() < 1e-6);

        let b = [0.0, 1.0, 0.0];
        assert_eq!(engine.similarity(&a, &b), 0.0);

        // Length mismatch short-circuits to 0.0.
        assert_eq!(engine.similarity(&a, &[1.0, 0.0]), 0.0);

        // A zero vector yields 0.0 rather than NaN from divide-by-zero.
        assert_eq!(engine.similarity(&a, &[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn dimension_and_model_name_reflect_backend() {
        let (engine, _) = engine_with(7);
        assert_eq!(engine.dimension(), 7);
        assert_eq!(engine.model_name(), "mock-embedder");
    }

    #[test]
    fn clear_cache_empties_the_cache() {
        let (engine, _) = engine_with(4);
        engine.embed("x").unwrap();
        engine.embed("yy").unwrap();
        assert_eq!(engine.cache_size(), 2);

        engine.clear_cache();
        assert_eq!(engine.cache_size(), 0);
    }

    #[test]
    fn similarity_opposite_vectors_returns_negative_one() {
        let (engine, _) = engine_with(2);
        // Anti-parallel unit vectors: dot = -1, norms = 1, so cosine = -1.0.
        assert!((engine.similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn similarity_left_zero_vector_returns_zero() {
        let (engine, _) = engine_with(3);
        // Existing coverage exercises the right-operand zero (norm_b == 0); this
        // pins the left-operand branch of `norm_a == 0.0 || norm_b == 0.0` so a
        // zero first argument yields 0.0 rather than a NaN from divide-by-zero.
        assert_eq!(engine.similarity(&[0.0, 0.0, 0.0], &[1.0, 2.0, 3.0]), 0.0);
        // Both zero also short-circuits to 0.0.
        assert_eq!(engine.similarity(&[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn with_static_model_errors_on_missing_model_dir() {
        // An empty directory has no config.json, so StaticEmbedding::from_pretrained
        // fails and with_static_model propagates the error rather than constructing
        // a half-built engine. Every other engine test builds via the struct literal
        // with a MockEmbedder, so this real constructor's failure arm was unexercised.
        let dir = tempfile::tempdir().unwrap();
        let result = VectorEngine::with_static_model(dir.path());
        assert!(
            result.is_err(),
            "with_static_model should fail when the model dir has no config.json"
        );
    }

    #[test]
    fn from_backend_static_dispatches_to_static_model_and_propagates_error() {
        // from_backend's Static arm routes to with_static_model; pointing it at an
        // empty dir exercises that dispatch branch and confirms the load error
        // surfaces through from_backend. The Fastembed arm can't be tested without
        // downloading an ONNX model, so the Static error path is the model-free frontier.
        let dir = tempfile::tempdir().unwrap();
        let backend = EmbeddingBackend::Static(dir.path().to_path_buf());
        let result = VectorEngine::from_backend(PathBuf::from("unused-cache"), &backend);
        assert!(
            result.is_err(),
            "from_backend(Static) should surface the static-model load failure"
        );
    }

    #[test]
    fn default_cache_dir_ends_with_codegraph_fastembed_cache() {
        // The default cache dir always resolves under a `.codegraph/fastembed_cache`
        // suffix regardless of which home var (HOME/USERPROFILE) or fallback (".")
        // supplies the base, so pin the stable trailing components.
        let dir = default_cache_dir();
        assert!(
            dir.ends_with(PathBuf::from(".codegraph").join("fastembed_cache")),
            "unexpected cache dir: {dir:?}"
        );
    }
}
