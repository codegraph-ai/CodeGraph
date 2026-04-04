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
