// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Static (lookup-table) embeddings — the fast indexing path.
//!
//! A static embedder runs no neural network at inference. It loads a
//! `token -> vector` matrix (distilled offline from a transformer teacher) plus
//! that teacher's tokenizer, and embeds text as: tokenize -> gather the token
//! rows -> (optional per-token weight) -> mean-pool -> L2-normalize. ~100x
//! faster than an ONNX transformer forward pass, trading away contextualization.
//!
//! Loads the model2vec on-disk format (`config.json` + `tokenizer.json` +
//! `model.safetensors`), so any model2vec / distilled static model — including a
//! Jina-Code distillation — drops in unchanged. Matches model2vec's encode
//! exactly (`model.py`): `emb[ids] * weights[ids]`, then `.mean(axis=0)`, then
//! `/ norm`. The matrix may be F32 (older potions bake the weighting in) or F16
//! with a separate F64 `weights` vector (model2vec 0.7's SIF weighting).

use super::Embedder;
use crate::error::{MemoryError, Result};
use safetensors::tensor::TensorView;
use safetensors::{Dtype, SafeTensors};
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
    /// Optional per-token weight (model2vec SIF). `None` => unweighted mean.
    weights: Option<Vec<f32>>,
    vocab: usize,
    dim: usize,
    normalize: bool,
    name: String,
}

impl StaticEmbedding {
    /// Load a model2vec-format directory: `config.json`, `tokenizer.json`, and
    /// `model.safetensors` (an `embeddings` [vocab, dim] tensor, F32 or F16, and
    /// an optional `weights` [vocab] tensor).
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

        let emb = st
            .tensor("embeddings")
            .map_err(|e| MemoryError::model(format!("no 'embeddings' tensor: {e}")))?;
        let shape = emb.shape();
        if shape.len() != 2 {
            return Err(MemoryError::model(format!(
                "embeddings must be 2-D [vocab, dim], got {shape:?}"
            )));
        }
        let (vocab, dim) = (shape[0], shape[1]);
        let matrix = tensor_to_f32(&emb)?;
        if matrix.len() != vocab * dim {
            return Err(MemoryError::model(format!(
                "embeddings length {} != vocab*dim {}",
                matrix.len(),
                vocab * dim
            )));
        }

        // Optional per-token SIF weights (model2vec >= 0.4 stores them separately
        // instead of baking them into `embeddings`).
        let weights = match st.tensor("weights") {
            Ok(w) => {
                let v = tensor_to_f32(&w)?;
                if v.len() != vocab {
                    return Err(MemoryError::model(format!(
                        "weights length {} != vocab {vocab}",
                        v.len()
                    )));
                }
                Some(v)
            }
            Err(_) => None,
        };

        let name = format!(
            "{} ({}d, static)",
            dir.file_name().and_then(|s| s.to_str()).unwrap_or("static"),
            cfg.hidden_dim.unwrap_or(dim)
        );

        Ok(Self {
            tokenizer,
            matrix,
            weights,
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
        Ok(weighted_mean_l2(
            &self.matrix,
            self.weights.as_deref(),
            enc.get_ids(),
            self.dim,
            self.vocab,
            self.normalize,
        ))
    }
}

/// Decode a safetensors float tensor (F32 / F16 / F64) into `Vec<f32>`.
fn tensor_to_f32(t: &TensorView) -> Result<Vec<f32>> {
    let raw = t.data();
    let v = match t.dtype() {
        Dtype::F32 => raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        Dtype::F16 => raw
            .chunks_exact(2)
            .map(|b| half::f16::from_le_bytes(b.try_into().unwrap()).to_f32())
            .collect(),
        Dtype::F64 => raw
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes(b.try_into().unwrap()) as f32)
            .collect(),
        other => {
            return Err(MemoryError::model(format!(
                "unsupported tensor dtype {other:?} (want F32/F16/F64)"
            )))
        }
    };
    Ok(v)
}

/// model2vec's pooling: each token row times its weight, mean over tokens, then
/// optional L2-normalize. Out-of-vocab ids are skipped. The whole hot path.
fn weighted_mean_l2(
    matrix: &[f32],
    weights: Option<&[f32]>,
    ids: &[u32],
    dim: usize,
    vocab: usize,
    normalize: bool,
) -> Vec<f32> {
    let mut acc = vec![0f32; dim];
    let mut n = 0usize;
    for &id in ids {
        let i = id as usize;
        if i >= vocab {
            continue;
        }
        let w = weights.map_or(1.0, |ws| ws[i]);
        let row = &matrix[i * dim..(i + 1) * dim];
        for (a, r) in acc.iter_mut().zip(row) {
            *a += w * *r;
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
    fn weighted_mean_l2_math() {
        // vocab 2, dim 2: row0 = [3,0], row1 = [0,4].
        let matrix = vec![3.0, 0.0, 0.0, 4.0];
        // unweighted: mean of the two rows
        assert_eq!(
            weighted_mean_l2(&matrix, None, &[0, 1], 2, 2, false),
            vec![1.5, 2.0]
        );
        // out-of-vocab id (99) skipped -> only row0 counts
        assert_eq!(
            weighted_mean_l2(&matrix, None, &[0, 99], 2, 2, false),
            vec![3.0, 0.0]
        );
        // all-OOV -> zero vector, no NaN even with normalize
        assert_eq!(
            weighted_mean_l2(&matrix, None, &[7], 2, 2, true),
            vec![0.0, 0.0]
        );
        // normalize -> unit length
        let v = weighted_mean_l2(&matrix, None, &[0, 1], 2, 2, true);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "norm={norm}");
        // weighted: row0*2 + row1*0.5, mean over 2 -> [3.0, 1.0]
        let w = vec![2.0, 0.5];
        assert_eq!(
            weighted_mean_l2(&matrix, Some(&w), &[0, 1], 2, 2, false),
            vec![3.0, 1.0]
        );
    }

    fn static_dir(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".codegraph/static_models")
            .join(name)
    }

    fn assert_semantically_sane(m: &StaticEmbedding, expected_dim: usize) {
        assert_eq!(m.dimension(), expected_dim);
        let v = m.embed_text("database connection pool").unwrap();
        assert_eq!(v.len(), expected_dim);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-3,
            "should be L2-normalized, norm={norm}"
        );

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

    #[test]
    fn loads_potion_f32_no_weights() {
        let dir = static_dir("potion-base-8M");
        if !dir.join("model.safetensors").exists() {
            eprintln!("skip: potion-base-8M not present");
            return;
        }
        let m = StaticEmbedding::from_pretrained(&dir).expect("load potion-base-8M");
        assert!(
            m.weights.is_none(),
            "potion-base-8M bakes weights into embeddings"
        );
        assert_semantically_sane(&m, 256);
    }

    #[test]
    fn loads_jina_code_static_f16_weighted() {
        let dir = static_dir("jina-code-static-256");
        if !dir.join("model.safetensors").exists() {
            eprintln!("skip: jina-code-static-256 not present");
            return;
        }
        let m = StaticEmbedding::from_pretrained(&dir).expect("load jina-code-static-256");
        assert!(
            m.weights.is_some(),
            "model2vec 0.7 stores SIF weights separately"
        );
        assert_semantically_sane(&m, 256);
    }
}
