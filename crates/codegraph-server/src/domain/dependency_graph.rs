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

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{NodeType, PropertyMap, PropertyValue};

    /// Add a CodeFile node with name/path/language properties, returning its id.
    fn add_file(graph: &mut CodeGraph, name: &str, path: &str, language: &str) -> NodeId {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), PropertyValue::String(name.to_string()));
        props.insert("path".to_string(), PropertyValue::String(path.to_string()));
        props.insert(
            "language".to_string(),
            PropertyValue::String(language.to_string()),
        );
        graph.add_node(NodeType::CodeFile, props).expect("add_node")
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    #[test]
    fn missing_file_returns_empty() {
        let g = CodeGraph::in_memory().expect("in_memory");
        let result = get_dependency_graph(&g, "/src/nope.rs", 3, "both");
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
    }

    #[test]
    fn imports_direction_follows_outgoing_only() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // c.rs imports a.rs imports b.rs. Starting at a.rs with "imports"
        // should reach b.rs (dependency) but not c.rs (dependent).
        let a = add_file(&mut g, "a.rs", "/src/a.rs", "rust");
        let b = add_file(&mut g, "b.rs", "/src/b.rs", "rust");
        let c = add_file(&mut g, "c.rs", "/src/c.rs", "rust");
        edge(&mut g, a, b, EdgeType::Imports);
        edge(&mut g, c, a, EdgeType::Imports);

        let result = get_dependency_graph(&g, "/src/a.rs", 3, "imports");
        let mut paths: Vec<_> = result.nodes.iter().map(|n| n.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["/src/a.rs", "/src/b.rs"]);
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].edge_type, "import");
    }

    #[test]
    fn imported_by_direction_follows_incoming_only() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_file(&mut g, "a.rs", "/src/a.rs", "rust");
        let b = add_file(&mut g, "b.rs", "/src/b.rs", "rust");
        let c = add_file(&mut g, "c.rs", "/src/c.rs", "rust");
        edge(&mut g, a, b, EdgeType::Imports);
        edge(&mut g, c, a, EdgeType::Imports);

        // Starting at a.rs with "importedBy" should reach c.rs, not b.rs.
        let result = get_dependency_graph(&g, "/src/a.rs", 3, "importedBy");
        let mut paths: Vec<_> = result.nodes.iter().map(|n| n.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["/src/a.rs", "/src/c.rs"]);
    }

    #[test]
    fn both_direction_includes_dependencies_and_dependents() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_file(&mut g, "a.rs", "/src/a.rs", "rust");
        let b = add_file(&mut g, "b.rs", "/src/b.rs", "rust");
        let c = add_file(&mut g, "c.rs", "/src/c.rs", "rust");
        edge(&mut g, a, b, EdgeType::Imports);
        edge(&mut g, c, a, EdgeType::Imports);

        let result = get_dependency_graph(&g, "/src/a.rs", 3, "both");
        assert_eq!(result.nodes.len(), 3);
        // Both import edges are between reachable nodes.
        assert_eq!(result.edges.len(), 2);
    }

    #[test]
    fn depth_limits_transitive_reach() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // a -> b -> c chain of imports.
        let a = add_file(&mut g, "a.rs", "/src/a.rs", "rust");
        let b = add_file(&mut g, "b.rs", "/src/b.rs", "rust");
        let c = add_file(&mut g, "c.rs", "/src/c.rs", "rust");
        edge(&mut g, a, b, EdgeType::Imports);
        edge(&mut g, b, c, EdgeType::Imports);

        // depth 1 reaches only b, not the transitive c.
        let result = get_dependency_graph(&g, "/src/a.rs", 1, "imports");
        let mut paths: Vec<_> = result.nodes.iter().map(|n| n.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["/src/a.rs", "/src/b.rs"]);
    }

    #[test]
    fn non_import_edges_are_excluded_from_edge_list() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // a Contains b (not an import) plus a imports b. Only the import edge is emitted,
        // but b is still reachable only if an import edge exists, so add one.
        let a = add_file(&mut g, "a.rs", "/src/a.rs", "rust");
        let b = add_file(&mut g, "b.rs", "/src/b.rs", "rust");
        edge(&mut g, a, b, EdgeType::Imports);
        edge(&mut g, a, b, EdgeType::Contains);

        let result = get_dependency_graph(&g, "/src/a.rs", 3, "imports");
        // Both nodes reachable, but only the single Imports edge is reported.
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].edge_type, "import");
    }

    #[test]
    fn external_flag_and_missing_language_defaults() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_file(&mut g, "a.rs", "/src/a.rs", "rust");
        // b has no language and an external=true flag.
        let mut props = PropertyMap::new();
        props.insert(
            "name".to_string(),
            PropertyValue::String("serde".to_string()),
        );
        props.insert(
            "path".to_string(),
            PropertyValue::String("/ext/serde.rs".to_string()),
        );
        props.insert(
            "external".to_string(),
            PropertyValue::String("true".to_string()),
        );
        let b = g.add_node(NodeType::CodeFile, props).expect("add_node");
        edge(&mut g, a, b, EdgeType::Imports);

        let result = get_dependency_graph(&g, "/src/a.rs", 3, "imports");
        let b_node = result
            .nodes
            .iter()
            .find(|n| n.path == "/ext/serde.rs")
            .expect("b node present");
        assert!(b_node.is_external);
        assert_eq!(b_node.language, "unknown");
        assert_eq!(b_node.node_type, "codefile");

        let a_node = result
            .nodes
            .iter()
            .find(|n| n.path == "/src/a.rs")
            .expect("a node present");
        assert!(!a_node.is_external);
        assert_eq!(a_node.language, "rust");
    }

    #[test]
    fn imports_from_edges_traversed_but_only_imports_edges_reported() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // ImportsFrom is followed during reachability (transitive_dependencies accepts it)
        // but get_dependency_graph only emits edges of type Imports.
        let a = add_file(&mut g, "a.rs", "/src/a.rs", "rust");
        let b = add_file(&mut g, "b.rs", "/src/b.rs", "rust");
        edge(&mut g, a, b, EdgeType::ImportsFrom);

        let result = get_dependency_graph(&g, "/src/a.rs", 3, "imports");
        assert_eq!(result.nodes.len(), 2);
        assert!(result.edges.is_empty());
    }
}
