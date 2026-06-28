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
use std::path::PathBuf;

/// Pluggable embedding backend. The transformer path (fastembed/ONNX) and a
/// future static (lookup-table) path both implement this; `VectorEngine` holds a
/// `dyn Embedder` so the backend is swappable without touching callers.
pub(crate) trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
    fn model_name(&self) -> &str;
}

/// Selects which backend `VectorEngine` builds: the ONNX transformer path
/// (fastembed) or the static (model2vec) lookup path.
#[derive(Debug, Clone)]
pub enum EmbeddingBackend {
    /// ONNX transformer via fastembed (bge-small / jina-code-v2 / granite-97m).
    Fastembed(CodeGraphEmbeddingModel),
    /// Static model2vec model from a local directory (~100x faster indexing, no
    /// ONNX). The directory holds `config.json` + `tokenizer.json` +
    /// `model.safetensors`.
    Static(PathBuf),
}

impl Default for EmbeddingBackend {
    fn default() -> Self {
        Self::Fastembed(CodeGraphEmbeddingModel::default())
    }
}

impl EmbeddingBackend {
    /// Parse a `--embedding-model` value. `static` selects the model2vec path,
    /// resolving the model directory from `CODEGRAPH_STATIC_MODEL` or the default
    /// `~/.codegraph/static_models/jina-code-static-256`. Unknown values fall
    /// back to bge-small.
    pub fn parse(s: &str) -> Self {
        match s {
            "static" | "static-code" | "model2vec" => Self::Static(default_static_model_dir()),
            "jina-code-v2" => Self::Fastembed(CodeGraphEmbeddingModel::JinaCodeV2),
            "granite-97m" | "granite" | "granite-97m-multilingual-r2" => {
                Self::Fastembed(CodeGraphEmbeddingModel::Granite97mMultilingualR2)
            }
            _ => Self::Fastembed(CodeGraphEmbeddingModel::BgeSmall),
        }
    }

    /// Human-readable name for logging.
    pub fn display_name(&self) -> String {
        match self {
            Self::Fastembed(m) => m.display_name().to_string(),
            Self::Static(dir) => format!(
                "static:{} (model2vec)",
                dir.file_name().and_then(|s| s.to_str()).unwrap_or("model")
            ),
        }
    }
}

/// Resolve the static-model directory: `CODEGRAPH_STATIC_MODEL` env override,
/// else `~/.codegraph/static_models/jina-code-static-256`.
fn default_static_model_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CODEGRAPH_STATIC_MODEL") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".codegraph")
        .join("static_models")
        .join("jina-code-static-256")
}
