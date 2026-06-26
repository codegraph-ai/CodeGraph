// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Real-corpus retrieval eval: static (model2vec) vs ONNX BGE baseline.
//!
//! Reads a `[{id, signature, doc}]` corpus (from `scripts/extract_eval_corpus.py`)
//! and runs an N-way retrieval: embed `"id: signature"` per symbol, use each
//! symbol's `doc` as the query, target = that symbol. Scored by recall@{1,5,10}
//! and MRR. The same task runs for every embedder, so the comparison is fair.
//!
//!   python scripts/extract_eval_corpus.py crates /tmp/codegraph_eval_corpus.json
//!   CODEGRAPH_STATIC_MODEL=~/.codegraph/static_models/jina-code-static-256 \
//!     cargo run --release -p codegraph-memory --example embed_eval
//!
//! Corpus path overridable via CODEGRAPH_EVAL_CORPUS.

use codegraph_memory::embedding::{CodeGraphEmbeddingModel, VectorEngine};
use serde::Deserialize;

#[derive(Deserialize)]
struct Sym {
    id: String,
    signature: String,
    doc: String,
}

struct Scores {
    r1: f64,
    r5: f64,
    r10: f64,
    mrr: f64,
}

fn evaluate(engine: &VectorEngine, syms: &[Sym]) -> Scores {
    let sym_texts: Vec<String> = syms
        .iter()
        .map(|s| format!("{}: {}", s.id, s.signature))
        .collect();
    let sym_refs: Vec<&str> = sym_texts.iter().map(|s| s.as_str()).collect();
    let sym_vecs = engine.embed_batch(&sym_refs).expect("embed symbols");

    let q_refs: Vec<&str> = syms.iter().map(|s| s.doc.as_str()).collect();
    let q_vecs = engine.embed_batch(&q_refs).expect("embed queries");

    let (mut r1, mut r5, mut r10, mut mrr) = (0.0, 0.0, 0.0, 0.0);
    for (i, qv) in q_vecs.iter().enumerate() {
        let target = engine.similarity(qv, &sym_vecs[i]);
        // rank = 1 + (# symbols strictly more similar to the query than the target)
        let mut greater = 0usize;
        for (j, sv) in sym_vecs.iter().enumerate() {
            if j != i && engine.similarity(qv, sv) > target {
                greater += 1;
            }
        }
        let rank = greater + 1;
        if rank <= 1 {
            r1 += 1.0;
        }
        if rank <= 5 {
            r5 += 1.0;
        }
        if rank <= 10 {
            r10 += 1.0;
        }
        mrr += 1.0 / rank as f64;
    }
    let n = syms.len() as f64;
    Scores {
        r1: r1 / n,
        r5: r5 / n,
        r10: r10 / n,
        mrr: mrr / n,
    }
}

fn report(label: &str, engine: Result<VectorEngine, impl std::fmt::Display>, syms: &[Sym]) {
    match engine {
        Ok(engine) => {
            let s = evaluate(&engine, syms);
            println!(
                "{label:<8} {:<30}  R@1 {:.3}  R@5 {:.3}  R@10 {:.3}  MRR {:.3}",
                engine.model_name(),
                s.r1,
                s.r5,
                s.r10,
                s.mrr
            );
        }
        Err(e) => println!("{label:<8} skipped ({e})"),
    }
}

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let corpus_path = std::env::var("CODEGRAPH_EVAL_CORPUS")
        .unwrap_or_else(|_| "/tmp/codegraph_eval_corpus.json".to_string());
    let syms: Vec<Sym> = serde_json::from_slice(
        &std::fs::read(&corpus_path).unwrap_or_else(|e| panic!("read {corpus_path}: {e}")),
    )
    .expect("parse corpus json");
    println!(
        "Retrieval eval: {} symbols (doc -> symbol), corpus {corpus_path}\n",
        syms.len()
    );

    let static_dir = std::env::var("CODEGRAPH_STATIC_MODEL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(&home).join(".codegraph/static_models/potion-base-8M"));
    report("static", VectorEngine::with_static_model(&static_dir), &syms);

    let cache = std::path::PathBuf::from(&home).join(".codegraph/fastembed_cache");
    report("onnx-bge", VectorEngine::with_model(cache, CodeGraphEmbeddingModel::BgeSmall), &syms);
}
