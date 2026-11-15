// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! ONNX transformer embeddings for codegraph-memory
//!
//! Supports configurable embedding models:
//! - **BGE-Small-EN-v1.5** (384d, 512-tok context) — fast general-purpose default.
//! - **Jina Code V2** (768d) — code-aware, 6× slower than BGE.
//! - **Granite-97M-Multilingual-R2** (384d, 32K context) — IBM's
//!   ModernBERT-based multilingual embedder. 200+ languages, 9
//!   programming languages explicitly trained, MTEB Code Retrieval 57.
//!   Fetched from its own HuggingFace repo; fastembed-rs <= 4.9.x
//!   doesn't ship the model in its built-in enum.
//!
//! The ONNX session is built here rather than by fastembed's
//! `TextEmbedding`, which sizes ONNX Runtime's thread pool to every core
//! and leaves its threads spin-waiting, with no way to change either
//! (issue #29). fastembed still supplies the model catalogue and pooling;
//! downloading, tokenizer setup and batching mirror what 4.9.1 does, so
//! vectors match the ones it produced.

use crate::error::{MemoryError, Result};
use fastembed::{
    EmbeddingModel, OutputKey, Pooling, SingleBatchOutput, TextEmbedding, TokenizerFiles,
};
use ndarray::Array2;
use ort::{
    session::{builder::GraphOptimizationLevel, builder::SessionBuilder, Session},
    value::Value,
};
use std::path::{Path, PathBuf};
use tokenizers::{AddedToken, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

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

/// Token limit fastembed 4.9.1 applied to its built-in models when the
/// caller set none, which this crate never did. Kept so stored vectors stay
/// valid: Jina Code V2 has always been truncated here, not at its 8K window.
const BUILTIN_MAX_LENGTH: usize = 512;

/// Output tensor lookup order, as fastembed's `TextEmbedding` uses it.
const OUTPUT_PRECEDENCE: &[OutputKey] = &[
    OutputKey::OnlyOne,
    OutputKey::ByName("last_hidden_state"),
    OutputKey::ByName("sentence_embedding"),
];

/// ONNX transformer embedding model
pub(crate) struct FastembedEmbedding {
    session: Session,
    tokenizer: Tokenizer,
    need_token_type_ids: bool,
    pooling: Pooling,
    model_type: CodeGraphEmbeddingModel,
}

impl FastembedEmbedding {
    /// Create a new FastembedEmbedding with the specified model.
    ///
    /// The model is automatically downloaded to `cache_dir` on first use.
    /// On Windows, also ensures onnxruntime.dll is available (downloaded if needed).
    pub(crate) fn new(cache_dir: PathBuf, model_type: CodeGraphEmbeddingModel) -> Result<Self> {
        // On Windows with ort-load-dynamic, ensure onnxruntime.dll is available
        #[cfg(target_os = "windows")]
        ensure_ort_dll(&cache_dir)?;

        let threads = super::onnx_threads();
        log::info!(
            "Loading embedding model: {} ({threads} ONNX threads)",
            model_type.display_name()
        );

        let (session, tokenizer, pooling) = match model_type.to_fastembed_builtin() {
            Some(builtin) => Self::load_builtin(&cache_dir, builtin, model_type, threads)?,
            None => Self::load_user_defined(&cache_dir, model_type, threads)?,
        };

        let need_token_type_ids = session
            .inputs
            .iter()
            .any(|input| input.name == "token_type_ids");

        Ok(Self {
            session,
            tokenizer,
            need_token_type_ids,
            pooling,
            model_type,
        })
    }

    fn load_builtin(
        cache_dir: &Path,
        builtin: EmbeddingModel,
        model_type: CodeGraphEmbeddingModel,
        threads: usize,
    ) -> Result<Loaded> {
        let info = TextEmbedding::get_model_info(&builtin)
            .map_err(|e| MemoryError::model(format!("Unknown model {builtin:?}: {e}")))?;
        let repo = hf_repo(cache_dir, &info.model_code)?;

        let onnx = repo.get(&info.model_file).map_err(|e| {
            MemoryError::model(format!("Failed to retrieve {}: {e}", info.model_file))
        })?;
        for file in &info.additional_files {
            repo.get(file)
                .map_err(|e| MemoryError::model(format!("Failed to retrieve {file}: {e}")))?;
        }
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: read_repo_file(&repo, "tokenizer.json")?,
            config_file: read_repo_file(&repo, "config.json")?,
            special_tokens_map_file: read_repo_file(&repo, "special_tokens_map.json")?,
            tokenizer_config_file: read_repo_file(&repo, "tokenizer_config.json")?,
        };
        let pooling = TextEmbedding::get_default_pooling_method(&builtin).unwrap_or_default();

        let session = session_builder(threads)
            .and_then(|b| b.commit_from_file(onnx))
            .map_err(|e| load_error(model_type, e))?;
        let tokenizer = load_tokenizer(tokenizer_files, BUILTIN_MAX_LENGTH)
            .map_err(|e| load_error(model_type, e))?;
        Ok((session, tokenizer, pooling))
    }

    /// Load Granite (or other user-defined) ONNX + tokenizer files
    /// from HuggingFace via hf-hub.
    fn load_user_defined(
        cache_dir: &Path,
        model_type: CodeGraphEmbeddingModel,
        threads: usize,
    ) -> Result<Loaded> {
        let (repo, pooling) = match model_type {
            CodeGraphEmbeddingModel::Granite97mMultilingualR2 => (GRANITE_97M_REPO, Pooling::Cls),
            other => {
                return Err(MemoryError::model(format!(
                    "load_user_defined called for non-user-defined model: {other:?}"
                )));
            }
        };

        let bundle = download_user_defined_model(repo, cache_dir, model_type)?;
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: bundle.tokenizer_file,
            config_file: bundle.config_file,
            special_tokens_map_file: bundle.special_tokens_map_file,
            tokenizer_config_file: bundle.tokenizer_config_file,
        };

        let session = session_builder(threads)
            .and_then(|b| b.commit_from_memory(&bundle.onnx_bytes))
            .map_err(|e| load_error(model_type, e))?;
        let tokenizer = load_tokenizer(tokenizer_files, model_type.max_length())
            .map_err(|e| load_error(model_type, e))?;
        Ok((session, tokenizer, pooling))
    }

    /// Generate embedding for a single text
    pub(crate) fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.run_batch(&[text])
            .map_err(|e| MemoryError::embedding(format!("Embedding failed: {e}")))?
            .into_iter()
            .next()
            .ok_or_else(|| MemoryError::embedding("Empty embedding result"))
    }

    /// Generate embeddings for a batch of texts.
    /// Uses batch_size=64 to limit ONNX Runtime peak memory allocation.
    pub(crate) fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        // Granite-97m's intermediate tensors at 32K tokens are larger
        // than BGE-small's at 512. Halve the batch when we know we're
        // running the long-context model — peak memory stays bounded
        // even when full-body texts approach the upper context bound.
        let batch_size = match self.model_type {
            CodeGraphEmbeddingModel::Granite97mMultilingualR2 => 32,
            _ => 64,
        };
        // One batch at a time. fastembed ran them in parallel on the rayon
        // pool, each one on an ONNX pool already as wide as the machine.
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(batch_size) {
            vectors.extend(
                self.run_batch(batch)
                    .map_err(|e| MemoryError::embedding(format!("Batch embedding failed: {e}")))?,
            );
        }
        Ok(vectors)
    }

    /// Tokenize, run and pool one batch, as fastembed's `TextEmbedding::embed`
    /// does for each of its batches.
    fn run_batch(&self, batch: &[&str]) -> std::result::Result<Vec<Vec<f32>>, String> {
        let encodings = self
            .tokenizer
            .encode_batch(batch.to_vec(), true)
            .map_err(|e| format!("Failed to encode the batch: {e}"))?;
        let Some(first) = encodings.first() else {
            return Ok(Vec::new());
        };

        // Padding is BatchLongest, so every encoding has the same length.
        let shape = (encodings.len(), first.len());
        let mut ids = Vec::with_capacity(shape.0 * shape.1);
        let mut mask = Vec::with_capacity(shape.0 * shape.1);
        let mut type_ids = Vec::with_capacity(shape.0 * shape.1);
        for encoding in &encodings {
            ids.extend(encoding.get_ids().iter().map(|&x| i64::from(x)));
            mask.extend(encoding.get_attention_mask().iter().map(|&x| i64::from(x)));
            type_ids.extend(encoding.get_type_ids().iter().map(|&x| i64::from(x)));
        }
        let to_err = |e: ndarray::ShapeError| e.to_string();
        let ids = Array2::from_shape_vec(shape, ids).map_err(to_err)?;
        let mask = Array2::from_shape_vec(shape, mask).map_err(to_err)?;
        let type_ids = Array2::from_shape_vec(shape, type_ids).map_err(to_err)?;

        let ort_err = |e: ort::Error| e.to_string();
        let ids = Value::from_array(ids).map_err(ort_err)?;
        let attention_mask = Value::from_array(mask.view()).map_err(ort_err)?;
        let mut inputs = ort::inputs![
            "input_ids" => ids,
            "attention_mask" => attention_mask,
        ]
        .map_err(ort_err)?;
        if self.need_token_type_ids {
            inputs.push((
                "token_type_ids".into(),
                Value::from_array(type_ids).map_err(ort_err)?.into(),
            ));
        }

        let output = SingleBatchOutput {
            session_outputs: self.session.run(inputs).map_err(ort_err)?,
            attention_mask_array: mask,
        };
        let pooled = output
            .select_and_pool_output(&OUTPUT_PRECEDENCE, Some(self.pooling.clone()))
            .map_err(|e| e.to_string())?;
        Ok(pooled
            .rows()
            .into_iter()
            .map(|row| normalize(row.iter().copied()))
            .collect())
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

type Loaded = (Session, Tokenizer, Pooling);

fn load_error(model_type: CodeGraphEmbeddingModel, e: impl std::fmt::Display) -> MemoryError {
    MemoryError::model(format!(
        "Failed to load {} model: {e}",
        model_type.display_name()
    ))
}

/// Session options for every model: a capped intra-op pool whose idle
/// threads block instead of spinning. Spinning kept every core busy between
/// batches and, on a 12-core Mac, halved embedding throughput while doing it.
fn session_builder(threads: usize) -> ort::Result<SessionBuilder> {
    Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(threads)?
        .with_intra_op_spinning(false)
}

/// L2-normalise, with fastembed's epsilon.
fn normalize(values: impl Iterator<Item = f32> + Clone) -> Vec<f32> {
    let norm = values.clone().map(|v| v * v).sum::<f32>().sqrt();
    values.map(|v| v / (norm + 1e-12)).collect()
}

/// The HuggingFace repo for a built-in model, in the cache layout
/// fastembed 4.9.1 used (`models--<org>--<name>` under `cache_dir`), so
/// existing downloads are reused. Like fastembed, `HF_HOME` and
/// `HF_ENDPOINT` override the cache and the server.
fn hf_repo(cache_dir: &Path, model_code: &str) -> Result<hf_hub::api::sync::ApiRepo> {
    let cache = std::env::var_os("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache_dir.to_path_buf());
    let endpoint =
        std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".to_string());
    let api = hf_hub::api::sync::ApiBuilder::new()
        .with_cache_dir(cache)
        .with_endpoint(endpoint)
        .with_progress(true)
        .build()
        .map_err(|e| MemoryError::model(format!("Failed to initialise hf-hub API: {e}")))?;
    Ok(api.model(model_code.to_string()))
}

fn read_repo_file(repo: &hf_hub::api::sync::ApiRepo, name: &str) -> Result<Vec<u8>> {
    let local = repo
        .get(name)
        .map_err(|e| MemoryError::model(format!("Failed to retrieve {name}: {e}")))?;
    std::fs::read(&local)
        .map_err(|e| MemoryError::model(format!("Failed to read {}: {e}", local.display())))
}

/// fastembed 4.9.1's `load_tokenizer`, which it does not export: padding to
/// the longest text in the batch, truncation at the smaller of `max_length`
/// and the tokenizer's own limit, and the model's special tokens.
fn load_tokenizer(
    files: TokenizerFiles,
    max_length: usize,
) -> std::result::Result<Tokenizer, String> {
    let json = |bytes: &[u8], name: &str| {
        serde_json::from_slice::<serde_json::Value>(bytes)
            .map_err(|e| format!("Could not read {name}: {e}"))
    };
    let config = json(&files.config_file, "config.json")?;
    let special_tokens_map = json(&files.special_tokens_map_file, "special_tokens_map.json")?;
    let tokenizer_config = json(&files.tokenizer_config_file, "tokenizer_config.json")?;
    let mut tokenizer = Tokenizer::from_bytes(&files.tokenizer_file)
        .map_err(|e| format!("Could not read tokenizer.json: {e}"))?;

    // BGE sets model_max_length to 1e30, which only fits in an f64.
    let model_max_length = tokenizer_config["model_max_length"]
        .as_f64()
        .ok_or("tokenizer_config.json has no model_max_length")? as f32;
    let max_length = max_length.min(model_max_length as usize);
    let pad_id = config["pad_token_id"].as_u64().unwrap_or(0) as u32;
    let pad_token = tokenizer_config["pad_token"]
        .as_str()
        .ok_or("tokenizer_config.json has no pad_token")?
        .to_string();

    tokenizer
        .with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_token,
            pad_id,
            ..Default::default()
        }))
        .with_truncation(Some(TruncationParams {
            max_length,
            ..Default::default()
        }))
        .map_err(|e| format!("Could not configure the tokenizer: {e}"))?;

    if let serde_json::Value::Object(tokens) = special_tokens_map {
        for value in tokens.values() {
            let token = match value {
                serde_json::Value::String(content) => AddedToken {
                    content: content.clone(),
                    special: true,
                    ..Default::default()
                },
                serde_json::Value::Object(_) => AddedToken {
                    content: value["content"].as_str().unwrap_or_default().to_string(),
                    special: true,
                    single_word: value["single_word"].as_bool().unwrap_or_default(),
                    lstrip: value["lstrip"].as_bool().unwrap_or_default(),
                    rstrip: value["rstrip"].as_bool().unwrap_or_default(),
                    normalized: value["normalized"].as_bool().unwrap_or_default(),
                },
                _ => continue,
            };
            tokenizer.add_special_tokens(&[token]);
        }
    }
    Ok(tokenizer)
}

impl super::Embedder for FastembedEmbedding {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Explicit path → the inherent method (avoids any trait-method ambiguity).
        FastembedEmbedding::embed(self, text)
    }
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        FastembedEmbedding::embed_batch(self, texts)
    }
    fn dimension(&self) -> usize {
        FastembedEmbedding::dimension(self)
    }
    fn model_name(&self) -> &str {
        self.model_type().display_name()
    }
}

/// ONNX and tokenizer bytes for a user-defined model.
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
    cache_dir: &Path,
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
        MemoryError::model(format!("Failed to download {onnx_path} from {repo}: {e}"))
    })?;
    let onnx_bytes = std::fs::read(&onnx_local).map_err(|e| {
        MemoryError::model(format!(
            "Failed to read {} after download: {e}",
            onnx_local.display()
        ))
    })?;

    let read_required = |name: &str, repo: &hf_hub::api::sync::ApiRepo| -> Result<Vec<u8>> {
        let local = repo
            .get(name)
            .map_err(|e| MemoryError::model(format!("Failed to download {name}: {e}")))?;
        std::fs::read(&local)
            .map_err(|e| MemoryError::model(format!("Failed to read {}: {e}", local.display())))
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
                log::info!(
                    "ONNX Runtime DLL found alongside binary: {}",
                    bundled_dll.display()
                );
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
        assert!(CodeGraphEmbeddingModel::BgeSmall
            .to_fastembed_builtin()
            .is_some());
        assert!(CodeGraphEmbeddingModel::JinaCodeV2
            .to_fastembed_builtin()
            .is_some());
    }

    /// Texts that exercise padding (mixed lengths in one batch), truncation
    /// (well past 512 tokens) and non-ASCII input.
    fn parity_texts() -> Vec<String> {
        let long_body =
            "fn step(state: &mut State) -> Result<()> { state.tick += 1; } ".repeat(120);
        vec![
            "getUserById".to_string(),
            "fn parse_config(path: &Path) -> Result<Config>".to_string(),
            long_body,
            "def überprüfen(数据): return 数据 is not None  # проверка".to_string(),
            "class LruCache { get(key) { return this.map.get(key); } }".to_string(),
        ]
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm(a) * norm(b))
    }

    /// The owned session must reproduce fastembed's `TextEmbedding` vectors,
    /// or every stored vector would silently stop matching new queries.
    fn assert_matches_fastembed(model_type: CodeGraphEmbeddingModel) {
        let cache = super::super::engine::default_cache_dir();
        let ours = FastembedEmbedding::new(cache.clone(), model_type).expect("load model");

        let theirs = match model_type.to_fastembed_builtin() {
            Some(builtin) => fastembed::TextEmbedding::try_new(
                fastembed::InitOptions::new(builtin).with_cache_dir(cache),
            )
            .expect("load fastembed model"),
            None => {
                let b = download_user_defined_model(GRANITE_97M_REPO, &cache, model_type)
                    .expect("download");
                let model = fastembed::UserDefinedEmbeddingModel::new(
                    b.onnx_bytes,
                    TokenizerFiles {
                        tokenizer_file: b.tokenizer_file,
                        config_file: b.config_file,
                        special_tokens_map_file: b.special_tokens_map_file,
                        tokenizer_config_file: b.tokenizer_config_file,
                    },
                )
                .with_pooling(Pooling::Cls);
                fastembed::TextEmbedding::try_new_from_user_defined(
                    model,
                    fastembed::InitOptionsUserDefined::new()
                        .with_max_length(model_type.max_length()),
                )
                .expect("load fastembed model")
            }
        };

        let texts = parity_texts();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let expected = theirs
            .embed(texts.clone(), Some(64))
            .expect("fastembed embed");
        let batched = ours.embed_batch(&refs).expect("batch embed");
        assert_eq!(batched.len(), expected.len());

        for (i, text) in texts.iter().enumerate() {
            let single = ours.embed(text).expect("single embed");
            assert_eq!(single.len(), model_type.dimension());
            for (label, got) in [("batched", &batched[i]), ("single", &single)] {
                let sim = cosine(got, &expected[i]);
                assert!(
                    sim > 0.9999,
                    "{model_type:?} text {i} ({label}): cosine {sim} against fastembed"
                );
            }
        }
    }

    #[test]
    #[ignore = "downloads the BGE Small EN v1.5 model"]
    fn bge_small_matches_fastembed() {
        assert_matches_fastembed(CodeGraphEmbeddingModel::BgeSmall);
    }

    #[test]
    #[ignore = "downloads the 640 MB Jina Code V2 model"]
    fn jina_code_matches_fastembed() {
        assert_matches_fastembed(CodeGraphEmbeddingModel::JinaCodeV2);
    }

    #[test]
    #[ignore = "downloads the Granite 97M model"]
    fn granite_matches_fastembed() {
        assert_matches_fastembed(CodeGraphEmbeddingModel::Granite97mMultilingualR2);
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
