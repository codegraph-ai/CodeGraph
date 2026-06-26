// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Static (lookup-table) embeddings — the fast indexing path.
//!
//! A static embedder runs no neural network at inference. It loads a
//! `token -> vector` matrix (distilled offline from a transformer teacher) plus
//! that teacher's tokenizer, and embeds text as: tokenize -> gather the token
//! rows -> mean-pool -> L2-normalize. ~100x faster than an ONNX transformer
//! forward pass, trading away contextualization.
//!
//! Loads the model2vec on-disk format (`config.json` + `tokenizer.json` +
//! `model.safetensors` holding an `embeddings` [vocab, dim] F32 tensor), so any
//! model2vec / distilled static model — including a Jina-Code distillation —
//! drops in unchanged.

use super::Embedder;
use crate::error::{MemoryError, Result};
use safetensors::SafeTensors;
use std::path::Path;
use tokenizers::Tokenizer;

/// The subset of model2vec's `config.json` we read (unknown fields ignored).
#[derive(serde::Deserialize)]
struct StaticConfig {
    #[serde(default = "default_true")]
    normalize: bool,
    #[serde(default)]
    hidden_dim: Option<usize>,
}
fn default_true() -> bool {
    true
}

/// A static, lookup-table embedder (no ONNX / no transformer at inference).
pub(crate) struct StaticEmbedding {
    tokenizer: Tokenizer,
    /// Row-major `vocab * dim` token vectors; token `i` is `matrix[i*dim..(i+1)*dim]`.
    matrix: Vec<f32>,
    vocab: usize,
    dim: usize,
    normalize: bool,
    name: String,
}

impl StaticEmbedding {
    /// Load a model2vec-format directory: `config.json`, `tokenizer.json`, and
    /// `model.safetensors` (with an `embeddings` [vocab, dim] F32 tensor).
    pub(crate) fn from_pretrained(dir: &Path) -> Result<Self> {
        let cfg: StaticConfig = serde_json::from_slice(
            &std::fs::read(dir.join("config.json"))
                .map_err(|e| MemoryError::model(format!("read config.json: {e}")))?,
        )?;

        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| MemoryError::model(format!("load tokenizer.json: {e}")))?;

        let bytes = std::fs::read(dir.join("model.safetensors"))
            .map_err(|e| MemoryError::model(format!("read model.safetensors: {e}")))?;
        let st = SafeTensors::deserialize(&bytes)
            .map_err(|e| MemoryError::model(format!("parse safetensors: {e}")))?;
        let tensor = st
            .tensor("embeddings")
            .map_err(|e| MemoryError::model(format!("no 'embeddings' tensor: {e}")))?;

        let shape = tensor.shape();
        if shape.len() != 2 {
            return Err(MemoryError::model(format!(
                "embeddings must be 2-D [vocab, dim], got {shape:?}"
            )));
        }
        let (vocab, dim) = (shape[0], shape[1]);
        if tensor.dtype() != safetensors::Dtype::F32 {
            return Err(MemoryError::model(format!(
                "embeddings dtype must be F32, got {:?}",
                tensor.dtype()
            )));
        }
        let matrix: Vec<f32> = tensor
            .data()
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        if matrix.len() != vocab * dim {
            return Err(MemoryError::model(format!(
                "embeddings length {} != vocab*dim {}",
                matrix.len(),
                vocab * dim
            )));
        }

        let name = format!(
            "{} ({}d, static)",
            dir.file_name().and_then(|s| s.to_str()).unwrap_or("static"),
            cfg.hidden_dim.unwrap_or(dim)
        );

        Ok(Self {
            tokenizer,
            matrix,
            vocab,
            dim,
            normalize: cfg.normalize,
            name,
        })
    }

    fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        // add_special_tokens = false: a bag-of-tokens static model gains nothing
        // from [CLS]/[SEP]; they would only dilute the mean.
        let enc = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| MemoryError::embedding(format!("tokenize: {e}")))?;
        Ok(mean_pool_l2(
            &self.matrix,
            enc.get_ids(),
            self.dim,
            self.vocab,
            self.normalize,
        ))
    }
}

/// Mean-pool the rows for `ids` from a row-major `vocab*dim` matrix, skipping
/// out-of-vocab ids; optionally L2-normalize. This is the whole hot path.
fn mean_pool_l2(matrix: &[f32], ids: &[u32], dim: usize, vocab: usize, normalize: bool) -> Vec<f32> {
    let mut acc = vec![0f32; dim];
    let mut n = 0usize;
    for &id in ids {
        let i = id as usize;
        if i >= vocab {
            continue;
        }
        let row = &matrix[i * dim..(i + 1) * dim];
        for (a, r) in acc.iter_mut().zip(row) {
            *a += *r;
        }
        n += 1;
    }
    if n > 0 {
        let inv = 1.0 / n as f32;
        for a in acc.iter_mut() {
            *a *= inv;
        }
    }
    if normalize {
        let norm: f32 = acc.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            let inv = 1.0 / norm;
            for a in acc.iter_mut() {
                *a *= inv;
            }
        }
    }
    acc
}

impl Embedder for StaticEmbedding {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_text(text)
    }
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed_text(t)).collect()
    }
    fn dimension(&self) -> usize {
        self.dim
    }
    fn model_name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_pool_l2_math() {
        // vocab 2, dim 2: row0 = [3,0], row1 = [0,4].
        let matrix = vec![3.0, 0.0, 0.0, 4.0];
        // mean of the two rows
        assert_eq!(mean_pool_l2(&matrix, &[0, 1], 2, 2, false), vec![1.5, 2.0]);
        // out-of-vocab id (99) is skipped -> only row0 counts
        assert_eq!(mean_pool_l2(&matrix, &[0, 99], 2, 2, false), vec![3.0, 0.0]);
        // all-OOV -> zero vector, no NaN even with normalize
        assert_eq!(mean_pool_l2(&matrix, &[7], 2, 2, true), vec![0.0, 0.0]);
        // normalize -> unit length
        let v = mean_pool_l2(&matrix, &[0, 1], 2, 2, true);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "norm={norm}");
    }

    fn potion_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".codegraph/static_models/potion-base-8M")
    }

    #[test]
    fn static_embedding_loads_and_embeds_potion() {
        let dir = potion_dir();
        if !dir.join("model.safetensors").exists() {
            eprintln!("skip: potion-base-8M not present at {}", dir.display());
            return;
        }
        let m = StaticEmbedding::from_pretrained(&dir).expect("load potion-base-8M");
        assert_eq!(m.dimension(), 256);

        let v = m.embed_text("database connection pool").unwrap();
        assert_eq!(v.len(), 256);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "should be L2-normalized, norm={norm}");

        // Semantic sanity: two related phrases closer than an unrelated one.
        let cos = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        let a = m.embed_text("open a database connection").unwrap();
        let b = m.embed_text("connect to the postgres database").unwrap();
        let u = m.embed_text("the cat sat on the warm windowsill").unwrap();
        assert!(
            cos(&a, &b) > cos(&a, &u),
            "related {:.3} should exceed unrelated {:.3}",
            cos(&a, &b),
            cos(&a, &u)
        );
    }
}
