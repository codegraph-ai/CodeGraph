// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Fastembed wrapper for codegraph-memory
//!
//! Supports configurable embedding models:
//! - Jina Code V2 (768d) — code-aware, trained on 150M+ code pairs. Best quality for clone detection.
//! - BGE-Small-EN-v1.5 (384d) — fast general-purpose. 4-5x faster, lower quality on code similarity.

use crate::error::{MemoryError, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::path::PathBuf;

/// Configurable embedding model selection
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodeGraphEmbeddingModel {
    /// Jina Code V2 (768d) — code-aware, 6x slower than BGE
    JinaCodeV2,
    /// BGE-Small-EN-v1.5 (384d) — fast, good quality with full-body embeddings
    #[default]
    BgeSmall,
}

impl CodeGraphEmbeddingModel {
    fn to_fastembed(self) -> EmbeddingModel {
        match self {
            Self::JinaCodeV2 => EmbeddingModel::JinaEmbeddingsV2BaseCode,
            Self::BgeSmall => EmbeddingModel::BGESmallENV15,
        }
    }

    pub fn dimension(self) -> usize {
        match self {
            Self::JinaCodeV2 => 768,
            Self::BgeSmall => 384,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::JinaCodeV2 => "Jina Code V2 (768d)",
            Self::BgeSmall => "BGE-Small-EN-v1.5 (384d)",
        }
    }
}

/// ONNX Runtime version required by ort-sys 2.0.0-rc.9
#[cfg(target_os = "windows")]
const ORT_VERSION: &str = "1.20.0";

/// Fastembed-based text embedding model
pub(crate) struct FastembedEmbedding {
    model: TextEmbedding,
    model_type: CodeGraphEmbeddingModel,
}

impl FastembedEmbedding {
    /// Create a new FastembedEmbedding with the specified model.
    ///
    /// The model is automatically downloaded to `cache_dir` on first use.
    /// On Windows, also ensures onnxruntime.dll is available (downloaded if needed).
    pub(crate) fn new(cache_dir: PathBuf, model_type: CodeGraphEmbeddingModel) -> Result<Self> {
        // MUST set FASTEMBED_CACHE_DIR before InitOptions::new() — its Default impl
        // calls get_cache_dir() which falls back to ".fastembed_cache" in CWD.
        // Note: the env var is FASTEMBED_CACHE_DIR (not _PATH).
        unsafe { std::env::set_var("FASTEMBED_CACHE_DIR", &cache_dir) };

        // On Windows with ort-load-dynamic, ensure onnxruntime.dll is available
        #[cfg(target_os = "windows")]
        ensure_ort_dll(&cache_dir)?;

        log::info!("Loading embedding model: {}", model_type.display_name());

        let options = InitOptions::new(model_type.to_fastembed())
            .with_cache_dir(cache_dir)
            .with_show_download_progress(true);

        let model = TextEmbedding::try_new(options)
            .map_err(|e| MemoryError::model(format!("Failed to load {} model: {e}", model_type.display_name())))?;

        Ok(Self { model, model_type })
    }

    /// Generate embedding for a single text
    pub(crate) fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self
            .model
            .embed(vec![text.to_string()], None)
            .map_err(|e| MemoryError::embedding(format!("Embedding failed: {e}")))?;

        results
            .into_iter()
            .next()
            .ok_or_else(|| MemoryError::embedding("Empty embedding result"))
    }

    /// Generate embeddings for a batch of texts.
    /// Uses batch_size=64 to limit ONNX Runtime peak memory allocation.
    pub(crate) fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let owned: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
        self.model
            .embed(owned, Some(64))
            .map_err(|e| MemoryError::embedding(format!("Batch embedding failed: {e}")))
    }

    /// Get the embedding dimension (depends on model)
    pub(crate) fn dimension(&self) -> usize {
        self.model_type.dimension()
    }

    /// Get the model type
    pub(crate) fn model_type(&self) -> CodeGraphEmbeddingModel {
        self.model_type
    }
}

/// Ensure onnxruntime.dll is present for ort-load-dynamic on Windows.
///
/// Search order:
/// 1. Next to the executable (shipped in VSIX/npm package)
/// 2. In the cache directory (previously downloaded)
/// 3. Download from GitHub releases (fallback)
///
/// Sets `ORT_DYLIB_PATH` so ort can find it.
#[cfg(target_os = "windows")]
fn ensure_ort_dll(cache_dir: &std::path::Path) -> Result<()> {
    // Check next to the executable first (shipped alongside binary)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled_dll = exe_dir.join("onnxruntime.dll");
            if bundled_dll.exists() {
                log::info!("ONNX Runtime DLL found alongside binary: {}", bundled_dll.display());
                std::env::set_var("ORT_DYLIB_PATH", &bundled_dll);
                return Ok(());
            }
        }
    }

    let dll_dir = cache_dir.join("ort");
    let dll_path = dll_dir.join("onnxruntime.dll");

    // Check cache directory
    if dll_path.exists() {
        log::info!("ONNX Runtime DLL found in cache: {}", dll_path.display());
        std::env::set_var("ORT_DYLIB_PATH", &dll_path);
        return Ok(());
    }

    log::info!(
        "ONNX Runtime DLL not found — downloading v{} (one-time setup)...",
        ORT_VERSION
    );

    std::fs::create_dir_all(&dll_dir)
        .map_err(|e| MemoryError::model(format!("Failed to create ORT cache dir: {e}")))?;

    // Download the official release zip
    let url = format!(
        "https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}/onnxruntime-win-x64-{ORT_VERSION}.zip"
    );

    let response = ureq::get(&url)
        .call()
        .map_err(|e| MemoryError::model(format!("Failed to download ONNX Runtime: {e}")))?;

    // Stream to a temp file
    let zip_path = dll_dir.join("onnxruntime.zip");
    let mut zip_file = std::fs::File::create(&zip_path)
        .map_err(|e| MemoryError::model(format!("Failed to create temp zip: {e}")))?;
    std::io::copy(&mut response.into_reader(), &mut zip_file)
        .map_err(|e| MemoryError::model(format!("Failed to write zip: {e}")))?;
    drop(zip_file);

    // Extract onnxruntime.dll from the zip
    let zip_file = std::fs::File::open(&zip_path)
        .map_err(|e| MemoryError::model(format!("Failed to open zip: {e}")))?;
    let mut archive = zip::ZipArchive::new(zip_file)
        .map_err(|e| MemoryError::model(format!("Failed to read zip: {e}")))?;

    let dll_name_in_zip = format!("onnxruntime-win-x64-{ORT_VERSION}/lib/onnxruntime.dll");

    let mut dll_entry = archive.by_name(&dll_name_in_zip).map_err(|e| {
        MemoryError::model(format!(
            "onnxruntime.dll not found in zip at '{dll_name_in_zip}': {e}"
        ))
    })?;

    let mut out_file = std::fs::File::create(&dll_path)
        .map_err(|e| MemoryError::model(format!("Failed to create DLL file: {e}")))?;
    std::io::copy(&mut dll_entry, &mut out_file)
        .map_err(|e| MemoryError::model(format!("Failed to extract DLL: {e}")))?;

    // Clean up zip
    let _ = std::fs::remove_file(&zip_path);

    log::info!(
        "ONNX Runtime v{} DLL installed at {}",
        ORT_VERSION,
        dll_path.display()
    );
    std::env::set_var("ORT_DYLIB_PATH", &dll_path);

    Ok(())
}
