// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AI Query Engine
//!
//! Main query engine that provides fast, composable query primitives for AI agents.
//! Integrates with CodeGraph for graph-based code intelligence.

use super::primitives::{
    truncate_string, CallInfo, ClusterMember, ClusterResult, DetailedSymbolInfo, DuplicatePair,
    DuplicateResult, EntryPoint, EntryType, ImplementorInfo, ImportMatchMode, ImportSearchOptions,
    SearchOptions, SignaturePattern, StructuralComparison, SymbolCluster, SymbolComparison,
    SymbolInfo, SymbolLocation, SymbolMatch, SymbolSearchResult, SymbolType, TraversalDirection,
    TraversalFilter, TraversalNode, MAX_SIGNATURE_LENGTH,
};
use super::text_index::{TextIndex, TextIndexBuilder};
use crate::domain::node_props;
use codegraph::{CodeGraph, Direction, EdgeType, NodeId, NodeType};
use codegraph_memory::VectorEngine;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Weight for BM25 score in hybrid search (0-1)
const BM25_WEIGHT: f32 = 0.4;
/// Weight for semantic similarity score in hybrid search (0-1)
const SEMANTIC_WEIGHT: f32 = 0.6;

/// AI Query Engine for fast code exploration.
pub struct QueryEngine {
    /// Reference to the code graph
    graph: Arc<RwLock<CodeGraph>>,
    /// Text index for fast symbol search
    text_index: Arc<RwLock<TextIndex>>,
    /// Import index: library name -> importing files
    import_index: Arc<RwLock<HashMap<String, Vec<NodeId>>>>,
    /// Caller index: function -> list of callers
    caller_index: Arc<RwLock<HashMap<NodeId, Vec<NodeId>>>>,
    /// Callee index: function -> list of callees
    callee_index: Arc<RwLock<HashMap<NodeId, Vec<NodeId>>>>,
    /// Shared vector engine for semantic embedding (set after memory init)
    vector_engine: Arc<RwLock<Option<Arc<VectorEngine>>>>,
    /// Symbol embeddings: NodeId -> 768-dim vector (Jina Code V2)
    symbol_vectors: Arc<RwLock<HashMap<NodeId, Vec<f32>>>>,
    /// Symbol text used for embedding (for rebuilding)
    symbol_texts: Arc<RwLock<HashMap<NodeId, String>>>,
    /// Embed full function body (true) or just name+signature (false, default)
    full_body_embedding: std::sync::atomic::AtomicBool,
}

/// Max characters of function body for full-body embedding.
/// ~512 tokens ≈ first 40-50 lines of code.
const FULL_BODY_MAX_CHARS: usize = 2048;

impl QueryEngine {
    /// Create a new query engine with the given graph.
    pub fn new(graph: Arc<RwLock<CodeGraph>>) -> Self {
        Self {
            graph,
            text_index: Arc::new(RwLock::new(TextIndex::new())),
            import_index: Arc::new(RwLock::new(HashMap::new())),
            caller_index: Arc::new(RwLock::new(HashMap::new())),
            callee_index: Arc::new(RwLock::new(HashMap::new())),
            vector_engine: Arc::new(RwLock::new(None)),
            symbol_vectors: Arc::new(RwLock::new(HashMap::new())),
            symbol_texts: Arc::new(RwLock::new(HashMap::new())),
            full_body_embedding: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Enable or disable full-body embedding mode.
    pub fn set_full_body_embedding(&self, enabled: bool) {
        self.full_body_embedding
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Build the embedding text for a symbol node.
    /// In signature mode: "name: signature — docstring"
    /// In full-body mode: "name: signature\n<truncated source body>"
    fn build_embed_text(
        node: &codegraph::Node,
        node_id: NodeId,
        name: &str,
        full_body: bool,
        graph: &CodeGraph,
    ) -> String {
        let signature = node.properties.get_string("signature").unwrap_or("");
        let docstring = node.properties.get_string("doc").unwrap_or("");

        // Base: name + signature + docstring (always included)
        let base = if !docstring.is_empty() && !signature.is_empty() {
            format!("{name}: {signature} — {docstring}")
        } else if !signature.is_empty() {
            format!("{name}: {signature}")
        } else if !docstring.is_empty() {
            format!("{name} — {docstring}")
        } else {
            name.to_string()
        };

        if !full_body {
            return base;
        }

        // Full-body: prefer body_prefix from graph (captured at parse time, no disk I/O)
        // Fall back to disk read only if body_prefix is not available
        let body_text = node
            .properties
            .get_string("body_prefix")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| crate::domain::source_code::get_symbol_source(graph, node_id));

        match body_text {
            Some(body) if body.len() > base.len() + 10 => {
                let truncated = if body.len() > FULL_BODY_MAX_CHARS {
                    &body[..FULL_BODY_MAX_CHARS]
                } else {
                    &body
                };
                format!("{base}\n{truncated}")
            }
            _ => base, // fallback to signature mode if source unavailable
        }
    }

    /// Build indexes from the current graph state.
    /// Should be called after initial parsing or reindexing.
    pub async fn build_indexes(&self) {
        let graph = self.graph.read().await;

        // Build text index
        let mut text_builder = TextIndexBuilder::new();
        let mut import_map: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut caller_map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut callee_map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // Iterate over all nodes using iter_nodes()
        for (node_id, node) in graph.iter_nodes() {
            let name = node_props::name(node).to_string();
            let docstring = node.properties.get_string("doc").map(|s| s.to_string());

            // Add to text index
            text_builder.add_document(node_id, &name, docstring.as_deref(), &[]);

            // Build import index from Imports edges
            if let Ok(neighbors) = graph.get_neighbors(node_id, Direction::Outgoing) {
                for neighbor_id in neighbors {
                    if let Ok(edges) = graph.get_edges_between(node_id, neighbor_id) {
                        for edge_id in edges {
                            if let Ok(edge) = graph.get_edge(edge_id) {
                                match edge.edge_type {
                                    EdgeType::Imports => {
                                        // Get the imported module name
                                        if let Ok(target_node) = graph.get_node(neighbor_id) {
                                            let module_name =
                                                node_props::name(target_node).to_string();
                                            if !module_name.is_empty() {
                                                import_map
                                                    .entry(module_name)
                                                    .or_default()
                                                    .push(node_id);
                                            }
                                        }
                                    }
                                    EdgeType::Calls => {
                                        // Build caller/callee indexes
                                        callee_map.entry(node_id).or_default().push(neighbor_id);
                                        caller_map.entry(neighbor_id).or_default().push(node_id);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        // Store built indexes
        *self.text_index.write().await = text_builder.build();
        *self.import_index.write().await = import_map;
        *self.caller_index.write().await = caller_map;
        *self.callee_index.write().await = callee_map;
    }

    /// Set the shared vector engine for semantic search.
    /// Called after MemoryManager initialization provides the engine.
    pub async fn set_vector_engine(&self, engine: Arc<VectorEngine>) {
        *self.vector_engine.write().await = Some(engine);
    }

    /// Build symbol embedding vectors for semantic search.
    /// Requires vector_engine to be set first. Embeds name + signature + docstring
    /// for each symbol in batch for efficiency.
    pub async fn build_symbol_vectors(&self) {
        let engine = match self.vector_engine.read().await.clone() {
            Some(e) => e,
            None => {
                tracing::warn!("[QueryEngine] No vector engine available, skipping symbol vectors");
                return;
            }
        };

        let start = Instant::now();
        let graph = self.graph.read().await;

        // Collect symbol texts for embedding
        let mut node_ids = Vec::new();
        let mut texts = Vec::new();
        let mut text_map = HashMap::new();

        for (node_id, node) in graph.iter_nodes() {
            // Only embed meaningful symbol types
            if !matches!(
                node.node_type,
                NodeType::Function
                    | NodeType::Class
                    | NodeType::Variable
                    | NodeType::Interface
                    | NodeType::Type
            ) {
                continue;
            }

            let name = node_props::name(node);
            if name.is_empty() || name == "arrow_function" || name == "anonymous" {
                continue;
            }

            // Build embedding text
            let embed_text = Self::build_embed_text(
                node,
                node_id,
                name,
                self.full_body_embedding
                    .load(std::sync::atomic::Ordering::Relaxed),
                &graph,
            );

            node_ids.push(node_id);
            texts.push(embed_text.clone());
            text_map.insert(node_id, embed_text);
        }

        drop(graph); // Release graph lock before embedding

        if texts.is_empty() {
            return;
        }

        let model_name = engine.model_name();
        tracing::info!(
            "[QueryEngine] Embedding {} symbols ({})...",
            texts.len(),
            model_name,
        );

        // Embed in small chunks to limit peak memory usage.
        // ONNX Runtime allocates intermediate tensors proportional to batch size × token count.
        // Signature mode (~20 tokens/item): batch 64 is fine.
        // Full-body mode (~500 tokens/item): reduce batch to 16 to stay within memory.
        let is_full_body = self
            .full_body_embedding
            .load(std::sync::atomic::Ordering::Relaxed);
        let chunk_size: usize = if is_full_body { 16 } else { 64 };
        let mut symbol_vecs = HashMap::with_capacity(texts.len());
        let total_chunks = texts.len().div_ceil(chunk_size);

        for (chunk_idx, chunk_start) in (0..texts.len()).step_by(chunk_size).enumerate() {
            let chunk_end = (chunk_start + chunk_size).min(texts.len());
            let chunk_refs: Vec<&str> = texts[chunk_start..chunk_end]
                .iter()
                .map(|s| s.as_str())
                .collect();

            match engine.embed_batch(&chunk_refs) {
                Ok(vectors) => {
                    for (i, vec) in vectors.into_iter().enumerate() {
                        symbol_vecs.insert(node_ids[chunk_start + i], vec);
                    }
                    if total_chunks > 1 && (chunk_idx + 1) % 10 == 0 {
                        tracing::info!(
                            "[QueryEngine] Embedded chunk {}/{} ({} symbols so far)",
                            chunk_idx + 1,
                            total_chunks,
                            symbol_vecs.len()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "[QueryEngine] Failed to embed chunk {}/{}: {:?}",
                        chunk_idx + 1,
                        total_chunks,
                        e
                    );
                }
            }
        }

        let count = symbol_vecs.len();
        *self.symbol_vectors.write().await = symbol_vecs;
        *self.symbol_texts.write().await = text_map;
        tracing::info!(
            "[QueryEngine] Built {} symbol vectors in {:?}",
            count,
            start.elapsed()
        );
    }

    /// Persist symbol vectors to RocksDB alongside the graph.
    ///
    /// Each vector is stored as key `vec:{node_id}` → binary `[f32]` (little-endian).
    /// Uses the namespaced backend so vectors are scoped per project.
    pub async fn save_symbol_vectors(&self, slug: &str) -> std::result::Result<(), String> {
        use codegraph::{NamespacedBackend, RocksDBBackend, StorageBackend};

        let vecs = self.symbol_vectors.read().await;
        if vecs.is_empty() {
            return Ok(());
        }

        let db_path = crate::memory::shared_graph_db_path().map_err(|e| format!("{e}"))?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create ~/.codegraph: {e}"))?;
        }

        let rocks =
            RocksDBBackend::open(&db_path).map_err(|e| format!("Failed to open graph.db: {e}"))?;
        let mut namespaced = NamespacedBackend::new(Box::new(rocks), slug);

        // Write each vector as binary f32 array
        for (&node_id, vec) in vecs.iter() {
            let key = format!("vec:{node_id}");
            let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
            namespaced
                .put(key.as_bytes(), &bytes)
                .map_err(|e| format!("Failed to write vector: {e}"))?;
        }

        tracing::info!(
            "[QueryEngine] Saved {} symbol vectors to graph.db (namespace: {})",
            vecs.len(),
            slug
        );
        Ok(())
    }

    /// Load persisted symbol vectors from RocksDB.
    ///
    /// Scans for keys prefixed with `vec:` in the project namespace.
    /// Returns the number of vectors loaded, or 0 if none found.
    pub async fn load_symbol_vectors(&self, slug: &str) -> usize {
        use codegraph::{NamespacedBackend, RocksDBBackend, StorageBackend};

        let db_path = match crate::memory::shared_graph_db_path() {
            Ok(p) => p,
            Err(_) => return 0,
        };

        if !db_path.exists() {
            return 0;
        }

        let rocks = match RocksDBBackend::open(&db_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[QueryEngine] Failed to open graph.db for vectors: {}", e);
                return 0;
            }
        };
        let namespaced = NamespacedBackend::new(Box::new(rocks), slug);

        let entries = match namespaced.scan_prefix(b"vec:") {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("[QueryEngine] Failed to scan vectors: {}", e);
                return 0;
            }
        };

        let mut symbol_vecs = self.symbol_vectors.write().await;
        let mut loaded = 0;

        for (key, value) in entries {
            // Key format: "vec:{node_id}" (already stripped of namespace prefix by NamespacedBackend)
            let key_str = match std::str::from_utf8(&key) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let node_id_str = match key_str.strip_prefix("vec:") {
                Some(s) => s,
                None => continue,
            };
            let node_id = match node_id_str.parse::<NodeId>() {
                Ok(id) => id,
                Err(_) => continue,
            };

            // Decode binary f32 array (little-endian, 4 bytes per float)
            if value.len() % 4 != 0 {
                continue;
            }
            let vec: Vec<f32> = value
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();

            symbol_vecs.insert(node_id, vec);
            loaded += 1;
        }

        tracing::info!(
            "[QueryEngine] Loaded {} symbol vectors from graph.db (namespace: {})",
            loaded,
            slug
        );
        loaded
    }

    /// Remove vectors for symbols from a deleted file.
    pub async fn remove_file_vectors(&self, file_path: &str) {
        let graph = self.graph.read().await;
        let mut symbol_vecs = self.symbol_vectors.write().await;
        let mut symbol_texts = self.symbol_texts.write().await;

        // Find node IDs that belonged to this file path and remove their vectors
        let mut removed = 0;
        let stale_ids: Vec<NodeId> = symbol_vecs
            .keys()
            .copied()
            .filter(|&node_id| {
                graph
                    .get_node(node_id)
                    .map(|n| {
                        n.properties
                            .get_string("path")
                            .map(|p| p == file_path)
                            .unwrap_or(false)
                    })
                    // Node was deleted from graph — its vector is stale
                    .unwrap_or(true)
            })
            .collect();

        // Only remove vectors for nodes that are actually gone from the graph
        for node_id in &stale_ids {
            if graph.get_node(*node_id).is_err() {
                symbol_vecs.remove(node_id);
                symbol_texts.remove(node_id);
                removed += 1;
            }
        }

        if removed > 0 {
            tracing::info!(
                "[QueryEngine] Removed {} stale vectors for {}",
                removed,
                file_path
            );
        }
    }

    /// Re-embed only symbols from a specific file path.
    /// Called on did_save to incrementally update embeddings without rebuilding all.
    pub async fn update_file_vectors(&self, file_path: &str) {
        let engine = match self.vector_engine.read().await.clone() {
            Some(e) => e,
            None => {
                tracing::debug!(
                    "[QueryEngine] No vector engine, skipping file embedding for {}",
                    file_path
                );
                return;
            }
        };

        let graph = self.graph.read().await;

        // Collect symbols from this file only
        let mut node_ids = Vec::new();
        let mut texts = Vec::new();

        for (node_id, node) in graph.iter_nodes() {
            if !matches!(
                node.node_type,
                NodeType::Function
                    | NodeType::Class
                    | NodeType::Variable
                    | NodeType::Interface
                    | NodeType::Type
            ) {
                continue;
            }

            let path = node_props::path(node);
            if !path.ends_with(file_path) && !file_path.ends_with(path) {
                continue;
            }

            let name = node_props::name(node);
            if name.is_empty() || name == "arrow_function" || name == "anonymous" {
                continue;
            }

            let embed_text = Self::build_embed_text(
                node,
                node_id,
                name,
                self.full_body_embedding
                    .load(std::sync::atomic::Ordering::Relaxed),
                &graph,
            );

            node_ids.push(node_id);
            texts.push(embed_text);
        }

        drop(graph);

        if texts.is_empty() {
            return;
        }

        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        match engine.embed_batch(&text_refs) {
            Ok(vectors) => {
                let mut symbol_vecs = self.symbol_vectors.write().await;
                let mut symbol_texts = self.symbol_texts.write().await;
                for (i, vec) in vectors.into_iter().enumerate() {
                    symbol_vecs.insert(node_ids[i], vec);
                    symbol_texts.insert(node_ids[i], texts[i].clone());
                }
                tracing::info!(
                    "[QueryEngine] Re-embedded {} symbols from {}",
                    node_ids.len(),
                    file_path
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[QueryEngine] Failed to re-embed file {}: {:?}",
                    file_path,
                    e
                );
            }
        }
    }

    /// Search for symbols by name, docstring, or comments.
    /// Uses hybrid BM25 + semantic scoring when vector engine is available.
    pub async fn symbol_search(&self, query: &str, options: &SearchOptions) -> SymbolSearchResult {
        let start = Instant::now();

        let text_index = self.text_index.read().await;
        let graph = self.graph.read().await;

        // Fetch more candidates when type-filtering to avoid missing targets ranked lower in BM25
        let fetch_multiplier = if options.symbol_types.is_empty() {
            2
        } else {
            10
        };
        let text_results = text_index.search(query, options.limit * fetch_multiplier);

        // Compute semantic scores if vector engine is available
        let semantic_scores = self.compute_semantic_scores(query).await;
        let has_semantic = !semantic_scores.is_empty();

        // Find max BM25 score for normalization
        let max_bm25 = text_results
            .iter()
            .map(|r| r.score)
            .fold(0.0f32, f32::max)
            .max(0.001); // avoid division by zero

        // Merge BM25 candidates with semantic-only candidates
        let mut all_candidate_ids: HashSet<NodeId> =
            text_results.iter().map(|r| r.node_id).collect();
        if has_semantic {
            // Add top semantic candidates that BM25 missed (the key value of semantic search)
            let mut semantic_sorted: Vec<_> = semantic_scores.iter().collect();
            semantic_sorted
                .sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (node_id, _) in semantic_sorted
                .iter()
                .take(options.limit * fetch_multiplier)
            {
                all_candidate_ids.insert(**node_id);
            }
        }

        // Build BM25 score lookup
        let bm25_scores: HashMap<NodeId, f32> =
            text_results.iter().map(|r| (r.node_id, r.score)).collect();
        let bm25_reasons: HashMap<NodeId, &str> = text_results
            .iter()
            .map(|r| {
                (
                    r.node_id,
                    match r.match_reason {
                        super::text_index::MatchReason::SymbolName => "SymbolName",
                        super::text_index::MatchReason::Docstring => "Docstring",
                        super::text_index::MatchReason::Comment => "Comment",
                        super::text_index::MatchReason::Multiple => "Multiple",
                    },
                )
            })
            .collect();

        let total_matches = all_candidate_ids.len();

        // Score and filter all candidates
        let mut scored_results = Vec::new();
        for &node_id in &all_candidate_ids {
            if let Ok(node) = graph.get_node(node_id) {
                // Apply symbol type filter
                if !options.symbol_types.is_empty() {
                    let node_type_matches = options.symbol_types.iter().any(|st| {
                        matches!(
                            (st, &node.node_type),
                            (SymbolType::Function, NodeType::Function)
                                | (SymbolType::Class, NodeType::Class)
                                | (SymbolType::Variable, NodeType::Variable)
                                | (SymbolType::Module, NodeType::Module)
                                | (SymbolType::Interface, NodeType::Interface)
                                | (SymbolType::Type, NodeType::Type)
                        )
                    });
                    if !node_type_matches {
                        continue;
                    }
                }

                let symbol_info = self.node_to_symbol_info_opts(&graph, node_id, options.compact);
                if let Some(symbol) = symbol_info {
                    if !options.include_private && !symbol.is_public {
                        continue;
                    }

                    // Compute hybrid score
                    let bm25_norm = bm25_scores.get(&node_id).copied().unwrap_or(0.0) / max_bm25;
                    let semantic_sim = semantic_scores.get(&node_id).copied().unwrap_or(0.0);

                    let score = if has_semantic {
                        BM25_WEIGHT * bm25_norm + SEMANTIC_WEIGHT * semantic_sim
                    } else {
                        bm25_norm // pure BM25 fallback
                    };

                    let match_reason = if let Some(&reason) = bm25_reasons.get(&node_id) {
                        if has_semantic && semantic_sim > 0.3 && bm25_norm < 0.01 {
                            "Semantic".to_string()
                        } else {
                            reason.to_string()
                        }
                    } else {
                        "Semantic".to_string()
                    };

                    scored_results.push(SymbolMatch {
                        node_id,
                        symbol,
                        score,
                        match_reason,
                    });
                }
            }
        }

        // Sort by hybrid score descending
        scored_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored_results.truncate(options.limit);

        let query_time_ms = start.elapsed().as_millis() as u64;

        SymbolSearchResult {
            results: scored_results,
            total_matches,
            query_time_ms,
        }
    }

    /// Compute semantic similarity scores for all indexed symbols against a query.
    /// Returns empty map if vector engine or symbol vectors aren't available.
    async fn compute_semantic_scores(&self, query: &str) -> HashMap<NodeId, f32> {
        let engine = match self.vector_engine.read().await.clone() {
            Some(e) => e,
            None => return HashMap::new(),
        };

        let symbol_vecs = self.symbol_vectors.read().await;
        if symbol_vecs.is_empty() {
            return HashMap::new();
        }

        // Embed the query
        let query_vec = match engine.embed(query) {
            Ok(v) => v,
            Err(_) => return HashMap::new(),
        };

        // Brute-force cosine similarity against all symbol vectors
        let mut scores = HashMap::with_capacity(symbol_vecs.len());
        for (&node_id, symbol_vec) in symbol_vecs.iter() {
            let sim = cosine_similarity(&query_vec, symbol_vec);
            if sim > 0.1 {
                // Skip very low similarity to reduce noise
                scores.insert(node_id, sim);
            }
        }
        scores
    }

    /// Find code by imported libraries/modules.
    pub async fn find_by_imports(
        &self,
        library: &str,
        options: &ImportSearchOptions,
    ) -> Vec<SymbolMatch> {
        let import_index = self.import_index.read().await;
        let graph = self.graph.read().await;

        let mut matching_nodes = Vec::new();

        match options.match_mode {
            ImportMatchMode::Exact => {
                if let Some(nodes) = import_index.get(library) {
                    matching_nodes.extend(nodes.iter().copied());
                }
            }
            ImportMatchMode::Prefix => {
                for (module, nodes) in import_index.iter() {
                    if module.starts_with(library) {
                        matching_nodes.extend(nodes.iter().copied());
                    }
                }
            }
            ImportMatchMode::Fuzzy => {
                let library_lower = library.to_lowercase();
                for (module, nodes) in import_index.iter() {
                    if module.to_lowercase().contains(&library_lower) {
                        matching_nodes.extend(nodes.iter().copied());
                    }
                }
            }
        }

        // Convert to SymbolMatch
        matching_nodes
            .into_iter()
            .filter_map(|node_id| {
                self.node_to_symbol_info(&graph, node_id)
                    .map(|symbol| SymbolMatch {
                        node_id,
                        symbol,
                        score: 1.0, // No ranking for import-based search
                        match_reason: format!("imports {library}"),
                    })
            })
            .collect()
    }

    /// Get direct callers of a function.
    pub async fn get_callers(&self, node_id: NodeId, depth: u32) -> Vec<CallInfo> {
        let caller_index = self.caller_index.read().await;
        let graph = self.graph.read().await;

        self.get_call_chain(&graph, &caller_index, node_id, depth)
    }

    /// Get direct callees of a function.
    pub async fn get_callees(&self, node_id: NodeId, depth: u32) -> Vec<CallInfo> {
        let callee_index = self.callee_index.read().await;
        let graph = self.graph.read().await;

        self.get_call_chain(&graph, &callee_index, node_id, depth)
    }

    /// Traverse the graph from a starting node with filters.
    pub async fn traverse_graph(
        &self,
        start_node: NodeId,
        direction: TraversalDirection,
        max_depth: u32,
        filter: &TraversalFilter,
    ) -> Vec<TraversalNode> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(NodeId, u32, Vec<NodeId>, String)> = VecDeque::new();

        queue.push_back((start_node, 0, vec![start_node], String::new()));
        visited.insert(start_node);

        let codegraph_direction = match direction {
            TraversalDirection::Outgoing => Direction::Outgoing,
            TraversalDirection::Incoming => Direction::Incoming,
            TraversalDirection::Both => Direction::Both,
        };

        while let Some((current, depth, path, incoming_edge_type)) = queue.pop_front() {
            if depth > max_depth {
                break;
            }

            // Phase 1: Add matching nodes to results (skip the start node)
            if depth > 0 && results.len() < filter.max_nodes {
                if let Ok(node) = graph.get_node(current) {
                    let type_matches = filter.symbol_types.is_empty()
                        || filter.symbol_types.iter().any(|st| {
                            matches!(
                                (st, &node.node_type),
                                (SymbolType::Function, NodeType::Function)
                                    | (SymbolType::Class, NodeType::Class)
                                    | (SymbolType::Variable, NodeType::Variable)
                                    | (SymbolType::Module, NodeType::Module)
                                    | (SymbolType::Interface, NodeType::Interface)
                                    | (SymbolType::Type, NodeType::Type)
                            )
                        });

                    if type_matches {
                        if let Some(symbol) = self.node_to_symbol_info(&graph, current) {
                            results.push(TraversalNode {
                                node_id: current,
                                depth,
                                path: path.clone(),
                                edge_type: incoming_edge_type.clone(),
                                symbol,
                            });
                        }
                    }
                }
            }

            // Phase 2: Always expand neighbors (edge type filter still applies)
            if let Ok(neighbors) = graph.get_neighbors(current, codegraph_direction) {
                for neighbor in neighbors {
                    if visited.contains(&neighbor) {
                        continue;
                    }

                    // Resolve all edge types between the two nodes
                    let edge_types_between = Self::resolve_edge_types(&graph, current, neighbor);

                    // Apply edge type filter
                    if !filter.edge_types.is_empty() {
                        let any_match = edge_types_between.iter().any(|et| {
                            filter
                                .edge_types
                                .iter()
                                .any(|ft| ft.eq_ignore_ascii_case(et))
                        });
                        if !any_match {
                            continue;
                        }
                    }

                    // Use first matching edge type for the record
                    let edge_type_str = if filter.edge_types.is_empty() {
                        edge_types_between.into_iter().next().unwrap_or_default()
                    } else {
                        edge_types_between
                            .into_iter()
                            .find(|et| {
                                filter
                                    .edge_types
                                    .iter()
                                    .any(|ft| ft.eq_ignore_ascii_case(et))
                            })
                            .unwrap_or_default()
                    };

                    visited.insert(neighbor);
                    let mut new_path = path.clone();
                    new_path.push(neighbor);
                    queue.push_back((neighbor, depth + 1, new_path, edge_type_str));
                }
            }
        }

        results
    }

    /// Resolve all edge types between two nodes, checking both directions.
    fn resolve_edge_types(
        graph: &CodeGraph,
        a: codegraph::NodeId,
        b: codegraph::NodeId,
    ) -> Vec<String> {
        let mut types = Vec::new();
        if let Ok(edges) = graph.get_edges_between(a, b) {
            for eid in edges {
                if let Ok(edge) = graph.get_edge(eid) {
                    types.push(edge.edge_type.to_string());
                }
            }
        }
        if types.is_empty() {
            if let Ok(edges) = graph.get_edges_between(b, a) {
                for eid in edges {
                    if let Ok(edge) = graph.get_edge(eid) {
                        types.push(edge.edge_type.to_string());
                    }
                }
            }
        }
        types
    }

    /// Get detailed information about a symbol.
    pub async fn get_symbol_info(&self, node_id: NodeId) -> Option<DetailedSymbolInfo> {
        let graph = self.graph.read().await;
        let caller_index = self.caller_index.read().await;
        let callee_index = self.callee_index.read().await;

        let node = graph.get_node(node_id).ok()?;
        let symbol = self.node_to_symbol_info(&graph, node_id)?;

        // Get callers and callees
        let callers = self.get_call_chain(&graph, &caller_index, node_id, 1);
        let callees = self.get_call_chain(&graph, &callee_index, node_id, 1);

        // Count references
        let reference_count = graph
            .get_neighbors(node_id, Direction::Incoming)
            .map(|n| n.len())
            .unwrap_or(0);

        // Get complexity if available
        let complexity = node.properties.get_int("complexity").map(|c| c as u32);

        // Get lines of code
        let lines_of_code = {
            let start_line = node_props::line_start(node) as i64;
            let end_line = node_props::line_end(node) as i64;
            (end_line - start_line + 1).max(1) as usize
        };

        // Check if public — fall back to visibility string when booleans are absent
        let is_public = node_props::is_public(node);

        // Check if deprecated
        let is_deprecated = node.properties.get_bool("deprecated").unwrap_or(false);

        // Collect dependencies (outgoing import edges)
        let mut dependencies = Vec::new();
        if let Ok(neighbors) = graph.get_neighbors(node_id, Direction::Outgoing) {
            for neighbor_id in neighbors {
                if let Ok(edges) = graph.get_edges_between(node_id, neighbor_id) {
                    let is_import = edges.iter().any(|eid| {
                        graph.get_edge(*eid).is_ok_and(|e| {
                            matches!(e.edge_type, EdgeType::Imports | EdgeType::ImportsFrom)
                        })
                    });
                    if is_import {
                        if let Ok(target) = graph.get_node(neighbor_id) {
                            let name = node_props::name(target).to_string();
                            if !name.is_empty() && !dependencies.contains(&name) {
                                dependencies.push(name);
                            }
                        }
                    }
                }
            }
        }

        // Collect dependents (incoming import edges)
        let mut dependents = Vec::new();
        if let Ok(neighbors) = graph.get_neighbors(node_id, Direction::Incoming) {
            for neighbor_id in neighbors {
                if let Ok(edges) = graph.get_edges_between(neighbor_id, node_id) {
                    let is_import = edges.iter().any(|eid| {
                        graph.get_edge(*eid).is_ok_and(|e| {
                            matches!(e.edge_type, EdgeType::Imports | EdgeType::ImportsFrom)
                        })
                    });
                    if is_import {
                        if let Ok(source) = graph.get_node(neighbor_id) {
                            let name = node_props::name(source).to_string();
                            if !name.is_empty() && !dependents.contains(&name) {
                                dependents.push(name);
                            }
                        }
                    }
                }
            }
        }

        // Detect test associations by checking if any caller is a test node
        let has_tests = callers.iter().any(|caller| {
            graph.get_node(caller.node_id).is_ok_and(|n| {
                let name = node_props::name(n);
                let path = node_props::path(n);
                name.starts_with("test_")
                    || name.ends_with("_test")
                    || name.contains("test ")
                    || path.contains("/test")
                    || path.contains("/tests")
            })
        });

        Some(DetailedSymbolInfo {
            symbol,
            callers,
            callees,
            dependencies,
            dependents,
            complexity,
            lines_of_code,
            has_tests,
            is_public,
            is_deprecated,
            reference_count,
        })
    }

    /// Find functions by signature patterns.
    pub async fn find_by_signature(
        &self,
        pattern: &SignaturePattern,
        limit: Option<usize>,
    ) -> Vec<SymbolMatch> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();

        // Compile regex from name pattern, converting glob wildcards to regex
        let name_regex = pattern.name_pattern.as_ref().and_then(|p| {
            let regex_str = Self::glob_to_anchored_regex(p);
            regex::Regex::new(&regex_str).ok()
        });

        // Iterate over all function nodes using iter_nodes()
        for (node_id, node) in graph.iter_nodes() {
            // Only check functions
            if node.node_type != NodeType::Function {
                continue;
            }

            let name = node_props::name(node);

            // Check name pattern
            if let Some(ref regex) = name_regex {
                if !regex.is_match(name) {
                    continue;
                }
            }

            let signature = node.properties.get_string("signature").unwrap_or("");

            // Check return type
            if let Some(ref expected_return) = pattern.return_type {
                let actual_return = node.properties.get_string("return_type").unwrap_or("");
                // Fall back to extracting return type from signature
                let actual_return = if actual_return.is_empty() {
                    Self::extract_return_type_from_signature(signature)
                } else {
                    actual_return.to_string()
                };
                if !self.type_matches(&actual_return, expected_return) {
                    continue;
                }
            }

            // Check parameter count
            if let Some((min, max)) = pattern.param_count {
                // Try stored param_count first, fall back to parsing from signature
                let param_count = if let Some(count) = node.properties.get_int("param_count") {
                    count as usize
                } else {
                    Self::count_params_from_signature(signature)
                };
                if param_count < min || param_count > max {
                    continue;
                }
            }

            // Check modifiers
            if !pattern.modifiers.is_empty() {
                let visibility = node.properties.get_string("visibility").unwrap_or("");
                let mut all_modifiers_match = true;
                for modifier in &pattern.modifiers {
                    let has_modifier = match modifier.as_str() {
                        "async" => node.properties.get_bool("is_async").unwrap_or(false),
                        "public" | "pub" => {
                            node.properties
                                .get_bool("is_public")
                                .or_else(|| node.properties.get_bool("exported"))
                                .unwrap_or(false)
                                || visibility == "public"
                                || visibility == "pub"
                        }
                        "private" => {
                            visibility == "private"
                                || (!node
                                    .properties
                                    .get_bool("is_public")
                                    .or_else(|| node.properties.get_bool("exported"))
                                    .unwrap_or(false)
                                    && visibility != "public"
                                    && visibility != "pub"
                                    && !visibility.is_empty())
                        }
                        "protected" => visibility == "protected",
                        "static" => node.properties.get_bool("is_static").unwrap_or(false),
                        "const" => node.properties.get_bool("is_const").unwrap_or(false),
                        _ => false,
                    };
                    if !has_modifier {
                        all_modifiers_match = false;
                        break;
                    }
                }
                if !all_modifiers_match {
                    continue;
                }
            }

            // Build symbol info for matching function
            if let Some(symbol) = self.node_to_symbol_info(&graph, node_id) {
                let match_reason = self.build_signature_match_reason(pattern);
                results.push(SymbolMatch {
                    node_id,
                    symbol,
                    score: 1.0, // All matches are equally relevant for signature search
                    match_reason,
                });

                // Check limit and return early if reached
                if let Some(max) = limit {
                    if results.len() >= max {
                        return results;
                    }
                }
            }
        }

        results
    }

    /// Check if actual type matches expected type pattern.
    fn type_matches(&self, actual: &str, expected: &str) -> bool {
        // Handle exact match
        if actual == expected {
            return true;
        }

        // Handle primitive type aliases
        let actual_normalized = match actual.to_lowercase().as_str() {
            "boolean" => "bool",
            "integer" | "int" | "i32" | "i64" => "int",
            "string" | "str" | "&str" => "string",
            "void" | "()" | "none" | "null" => "void",
            _ => actual,
        };

        let expected_normalized = match expected.to_lowercase().as_str() {
            "boolean" => "bool",
            "integer" | "int" | "i32" | "i64" => "int",
            "string" | "str" | "&str" => "string",
            "void" | "()" | "none" | "null" => "void",
            _ => expected,
        };

        if actual_normalized == expected_normalized {
            return true;
        }

        // Handle wildcard patterns (e.g., "Result<*, *>")
        if expected.contains('*') {
            let pattern = expected.replace('*', ".*");
            if let Ok(regex) = regex::Regex::new(&format!("^{pattern}$")) {
                return regex.is_match(actual);
            }
        }

        // Handle generic type prefix matching: "Result" matches "Result<T, E>"
        if actual.starts_with(expected) && actual[expected.len()..].starts_with('<') {
            return true;
        }

        // Case-insensitive prefix matching for the base type
        let actual_base = actual.split('<').next().unwrap_or(actual).trim();
        let expected_base = expected.split('<').next().unwrap_or(expected).trim();
        if !actual_base.is_empty()
            && !expected_base.is_empty()
            && actual_base.eq_ignore_ascii_case(expected_base)
        {
            return true;
        }

        false
    }

    /// Convert a name pattern to an anchored regex.
    /// Detects whether the input is a glob pattern or regex:
    /// - Glob: standalone `*` (not preceded by `.`), `?` wildcards
    /// - Regex: `.*`, `\w`, `[`, `(`, `|`, `+`, `{`
    ///
    /// Both get anchored to match the full name.
    fn glob_to_anchored_regex(pattern: &str) -> String {
        // Detect if this is regex syntax (contains regex-specific constructs)
        let is_regex = pattern.contains(".*")
            || pattern.contains("\\w")
            || pattern.contains("\\d")
            || pattern.contains('(')
            || pattern.contains('|')
            || pattern.contains('[')
            || pattern.contains('+')
            || pattern.contains('{');

        if is_regex {
            // Already regex — just anchor it if not already anchored
            let anchored = if pattern.starts_with('^') && pattern.ends_with('$') {
                pattern.to_string()
            } else if pattern.starts_with('^') {
                format!("{pattern}$")
            } else if pattern.ends_with('$') {
                format!("^{pattern}")
            } else {
                format!("^(?:{pattern})$")
            };
            return anchored;
        }

        // Glob mode: convert glob wildcards to regex
        let mut regex = String::with_capacity(pattern.len() + 4);
        regex.push('^');
        for ch in pattern.chars() {
            match ch {
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                '.' | '^' | '$' | '\\' | '/' => {
                    regex.push('\\');
                    regex.push(ch);
                }
                _ => regex.push(ch),
            }
        }
        regex.push('$');
        regex
    }

    /// Count parameters from a function signature string.
    /// Handles `fn foo()` (0 params), `fn foo(a: i32)` (1 param),
    /// `fn foo(a: i32, b: String)` (2 params), etc.
    /// Also handles `self`/`&self`/`&mut self` — not counted as params.
    fn count_params_from_signature(signature: &str) -> usize {
        // Find the parameter list between first ( and matching )
        let paren_start = match signature.find('(') {
            Some(pos) => pos,
            None => return 0,
        };

        // Find matching closing paren, accounting for nested parens/generics
        let chars: Vec<char> = signature.chars().collect();
        let mut depth = 0;
        let mut paren_end = None;
        for (i, &ch) in chars.iter().enumerate().skip(paren_start) {
            match ch {
                '(' | '<' => depth += 1,
                ')' | '>' => {
                    depth -= 1;
                    if depth == 0 && ch == ')' {
                        paren_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        let paren_end = match paren_end {
            Some(pos) => pos,
            None => return 0,
        };

        let params_str: String = chars[paren_start + 1..paren_end].iter().collect();
        let params_str = params_str.trim();

        if params_str.is_empty() {
            return 0;
        }

        // Split by commas at top-level (not inside <> or ())
        let mut count: usize = 0;
        let mut depth = 0;
        let mut has_content = false;
        for ch in params_str.chars() {
            match ch {
                '<' | '(' | '[' => depth += 1,
                '>' | ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    if has_content {
                        count += 1;
                    }
                    has_content = false;
                    continue;
                }
                _ if !ch.is_whitespace() => has_content = true,
                _ => {}
            }
        }
        if has_content {
            count += 1;
        }

        // Subtract self/&self/&mut self (Rust methods)
        let first_param = params_str.split(',').next().unwrap_or("").trim();
        if first_param == "self"
            || first_param == "&self"
            || first_param == "&mut self"
            || first_param.starts_with("self:")
        {
            count = count.saturating_sub(1);
        }

        count
    }

    /// Extract return type from a function signature string.
    fn extract_return_type_from_signature(signature: &str) -> String {
        // Rust: `fn foo(args) -> ReturnType`
        if let Some(pos) = signature.rfind("->") {
            let ret = signature[pos + 2..].trim();
            // Strip trailing braces/semicolons
            let ret = ret.trim_end_matches(|c: char| c == '{' || c == ';' || c.is_whitespace());
            if !ret.is_empty() {
                return ret.to_string();
            }
        }

        // TypeScript/Java: `function foo(args): ReturnType` or type annotation after `)`
        // Find closing paren, then look for `: Type`
        if let Some(paren_pos) = signature.rfind(')') {
            let after_paren = &signature[paren_pos + 1..];
            if let Some(colon_pos) = after_paren.find(':') {
                let ret = after_paren[colon_pos + 1..].trim();
                let ret = ret.trim_end_matches(|c: char| c == '{' || c == ';' || c.is_whitespace());
                if !ret.is_empty() {
                    return ret.to_string();
                }
            }
        }

        String::new()
    }

    /// Build a human-readable match reason for signature search.
    fn build_signature_match_reason(&self, pattern: &SignaturePattern) -> String {
        let mut parts = Vec::new();

        if let Some(ref name_pattern) = pattern.name_pattern {
            parts.push(format!("name matches /{name_pattern}/"));
        }
        if let Some(ref return_type) = pattern.return_type {
            parts.push(format!("returns {return_type}"));
        }
        if let Some((min, max)) = pattern.param_count {
            if min == max {
                parts.push(format!("{min} parameters"));
            } else {
                parts.push(format!("{min}-{max} parameters"));
            }
        }
        if !pattern.modifiers.is_empty() {
            let mods = pattern.modifiers.join(", ");
            parts.push(format!("modifiers: {mods}"));
        }

        if parts.is_empty() {
            "Signature match".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Find entry points in the codebase.
    pub async fn find_entry_points(&self, entry_types: &[EntryType]) -> Vec<EntryPoint> {
        self.find_entry_points_opts(entry_types, false, None).await
    }

    /// Find entry points with compact option and optional limit.
    pub async fn find_entry_points_opts(
        &self,
        entry_types: &[EntryType],
        compact: bool,
        limit: Option<usize>,
    ) -> Vec<EntryPoint> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();

        // Iterate over all nodes using iter_nodes()
        for (node_id, node) in graph.iter_nodes() {
            // Only check functions
            if node.node_type != NodeType::Function {
                continue;
            }

            let name = node_props::name(node);

            // Detect entry type
            let entry_type = self.detect_entry_type(node, name);

            if let Some(et) = entry_type {
                // Filter by requested entry types
                if entry_types.is_empty() || entry_types.contains(&et) {
                    if let Some(symbol) = self.node_to_symbol_info_opts(&graph, node_id, compact) {
                        // In compact mode, also truncate description
                        let description = if compact {
                            None
                        } else {
                            node.properties
                                .get_string("doc")
                                .map(|s| truncate_string(s, MAX_SIGNATURE_LENGTH))
                        };

                        results.push(EntryPoint {
                            node_id,
                            entry_type: et,
                            route: node.properties.get_string("route").map(|s| s.to_string()),
                            method: node
                                .properties
                                .get_string("http_method")
                                .map(|s| s.to_string()),
                            description,
                            symbol,
                        });

                        // Check limit and return early if reached
                        if let Some(max) = limit {
                            if results.len() >= max {
                                return results;
                            }
                        }
                    }
                }
            }
        }

        results
    }

    // Helper methods

    /// Convert a node to SymbolInfo with default options (truncated signatures)
    fn node_to_symbol_info(&self, graph: &CodeGraph, node_id: NodeId) -> Option<SymbolInfo> {
        self.node_to_symbol_info_opts(graph, node_id, false)
    }

    /// Convert a node to SymbolInfo with options
    /// - compact: if true, omit signature and docstring entirely
    /// - if false, truncate signature to MAX_SIGNATURE_LENGTH
    fn node_to_symbol_info_opts(
        &self,
        graph: &CodeGraph,
        node_id: NodeId,
        compact: bool,
    ) -> Option<SymbolInfo> {
        let node = graph.get_node(node_id).ok()?;

        let name = node.properties.get_string("name")?.to_string();
        let kind = format!("{}", node.node_type);

        // Use canonical property accessors (with fallback for old-style keys)
        let line = node_props::line_start(node).max(1);
        let column = node_props::col_start_from_props(&node.properties);
        let end_line = {
            let e = node_props::line_end(node);
            if e == 0 {
                line
            } else {
                e
            }
        };
        let end_column = node_props::col_end_from_props(&node.properties);

        let file = node_props::path(node).to_string();

        let location = SymbolLocation {
            file,
            line,
            column,
            end_line,
            end_column,
        };

        // In compact mode, omit signature and docstring
        // Otherwise, truncate signature to prevent huge responses
        let (signature, docstring) = if compact {
            (None, None)
        } else {
            let sig = node
                .properties
                .get_string("signature")
                .map(|s| truncate_string(s, MAX_SIGNATURE_LENGTH));
            let doc = node
                .properties
                .get_string("doc")
                .map(|s| truncate_string(s, MAX_SIGNATURE_LENGTH));
            (sig, doc)
        };

        let is_public = node_props::is_public(node);

        let visibility = node
            .properties
            .get_string("visibility")
            .unwrap_or(if is_public { "public" } else { "private" })
            .to_string();

        Some(SymbolInfo {
            name,
            kind,
            location,
            signature,
            docstring,
            is_public,
            visibility,
        })
    }

    fn get_call_chain(
        &self,
        graph: &CodeGraph,
        index: &HashMap<NodeId, Vec<NodeId>>,
        start: NodeId,
        max_depth: u32,
    ) -> Vec<CallInfo> {
        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();

        if let Some(direct) = index.get(&start) {
            for &node_id in direct {
                queue.push_back((node_id, 1));
            }
        }

        while let Some((current, depth)) = queue.pop_front() {
            if depth > max_depth || visited.contains(&current) {
                continue;
            }
            visited.insert(current);

            if let Some(symbol) = self.node_to_symbol_info(graph, current) {
                // Check edge properties for ops struct metadata
                let (via_ops_struct, ops_field) =
                    Self::get_ops_struct_from_edge(graph, current, start);

                results.push(CallInfo {
                    node_id: current,
                    symbol: symbol.clone(),
                    call_site: symbol.location.clone(),
                    depth,
                    via_ops_struct,
                    ops_field,
                });
            }

            // Continue to next depth if needed
            if depth < max_depth {
                if let Some(next_level) = index.get(&current) {
                    for &node_id in next_level {
                        if !visited.contains(&node_id) {
                            queue.push_back((node_id, depth + 1));
                        }
                    }
                }
            }
        }

        results
    }

    /// Look up ops struct metadata from the Calls edge between two nodes.
    fn get_ops_struct_from_edge(
        graph: &CodeGraph,
        source: NodeId,
        target: NodeId,
    ) -> (Option<String>, Option<String>) {
        // Check both directions — for callers, edge is source→target.
        // For callees (same function reused), edge is target→source.
        for (s, t) in [(source, target), (target, source)] {
            if let Ok(edge_ids) = graph.get_edges_between(s, t) {
                for edge_id in edge_ids {
                    if let Ok(edge) = graph.get_edge(edge_id) {
                        if edge.edge_type == codegraph::EdgeType::Calls {
                            let st = edge
                                .properties
                                .get_string("struct_type")
                                .map(|s| s.to_string());
                            let field = edge
                                .properties
                                .get_string("field_name")
                                .map(|s| s.to_string());
                            if st.is_some() || field.is_some() {
                                return (st, field);
                            }
                        }
                    }
                }
            }
        }
        (None, None)
    }

    /// Find all functions that implement a given ops struct field.
    ///
    /// Scans all Calls edges for `struct_type` and `field_name` properties.
    /// If `struct_type` is provided, filters by struct. If `field_name` is
    /// provided, filters by field. Returns matching functions with their
    /// struct/field context.
    pub async fn find_implementors(
        &self,
        struct_type: Option<&str>,
        field_name: Option<&str>,
    ) -> Vec<ImplementorInfo> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (_, edge) in graph.iter_edges() {
            if edge.edge_type != codegraph::EdgeType::Calls {
                continue;
            }

            let edge_struct = edge.properties.get_string("struct_type");
            let edge_field = edge.properties.get_string("field_name");

            // Skip edges without ops struct metadata
            if edge_struct.is_none() {
                continue;
            }

            // Filter by struct_type if provided
            if let Some(st) = struct_type {
                if edge_struct.map(|s| s != st).unwrap_or(true) {
                    continue;
                }
            }

            // Filter by field_name if provided
            if let Some(fn_name) = field_name {
                if edge_field.map(|f| f != fn_name).unwrap_or(true) {
                    continue;
                }
            }

            // Get the target function (the implementor)
            let target_id = edge.target_id;
            if !seen.insert((
                target_id,
                edge_struct.map(|s| s.to_string()),
                edge_field.map(|f| f.to_string()),
            )) {
                continue;
            }

            if let Some(symbol) = self.node_to_symbol_info(&graph, target_id) {
                results.push(ImplementorInfo {
                    node_id: target_id,
                    symbol,
                    struct_type: edge_struct.map(|s| s.to_string()).unwrap_or_default(),
                    field_name: edge_field.map(|f| f.to_string()).unwrap_or_default(),
                });
            }
        }

        // Sort by struct_type, then field_name for consistent output
        results.sort_by(|a, b| {
            a.struct_type
                .cmp(&b.struct_type)
                .then(a.field_name.cmp(&b.field_name))
                .then(a.symbol.name.cmp(&b.symbol.name))
        });

        results
    }

    fn detect_entry_type(&self, node: &codegraph::Node, name: &str) -> Option<EntryType> {
        let name_lower = name.to_lowercase();

        // Check for HTTP handlers
        if node.properties.get_string("route").is_some()
            || node.properties.get_string("http_method").is_some()
        {
            return Some(EntryType::HttpHandler);
        }

        // Check for main function
        if name == "main" || name == "__main__" {
            return Some(EntryType::Main);
        }

        // Check for test functions (by name, property, or file path)
        if name_lower.starts_with("test_")
            || name_lower.ends_with("_test")
            || name.starts_with("Test")
            || node.properties.get_bool("is_test").unwrap_or(false)
        {
            return Some(EntryType::TestEntry);
        }
        // Check if function lives in a test file (path-based detection)
        let path = node_props::path(node);
        if path.contains("/test/")
            || path.contains("/tests/")
            || path.contains("/__tests__/")
            || path.contains(".test.")
            || path.contains(".spec.")
            || path.contains("_test.")
        {
            return Some(EntryType::TestEntry);
        }

        // Check for CLI commands
        if name_lower.contains("command")
            || name_lower.contains("cli")
            || node.properties.get_bool("is_cli").unwrap_or(false)
        {
            return Some(EntryType::CliCommand);
        }

        // Check for event handlers
        if name_lower.starts_with("on_")
            || name_lower.starts_with("handle_")
            || name_lower.ends_with("_handler")
            || name_lower.ends_with("_callback")
        {
            return Some(EntryType::EventHandler);
        }

        // Check for public API (exported functions)
        if node_props::is_public(node) {
            return Some(EntryType::PublicApi);
        }

        None
    }

    /// Find duplicate/similar functions across the codebase.
    ///
    /// Compares all function embedding vectors pairwise and returns pairs
    /// above the similarity threshold. With Jina Code V2, a threshold of
    /// 0.70 reliably identifies clones while filtering noise.
    pub async fn find_duplicates(
        &self,
        threshold: f32,
        limit: usize,
        uri_filter: Option<&str>,
    ) -> DuplicateResult {
        let start = Instant::now();
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        // Collect function vectors, optionally filtered by file
        let entries: Vec<(NodeId, &Vec<f32>)> = symbol_vecs
            .iter()
            .filter(|(node_id, _)| {
                let Ok(node) = graph.get_node(**node_id) else {
                    return false;
                };
                if node.node_type != NodeType::Function {
                    return false;
                }
                if let Some(filter) = uri_filter {
                    let path = node_props::path(node);
                    path.contains(filter)
                } else {
                    true
                }
            })
            .map(|(id, vec)| (*id, vec))
            .collect();

        let total_symbols = entries.len();
        let mut pairs: Vec<DuplicatePair> = Vec::new();

        // Pairwise comparison (O(n²) but n is typically <5000)
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                let sim = cosine_similarity(entries[i].1, entries[j].1);
                if sim >= threshold {
                    let node_a = graph.get_node(entries[i].0);
                    let node_b = graph.get_node(entries[j].0);
                    if let (Ok(na), Ok(nb)) = (node_a, node_b) {
                        // Skip common trait impl names (default, new, from, etc.)
                        let name_a = node_props::name(na).to_lowercase();
                        let name_b = node_props::name(nb).to_lowercase();
                        if BOILERPLATE_NAMES.contains(&name_a.as_str())
                            && BOILERPLATE_NAMES.contains(&name_b.as_str())
                        {
                            continue;
                        }

                        // Skip pairs in the same file at similar lines (likely the same function)
                        let path_a = node_props::path(na);
                        let path_b = node_props::path(nb);
                        let line_a = node_props::line_start(na);
                        let line_b = node_props::line_start(nb);
                        if path_a == path_b && (line_a as i64 - line_b as i64).unsigned_abs() < 5 {
                            continue;
                        }

                        if let (Some(sym_a), Some(sym_b)) = (
                            self.node_to_symbol_info(&graph, entries[i].0),
                            self.node_to_symbol_info(&graph, entries[j].0),
                        ) {
                            pairs.push(DuplicatePair {
                                symbol_a: sym_a,
                                node_id_a: entries[i].0,
                                symbol_b: sym_b,
                                node_id_b: entries[j].0,
                                similarity: sim,
                            });
                        }
                    }
                }
            }
        }

        // Sort by similarity descending
        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pairs.truncate(limit);

        DuplicateResult {
            pairs,
            total_symbols_compared: total_symbols,
            threshold,
            query_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Find functions most similar to a given function.
    ///
    /// Returns the top N functions closest to the target in embedding space.
    /// Useful for "is there already a function that does this?" checks.
    pub async fn find_similar(&self, target_node_id: NodeId, limit: usize) -> DuplicateResult {
        let start = Instant::now();
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        let target_vec = match symbol_vecs.get(&target_node_id) {
            Some(v) => v.clone(),
            None => {
                return DuplicateResult {
                    pairs: Vec::new(),
                    total_symbols_compared: 0,
                    threshold: 0.0,
                    query_time_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        let target_node = graph.get_node(target_node_id).ok();

        let mut scored: Vec<(NodeId, f32)> = symbol_vecs
            .iter()
            .filter(|(id, _)| **id != target_node_id)
            .filter(|(id, _)| {
                graph
                    .get_node(**id)
                    .map(|n| n.node_type == NodeType::Function)
                    .unwrap_or(false)
            })
            .map(|(id, vec)| (*id, cosine_similarity(&target_vec, vec)))
            .filter(|(_, sim)| *sim > 0.1)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        let pairs: Vec<DuplicatePair> = scored
            .into_iter()
            .filter_map(|(node_id, sim)| {
                // Verify both nodes still exist
                graph.get_node(node_id).ok()?;
                target_node?;
                let sym_a = self.node_to_symbol_info(&graph, target_node_id)?;
                let sym_b = self.node_to_symbol_info(&graph, node_id)?;
                Some(DuplicatePair {
                    symbol_a: sym_a,
                    node_id_a: target_node_id,
                    symbol_b: sym_b,
                    node_id_b: node_id,
                    similarity: sim,
                })
            })
            .collect();

        let total = symbol_vecs.len();
        DuplicateResult {
            pairs,
            total_symbols_compared: total,
            threshold: 0.0,
            query_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Cluster functions into semantic groups using greedy clustering.
    ///
    /// Picks an unassigned function, finds all functions within `threshold`
    /// similarity, forms a cluster, repeats. Labels clusters by the most
    /// central member's name.
    pub async fn cluster_symbols(
        &self,
        threshold: f32,
        min_cluster_size: usize,
        limit: usize,
    ) -> ClusterResult {
        let start = Instant::now();
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        // Collect function vectors, excluding boilerplate names
        let entries: Vec<(NodeId, &Vec<f32>)> = symbol_vecs
            .iter()
            .filter(|(node_id, _)| {
                let Ok(node) = graph.get_node(**node_id) else {
                    return false;
                };
                if node.node_type != NodeType::Function {
                    return false;
                }
                let name = node_props::name(node).to_lowercase();
                !BOILERPLATE_NAMES.contains(&name.as_str())
            })
            .map(|(id, vec)| (*id, vec))
            .collect();

        let total_symbols = entries.len();
        let mut assigned = vec![false; entries.len()];
        let mut clusters: Vec<SymbolCluster> = Vec::new();

        for i in 0..entries.len() {
            if assigned[i] {
                continue;
            }

            // Find all unassigned entries similar to entries[i]
            let mut members = vec![i];
            for j in (i + 1)..entries.len() {
                if assigned[j] {
                    continue;
                }
                let sim = cosine_similarity(entries[i].1, entries[j].1);
                if sim >= threshold {
                    members.push(j);
                }
            }

            if members.len() < min_cluster_size {
                continue;
            }

            // Mark as assigned
            for &idx in &members {
                assigned[idx] = true;
            }

            // Build cluster
            let cluster_members: Vec<ClusterMember> = members
                .iter()
                .filter_map(|&idx| {
                    let node = graph.get_node(entries[idx].0).ok()?;
                    let sim = cosine_similarity(entries[i].1, entries[idx].1);
                    Some(ClusterMember {
                        node_id: entries[idx].0,
                        name: node_props::name(node).to_string(),
                        file: node_props::path(node).to_string(),
                        line: node_props::line_start(node),
                        similarity_to_centroid: sim,
                    })
                })
                .collect();

            let label = cluster_members
                .first()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| "unknown".to_string());

            let size = cluster_members.len();
            clusters.push(SymbolCluster {
                label,
                members: cluster_members,
                size,
            });
        }

        // Sort by size descending
        clusters.sort_by(|a, b| b.size.cmp(&a.size));
        clusters.truncate(limit);

        let unclustered = assigned.iter().filter(|&&a| !a).count();

        ClusterResult {
            clusters,
            total_symbols,
            unclustered,
            query_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Compare two functions structurally and semantically.
    pub async fn compare_symbols(
        &self,
        node_id_a: NodeId,
        node_id_b: NodeId,
    ) -> Option<SymbolComparison> {
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        let node_a = graph.get_node(node_id_a).ok()?;
        let node_b = graph.get_node(node_id_b).ok()?;

        let sym_a = self.node_to_symbol_info(&graph, node_id_a)?;
        let sym_b = self.node_to_symbol_info(&graph, node_id_b)?;

        // Semantic similarity
        let similarity = match (symbol_vecs.get(&node_id_a), symbol_vecs.get(&node_id_b)) {
            (Some(va), Some(vb)) => cosine_similarity(va, vb),
            _ => 0.0,
        };

        // Structural comparison
        let path_a = node_props::path(node_a);
        let path_b = node_props::path(node_b);
        let lang_a = path_a.rsplit('.').next().unwrap_or("");
        let lang_b = path_b.rsplit('.').next().unwrap_or("");

        let complexity_a = node_a
            .properties
            .get_int("complexity_cyclomatic")
            .unwrap_or(0) as u32;
        let complexity_b = node_b
            .properties
            .get_int("complexity_cyclomatic")
            .unwrap_or(0) as u32;

        let structural = StructuralComparison {
            same_file: path_a == path_b,
            same_language: lang_a == lang_b,
            complexity_a,
            complexity_b,
            lines_a: node_props::line_end(node_a).saturating_sub(node_props::line_start(node_a))
                + 1,
            lines_b: node_props::line_end(node_b).saturating_sub(node_props::line_start(node_b))
                + 1,
            param_count_a: node_a
                .properties
                .get_string_list_compat("parameters")
                .map(|p| p.len())
                .unwrap_or(0),
            param_count_b: node_b
                .properties
                .get_string_list_compat("parameters")
                .map(|p| p.len())
                .unwrap_or(0),
        };

        // Find shared callers/callees by scanning all edges
        let mut callers_a = std::collections::HashSet::new();
        let mut callers_b = std::collections::HashSet::new();
        let mut callees_a = std::collections::HashSet::new();
        let mut callees_b = std::collections::HashSet::new();

        for (_eid, edge) in graph.iter_edges() {
            if edge.edge_type != EdgeType::Calls {
                continue;
            }
            if let Ok(src) = graph.get_node(edge.source_id) {
                let src_name = node_props::name(src).to_string();
                if edge.target_id == node_id_a {
                    callers_a.insert(src_name.clone());
                }
                if edge.target_id == node_id_b {
                    callers_b.insert(src_name);
                }
            }
            if let Ok(tgt) = graph.get_node(edge.target_id) {
                let tgt_name = node_props::name(tgt).to_string();
                if edge.source_id == node_id_a {
                    callees_a.insert(tgt_name.clone());
                }
                if edge.source_id == node_id_b {
                    callees_b.insert(tgt_name);
                }
            }
        }

        let shared_callers: Vec<String> = callers_a.intersection(&callers_b).cloned().collect();
        let shared_callees: Vec<String> = callees_a.intersection(&callees_b).cloned().collect();

        // Generate verdict
        let verdict = if similarity > 0.9 {
            "Near-identical: likely copy-paste clone. Consider extracting shared function."
                .to_string()
        } else if similarity > 0.7 {
            "Highly similar: same algorithm with different details. Review for consolidation."
                .to_string()
        } else if similarity > 0.5 {
            "Moderately similar: related functionality but distinct implementations.".to_string()
        } else if similarity > 0.3 {
            "Loosely related: some conceptual overlap but different purposes.".to_string()
        } else {
            "Unrelated: different functionality despite any naming similarity.".to_string()
        };

        Some(SymbolComparison {
            symbol_a: sym_a,
            symbol_b: sym_b,
            similarity,
            verdict,
            structural,
            shared_callers,
            shared_callees,
        })
    }
}

/// Common boilerplate function names across all supported languages.
/// These produce false positive duplicates/clusters because they share identical
/// signatures despite being unrelated implementations.
const BOILERPLATE_NAMES: &[&str] = &[
    // Rust traits
    "default",
    "new",
    "from",
    "fmt",
    "clone",
    "drop",
    "deref",
    "eq",
    "hash",
    "into",
    "try_from",
    "try_into",
    "serialize",
    "deserialize",
    // Java/Kotlin/C#
    "tostring",
    "hashcode",
    "equals",
    "compareto",
    "getclass",
    "finalize",
    "dispose",
    "close",
    "gettype",
    "gethashcode",
    // Python
    "__init__",
    "__str__",
    "__repr__",
    "__eq__",
    "__hash__",
    "__len__",
    "__iter__",
    "__next__",
    "__enter__",
    "__exit__",
    "__del__",
    // JavaScript/TypeScript
    "constructor",
    "tostring",
    "valueof",
    "tolocalestring",
    "tojson",
    // C/C++
    "main",
    "init",
    "destroy",
    "free",
    "malloc",
    "realloc",
    // Go
    "string",
    "error",
    "len",
    "close",
    // PHP
    "__construct",
    "__destruct",
    "__tostring",
    "__clone",
    "__get",
    "__set",
    // Ruby
    "initialize",
    "to_s",
    "to_str",
    "inspect",
    "hash",
    "eql?",
    // Swift
    "init",
    "deinit",
    "description",
    // Common getters/setters (all languages)
    "get",
    "set",
    "getvalue",
    "setvalue",
];

/// Cosine similarity between two vectors. Returns 0.0 for zero-length vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::PropertyMap;

    async fn create_test_engine() -> (QueryEngine, Arc<RwLock<CodeGraph>>) {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create in-memory graph"),
        ));
        let engine = QueryEngine::new(Arc::clone(&graph));
        (engine, graph)
    }

    #[tokio::test]
    async fn test_engine_creation() {
        let (engine, _) = create_test_engine().await;
        // Engine should be created successfully
        let text_index = engine.text_index.read().await;
        assert_eq!(text_index.document_count(), 0);
    }

    #[tokio::test]
    async fn test_symbol_search_empty() {
        let (engine, _) = create_test_engine().await;

        let results = engine.symbol_search("test", &SearchOptions::new()).await;

        assert_eq!(results.results.len(), 0);
        assert_eq!(results.total_matches, 0);
    }

    #[tokio::test]
    async fn test_symbol_search_with_data() {
        let (engine, graph) = create_test_engine().await;

        // Add a function node to the graph
        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("validateEmail".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            props.insert("line_end".to_string(), codegraph::PropertyValue::Int(20));

            let node_id = g
                .add_node(NodeType::Function, props)
                .expect("Failed to add node");

            // Node ID is always valid (can be 0 for first node)
            let _ = node_id;
        }

        // Build indexes
        engine.build_indexes().await;

        // Search should find the function
        let results = engine
            .symbol_search("validate", &SearchOptions::new())
            .await;

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].symbol.name, "validateEmail");
    }

    #[tokio::test]
    async fn test_symbol_search_with_type_filter() {
        let (engine, graph) = create_test_engine().await;

        // Add a function and a class
        {
            let mut g = graph.write().await;

            let mut func_props = PropertyMap::new();
            func_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("processData".to_string()),
            );
            func_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            func_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, func_props)
                .expect("Failed to add function");

            let mut class_props = PropertyMap::new();
            class_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("DataProcessor".to_string()),
            );
            class_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            class_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Class, class_props)
                .expect("Failed to add class");
        }

        engine.build_indexes().await;

        // Search with function type filter
        let options = SearchOptions::new().with_symbol_types(vec![SymbolType::Function]);
        let results = engine.symbol_search("data", &options).await;

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].symbol.kind, "Function");
    }

    #[tokio::test]
    async fn test_traverse_graph() {
        let (engine, graph) = create_test_engine().await;

        // Create a simple call chain: A -> B -> C
        let (a, b, c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionA".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g
                .add_node(NodeType::Function, props_a)
                .expect("Failed to add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionB".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            b = g
                .add_node(NodeType::Function, props_b)
                .expect("Failed to add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionC".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            c = g
                .add_node(NodeType::Function, props_c)
                .expect("Failed to add node");

            // A calls B, B calls C
            g.add_edge(a, b, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
            g.add_edge(b, c, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
        }

        engine.build_indexes().await;

        // Traverse from A with depth 2
        let filter = TraversalFilter::new().with_max_nodes(100);
        let results = engine
            .traverse_graph(a, TraversalDirection::Outgoing, 2, &filter)
            .await;

        // Should find B and C
        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"functionB"));
        assert!(names.contains(&"functionC"));
    }

    #[tokio::test]
    async fn test_traverse_graph_node_type_filter() {
        let (engine, graph) = create_test_engine().await;

        // Graph: functionA -> ClassB -> functionC
        // With nodeTypes: [Function], ClassB should be traversed through but not in results
        let (a, _b, c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionA".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g.add_node(NodeType::Function, props_a).expect("add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("ClassB".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            _b = g.add_node(NodeType::Class, props_b).expect("add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionC".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            c = g.add_node(NodeType::Function, props_c).expect("add node");

            g.add_edge(a, _b, EdgeType::Calls, PropertyMap::new())
                .expect("add edge");
            g.add_edge(_b, c, EdgeType::Calls, PropertyMap::new())
                .expect("add edge");
        }

        engine.build_indexes().await;

        // Filter for only Function nodes — should still reach functionC through ClassB
        let filter = TraversalFilter::new()
            .with_max_nodes(100)
            .with_symbol_types(vec![SymbolType::Function]);
        let results = engine
            .traverse_graph(a, TraversalDirection::Outgoing, 3, &filter)
            .await;

        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(
            names.contains(&"functionC"),
            "functionC should be reachable through filtered ClassB"
        );
        assert!(
            !names.contains(&"ClassB"),
            "ClassB should be filtered from results"
        );
        assert_eq!(results.len(), 1, "only functionC should be in results");
    }

    #[tokio::test]
    async fn test_traverse_graph_edge_type_filter() {
        let (engine, graph) = create_test_engine().await;

        // Graph: A -calls-> B -imports-> C
        // With edgeTypes: ["Calls"], should only reach B (not C via Imports)
        let (a, _b, _c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("funcA".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g.add_node(NodeType::Function, props_a).expect("add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("funcB".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            _b = g.add_node(NodeType::Function, props_b).expect("add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("funcC".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            _c = g.add_node(NodeType::Function, props_c).expect("add node");

            g.add_edge(a, _b, EdgeType::Calls, PropertyMap::new())
                .expect("add edge");
            g.add_edge(_b, _c, EdgeType::Imports, PropertyMap::new())
                .expect("add edge");
        }

        engine.build_indexes().await;

        // Only follow Calls edges
        let filter = TraversalFilter::new()
            .with_max_nodes(100)
            .with_edge_types(vec!["Calls".to_string()]);
        let results = engine
            .traverse_graph(a, TraversalDirection::Outgoing, 3, &filter)
            .await;

        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"funcB"), "funcB reachable via Calls edge");
        assert!(
            !names.contains(&"funcC"),
            "funcC should not be reachable via Imports edge when filtering for Calls"
        );
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_get_callers() {
        let (engine, graph) = create_test_engine().await;

        let (a, b, c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("caller1".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g
                .add_node(NodeType::Function, props_a)
                .expect("Failed to add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("caller2".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            b = g
                .add_node(NodeType::Function, props_b)
                .expect("Failed to add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("target".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            c = g
                .add_node(NodeType::Function, props_c)
                .expect("Failed to add node");

            // A and B both call C
            g.add_edge(a, c, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
            g.add_edge(b, c, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
        }

        engine.build_indexes().await;

        // Get callers of C
        let callers = engine.get_callers(c, 1).await;

        assert_eq!(callers.len(), 2);
        let caller_names: Vec<&str> = callers.iter().map(|c| c.symbol.name.as_str()).collect();
        assert!(caller_names.contains(&"caller1"));
        assert!(caller_names.contains(&"caller2"));
    }

    #[tokio::test]
    async fn test_find_entry_points() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Main function
            let mut main_props = PropertyMap::new();
            main_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("main".to_string()),
            );
            main_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/main.rs".to_string()),
            );
            main_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, main_props)
                .expect("Failed to add main");

            // Test function
            let mut test_props = PropertyMap::new();
            test_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("test_something".to_string()),
            );
            test_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            test_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Function, test_props)
                .expect("Failed to add test");

            // Regular function (not an entry point)
            let mut helper_props = PropertyMap::new();
            helper_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("helper".to_string()),
            );
            helper_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            helper_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            g.add_node(NodeType::Function, helper_props)
                .expect("Failed to add helper");
        }

        engine.build_indexes().await;

        // Find main entry points
        let mains = engine.find_entry_points(&[EntryType::Main]).await;
        assert_eq!(mains.len(), 1);
        assert_eq!(mains[0].symbol.name, "main");

        // Find test entry points
        let tests = engine.find_entry_points(&[EntryType::TestEntry]).await;
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].symbol.name, "test_something");
    }

    #[tokio::test]
    async fn test_get_symbol_info() {
        let (engine, graph) = create_test_engine().await;

        let node_id;
        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("processData".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            props.insert("line_end".to_string(), codegraph::PropertyValue::Int(25));
            props.insert(
                "doc".to_string(),
                codegraph::PropertyValue::String("Processes input data".to_string()),
            );
            props.insert(
                "is_public".to_string(),
                codegraph::PropertyValue::Bool(true),
            );

            node_id = g
                .add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let info = engine.get_symbol_info(node_id).await;

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.symbol.name, "processData");
        assert_eq!(info.lines_of_code, 16); // 25 - 10 + 1
        assert!(info.is_public);
    }

    #[tokio::test]
    async fn test_query_performance() {
        let (engine, graph) = create_test_engine().await;

        // Add 1000 nodes using camelCase so tokens are properly split
        {
            let mut g = graph.write().await;
            for i in 0..1000 {
                let mut props = PropertyMap::new();
                // Use functionXXX format like "functionProcess0" so "function" is a separate token
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(format!("functionProcess{i}")),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/test.rs".to_string()),
                );
                props.insert(
                    "line_start".to_string(),
                    codegraph::PropertyValue::Int(i as i64),
                );

                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search should complete quickly (< 10ms)
        let start = Instant::now();
        let results = engine
            .symbol_search("function", &SearchOptions::new())
            .await;
        let duration = start.elapsed();

        assert!(
            duration.as_millis() < 10,
            "Search took too long: {duration:?}"
        );
        assert!(!results.results.is_empty());
    }

    // ==========================================
    // find_by_signature tests
    // ==========================================

    #[tokio::test]
    async fn test_find_by_signature_name_pattern() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different names
            for (name, is_async) in [
                ("getUserById", false),
                ("getOrderById", false),
                ("createUser", true),
                ("deleteUser", false),
                ("processData", false),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/api.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "is_async".to_string(),
                    codegraph::PropertyValue::Bool(is_async),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for functions matching "get.*ById" pattern
        let pattern = SignaturePattern {
            name_pattern: Some("get.*ById".to_string()),
            return_type: None,
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"getUserById"));
        assert!(names.contains(&"getOrderById"));
    }

    #[tokio::test]
    async fn test_find_by_signature_return_type() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different return types
            for (name, return_type) in [
                ("getString", "String"),
                ("getInt", "i32"),
                ("getBool", "bool"),
                ("getResult", "Result<String, Error>"),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for functions returning String
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("String".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "getString");
    }

    #[tokio::test]
    async fn test_find_by_signature_return_type_normalized() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with equivalent return types
            for (name, return_type) in [
                ("fn1", "boolean"),
                ("fn2", "bool"),
                ("fn3", "void"),
                ("fn4", "()"),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for boolean (should match both "boolean" and "bool")
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("bool".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"fn1"));
        assert!(names.contains(&"fn2"));

        // Search for void (should match "void" and "()")
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("void".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"fn3"));
        assert!(names.contains(&"fn4"));
    }

    #[tokio::test]
    async fn test_find_by_signature_param_count() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different param counts
            for (name, param_count) in [
                ("noParams", 0),
                ("oneParam", 1),
                ("twoParams", 2),
                ("threeParams", 3),
                ("manyParams", 5),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "param_count".to_string(),
                    codegraph::PropertyValue::Int(param_count),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for functions with 1-2 parameters
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: Some((1, 2)),
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"oneParam"));
        assert!(names.contains(&"twoParams"));

        // Search for functions with exactly 0 parameters
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: Some((0, 0)),
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "noParams");
    }

    #[tokio::test]
    async fn test_find_by_signature_modifiers() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different modifiers
            let configs = [
                ("syncPublic", false, true, false),
                ("asyncPublic", true, true, false),
                ("syncPrivate", false, false, false),
                ("asyncPrivate", true, false, false),
                ("staticFunc", false, true, true),
            ];

            for (name, is_async, is_public, is_static) in configs {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "is_async".to_string(),
                    codegraph::PropertyValue::Bool(is_async),
                );
                props.insert(
                    "is_public".to_string(),
                    codegraph::PropertyValue::Bool(is_public),
                );
                props.insert(
                    "is_static".to_string(),
                    codegraph::PropertyValue::Bool(is_static),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for async functions
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: None,
            modifiers: vec!["async".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"asyncPublic"));
        assert!(names.contains(&"asyncPrivate"));

        // Search for public async functions
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: None,
            modifiers: vec!["async".to_string(), "public".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "asyncPublic");

        // Search for static functions
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: None,
            modifiers: vec!["static".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "staticFunc");
    }

    #[tokio::test]
    async fn test_find_by_signature_combined_filters() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add various functions
            let configs = [
                ("getUserById", "User", 1, true, true),
                ("getOrderById", "Order", 1, true, false),
                ("fetchUserData", "User", 2, true, true),
                ("createUser", "User", 3, false, true),
                ("processRequest", "Response", 1, true, true),
            ];

            for (name, return_type, param_count, is_async, is_public) in configs {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/api.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                props.insert(
                    "param_count".to_string(),
                    codegraph::PropertyValue::Int(param_count),
                );
                props.insert(
                    "is_async".to_string(),
                    codegraph::PropertyValue::Bool(is_async),
                );
                props.insert(
                    "is_public".to_string(),
                    codegraph::PropertyValue::Bool(is_public),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for async public functions returning User with 1 param
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("User".to_string()),
            param_count: Some((1, 1)),
            modifiers: vec!["async".to_string(), "public".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "getUserById");
    }

    #[tokio::test]
    async fn test_find_by_signature_no_matches() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("someFunction".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        // Search for a pattern that won't match
        let pattern = SignaturePattern {
            name_pattern: Some("nonexistent.*".to_string()),
            return_type: None,
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_find_by_signature_only_matches_functions() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add a function
            let mut func_props = PropertyMap::new();
            func_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("myFunction".to_string()),
            );
            func_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            func_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, func_props)
                .expect("Failed to add function");

            // Add a class with similar name
            let mut class_props = PropertyMap::new();
            class_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("myClass".to_string()),
            );
            class_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            class_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Class, class_props)
                .expect("Failed to add class");

            // Add a variable with similar name
            let mut var_props = PropertyMap::new();
            var_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("myVariable".to_string()),
            );
            var_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            var_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            g.add_node(NodeType::Variable, var_props)
                .expect("Failed to add variable");
        }

        engine.build_indexes().await;

        // Search with pattern matching all "my*"
        let pattern = SignaturePattern {
            name_pattern: Some("my.*".to_string()),
            return_type: None,
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        // Should only match the function
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "myFunction");
        assert_eq!(results[0].symbol.kind, "Function");
    }

    #[tokio::test]
    async fn test_find_by_signature_match_reason() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("testFunc".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            props.insert(
                "return_type".to_string(),
                codegraph::PropertyValue::String("bool".to_string()),
            );
            props.insert("param_count".to_string(), codegraph::PropertyValue::Int(2));
            props.insert("is_async".to_string(), codegraph::PropertyValue::Bool(true));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let pattern = SignaturePattern {
            name_pattern: Some("test.*".to_string()),
            return_type: Some("bool".to_string()),
            param_count: Some((2, 2)),
            modifiers: vec!["async".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        let match_reason = &results[0].match_reason;
        assert!(match_reason.contains("name matches /test.*/"));
        assert!(match_reason.contains("returns bool"));
        assert!(match_reason.contains("2 parameters"));
        assert!(match_reason.contains("modifiers: async"));
    }

    #[tokio::test]
    async fn test_type_matches_wildcard() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            for (name, return_type) in [
                ("fn1", "Result<String, Error>"),
                ("fn2", "Result<i32, Error>"),
                ("fn3", "Option<String>"),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for Result<*, Error> pattern
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("Result<*, Error>".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"fn1"));
        assert!(names.contains(&"fn2"));
    }

    // ==========================================
    // detect_entry_type tests
    // ==========================================

    #[tokio::test]
    async fn test_detect_entry_type_http_handler() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("getUsers".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/handlers.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            props.insert(
                "route".to_string(),
                codegraph::PropertyValue::String("/api/users".to_string()),
            );
            props.insert(
                "http_method".to_string(),
                codegraph::PropertyValue::String("GET".to_string()),
            );
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let results = engine.find_entry_points(&[EntryType::HttpHandler]).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "getUsers");
        assert!(matches!(results[0].entry_type, EntryType::HttpHandler));
        assert_eq!(results[0].route, Some("/api/users".to_string()));
        assert_eq!(results[0].method, Some("GET".to_string()));
    }

    #[tokio::test]
    async fn test_detect_entry_type_cli_command() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // CLI command by name
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("runCommand".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/cli.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");

            // CLI command by property
            let mut props2 = PropertyMap::new();
            props2.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("execute".to_string()),
            );
            props2.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/cli.rs".to_string()),
            );
            props2.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            props2.insert("is_cli".to_string(), codegraph::PropertyValue::Bool(true));
            g.add_node(NodeType::Function, props2)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let results = engine.find_entry_points(&[EntryType::CliCommand]).await;

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_detect_entry_type_event_handler() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Use names that match the actual detection patterns:
            // - starts_with("on_")
            // - starts_with("handle_")
            // - ends_with("_handler")
            // - ends_with("_callback")
            // Note: Avoid "cli" in names as it triggers CliCommand detection first
            for name in [
                "on_submit",
                "handle_submit",
                "button_handler",
                "data_callback",
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/events.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        let results = engine.find_entry_points(&[EntryType::EventHandler]).await;

        assert_eq!(results.len(), 4);

        // Verify all expected patterns are detected
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(
            names.contains(&"on_submit"),
            "on_submit should be detected (starts with on_)"
        );
        assert!(
            names.contains(&"handle_submit"),
            "handle_submit should be detected (starts with handle_)"
        );
        assert!(
            names.contains(&"button_handler"),
            "button_handler should be detected (ends with _handler)"
        );
        assert!(
            names.contains(&"data_callback"),
            "data_callback should be detected (ends with _callback)"
        );
    }

    #[tokio::test]
    async fn test_find_entry_points_all_types() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Main
            let mut main_props = PropertyMap::new();
            main_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("main".to_string()),
            );
            main_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/main.rs".to_string()),
            );
            main_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, main_props)
                .expect("Failed to add main");

            // Test
            let mut test_props = PropertyMap::new();
            test_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("test_something".to_string()),
            );
            test_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/tests.rs".to_string()),
            );
            test_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Function, test_props)
                .expect("Failed to add test");

            // Public API
            let mut api_props = PropertyMap::new();
            api_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("someApi".to_string()),
            );
            api_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            api_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            api_props.insert("exported".to_string(), codegraph::PropertyValue::Bool(true));
            g.add_node(NodeType::Function, api_props)
                .expect("Failed to add api");
        }

        engine.build_indexes().await;

        // Find all entry points (empty filter)
        let results = engine.find_entry_points(&[]).await;

        // Should find main, test, and public API
        assert!(results.len() >= 3);
    }
}
