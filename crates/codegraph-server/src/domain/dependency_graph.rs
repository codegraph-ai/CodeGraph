// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Dependency graph traversal — transport-agnostic.
//!
//! Extracts get_dependency_graph from MCP server. Synchronous (takes &CodeGraph).

use crate::domain::node_props;
use codegraph::{CodeGraph, Direction, EdgeType, NodeId};
use serde::Serialize;
use std::collections::HashSet;

// ============================================================
// Response Types
// ============================================================

/// A node in the dependency graph.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GraphNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    /// File path on disk (empty string if unknown).
    pub path: String,
    /// Language of the file (empty string if unknown).
    pub language: String,
    /// Whether this node is an external dependency.
    pub is_external: bool,
}

/// An edge in the dependency graph.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GraphEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: String,
}

/// Result of `get_dependency_graph`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct DependencyGraphResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

// ============================================================
// Domain Function
// ============================================================

/// Build a file-level dependency graph using import-aware traversal from the given file path.
///
/// `direction` is one of: "imports" | "importedBy" | "both"
///
/// Returns all nodes (including external) and import-only edges. Callers decide whether
/// to filter external nodes based on their `is_external` field.
pub(crate) fn get_dependency_graph(
    graph: &CodeGraph,
    file_path: &str,
    depth: usize,
    direction: &str,
) -> DependencyGraphResult {
    // Find the file node
    let start_node = match codegraph::helpers::find_file_by_path(graph, file_path) {
        Ok(Some(id)) => id,
        _ => {
            return DependencyGraphResult {
                nodes: vec![],
                edges: vec![],
            }
        }
    };

    let mut reachable_set: HashSet<NodeId> = HashSet::new();
    reachable_set.insert(start_node);

    // Use import-aware helpers for precise import-edge traversal
    if direction != "importedBy" {
        if let Ok(deps) =
            codegraph::helpers::transitive_dependencies(graph, start_node, Some(depth))
        {
            reachable_set.extend(deps);
        }
    }
    if direction != "imports" {
        if let Ok(deps) = codegraph::helpers::transitive_dependents(graph, start_node, Some(depth))
        {
            reachable_set.extend(deps);
        }
    }

    // Build response nodes with full metadata
    let mut nodes = Vec::new();
    for &node_id in &reachable_set {
        if let Ok(node) = graph.get_node(node_id) {
            let name = node_props::name(node).to_string();
            let node_type = format!("{:?}", node.node_type).to_lowercase();
            let path = node_props::path(node).to_string();
            let language = {
                let l = node_props::language(node);
                if l.is_empty() {
                    "unknown".to_string()
                } else {
                    l.to_string()
                }
            };
            let is_external = node
                .properties
                .get_string("external")
                .map(|v| v == "true")
                .unwrap_or(false);
            nodes.push(GraphNode {
                id: node_id.to_string(),
                name,
                node_type,
                path,
                language,
                is_external,
            });
        }
    }

    // Collect import-only edges between reachable nodes
    let mut edges = Vec::new();
    for &node_id in &reachable_set {
        if let Ok(neighbors) = graph.get_neighbors(node_id, Direction::Outgoing) {
            for neighbor_id in neighbors {
                if !reachable_set.contains(&neighbor_id) {
                    continue;
                }
                if let Ok(edge_ids) = graph.get_edges_between(node_id, neighbor_id) {
                    for edge_id in edge_ids {
                        if let Ok(edge) = graph.get_edge(edge_id) {
                            if edge.edge_type == EdgeType::Imports {
                                edges.push(GraphEdge {
                                    from: node_id.to_string(),
                                    to: neighbor_id.to_string(),
                                    edge_type: "import".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    DependencyGraphResult { nodes, edges }
}
