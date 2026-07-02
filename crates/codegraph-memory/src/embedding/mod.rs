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

    /// Short, stable tag for telemetry / adoption grouping.
    pub fn telemetry_id(&self) -> &'static str {
        match self {
            Self::Static(_) => "static",
            Self::Fastembed(CodeGraphEmbeddingModel::BgeSmall) => "bge-small",
            Self::Fastembed(CodeGraphEmbeddingModel::JinaCodeV2) => "jina-code-v2",
            Self::Fastembed(CodeGraphEmbeddingModel::Granite97mMultilingualR2) => "granite-97m",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Guards process-wide env mutation across the env-sensitive tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn is_fastembed(b: &EmbeddingBackend, model: CodeGraphEmbeddingModel) -> bool {
        matches!(b, EmbeddingBackend::Fastembed(m) if m.model_id_tag() == model.model_id_tag())
    }

    #[test]
    fn parse_static_aliases_select_static_backend() {
        for s in ["static", "static-code", "model2vec"] {
            assert!(
                matches!(EmbeddingBackend::parse(s), EmbeddingBackend::Static(_)),
                "{s} should parse to Static"
            );
        }
    }

    #[test]
    fn parse_jina_selects_jina() {
        assert!(is_fastembed(
            &EmbeddingBackend::parse("jina-code-v2"),
            CodeGraphEmbeddingModel::JinaCodeV2
        ));
    }

    #[test]
    fn parse_granite_aliases_select_granite() {
        for s in ["granite-97m", "granite", "granite-97m-multilingual-r2"] {
            assert!(
                is_fastembed(
                    &EmbeddingBackend::parse(s),
                    CodeGraphEmbeddingModel::Granite97mMultilingualR2
                ),
                "{s} should parse to Granite"
            );
        }
    }

    #[test]
    fn parse_unknown_and_empty_fall_back_to_bge() {
        for s in ["", "bge-small", "totally-unknown"] {
            assert!(
                is_fastembed(
                    &EmbeddingBackend::parse(s),
                    CodeGraphEmbeddingModel::BgeSmall
                ),
                "{s:?} should fall back to BgeSmall"
            );
        }
    }

    #[test]
    fn default_backend_is_fastembed_bge() {
        let b = EmbeddingBackend::default();
        assert!(is_fastembed(&b, CodeGraphEmbeddingModel::BgeSmall));
        assert_eq!(b.telemetry_id(), "bge-small");
    }

    #[test]
    fn display_name_fastembed_delegates_to_model() {
        let b = EmbeddingBackend::Fastembed(CodeGraphEmbeddingModel::JinaCodeV2);
        assert_eq!(
            b.display_name(),
            CodeGraphEmbeddingModel::JinaCodeV2.display_name()
        );
    }

    #[test]
    fn display_name_static_uses_dir_basename() {
        let b = EmbeddingBackend::Static(PathBuf::from("/models/jina-code-static-256"));
        assert_eq!(b.display_name(), "static:jina-code-static-256 (model2vec)");
    }

    #[test]
    fn telemetry_id_covers_every_backend() {
        assert_eq!(
            EmbeddingBackend::Static(PathBuf::from("/x")).telemetry_id(),
            "static"
        );
        assert_eq!(
            EmbeddingBackend::Fastembed(CodeGraphEmbeddingModel::BgeSmall).telemetry_id(),
            "bge-small"
        );
        assert_eq!(
            EmbeddingBackend::Fastembed(CodeGraphEmbeddingModel::JinaCodeV2).telemetry_id(),
            "jina-code-v2"
        );
        assert_eq!(
            EmbeddingBackend::Fastembed(CodeGraphEmbeddingModel::Granite97mMultilingualR2)
                .telemetry_id(),
            "granite-97m"
        );
    }

    #[test]
    fn static_model_dir_prefers_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved = std::env::var_os("CODEGRAPH_STATIC_MODEL");
        std::env::set_var("CODEGRAPH_STATIC_MODEL", "/custom/model/path");
        let dir = default_static_model_dir();
        match saved {
            Some(v) => std::env::set_var("CODEGRAPH_STATIC_MODEL", v),
            None => std::env::remove_var("CODEGRAPH_STATIC_MODEL"),
        }
        assert_eq!(dir, PathBuf::from("/custom/model/path"));
    }

    #[test]
    fn static_model_dir_default_path_under_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved_model = std::env::var_os("CODEGRAPH_STATIC_MODEL");
        let saved_home = std::env::var_os("HOME");
        std::env::remove_var("CODEGRAPH_STATIC_MODEL");
        std::env::set_var("HOME", "/home/tester");
        let dir = default_static_model_dir();
        match saved_model {
            Some(v) => std::env::set_var("CODEGRAPH_STATIC_MODEL", v),
            None => std::env::remove_var("CODEGRAPH_STATIC_MODEL"),
        }
        match saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        assert_eq!(
            dir,
            PathBuf::from("/home/tester/.codegraph/static_models/jina-code-static-256")
        );
    }
}
