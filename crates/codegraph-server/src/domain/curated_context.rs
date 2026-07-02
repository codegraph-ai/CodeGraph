// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Curated context assembly — transport-agnostic.
//!
//! Extracts get_curated_context from MCP server.
//! Pipeline: search → resolve → expand → enrich → curate.

use crate::ai_query::{QueryEngine, SearchOptions};
use crate::domain::{node_props, source_code};
use crate::memory::MemoryManager;
use codegraph::{CodeGraph, Direction, EdgeType, NodeId};
use serde::Serialize;
use std::collections::HashSet;
use tokio::sync::RwLock;

// ============================================================
// Response Types
// ============================================================

/// A primary symbol match in the curated context.
#[derive(Debug, Serialize)]
pub(crate) struct CuratedSymbol {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub score: f32,
    #[serde(rename = "matchReason")]
    pub match_reason: String,
    pub code: Option<String>,
}

/// A dependency of a primary symbol.
#[derive(Debug, Serialize)]
pub(crate) struct CuratedDependency {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub relationship: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// A memory entry in the curated context.
#[derive(Debug, Serialize)]
pub(crate) struct CuratedMemory {
    pub title: String,
    pub content: String,
    pub kind: String,
    #[serde(rename = "relatedFile")]
    pub related_file: String,
}

/// Metadata about the curated context response.
#[derive(Debug, Serialize)]
pub(crate) struct CuratedContextMetadata {
    #[serde(rename = "totalTokens")]
    pub total_tokens: usize,
    #[serde(rename = "maxTokens")]
    pub max_tokens: usize,
    #[serde(rename = "queryTime")]
    pub query_time: u64,
    #[serde(rename = "symbolsFound")]
    pub symbols_found: usize,
    #[serde(rename = "symbolsIncluded")]
    pub symbols_included: usize,
    #[serde(rename = "dependenciesIncluded")]
    pub dependencies_included: usize,
    #[serde(rename = "memoriesIncluded")]
    pub memories_included: usize,
}

/// Successful curated context result.
#[derive(Debug, Serialize)]
pub(crate) struct CuratedContextResult {
    pub query: String,
    pub symbols: Vec<CuratedSymbol>,
    pub dependencies: Vec<CuratedDependency>,
    pub memories: Vec<CuratedMemory>,
    pub metadata: CuratedContextMetadata,
}

/// Error result when no symbols are found.
#[derive(Debug, Serialize)]
pub(crate) struct CuratedContextError {
    pub error: String,
    pub query: String,
    pub suggestion: String,
}

// ============================================================
// Domain Function
// ============================================================

/// Discover and assemble cross-codebase context for a natural language query.
///
/// `anchor_path` is an optional resolved filesystem path used to prioritize
/// results from the anchor file.
pub(crate) async fn get_curated_context(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    memory_manager: &MemoryManager,
    query: &str,
    anchor_path: Option<&str>,
    max_tokens: usize,
    max_symbols: usize,
) -> Result<CuratedContextResult, CuratedContextError> {
    let start_time = std::time::Instant::now();
    let mut budget_remaining = max_tokens;

    // --- Step 1: Search for relevant symbols ---
    let options = SearchOptions {
        limit: max_symbols * 3,
        include_private: true,
        compact: false,
        ..Default::default()
    };
    let search_result = query_engine.symbol_search(query, &options).await;

    // Sort: anchor file matches first, then by score
    let mut matches = search_result.results;
    if let Some(anchor) = anchor_path {
        matches.sort_by(|a, b| {
            let a_anchor = a.symbol.location.file == anchor;
            let b_anchor = b.symbol.location.file == anchor;
            b_anchor.cmp(&a_anchor).then(b.score.total_cmp(&a.score))
        });
    }
    let top_matches: Vec<_> = matches.into_iter().take(max_symbols).collect();

    if top_matches.is_empty() {
        return Err(CuratedContextError {
            error: format!("No symbols found matching '{}'", query),
            query: query.to_string(),
            suggestion: "Try a different query or ensure the workspace is indexed.".to_string(),
        });
    }

    // --- Step 2: Resolve full source for top matches ---
    let symbol_budget = max_tokens * 40 / 100;
    let mut symbols = Vec::new();
    let mut primary_node_ids = Vec::new();
    let mut primary_files = HashSet::new();
    let mut symbols_tokens = 0usize;

    for m in &top_matches {
        if symbols_tokens >= symbol_budget {
            break;
        }
        let code = {
            let g = graph.read().await;
            source_code::get_symbol_source(&g, m.node_id)
        };
        let code_tokens = code.as_ref().map(|c| c.len() / 4).unwrap_or(0);
        symbols_tokens += code_tokens;
        primary_node_ids.push(m.node_id);
        primary_files.insert(m.symbol.location.file.clone());

        symbols.push(CuratedSymbol {
            name: m.symbol.name.clone(),
            kind: m.symbol.kind.clone(),
            file: m.symbol.location.file.clone(),
            line: m.symbol.location.line,
            score: m.score,
            match_reason: m.match_reason.clone(),
            code,
        });
    }
    budget_remaining = budget_remaining.saturating_sub(symbols_tokens);

    // --- Step 3: Expand — walk dependencies from primary symbols ---
    let dep_budget = max_tokens * 25 / 100;
    let mut dependencies = Vec::new();
    let mut dep_tokens = 0usize;
    let mut seen_dep_ids: HashSet<NodeId> = HashSet::new();
    for &nid in &primary_node_ids {
        seen_dep_ids.insert(nid);
    }

    for &nid in &primary_node_ids {
        if dep_tokens >= dep_budget {
            break;
        }
        let edges = {
            let g = graph.read().await;
            get_edges(&g, nid, Direction::Outgoing)
        };
        let import_edges: Vec<_> = edges
            .iter()
            .filter(|(_, _, t)| *t == EdgeType::Imports || *t == EdgeType::Calls)
            .take(5)
            .cloned()
            .collect();

        for (_, target, edge_type) in import_edges {
            if dep_tokens >= dep_budget || !seen_dep_ids.insert(target) {
                continue;
            }
            let (dep_name, dep_file, dep_kind, relationship) = {
                let g = graph.read().await;
                match g.get_node(target) {
                    Ok(dep_node) => (
                        node_props::name(dep_node).to_string(),
                        node_props::path(dep_node).to_string(),
                        format!("{:?}", dep_node.node_type).to_lowercase(),
                        format!("{:?}", edge_type).to_lowercase(),
                    ),
                    Err(_) => continue,
                }
            };

            let code = {
                let g = graph.read().await;
                source_code::get_symbol_source(&g, target)
            };
            let code_tokens = code.as_ref().map(|c| c.len() / 4).unwrap_or(0);
            if code_tokens > dep_budget / 3 {
                dependencies.push(CuratedDependency {
                    name: dep_name,
                    kind: dep_kind,
                    file: dep_file,
                    relationship,
                    code: None,
                });
            } else {
                dep_tokens += code_tokens;
                dependencies.push(CuratedDependency {
                    name: dep_name,
                    kind: dep_kind,
                    file: dep_file,
                    relationship,
                    code,
                });
            }
            // Process one dep per primary symbol per iteration (matches original behavior)
            break;
        }
    }
    budget_remaining = budget_remaining.saturating_sub(dep_tokens);

    // --- Step 4: Enrich — memories related to primary files ---
    let memory_budget = max_tokens * 15 / 100;
    let mut memories = Vec::new();
    let mut mem_tokens = 0usize;
    let mut seen_mem_titles = HashSet::new();

    for file in primary_files.iter().take(3) {
        if mem_tokens >= memory_budget {
            break;
        }
        let config = crate::memory::SearchConfig {
            limit: 3,
            current_only: true,
            ..Default::default()
        };
        if let Ok(results) = memory_manager.search(file, &config, &[]).await {
            for r in &results {
                if mem_tokens >= memory_budget {
                    break;
                }
                if !seen_mem_titles.insert(r.memory.title.clone()) {
                    continue;
                }
                let content_tokens = r.memory.content.len() / 4;
                mem_tokens += content_tokens;
                memories.push(CuratedMemory {
                    title: r.memory.title.clone(),
                    content: r.memory.content.clone(),
                    kind: r.memory.kind.discriminant_name().to_string(),
                    related_file: file.clone(),
                });
            }
        }
    }
    budget_remaining = budget_remaining.saturating_sub(mem_tokens);

    // --- Step 5: Curate — assemble response ---
    let query_time = start_time.elapsed().as_millis() as u64;
    let total_tokens = max_tokens.saturating_sub(budget_remaining);
    let symbols_found = search_result.total_matches;
    let symbols_included = symbols.len();
    let dependencies_included = dependencies.len();
    let memories_included = memories.len();

    Ok(CuratedContextResult {
        query: query.to_string(),
        symbols,
        dependencies,
        memories,
        metadata: CuratedContextMetadata {
            total_tokens,
            max_tokens,
            query_time,
            symbols_found,
            symbols_included,
            dependencies_included,
            memories_included,
        },
    })
}

// ============================================================
// Private Helpers
// ============================================================

/// Collect edges from a node in the given direction.
fn get_edges(
    graph: &CodeGraph,
    node_id: NodeId,
    direction: Direction,
) -> Vec<(NodeId, NodeId, EdgeType)> {
    let neighbors = match graph.get_neighbors(node_id, direction) {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };

    let mut edges = Vec::new();
    for neighbor_id in neighbors {
        let (source, target) = match direction {
            Direction::Outgoing => (node_id, neighbor_id),
            Direction::Incoming => (neighbor_id, node_id),
            Direction::Both => {
                if let Ok(edge_ids) = graph.get_edges_between(node_id, neighbor_id) {
                    for edge_id in edge_ids {
                        if let Ok(edge) = graph.get_edge(edge_id) {
                            edges.push((edge.source_id, edge.target_id, edge.edge_type));
                        }
                    }
                }
                if let Ok(edge_ids) = graph.get_edges_between(neighbor_id, node_id) {
                    for edge_id in edge_ids {
                        if let Ok(edge) = graph.get_edge(edge_id) {
                            edges.push((edge.source_id, edge.target_id, edge.edge_type));
                        }
                    }
                }
                continue;
            }
        };
        if let Ok(edge_ids) = graph.get_edges_between(source, target) {
            for edge_id in edge_ids {
                if let Ok(edge) = graph.get_edge(edge_id) {
                    edges.push((edge.source_id, edge.target_id, edge.edge_type));
                }
            }
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{NodeType, PropertyMap, PropertyValue};
    use std::sync::Arc;

    /// Add a node carrying the given key/value properties, returning its id.
    fn add_node(graph: &mut CodeGraph, ty: NodeType, props: &[(&str, &str)]) -> NodeId {
        let mut map = PropertyMap::new();
        for (k, v) in props {
            map.insert(k.to_string(), PropertyValue::String(v.to_string()));
        }
        graph.add_node(ty, map).expect("add_node")
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    /// Wrap a built graph in the Arc<RwLock<>> a QueryEngine owns and build the
    /// text/call indexes so symbol_search and dependency walks resolve.
    async fn engine_for(g: CodeGraph) -> (Arc<RwLock<CodeGraph>>, QueryEngine) {
        let graph = Arc::new(RwLock::new(g));
        let engine = QueryEngine::new(graph.clone());
        engine.build_indexes().await;
        (graph, engine)
    }

    // --- get_edges (pure helper) ---

    #[test]
    fn get_edges_outgoing_reports_only_forward_edge() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::Function, &[("name", "a")]);
        let b = add_node(&mut g, NodeType::Function, &[("name", "b")]);
        edge(&mut g, a, b, EdgeType::Calls);

        let out = get_edges(&g, a, Direction::Outgoing);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], (a, b, EdgeType::Calls));

        // b has no outgoing edges.
        assert!(get_edges(&g, b, Direction::Outgoing).is_empty());
    }

    #[test]
    fn get_edges_incoming_reports_only_backward_edge() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::Function, &[("name", "a")]);
        let b = add_node(&mut g, NodeType::Function, &[("name", "b")]);
        edge(&mut g, a, b, EdgeType::Calls);

        let inc = get_edges(&g, b, Direction::Incoming);
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0], (a, b, EdgeType::Calls));
    }

    #[test]
    fn get_edges_missing_node_returns_empty() {
        let g = CodeGraph::in_memory().expect("in_memory");
        assert!(get_edges(&g, 999, Direction::Outgoing).is_empty());
    }

    // --- get_curated_context ---

    #[tokio::test]
    async fn no_matching_symbols_returns_error() {
        let g = CodeGraph::in_memory().expect("in_memory");
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result =
            get_curated_context(&graph, &engine, &mem, "nonexistentquery", None, 4000, 5).await;

        let err = result.expect_err("empty graph should yield error");
        assert_eq!(err.query, "nonexistentquery");
        assert!(err.error.contains("nonexistentquery"));
        assert!(!err.suggestion.is_empty());
    }

    #[tokio::test]
    async fn matching_symbol_populated_with_inline_source() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "processData"),
                ("path", "/src/a.rs"),
                ("source", "fn processData() {}"),
            ],
        );
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "process", None, 4000, 5)
            .await
            .expect("match should produce a result");

        assert_eq!(result.query, "process");
        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.name, "processData");
        assert_eq!(sym.file, "/src/a.rs");
        assert_eq!(sym.code.as_deref(), Some("fn processData() {}"));
        assert_eq!(result.metadata.symbols_included, 1);
        assert!(result.metadata.symbols_found >= 1);
        // Memory manager is uninitialized, so no memories are enriched.
        assert!(result.memories.is_empty());
    }

    #[tokio::test]
    async fn calls_edge_surfaces_dependency() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let primary = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "handleRequest"),
                ("path", "/src/a.rs"),
                ("source", "fn handleRequest() { helper(); }"),
            ],
        );
        let dep = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "helper"),
                ("path", "/src/b.rs"),
                ("source", "fn helper() {}"),
            ],
        );
        edge(&mut g, primary, dep, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 4000, 5)
            .await
            .expect("match should produce a result");

        assert_eq!(result.dependencies.len(), 1);
        let d = &result.dependencies[0];
        assert_eq!(d.name, "helper");
        assert_eq!(d.file, "/src/b.rs");
        assert_eq!(d.relationship, "calls");
        assert_eq!(result.metadata.dependencies_included, 1);
    }

    #[test]
    fn get_edges_both_direction_reports_edges_in_both_orientations() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::Function, &[("name", "a")]);
        let b = add_node(&mut g, NodeType::Function, &[("name", "b")]);
        edge(&mut g, a, b, EdgeType::Calls);
        edge(&mut g, b, a, EdgeType::Imports);

        let both = get_edges(&g, a, Direction::Both);
        assert_eq!(both.len(), 2);
        assert!(both.contains(&(a, b, EdgeType::Calls)));
        assert!(both.contains(&(b, a, EdgeType::Imports)));
    }

    #[tokio::test]
    async fn imports_edge_surfaces_dependency() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let primary = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "handleRequest"),
                ("path", "/src/a.rs"),
                ("source", "fn handleRequest() {}"),
            ],
        );
        let dep = add_node(
            &mut g,
            NodeType::Module,
            &[
                ("name", "config"),
                ("path", "/src/config.rs"),
                ("source", "mod config {}"),
            ],
        );
        edge(&mut g, primary, dep, EdgeType::Imports);
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 4000, 5)
            .await
            .expect("match should produce a result");

        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(result.dependencies[0].name, "config");
        assert_eq!(result.dependencies[0].relationship, "imports");
    }

    #[tokio::test]
    async fn non_import_call_edge_is_not_a_dependency() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let primary = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "handleRequest"),
                ("path", "/src/a.rs"),
                ("source", "fn handleRequest() {}"),
            ],
        );
        let other = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "sibling"), ("path", "/src/b.rs")],
        );
        // References is neither Imports nor Calls, so it must not surface as a dep.
        edge(&mut g, primary, other, EdgeType::References);
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 4000, 5)
            .await
            .expect("match should produce a result");

        assert!(result.dependencies.is_empty());
        assert_eq!(result.metadata.dependencies_included, 0);
    }

    #[tokio::test]
    async fn max_symbols_caps_returned_symbols() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        for name in ["handleAlpha", "handleBeta", "handleGamma"] {
            add_node(
                &mut g,
                NodeType::Function,
                &[
                    ("name", name),
                    (
                        "path",
                        Box::leak(format!("/src/{name}.rs").into_boxed_str()),
                    ),
                ],
            );
        }
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 4000, 2)
            .await
            .expect("matches should produce a result");

        assert_eq!(result.symbols.len(), 2);
        assert_eq!(result.metadata.symbols_included, 2);
        // All three still counted in the pre-cap search total.
        assert!(result.metadata.symbols_found >= 3);
    }

    #[tokio::test]
    async fn oversized_dependency_source_is_omitted_but_listed() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let primary = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "handleRequest"),
                ("path", "/src/a.rs"),
                ("source", "fn handleRequest() {}"),
            ],
        );
        // dep_budget = 4000*25/100 = 1000; omitted when code_tokens (len/4) > 1000/3 ≈ 333,
        // i.e. source longer than ~1332 chars.
        let big_source = "x".repeat(2000);
        let dep = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "helper"),
                ("path", "/src/b.rs"),
                ("source", Box::leak(big_source.into_boxed_str())),
            ],
        );
        edge(&mut g, primary, dep, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 4000, 5)
            .await
            .expect("match should produce a result");

        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(result.dependencies[0].name, "helper");
        // Oversized source is dropped to keep the budget, but the dependency is still listed.
        assert!(result.dependencies[0].code.is_none());
    }

    #[tokio::test]
    async fn only_one_dependency_per_primary_symbol() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let primary = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "handleRequest"),
                ("path", "/src/a.rs"),
                ("source", "fn handleRequest() {}"),
            ],
        );
        let dep1 = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "helperOne"), ("path", "/src/b.rs")],
        );
        let dep2 = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "helperTwo"), ("path", "/src/c.rs")],
        );
        edge(&mut g, primary, dep1, EdgeType::Calls);
        edge(&mut g, primary, dep2, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 4000, 5)
            .await
            .expect("match should produce a result");

        // Despite two Calls edges, only one dependency is processed per primary symbol.
        assert_eq!(result.dependencies.len(), 1);
    }

    #[tokio::test]
    async fn shared_dependency_is_deduplicated_across_primaries() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let p1 = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "handleAlpha"),
                ("path", "/src/a.rs"),
                ("source", "fn handleAlpha() {}"),
            ],
        );
        let p2 = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "handleBeta"),
                ("path", "/src/b.rs"),
                ("source", "fn handleBeta() {}"),
            ],
        );
        let shared = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", "sharedHelper"),
                ("path", "/src/c.rs"),
                ("source", "fn sharedHelper() {}"),
            ],
        );
        edge(&mut g, p1, shared, EdgeType::Calls);
        edge(&mut g, p2, shared, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 4000, 5)
            .await
            .expect("matches should produce a result");

        assert_eq!(result.symbols.len(), 2);
        // The shared dependency is only recorded once thanks to seen_dep_ids.
        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(result.dependencies[0].name, "sharedHelper");
    }

    #[tokio::test]
    async fn symbol_token_budget_caps_included_symbols() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Two matching symbols, each with a source large enough that one alone
        // exhausts the symbol budget. With max_tokens = 100, symbol_budget =
        // 100 * 40 / 100 = 40 tokens; a 200-char source is 200/4 = 50 tokens,
        // so after the first symbol symbols_tokens (50) >= symbol_budget (40)
        // and the loop breaks before adding the second.
        for name in ["handleAlpha", "handleBeta"] {
            add_node(
                &mut g,
                NodeType::Function,
                &[
                    ("name", name),
                    (
                        "path",
                        Box::leak(format!("/src/{name}.rs").into_boxed_str()),
                    ),
                    ("source", Box::leak("x".repeat(200).into_boxed_str())),
                ],
            );
        }
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result = get_curated_context(&graph, &engine, &mem, "handle", None, 100, 5)
            .await
            .expect("matches should produce a result");

        // Only the first symbol fits within the token budget despite max_symbols = 5.
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.metadata.symbols_included, 1);
        // Both matches were still counted in the pre-budget search total.
        assert!(result.metadata.symbols_found >= 2);
    }

    #[tokio::test]
    async fn anchor_path_sorts_anchor_file_first() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", "handleAlpha"), ("path", "/src/a.rs")],
        );
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", "handleBeta"), ("path", "/src/b.rs")],
        );
        let (graph, engine) = engine_for(g).await;
        let mem = MemoryManager::new(None);

        let result =
            get_curated_context(&graph, &engine, &mem, "handle", Some("/src/b.rs"), 4000, 5)
                .await
                .expect("matches should produce a result");

        assert_eq!(result.symbols.len(), 2);
        // The anchor file's symbol is promoted to the front regardless of score.
        assert_eq!(result.symbols[0].file, "/src/b.rs");
    }
}
