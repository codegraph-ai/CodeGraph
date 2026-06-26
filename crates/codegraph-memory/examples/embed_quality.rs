// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Retrieval-quality micro-eval: static (model2vec) vs ONNX transformer.
//!
//! A small, hand-built `query -> target symbol` set scored by recall@1, recall@3
//! and MRR — the Phase-0 eval in miniature. It answers, directionally: how far
//! below BGE does the *generic* static floor (`potion-base-8M`) sit? That is the
//! gap a code-distilled Jina static would need to close. NOT a substitute for
//! the full eval on a real indexed repo (12 queries is noisy) — a first signal.
//!
//!   cargo run --release -p codegraph-memory --example embed_quality
//!
//! Needs `~/.codegraph/static_models/potion-base-8M`; BGE loads from the
//! fastembed cache.

use codegraph_memory::embedding::{CodeGraphEmbeddingModel, VectorEngine};

/// (symbol id, "name: signature — doc" embed text)
const SYMBOLS: &[(&str, &str)] = &[
    ("authenticate_user", "authenticate_user: fn authenticate_user(token: &Token) -> Result<User> — Verify the bearer token and return the user"),
    ("hash_password", "hash_password: fn hash_password(plain: &str) -> String — Bcrypt-hash a plaintext password before storage"),
    ("open_database_connection", "open_database_connection: fn open_database_connection(url: &str) -> Pool — Open a pooled postgres connection"),
    ("execute_sql_query", "execute_sql_query: fn execute_sql_query(sql: &str, params: &[Value]) -> Rows — Run a parameterized SQL statement"),
    ("parse_json_request", "parse_json_request: fn parse_json_request(body: &[u8]) -> Request — Deserialize the HTTP request body as JSON"),
    ("serialize_response", "serialize_response: fn serialize_response(resp: &Response) -> String — Encode a response struct to JSON text"),
    ("compute_cosine_similarity", "compute_cosine_similarity: fn compute_cosine_similarity(a: &[f32], b: &[f32]) -> f32 — Dot product over norms"),
    ("build_hnsw_index", "build_hnsw_index: fn build_hnsw_index(vectors: &[Vec<f32>]) -> Hnsw — Construct an approximate nearest-neighbor index"),
    ("retry_with_backoff", "retry_with_backoff: fn retry_with_backoff(op: impl Fn() -> Result<T>) -> T — Retry a fallible op with exponential backoff"),
    ("rate_limiter", "rate_limiter: struct RateLimiter { tokens: u32 } — Token-bucket throttling of incoming requests"),
    ("parse_config_file", "parse_config_file: fn parse_config_file(path: &Path) -> Config — Read and deserialize the TOML configuration file"),
    ("watch_file_changes", "watch_file_changes: fn watch_file_changes(dir: &Path) -> Receiver<Event> — Notify on filesystem modifications"),
    ("spawn_worker_pool", "spawn_worker_pool: fn spawn_worker_pool(n: usize, q: Arc<Queue>) -> Vec<JoinHandle> — Start N background consumer threads"),
    ("validate_email", "validate_email: fn validate_email(input: &str) -> bool — Lenient RFC-5322 email-address check"),
    ("encode_jwt_token", "encode_jwt_token: fn encode_jwt_token(claims: &Claims, secret: &[u8]) -> String — Sign a JSON Web Token with claims"),
    ("cache_get_or_insert", "cache_get_or_insert: fn cache_get_or_insert(key: K, f: impl Fn() -> V) -> V — Memoized lookup with fallback compute"),
];

/// (natural-language query, target symbol id)
const QUERIES: &[(&str, &str)] = &[
    ("check if a login token is valid", "authenticate_user"),
    ("store a user's password securely", "hash_password"),
    ("connect to the database", "open_database_connection"),
    ("run a SQL statement against the db", "execute_sql_query"),
    ("turn the request body into an object", "parse_json_request"),
    ("find the nearest vectors quickly", "build_hnsw_index"),
    ("measure how similar two embeddings are", "compute_cosine_similarity"),
    ("try the operation again if it fails", "retry_with_backoff"),
    ("limit how many requests per second", "rate_limiter"),
    ("load settings from a config file", "parse_config_file"),
    ("sign a json web token", "encode_jwt_token"),
    ("check an email address is well formed", "validate_email"),
];

struct Scores {
    recall_at_1: f64,
    recall_at_3: f64,
    mrr: f64,
}

fn evaluate(engine: &VectorEngine) -> Scores {
    let sym_texts: Vec<&str> = SYMBOLS.iter().map(|(_, t)| *t).collect();
    let sym_vecs = engine.embed_batch(&sym_texts).expect("embed symbols");

    let (mut r1, mut r3, mut mrr) = (0.0, 0.0, 0.0);
    for (query, target) in QUERIES {
        let qv = engine.embed(query).expect("embed query");
        // Rank symbols by cosine similarity to the query.
        let mut ranked: Vec<(usize, f32)> = sym_vecs
            .iter()
            .enumerate()
            .map(|(i, sv)| (i, engine.similarity(&qv, sv)))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let rank = 1 + ranked.iter().position(|(i, _)| SYMBOLS[*i].0 == *target).unwrap();
        if rank == 1 {
            r1 += 1.0;
        }
        if rank <= 3 {
            r3 += 1.0;
        }
        mrr += 1.0 / rank as f64;
    }
    let n = QUERIES.len() as f64;
    Scores {
        recall_at_1: r1 / n,
        recall_at_3: r3 / n,
        mrr: mrr / n,
    }
}

fn report(label: &str, engine: Result<VectorEngine, impl std::fmt::Display>) {
    match engine {
        Ok(engine) => {
            let s = evaluate(&engine);
            println!(
                "{label:<8} {:<30}  R@1 {:.2}   R@3 {:.2}   MRR {:.3}",
                engine.model_name(),
                s.recall_at_1,
                s.recall_at_3,
                s.mrr
            );
        }
        Err(e) => println!("{label:<8} skipped ({e})"),
    }
}

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    println!(
        "Retrieval micro-eval: {} queries over {} symbols.\n",
        QUERIES.len(),
        SYMBOLS.len()
    );

    // Static model dir: override with CODEGRAPH_STATIC_MODEL (e.g. a distilled
    // jina-code-static-256), else the potion-base-8M floor.
    let static_dir = std::env::var("CODEGRAPH_STATIC_MODEL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(&home).join(".codegraph/static_models/potion-base-8M"));
    report("static", VectorEngine::with_static_model(&static_dir));

    let cache = std::path::PathBuf::from(&home).join(".codegraph/fastembed_cache");
    report("onnx", VectorEngine::with_model(cache, CodeGraphEmbeddingModel::BgeSmall));
}
