// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Embedding throughput: static (model2vec) vs ONNX transformer.
//!
//! The core premise of the static-embeddings work — indexing large codebases is
//! bottlenecked on the per-symbol transformer forward pass; a static lookup
//! table removes it. This prints texts/sec for each backend and the speedup.
//!
//! Run (release is required for a fair number — the static path is pure Rust):
//!   cargo run --release -p codegraph-memory --example embed_throughput
//!
//! Needs a static model dir at `~/.codegraph/static_models/potion-base-8M`
//! (config.json + tokenizer.json + model.safetensors). BGE-small loads from the
//! existing fastembed cache (or downloads on first run).

use codegraph_memory::embedding::{CodeGraphEmbeddingModel, VectorEngine};
use std::time::Instant;

/// Synthetic but realistic "name: signature — doc" symbol texts, all unique
/// (the `(variant N)` suffix) so the engine's cache never short-circuits.
fn sample_texts(n: usize) -> Vec<String> {
    let templates = [
        "authenticate_user: fn authenticate_user(token: &Token) -> Result<User> — Verify the bearer token and return the user",
        "open_database_connection: fn open_database_connection(url: &str) -> Pool — Open a pooled postgres connection",
        "parse_config_file: fn parse_config_file(path: &Path) -> Config — Read and deserialize the TOML configuration",
        "compute_cosine_similarity: fn compute_cosine_similarity(a: &[f32], b: &[f32]) -> f32 — Dot product over norms",
        "RetryPolicy: struct RetryPolicy { max_attempts: u32, backoff: Duration } — Exponential-backoff retry config",
        "serialize_to_json: fn serialize_to_json<T: Serialize>(value: &T) -> String — Encode a value as JSON text",
        "spawn_worker_thread: fn spawn_worker_thread(queue: Arc<Queue>) -> JoinHandle — Start a background consumer",
        "validate_email_address: fn validate_email_address(input: &str) -> bool — Lenient RFC-5322 email check",
    ];
    (0..n)
        .map(|i| format!("{} (variant {i})", templates[i % templates.len()]))
        .collect()
}

/// Returns texts/sec, or None if the engine couldn't be built.
fn measure(label: &str, engine: Result<VectorEngine, impl std::fmt::Display>, texts: &[String]) -> Option<f64> {
    match engine {
        Ok(engine) => {
            let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            let t0 = Instant::now();
            let vecs = engine.embed_batch(&refs).expect("embed_batch");
            let secs = t0.elapsed().as_secs_f64();
            let sps = texts.len() as f64 / secs;
            let dim = vecs.first().map(|v| v.len()).unwrap_or(0);
            println!(
                "{label:<8} {:<30} {dim:>4}d  {sps:>10.0} texts/sec",
                engine.model_name()
            );
            Some(sps)
        }
        Err(e) => {
            println!("{label:<8} skipped ({e})");
            None
        }
    }
}

fn main() {
    let n = 512;
    let texts = sample_texts(n);
    let home = std::env::var("HOME").unwrap_or_default();
    println!("Embedding {n} unique symbol texts (cold, no cache hits).\n");

    let potion = std::path::PathBuf::from(&home).join(".codegraph/static_models/potion-base-8M");
    let static_sps = measure("static", VectorEngine::with_static_model(&potion), &texts);

    let cache = std::path::PathBuf::from(&home).join(".codegraph/fastembed_cache");
    let onnx_sps = measure(
        "onnx",
        VectorEngine::with_model(cache, CodeGraphEmbeddingModel::BgeSmall),
        &texts,
    );

    if let (Some(s), Some(o)) = (static_sps, onnx_sps) {
        println!("\nstatic is {:.1}x faster than ONNX BGE ({s:.0} vs {o:.0} texts/sec)", s / o);
    }
}
