// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Vector embedding engine
//!
//! High-level API for generating and caching embeddings.

use super::fastembed_embed::{CodeGraphEmbeddingModel, FastembedEmbedding};
use crate::error::Result;
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Vector embedding engine with caching
///
/// Wraps fastembed with a configurable model and DashMap cache for efficient repeated lookups.
pub struct VectorEngine {
    model: Arc<FastembedEmbedding>,
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
    pub fn model_name(&self) -> &'static str {
        self.model.model_type().display_name()
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
    #[test]
    fn test_cosine_similarity() {
        // Test cosine similarity calculation
        let a = [1.0_f32, 0.0, 0.0];
        let b = [1.0_f32, 0.0, 0.0];

        // Identical vectors should have similarity 1.0
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        let similarity = dot / (norm_a * norm_b);
        assert!((similarity - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_similarity_orthogonal() {
        let a = [1.0_f32, 0.0, 0.0];
        let b = [0.0_f32, 1.0, 0.0];

        // Orthogonal vectors should have similarity 0.0
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        assert_eq!(dot, 0.0);
    }
}
