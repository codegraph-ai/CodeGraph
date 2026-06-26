// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Embedding module for semantic search
//!
//! Supports configurable embedding models: Jina Code V2 (768d, code-aware) or BGE-Small (384d, fast).

mod engine;
mod fastembed_embed;
mod static_embed;

pub use engine::VectorEngine;
pub use fastembed_embed::CodeGraphEmbeddingModel;

use crate::error::Result;

/// Pluggable embedding backend. The transformer path (fastembed/ONNX) and a
/// future static (lookup-table) path both implement this; `VectorEngine` holds a
/// `dyn Embedder` so the backend is swappable without touching callers.
pub(crate) trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
    fn model_name(&self) -> &str;
}
