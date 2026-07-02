// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Call graph traversal — transport-agnostic.
//!
//! Extracts get_call_graph from MCP server.

use crate::ai_query::QueryEngine;
use crate::domain::node_props;
use codegraph::{CodeGraph, Node, NodeId};
use serde::Serialize;
use tokio::sync::RwLock;

// ============================================================
// Response Types
// ============================================================

/// A node in the call graph.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CallGraphNode {
    pub id: String,
    pub name: String,
    pub depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// File path on disk (empty string if unknown).
    pub path: String,
    /// Function signature (empty string if not available).
    pub signature: String,
    pub line_start: u32,
    pub line_end: u32,
    pub col_start: u32,
    pub col_end: u32,
    /// Language of the file (empty string if unknown).
    pub language: String,
}

/// An edge in the call graph.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CallGraphEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: String,
}

/// Diagnostic information when no call relationships are found.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CallGraphDiagnostic {
    pub node_found: bool,
    pub total_edges_in_graph: usize,
    pub note: String,
}

/// Result of `get_call_graph`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CallGraphResult {
    pub root: String,
    pub symbol_name: String,
    /// Full metadata for the root (queried) symbol. Used by LSP adapter to build root node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_node: Option<CallGraphNode>,
    pub nodes: Vec<CallGraphNode>,
    pub edges: Vec<CallGraphEdge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<CallGraphDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

// ============================================================
// Domain Function
// ============================================================

/// Build a call graph for a symbol.
///
/// `direction` is one of: "callers" | "callees" | "both"
///
/// `used_fallback` / `requested_line` add fallback metadata to the response.
pub(crate) async fn get_call_graph(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    start_node: NodeId,
    depth: u32,
    direction: &str,
    used_fallback: bool,
    requested_line: Option<u32>,
) -> CallGraphResult {
    // Get symbol name and root node metadata
    let (symbol_name, root_node) = {
        let g = graph.read().await;
        let name = g
            .get_node(start_node)
            .ok()
            .map(|n| node_props::name(n).to_string())
            .unwrap_or_default();
        let root = g
            .get_node(start_node)
            .ok()
            .map(|n| build_call_graph_node(start_node, n, 0, None));
        (name, root)
    };

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(start_node);

    match direction {
        "callers" => {
            let callers = query_engine.get_callers(start_node, depth).await;
            let g = graph.read().await;
            for caller in callers {
                if seen.insert(caller.node_id) {
                    let node = g
                        .get_node(caller.node_id)
                        .ok()
                        .map(|n| build_call_graph_node(caller.node_id, n, caller.depth, None))
                        .unwrap_or_else(|| CallGraphNode {
                            id: caller.node_id.to_string(),
                            name: caller.symbol.name,
                            depth: caller.depth,
                            direction: None,
                            path: String::new(),
                            signature: String::new(),
                            line_start: 0,
                            line_end: 0,
                            col_start: 0,
                            col_end: 0,
                            language: String::new(),
                        });
                    nodes.push(node);
                    edges.push(CallGraphEdge {
                        from: caller.node_id.to_string(),
                        to: start_node.to_string(),
                        edge_type: "calls".to_string(),
                    });
                }
            }
        }
        "callees" => {
            let callees = query_engine.get_callees(start_node, depth).await;
            let g = graph.read().await;
            for callee in callees {
                if seen.insert(callee.node_id) {
                    let node = g
                        .get_node(callee.node_id)
                        .ok()
                        .map(|n| build_call_graph_node(callee.node_id, n, callee.depth, None))
                        .unwrap_or_else(|| CallGraphNode {
                            id: callee.node_id.to_string(),
                            name: callee.symbol.name,
                            depth: callee.depth,
                            direction: None,
                            path: String::new(),
                            signature: String::new(),
                            line_start: 0,
                            line_end: 0,
                            col_start: 0,
                            col_end: 0,
                            language: String::new(),
                        });
                    nodes.push(node);
                    edges.push(CallGraphEdge {
                        from: start_node.to_string(),
                        to: callee.node_id.to_string(),
                        edge_type: "calls".to_string(),
                    });
                }
            }
        }
        _ => {
            // Both directions
            let callers = query_engine.get_callers(start_node, depth).await;
            let callees = query_engine.get_callees(start_node, depth).await;
            let g = graph.read().await;

            for caller in callers {
                if seen.insert(caller.node_id) {
                    let mut node = g
                        .get_node(caller.node_id)
                        .ok()
                        .map(|n| {
                            build_call_graph_node(
                                caller.node_id,
                                n,
                                caller.depth,
                                Some("caller".to_string()),
                            )
                        })
                        .unwrap_or_else(|| CallGraphNode {
                            id: caller.node_id.to_string(),
                            name: caller.symbol.name,
                            depth: caller.depth,
                            direction: Some("caller".to_string()),
                            path: String::new(),
                            signature: String::new(),
                            line_start: 0,
                            line_end: 0,
                            col_start: 0,
                            col_end: 0,
                            language: String::new(),
                        });
                    node.direction = Some("caller".to_string());
                    nodes.push(node);
                    edges.push(CallGraphEdge {
                        from: caller.node_id.to_string(),
                        to: start_node.to_string(),
                        edge_type: "calls".to_string(),
                    });
                }
            }

            for callee in callees {
                if seen.insert(callee.node_id) {
                    let mut node = g
                        .get_node(callee.node_id)
                        .ok()
                        .map(|n| {
                            build_call_graph_node(
                                callee.node_id,
                                n,
                                callee.depth,
                                Some("callee".to_string()),
                            )
                        })
                        .unwrap_or_else(|| CallGraphNode {
                            id: callee.node_id.to_string(),
                            name: callee.symbol.name,
                            depth: callee.depth,
                            direction: Some("callee".to_string()),
                            path: String::new(),
                            signature: String::new(),
                            line_start: 0,
                            line_end: 0,
                            col_start: 0,
                            col_end: 0,
                            language: String::new(),
                        });
                    node.direction = Some("callee".to_string());
                    nodes.push(node);
                    edges.push(CallGraphEdge {
                        from: start_node.to_string(),
                        to: callee.node_id.to_string(),
                        edge_type: "calls".to_string(),
                    });
                }
            }
        }
    }

    let diagnostic = if nodes.is_empty() {
        let edge_count = {
            let g = graph.read().await;
            g.edge_count()
        };
        Some(CallGraphDiagnostic {
            node_found: true,
            total_edges_in_graph: edge_count,
            note: "No call relationships found. Call graph analysis depends on language \
                   parser support for extracting call edges. Some parsers may have \
                   limited call extraction capabilities."
                .to_string(),
        })
    } else {
        None
    };

    let (used_fallback_field, fallback_message) = if used_fallback {
        (
            Some(true),
            Some(format!(
                "No symbol at line {}. Using nearest symbol '{}' instead.",
                requested_line.unwrap_or(0),
                symbol_name
            )),
        )
    } else {
        (None, None)
    };

    CallGraphResult {
        root: start_node.to_string(),
        symbol_name,
        root_node,
        nodes,
        edges,
        diagnostic,
        used_fallback: used_fallback_field,
        fallback_message,
    }
}

// ============================================================
// Private Helpers
// ============================================================

/// Build a `CallGraphNode` from a graph `Node`, populating all metadata fields.
fn build_call_graph_node(
    node_id: NodeId,
    node: &Node,
    depth: u32,
    direction: Option<String>,
) -> CallGraphNode {
    let name = node_props::name(node).to_string();
    let path = node_props::path(node).to_string();
    let signature = node
        .properties
        .get_string("signature")
        .unwrap_or("")
        .to_string();
    let line_start = node_props::line_start(node);
    let line_end = node_props::line_end(node);
    let col_start = node_props::col_start_from_props(&node.properties);
    let col_end = node_props::col_end_from_props(&node.properties);
    let language = {
        let l = node_props::language(node);
        if l.is_empty() {
            String::new()
        } else {
            l.to_string()
        }
    };
    CallGraphNode {
        id: node_id.to_string(),
        name,
        depth,
        direction,
        path,
        signature,
        line_start,
        line_end,
        col_start,
        col_end,
        language,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{EdgeType, NodeType, PropertyMap, PropertyValue};
    use std::sync::Arc;

    /// Add a node carrying the given key/value properties, returning its id.
    fn add_node(graph: &mut CodeGraph, ty: NodeType, props: &[(&str, PropertyValue)]) -> NodeId {
        let mut map = PropertyMap::new();
        for (k, v) in props {
            map.insert(k.to_string(), v.clone());
        }
        graph.add_node(ty, map).expect("add_node")
    }

    fn str_prop(v: &str) -> PropertyValue {
        PropertyValue::String(v.to_string())
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    /// Wrap a built graph in the Arc<RwLock<>> a QueryEngine owns and build the
    /// call indexes so get_callers/get_callees resolve from Calls edges.
    async fn engine_for(g: CodeGraph) -> (Arc<RwLock<CodeGraph>>, QueryEngine) {
        let graph = Arc::new(RwLock::new(g));
        let engine = QueryEngine::new(graph.clone());
        engine.build_indexes().await;
        (graph, engine)
    }

    #[tokio::test]
    async fn missing_start_node_yields_empty_result_with_diagnostic() {
        let g = CodeGraph::in_memory().expect("in_memory");
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, 999, 1, "both", false, None).await;

        assert!(result.symbol_name.is_empty());
        assert!(result.root_node.is_none());
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
        let diag = result.diagnostic.expect("diagnostic present when empty");
        assert!(diag.node_found);
    }

    #[tokio::test]
    async fn existing_start_node_populates_root_and_symbol_name() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "both", false, None).await;

        assert_eq!(result.symbol_name, "target");
        assert_eq!(result.root, target.to_string());
        let root = result
            .root_node
            .expect("root node built for existing start");
        assert_eq!(root.depth, 0);
        assert_eq!(root.name, "target");
    }

    #[tokio::test]
    async fn callers_direction_builds_incoming_edge() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let caller = add_node(&mut g, NodeType::Function, &[("name", str_prop("caller"))]);
        edge(&mut g, caller, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "callers", false, None).await;

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].name, "caller");
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].from, caller.to_string());
        assert_eq!(result.edges[0].to, target.to_string());
        assert_eq!(result.edges[0].edge_type, "calls");
        assert!(result.diagnostic.is_none());
    }

    #[tokio::test]
    async fn callees_direction_builds_outgoing_edge() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let source = add_node(&mut g, NodeType::Function, &[("name", str_prop("source"))]);
        let callee = add_node(&mut g, NodeType::Function, &[("name", str_prop("callee"))]);
        edge(&mut g, source, callee, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, source, 1, "callees", false, None).await;

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].name, "callee");
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].from, source.to_string());
        assert_eq!(result.edges[0].to, callee.to_string());
    }

    #[tokio::test]
    async fn both_direction_tags_caller_and_callee_directions() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let mid = add_node(&mut g, NodeType::Function, &[("name", str_prop("mid"))]);
        let caller = add_node(&mut g, NodeType::Function, &[("name", str_prop("caller"))]);
        let callee = add_node(&mut g, NodeType::Function, &[("name", str_prop("callee"))]);
        edge(&mut g, caller, mid, EdgeType::Calls);
        edge(&mut g, mid, callee, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, mid, 1, "both", false, None).await;

        assert_eq!(result.nodes.len(), 2);
        let caller_node = result
            .nodes
            .iter()
            .find(|n| n.name == "caller")
            .expect("caller node present");
        let callee_node = result
            .nodes
            .iter()
            .find(|n| n.name == "callee")
            .expect("callee node present");
        assert_eq!(caller_node.direction.as_deref(), Some("caller"));
        assert_eq!(callee_node.direction.as_deref(), Some("callee"));
        assert_eq!(result.edges.len(), 2);
    }

    #[tokio::test]
    async fn caller_node_metadata_is_populated_from_properties() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let caller = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("caller")),
                ("path", str_prop("/src/lib.rs")),
                ("signature", str_prop("fn caller()")),
                ("language", str_prop("rust")),
                ("line_start", PropertyValue::Int(10)),
                ("line_end", PropertyValue::Int(20)),
                ("col_start", PropertyValue::Int(4)),
                ("col_end", PropertyValue::Int(8)),
            ],
        );
        edge(&mut g, caller, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "callers", false, None).await;

        let node = &result.nodes[0];
        assert_eq!(node.path, "/src/lib.rs");
        assert_eq!(node.signature, "fn caller()");
        assert_eq!(node.language, "rust");
        assert_eq!(node.line_start, 10);
        assert_eq!(node.line_end, 20);
        assert_eq!(node.col_start, 4);
        assert_eq!(node.col_end, 8);
    }

    #[tokio::test]
    async fn used_fallback_sets_message_referencing_requested_line() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "both", true, Some(42)).await;

        assert_eq!(result.used_fallback, Some(true));
        let msg = result.fallback_message.expect("fallback message present");
        assert!(msg.contains("line 42"));
        assert!(msg.contains("target"));
    }

    #[tokio::test]
    async fn no_fallback_leaves_fallback_fields_none() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "both", false, None).await;

        assert!(result.used_fallback.is_none());
        assert!(result.fallback_message.is_none());
    }

    #[tokio::test]
    async fn multi_level_callers_report_increasing_depth() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let c1 = add_node(&mut g, NodeType::Function, &[("name", str_prop("c1"))]);
        let c2 = add_node(&mut g, NodeType::Function, &[("name", str_prop("c2"))]);
        // c2 -> c1 -> target
        edge(&mut g, c1, target, EdgeType::Calls);
        edge(&mut g, c2, c1, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 2, "callers", false, None).await;

        assert_eq!(result.nodes.len(), 2);
        let c1_node = result.nodes.iter().find(|n| n.name == "c1").expect("c1");
        let c2_node = result.nodes.iter().find(|n| n.name == "c2").expect("c2");
        assert_eq!(c1_node.depth, 1);
        assert_eq!(c2_node.depth, 2);
        // Every caller edge points at the queried root regardless of depth.
        assert!(result.edges.iter().all(|e| e.to == target.to_string()));
    }

    #[tokio::test]
    async fn both_direction_dedups_node_that_is_both_caller_and_callee() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::Function, &[("name", str_prop("a"))]);
        let b = add_node(&mut g, NodeType::Function, &[("name", str_prop("b"))]);
        // Mutual recursion: a <-> b, so b is both a caller and a callee of a.
        edge(&mut g, a, b, EdgeType::Calls);
        edge(&mut g, b, a, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, a, 1, "both", false, None).await;

        // `seen` dedup keeps b only once, tagged as the first-seen direction (caller).
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].name, "b");
        assert_eq!(result.nodes[0].direction.as_deref(), Some("caller"));
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].from, b.to_string());
        assert_eq!(result.edges[0].to, a.to_string());
    }

    #[tokio::test]
    async fn both_direction_with_only_callees_tags_callee_and_no_diagnostic() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let source = add_node(&mut g, NodeType::Function, &[("name", str_prop("source"))]);
        let callee = add_node(&mut g, NodeType::Function, &[("name", str_prop("callee"))]);
        edge(&mut g, source, callee, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, source, 1, "both", false, None).await;

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].name, "callee");
        assert_eq!(result.nodes[0].direction.as_deref(), Some("callee"));
        assert!(result.diagnostic.is_none());
    }

    #[tokio::test]
    async fn diagnostic_reports_total_edge_count_from_graph() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Queried node has no call relationships, but the graph is not empty.
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let x = add_node(&mut g, NodeType::Function, &[("name", str_prop("x"))]);
        let y = add_node(&mut g, NodeType::Function, &[("name", str_prop("y"))]);
        edge(&mut g, x, y, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "both", false, None).await;

        assert!(result.nodes.is_empty());
        let diag = result
            .diagnostic
            .expect("diagnostic present when no relationships");
        assert!(diag.node_found);
        assert!(diag.total_edges_in_graph >= 1);
    }

    #[tokio::test]
    async fn used_fallback_without_requested_line_reports_line_zero() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "both", true, None).await;

        assert_eq!(result.used_fallback, Some(true));
        let msg = result.fallback_message.expect("fallback message present");
        assert!(msg.contains("line 0"));
    }

    #[tokio::test]
    async fn depth_zero_yields_no_callers_and_a_diagnostic() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        let caller = add_node(&mut g, NodeType::Function, &[("name", str_prop("caller"))]);
        edge(&mut g, caller, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 0, "callers", false, None).await;

        // The call chain drops direct callers pushed at depth 1 when max_depth is 0.
        assert!(result.nodes.is_empty());
        assert!(result.diagnostic.is_some());
    }

    #[tokio::test]
    async fn node_language_is_empty_string_when_property_absent() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", str_prop("target"))]);
        // Caller carries no language property, exercising the is_empty branch.
        let caller = add_node(&mut g, NodeType::Function, &[("name", str_prop("caller"))]);
        edge(&mut g, caller, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;

        let result = get_call_graph(&graph, &engine, target, 1, "callers", false, None).await;

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].language, "");
    }

    #[test]
    fn build_call_graph_node_maps_all_fields_and_keeps_nonempty_language() {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), str_prop("do_work"));
        props.insert("path".to_string(), str_prop("src/lib.rs"));
        props.insert("signature".to_string(), str_prop("fn do_work() -> u32"));
        props.insert("language".to_string(), str_prop("rust"));
        props.insert("line_start".to_string(), PropertyValue::Int(10));
        props.insert("line_end".to_string(), PropertyValue::Int(20));
        props.insert("col_start".to_string(), PropertyValue::Int(4));
        props.insert("col_end".to_string(), PropertyValue::Int(40));
        let node = Node::new(7, NodeType::Function, props);

        let cg = build_call_graph_node(7, &node, 3, Some("callees".to_string()));

        assert_eq!(cg.id, "7");
        assert_eq!(cg.name, "do_work");
        assert_eq!(cg.path, "src/lib.rs");
        assert_eq!(cg.signature, "fn do_work() -> u32");
        assert_eq!(cg.language, "rust");
        assert_eq!(cg.depth, 3);
        assert_eq!(cg.direction, Some("callees".to_string()));
        assert_eq!(cg.line_start, 10);
        assert_eq!(cg.line_end, 20);
        assert_eq!(cg.col_start, 4);
        assert_eq!(cg.col_end, 40);
    }

    #[test]
    fn build_call_graph_node_uses_defaults_for_absent_props() {
        // A node with no properties exercises every fallback: empty strings for
        // name/path/signature/language, zero line span, and the column defaults.
        let node = Node::new(0, NodeType::Function, PropertyMap::new());

        let cg = build_call_graph_node(0, &node, 0, None);

        assert_eq!(cg.id, "0");
        assert_eq!(cg.name, "");
        assert_eq!(cg.path, "");
        assert_eq!(cg.signature, "");
        assert_eq!(cg.language, String::new());
        assert_eq!(cg.direction, None);
        assert_eq!(cg.line_start, 0);
        assert_eq!(cg.line_end, 0);
        assert_eq!(cg.col_start, 0);
        assert_eq!(cg.col_end, 10000);
    }
}
