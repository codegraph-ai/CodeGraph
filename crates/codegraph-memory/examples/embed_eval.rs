// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Real-corpus retrieval eval: static (model2vec) vs ONNX BGE baseline, scored
//! both **pure-semantic** and **hybrid (40% BM25 + 60% semantic)** — the latter
//! mirrors the real `symbol_search` blend, which is what actually ships.
//!
//! Reads a `[{id, signature, doc}]` corpus (from `scripts/extract_eval_corpus.py`):
//! embed `"id: signature"` per symbol, use each symbol's `doc` as the query,
//! target = that symbol. Same task for every embedder → fair comparison.
//!
//!   python scripts/extract_eval_corpus.py crates /tmp/codegraph_eval_corpus.json
//!   CODEGRAPH_STATIC_MODEL=~/.codegraph/static_models/jina-code-static-256 \
//!     cargo run --release -p codegraph-memory --example embed_eval
//!
//! Env: CODEGRAPH_EVAL_CORPUS (path), CODEGRAPH_SPLIT_IDS=1 (prepend split name).

use codegraph_memory::embedding::{CodeGraphEmbeddingModel, VectorEngine};
use serde::Deserialize;
use std::collections::HashMap;

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

/// Split an identifier into camelCase/snake_case words (lowercased).
fn split_id(id: &str) -> String {
    let mut out = String::new();
    let mut prev_upper = false;
    for ch in id.chars() {
        if ch == '_' || ch == '-' {
            out.push(' ');
        } else if ch.is_uppercase() && !out.is_empty() && !prev_upper {
            out.push(' ');
            out.extend(ch.to_lowercase());
        } else {
            out.extend(ch.to_lowercase());
        }
        prev_upper = ch.is_uppercase();
    }
    out
}

/// Tokenize on non-alphanumerics + camelCase, lowercase, drop 1-char tokens.
fn tokenize(s: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut prev_upper = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && !cur.is_empty() && !prev_upper {
                toks.push(std::mem::take(&mut cur));
            }
            cur.extend(ch.to_lowercase());
            prev_upper = ch.is_uppercase();
        } else {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            prev_upper = false;
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks.retain(|t| t.len() >= 2);
    toks
}

/// Standard BM25 over the symbol texts, with per-doc term frequencies precomputed.
struct Bm25 {
    tf: Vec<HashMap<String, usize>>,
    dl: Vec<f64>,
    df: HashMap<String, usize>,
    avgdl: f64,
    n: f64,
}

impl Bm25 {
    fn new(texts: &[String]) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut tf = Vec::with_capacity(texts.len());
        let mut dl = Vec::with_capacity(texts.len());
        for t in texts {
            let toks = tokenize(t);
            let mut m: HashMap<String, usize> = HashMap::new();
            for tok in &toks {
                *m.entry(tok.clone()).or_insert(0) += 1;
            }
            for k in m.keys() {
                *df.entry(k.clone()).or_insert(0) += 1;
            }
            dl.push(toks.len() as f64);
            tf.push(m);
        }
        let n = texts.len() as f64;
        let avgdl = dl.iter().sum::<f64>() / n.max(1.0);
        Self { tf, dl, df, avgdl, n }
    }

    fn score(&self, q: &[String], i: usize) -> f64 {
        let (k1, b) = (1.2_f64, 0.75_f64);
        let mut s = 0.0;
        for term in q {
            let f = *self.tf[i].get(term).unwrap_or(&0) as f64;
            if f == 0.0 {
                continue;
            }
            let df = *self.df.get(term).unwrap_or(&0) as f64;
            let idf = ((self.n - df + 0.5) / (df + 0.5) + 1.0).ln();
            s += idf * (f * (k1 + 1.0)) / (f + k1 * (1.0 - b + b * self.dl[i] / self.avgdl));
        }
        s
    }
}

fn minmax(v: &mut [f64]) {
    let (mn, mx) = v
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let r = mx - mn;
    for x in v.iter_mut() {
        *x = if r > 0.0 { (*x - mn) / r } else { 0.0 };
    }
}

fn rank_of(target: usize, scores: &[f64]) -> usize {
    let t = scores[target];
    1 + scores
        .iter()
        .enumerate()
        .filter(|(j, &s)| *j != target && s > t)
        .count()
}

fn metrics(ranks: &[usize]) -> Scores {
    let n = ranks.len() as f64;
    let (mut r1, mut r5, mut r10, mut mrr) = (0.0, 0.0, 0.0, 0.0);
    for &r in ranks {
        if r <= 1 {
            r1 += 1.0;
        }
        if r <= 5 {
            r5 += 1.0;
        }
        if r <= 10 {
            r10 += 1.0;
        }
        mrr += 1.0 / r as f64;
    }
    Scores {
        r1: r1 / n,
        r5: r5 / n,
        r10: r10 / n,
        mrr: mrr / n,
    }
}

/// Returns (semantic-only, hybrid 0.4*BM25 + 0.6*cosine) scores.
fn evaluate(engine: &VectorEngine, syms: &[Sym]) -> (Scores, Scores) {
    let split = std::env::var("CODEGRAPH_SPLIT_IDS").is_ok();
    let sym_texts: Vec<String> = syms
        .iter()
        .map(|s| {
            if split {
                format!("{} {}: {}", split_id(&s.id), s.id, s.signature)
            } else {
                format!("{}: {}", s.id, s.signature)
            }
        })
        .collect();
    let sym_refs: Vec<&str> = sym_texts.iter().map(|s| s.as_str()).collect();
    let sym_vecs = engine.embed_batch(&sym_refs).expect("embed symbols");
    let q_refs: Vec<&str> = syms.iter().map(|s| s.doc.as_str()).collect();
    let q_vecs = engine.embed_batch(&q_refs).expect("embed queries");

    let bm25 = Bm25::new(&sym_texts);
    let mut sem_ranks = Vec::with_capacity(syms.len());
    let mut hyb_ranks = Vec::with_capacity(syms.len());

    for (i, qv) in q_vecs.iter().enumerate() {
        let mut cos: Vec<f64> = sym_vecs
            .iter()
            .map(|sv| engine.similarity(qv, sv) as f64)
            .collect();
        sem_ranks.push(rank_of(i, &cos));

        let qterms = tokenize(&syms[i].doc);
        let mut bm: Vec<f64> = (0..sym_texts.len()).map(|j| bm25.score(&qterms, j)).collect();
        minmax(&mut bm);
        minmax(&mut cos);
        let hyb: Vec<f64> = bm.iter().zip(&cos).map(|(b, c)| 0.4 * b + 0.6 * c).collect();
        hyb_ranks.push(rank_of(i, &hyb));
    }

    (metrics(&sem_ranks), metrics(&hyb_ranks))
}

fn report(label: &str, engine: Result<VectorEngine, impl std::fmt::Display>, syms: &[Sym]) {
    match engine {
        Ok(engine) => {
            let (sem, hyb) = evaluate(&engine, syms);
            println!(
                "{label:<8} {:<28} semantic  R@1 {:.3}  R@5 {:.3}  R@10 {:.3}  MRR {:.3}",
                engine.model_name(),
                sem.r1,
                sem.r5,
                sem.r10,
                sem.mrr
            );
            println!(
                "{:<8} {:<28} HYBRID    R@1 {:.3}  R@5 {:.3}  R@10 {:.3}  MRR {:.3}",
                "",
                "(0.4 bm25 + 0.6 cos)",
                hyb.r1,
                hyb.r5,
                hyb.r10,
                hyb.mrr
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
