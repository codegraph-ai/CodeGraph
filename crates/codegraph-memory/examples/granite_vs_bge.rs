//! Side-by-side eval: Granite-97M-Multilingual-R2 vs BGE-Small-EN-v1.5 on
//! the same input set. Loads both models, embeds the same set of Rust
//! functions, and reports per-pair similarity differences.
//!
//! Usage:
//!   cargo run --example granite_vs_bge --release --
//!     /path/to/file.rs [/path/to/another.rs ...]
//!
//! Output: function inventory (name, line range, char count),
//! pairwise cosine similarity matrices for both models, and a
//! delta-table highlighting rank changes between the two models for
//! a few canonical query functions.
//!
//! No tempdir gymnastics — uses fastembed's per-model cache so both
//! models can co-exist. Granite is downloaded on first run (~388 MB
//! ONNX + tokenizer files).

use std::collections::HashMap;
use std::path::PathBuf;

use codegraph_memory::{CodeGraphEmbeddingModel, VectorEngine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: granite_vs_bge <rust_file> [<rust_file> ...]");
        std::process::exit(2);
    }

    // Build inventory of (name, embed_text) from the input files.
    let mut inventory: Vec<FnEntry> = Vec::new();
    for path in &args {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to read {path}: {e}");
                continue;
            }
        };
        for entry in extract_top_level_fns(&text, path) {
            inventory.push(entry);
        }
    }
    if inventory.is_empty() {
        eprintln!("No top-level functions extracted from inputs.");
        std::process::exit(2);
    }

    println!("=== Inventory: {} functions ===", inventory.len());
    let mut max_chars = 0usize;
    let mut total_chars = 0usize;
    let mut over_2k = 0usize;
    let mut over_8k = 0usize;
    for fn_e in &inventory {
        let n = fn_e.embed_text.len();
        total_chars += n;
        if n > max_chars {
            max_chars = n;
        }
        if n > 2_000 {
            over_2k += 1;
        }
        if n > 8_000 {
            over_8k += 1;
        }
    }
    println!(
        "  total chars: {}, mean: {}, max: {}",
        total_chars,
        total_chars / inventory.len(),
        max_chars
    );
    println!(
        "  >2k chars: {}, >8k chars: {} (rough proxy for BGE truncation risk)",
        over_2k, over_8k
    );
    println!();

    // Cache dir matches the running MCP's so we don't re-download.
    let cache_dir = default_cache_dir();
    println!("Using cache dir: {}", cache_dir.display());

    // Embed with BGE-Small first (smaller download, faster).
    println!("\n=== Loading BGE-Small-EN-v1.5 ===");
    let bge = match VectorEngine::with_model(cache_dir.clone(), CodeGraphEmbeddingModel::BgeSmall) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to load BGE: {e}");
            std::process::exit(1);
        }
    };
    let bge_vecs = embed_all(&bge, &inventory, "BGE-Small");

    println!("\n=== Loading Granite-97M-Multilingual-R2 ===");
    let granite = match VectorEngine::with_model(
        cache_dir.clone(),
        CodeGraphEmbeddingModel::Granite97mMultilingualR2,
    ) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to load Granite: {e}");
            std::process::exit(1);
        }
    };
    let granite_vecs = embed_all(&granite, &inventory, "Granite-97M");

    // For each query function, print top-5 neighbors under each model.
    let queries = ["convert_whiteout", "try_hardlink_fallback", "unpack"];
    for q in &queries {
        let q_idx = match inventory.iter().position(|fn_e| fn_e.name == *q) {
            Some(i) => i,
            None => {
                println!("\n[query '{q}' not found in inventory; skipping]");
                continue;
            }
        };
        println!("\n=== Top-5 neighbors of '{}' ({}) ===", q, inventory[q_idx].location_str());
        print_topk(q_idx, &inventory, &bge_vecs, "BGE-Small", 5);
        print_topk(q_idx, &inventory, &granite_vecs, "Granite-97M", 5);
    }

    // Print pairwise comparison for a few interesting pairs.
    println!("\n=== Pairwise similarity for canonical pairs ===");
    let pairs = [
        ("convert_whiteout", "try_hardlink_fallback"),
        ("convert_whiteout", "is_whiteout"),
        ("convert_whiteout", "unpack"),
        ("try_hardlink_fallback", "unpack"),
    ];
    println!(
        "  {:35} {:35}  BGE     Granite  Δ",
        "FN A", "FN B"
    );
    for (a, b) in pairs {
        let a_idx = inventory.iter().position(|f| f.name == a);
        let b_idx = inventory.iter().position(|f| f.name == b);
        if let (Some(ai), Some(bi)) = (a_idx, b_idx) {
            let bge_sim = cosine(&bge_vecs[ai], &bge_vecs[bi]);
            let g_sim = cosine(&granite_vecs[ai], &granite_vecs[bi]);
            let delta = g_sim - bge_sim;
            let delta_marker = if delta > 0.05 {
                "▲"
            } else if delta < -0.05 {
                "▼"
            } else {
                " "
            };
            println!(
                "  {:35} {:35}  {:.3}   {:.3}   {:+.3} {}",
                truncate_str(a, 35),
                truncate_str(b, 35),
                bge_sim,
                g_sim,
                delta,
                delta_marker,
            );
        } else {
            println!("  {:35} {:35}  [missing]", a, b);
        }
    }

    // Top-K agreement (Jaccard) per query.
    println!("\n=== Top-5 Jaccard overlap (BGE vs Granite per query) ===");
    for q in &queries {
        let q_idx = match inventory.iter().position(|f| f.name == *q) {
            Some(i) => i,
            None => continue,
        };
        let bge_top = topk_indices(q_idx, &bge_vecs, 5);
        let g_top = topk_indices(q_idx, &granite_vecs, 5);
        let bge_set: std::collections::HashSet<_> = bge_top.iter().copied().collect();
        let g_set: std::collections::HashSet<_> = g_top.iter().copied().collect();
        let inter = bge_set.intersection(&g_set).count();
        let union = bge_set.union(&g_set).count();
        let jaccard = if union == 0 {
            0.0
        } else {
            inter as f64 / union as f64
        };
        println!(
            "  {:30}  intersection: {}/5  jaccard: {:.2}",
            q, inter, jaccard
        );
    }
}

#[derive(Debug, Clone)]
struct FnEntry {
    name: String,
    file: String,
    line_start: usize,
    embed_text: String,
}

impl FnEntry {
    fn location_str(&self) -> String {
        format!(
            "{}:{}",
            self.file
                .rsplit('/')
                .next()
                .unwrap_or(&self.file),
            self.line_start
        )
    }
}

/// Extract top-level `fn` definitions from a Rust source file. Skips
/// trait method declarations (no body) and impl-block methods (those
/// require deeper parsing — the runc/image-rs canonicals we care
/// about are top-level free functions).
fn extract_top_level_fns(text: &str, file_path: &str) -> Vec<FnEntry> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut line_no = 1usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line_no += 1;
            i += 1;
            continue;
        }
        // Find next `fn ` with whitespace boundary.
        if i + 3 <= bytes.len()
            && &bytes[i..i + 3] == b"fn "
            && (i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n'))
        {
            // Parse function name. Strip generic params (e.g.
            // `try_hardlink_fallback<R: AsyncRead + Unpin>` →
            // `try_hardlink_fallback`) so inventory lookup by bare
            // identifier works.
            let after_fn = i + 3;
            let name_end = match text[after_fn..].find('(') {
                Some(p) => after_fn + p,
                None => {
                    i += 3;
                    continue;
                }
            };
            let raw_name = text[after_fn..name_end].trim();
            let name = match raw_name.find('<') {
                Some(p) => raw_name[..p].trim().to_string(),
                None => raw_name.to_string(),
            };
            // Find paren-balanced params, then body open `{`.
            let params_start = name_end;
            let params_end = match find_matching(&bytes, params_start, b'(', b')') {
                Some(p) => p,
                None => {
                    i = after_fn;
                    continue;
                }
            };
            let body_open = match text[params_end + 1..].find('{') {
                Some(p) => params_end + 1 + p,
                None => {
                    i = after_fn;
                    continue;
                }
            };
            let body_end = match find_matching(&bytes, body_open, b'{', b'}') {
                Some(p) => p,
                None => {
                    i = after_fn;
                    continue;
                }
            };
            // Embed text: signature + body.
            let signature = text[i..body_open].trim().to_string();
            let body = &text[body_open..=body_end];
            let embed_text = format!("{}\n{}", signature, body);
            out.push(FnEntry {
                name: name.clone(),
                file: file_path.to_string(),
                line_start: line_no,
                embed_text,
            });
            // Advance past body.
            let advance = body_end + 1;
            line_no += text[i..advance].bytes().filter(|&b| b == b'\n').count();
            i = advance;
            continue;
        }
        i += 1;
    }
    out
}

fn find_matching(bytes: &[u8], open_pos: usize, open: u8, close: u8) -> Option<usize> {
    if bytes.get(open_pos) != Some(&open) {
        return None;
    }
    let mut depth: i32 = 1;
    let mut i = open_pos + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn embed_all(engine: &VectorEngine, inventory: &[FnEntry], label: &str) -> Vec<Vec<f32>> {
    let texts: Vec<&str> = inventory.iter().map(|f| f.embed_text.as_str()).collect();
    let start = std::time::Instant::now();
    let vecs = engine
        .embed_batch(&texts)
        .expect("embed_batch failed");
    let elapsed = start.elapsed();
    println!(
        "  {} embedded {} fns in {:.2}s ({} dim)",
        label,
        vecs.len(),
        elapsed.as_secs_f64(),
        vecs.first().map(|v| v.len()).unwrap_or(0),
    );
    vecs
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na <= 0.0 || nb <= 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

fn topk_indices(query_idx: usize, vecs: &[Vec<f32>], k: usize) -> Vec<usize> {
    let mut sims: Vec<(usize, f32)> = vecs
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != query_idx)
        .map(|(i, v)| (i, cosine(&vecs[query_idx], v)))
        .collect();
    sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sims.into_iter().take(k).map(|(i, _)| i).collect()
}

fn print_topk(
    query_idx: usize,
    inventory: &[FnEntry],
    vecs: &[Vec<f32>],
    label: &str,
    k: usize,
) {
    let mut sims: Vec<(usize, f32)> = vecs
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != query_idx)
        .map(|(i, v)| (i, cosine(&vecs[query_idx], v)))
        .collect();
    sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    println!("  {}:", label);
    for (rank, (idx, sim)) in sims.iter().take(k).enumerate() {
        println!(
            "    #{}  {:.3}  {} ({})",
            rank + 1,
            sim,
            inventory[*idx].name,
            inventory[*idx].location_str()
        );
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max - 3])
    } else {
        s.to_string()
    }
}

fn default_cache_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".codegraph").join("fastembed_cache")
    } else {
        PathBuf::from(".fastembed_cache")
    }
}

#[allow(dead_code)]
fn _placate_unused_imports(_: HashMap<u8, u8>) {}
