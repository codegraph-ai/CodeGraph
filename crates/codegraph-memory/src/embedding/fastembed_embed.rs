// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Fastembed wrapper for codegraph-memory
//!
//! Supports configurable embedding models:
//! - **BGE-Small-EN-v1.5** (384d, 512-tok context) — fast general-purpose default.
//! - **Jina Code V2** (768d, 8K context) — code-aware, 6× slower than BGE.
//! - **Granite-97M-Multilingual-R2** (384d, 32K context) — IBM's
//!   ModernBERT-based multilingual embedder. 200+ languages, 9
//!   programming languages explicitly trained, MTEB Code Retrieval 57.
//!   Loaded via fastembed's `UserDefinedEmbeddingModel` path because
//!   fastembed-rs <= 4.9.x doesn't yet ship the model in its built-in
//!   enum.

use crate::error::{MemoryError, Result};
use fastembed::{
    EmbeddingModel, InitOptions, InitOptionsUserDefined, Pooling, TextEmbedding,
    TokenizerFiles, UserDefinedEmbeddingModel,
};
use std::path::PathBuf;

/// Configurable embedding model selection
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodeGraphEmbeddingModel {
    /// Jina Code V2 (768d, 8K context) — code-aware, 6× slower than BGE
    JinaCodeV2,
    /// BGE-Small-EN-v1.5 (384d, 512-tok context) — fast, good quality with full-body embeddings
    #[default]
    BgeSmall,
    /// Granite-97M-Multilingual-R2 (384d, 32K context) — IBM ModernBERT-based,
    /// 200+ languages, 9 programming languages explicitly trained.
    /// Storage-compatible with BgeSmall (same 384 dim).
    Granite97mMultilingualR2,
}

impl CodeGraphEmbeddingModel {
    /// Map to fastembed's built-in enum where one exists. Returns None
    /// for user-defined models that fastembed doesn't ship.
    fn to_fastembed_builtin(self) -> Option<EmbeddingModel> {
        match self {
            Self::JinaCodeV2 => Some(EmbeddingModel::JinaEmbeddingsV2BaseCode),
            Self::BgeSmall => Some(EmbeddingModel::BGESmallENV15),
            Self::Granite97mMultilingualR2 => None,
        }
    }

    pub fn dimension(self) -> usize {
        match self {
            Self::JinaCodeV2 => 768,
            Self::BgeSmall => 384,
            Self::Granite97mMultilingualR2 => 384,
        }
    }

    /// Maximum input length the model accepts before truncation. The
    /// existing BGE-Small / Jina path uses fastembed's defaults (~512
    /// for BGE, ~8192 for Jina). Granite supports 32K natively.
    pub fn max_length(self) -> usize {
        match self {
            Self::JinaCodeV2 => 8192,
            Self::BgeSmall => 512,
            Self::Granite97mMultilingualR2 => 32_768,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::JinaCodeV2 => "Jina Code V2 (768d, 8K context)",
            Self::BgeSmall => "BGE-Small-EN-v1.5 (384d, 512-tok context)",
            Self::Granite97mMultilingualR2 => {
                "Granite-Embedding-97M-Multilingual-R2 (384d, 32K context)"
            }
        }
    }

    /// Stable identifier persisted alongside vectors so we can detect
    /// model swaps (even when dimensions match) and trigger
    /// re-embedding. The format is `<name>:<version>` and is forward-
    /// compatible — old vectors without this tag are treated as
    /// `bge-small:v15` for backwards compatibility.
    pub fn model_id_tag(self) -> &'static str {
        match self {
            Self::JinaCodeV2 => "jina-code:v2",
            Self::BgeSmall => "bge-small:v15",
            Self::Granite97mMultilingualR2 => "granite-97m-multi:r2",
        }
    }
}

/// HuggingFace repo ID for the Granite 97M multilingual R2 model.
const GRANITE_97M_REPO: &str = "ibm-granite/granite-embedding-97m-multilingual-r2";

/// ONNX file path within the repo. IBM ships a default and several
/// optimised variants (`model_O1.onnx`, `model_O2.onnx`); use the
/// default for portability.
const GRANITE_97M_ONNX_PATH: &str = "onnx/model.onnx";

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

        let model = match model_type.to_fastembed_builtin() {
            Some(builtin) => Self::load_builtin(cache_dir, builtin, model_type)?,
            None => Self::load_user_defined(cache_dir, model_type)?,
        };

        Ok(Self { model, model_type })
    }

    fn load_builtin(
        cache_dir: PathBuf,
        builtin: EmbeddingModel,
        model_type: CodeGraphEmbeddingModel,
    ) -> Result<TextEmbedding> {
        let options = InitOptions::new(builtin)
            .with_cache_dir(cache_dir)
            .with_show_download_progress(true);

        TextEmbedding::try_new(options).map_err(|e| {
            MemoryError::model(format!(
                "Failed to load {} model: {e}",
                model_type.display_name()
            ))
        })
    }

    /// Load Granite (or other user-defined) ONNX + tokenizer files
    /// from HuggingFace via hf-hub, then hand them to fastembed's
    /// `try_new_from_user_defined` path.
    fn load_user_defined(
        cache_dir: PathBuf,
        model_type: CodeGraphEmbeddingModel,
    ) -> Result<TextEmbedding> {
        let (repo, pooling) = match model_type {
            CodeGraphEmbeddingModel::Granite97mMultilingualR2 => {
                (GRANITE_97M_REPO, Pooling::Cls)
            }
            other => {
                return Err(MemoryError::model(format!(
                    "load_user_defined called for non-user-defined model: {other:?}"
                )));
            }
        };

        let bundle = download_user_defined_model(repo, &cache_dir, model_type)?;
        let user_model = UserDefinedEmbeddingModel::new(
            bundle.onnx_bytes,
            TokenizerFiles {
                tokenizer_file: bundle.tokenizer_file,
                config_file: bundle.config_file,
                special_tokens_map_file: bundle.special_tokens_map_file,
                tokenizer_config_file: bundle.tokenizer_config_file,
            },
        )
        .with_pooling(pooling);

        let options = InitOptionsUserDefined::new().with_max_length(model_type.max_length());

        TextEmbedding::try_new_from_user_defined(user_model, options).map_err(|e| {
            MemoryError::model(format!(
                "Failed to load {} from user-defined ONNX: {e}",
                model_type.display_name()
            ))
        })
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
        // Granite-97m's intermediate tensors at 32K tokens are larger
        // than BGE-small's at 512. Halve the batch when we know we're
        // running the long-context model — peak memory stays bounded
        // even when full-body texts approach the upper context bound.
        let batch_size = match self.model_type {
            CodeGraphEmbeddingModel::Granite97mMultilingualR2 => 32,
            _ => 64,
        };
        self.model
            .embed(owned, Some(batch_size))
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

/// Bundle of bytes needed to construct a `UserDefinedEmbeddingModel`.
struct UserDefinedModelBundle {
    onnx_bytes: Vec<u8>,
    tokenizer_file: Vec<u8>,
    config_file: Vec<u8>,
    special_tokens_map_file: Vec<u8>,
    tokenizer_config_file: Vec<u8>,
}

/// Download the ONNX file + tokenizer config bundle for `repo` via
/// hf-hub, caching to `cache_dir/<repo>`. Returns the bytes loaded.
///
/// hf-hub's cache structure differs from fastembed's; we deliberately
/// use a sub-namespace so a user's existing fastembed cache isn't
/// disturbed.
fn download_user_defined_model(
    repo: &str,
    cache_dir: &std::path::Path,
    model_type: CodeGraphEmbeddingModel,
) -> Result<UserDefinedModelBundle> {
    use hf_hub::api::sync::ApiBuilder;

    let api_cache = cache_dir.join("hf_hub");
    std::fs::create_dir_all(&api_cache).map_err(|e| {
        MemoryError::model(format!(
            "Failed to create hf-hub cache dir at {}: {e}",
            api_cache.display()
        ))
    })?;

    let api = ApiBuilder::new()
        .with_cache_dir(api_cache)
        .with_progress(true)
        .build()
        .map_err(|e| {
            MemoryError::model(format!(
                "Failed to initialise hf-hub API for {}: {e}",
                model_type.display_name()
            ))
        })?;

    let model_repo = api.model(repo.to_string());

    // Required files for UserDefinedEmbeddingModel + TokenizerFiles.
    let onnx_path = match model_type {
        CodeGraphEmbeddingModel::Granite97mMultilingualR2 => GRANITE_97M_ONNX_PATH,
        _ => GRANITE_97M_ONNX_PATH,
    };
    let onnx_local = model_repo.get(onnx_path).map_err(|e| {
        MemoryError::model(format!(
            "Failed to download {onnx_path} from {repo}: {e}"
        ))
    })?;
    let onnx_bytes = std::fs::read(&onnx_local).map_err(|e| {
        MemoryError::model(format!(
            "Failed to read {} after download: {e}",
            onnx_local.display()
        ))
    })?;

    let read_required =
        |name: &str, repo: &hf_hub::api::sync::ApiRepo| -> Result<Vec<u8>> {
            let local = repo.get(name).map_err(|e| {
                MemoryError::model(format!("Failed to download {name}: {e}"))
            })?;
            std::fs::read(&local).map_err(|e| {
                MemoryError::model(format!("Failed to read {}: {e}", local.display()))
            })
        };

    Ok(UserDefinedModelBundle {
        onnx_bytes,
        tokenizer_file: read_required("tokenizer.json", &model_repo)?,
        config_file: read_required("config.json", &model_repo)?,
        special_tokens_map_file: read_required("special_tokens_map.json", &model_repo)?,
        tokenizer_config_file: read_required("tokenizer_config.json", &model_repo)?,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_dimensions_are_correct() {
        assert_eq!(CodeGraphEmbeddingModel::BgeSmall.dimension(), 384);
        assert_eq!(CodeGraphEmbeddingModel::JinaCodeV2.dimension(), 768);
        assert_eq!(
            CodeGraphEmbeddingModel::Granite97mMultilingualR2.dimension(),
            384
        );
    }

    #[test]
    fn max_length_reflects_context_window() {
        assert_eq!(CodeGraphEmbeddingModel::BgeSmall.max_length(), 512);
        assert_eq!(CodeGraphEmbeddingModel::JinaCodeV2.max_length(), 8192);
        assert_eq!(
            CodeGraphEmbeddingModel::Granite97mMultilingualR2.max_length(),
            32_768
        );
    }

    #[test]
    fn granite_is_user_defined_only() {
        // Granite is loaded via the user-defined path because fastembed
        // doesn't ship it in its enum.
        assert!(CodeGraphEmbeddingModel::Granite97mMultilingualR2
            .to_fastembed_builtin()
            .is_none());
        assert!(CodeGraphEmbeddingModel::BgeSmall.to_fastembed_builtin().is_some());
        assert!(CodeGraphEmbeddingModel::JinaCodeV2.to_fastembed_builtin().is_some());
    }

    #[test]
    fn model_id_tags_are_distinct() {
        let tags = [
            CodeGraphEmbeddingModel::BgeSmall.model_id_tag(),
            CodeGraphEmbeddingModel::JinaCodeV2.model_id_tag(),
            CodeGraphEmbeddingModel::Granite97mMultilingualR2.model_id_tag(),
        ];
        for (i, a) in tags.iter().enumerate() {
            for (j, b) in tags.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "model_id_tag must be unique across variants");
                }
            }
        }
    }
}
