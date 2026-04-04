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
