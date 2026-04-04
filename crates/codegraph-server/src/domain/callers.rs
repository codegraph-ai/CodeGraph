// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Caller/callee graph traversal — transport-agnostic.
//!
//! Extracts the shared pattern from MCP get_callers/get_callees handlers.

use crate::ai_query::{CallInfo, QueryEngine};
use crate::domain::node_props;
use codegraph::{CodeGraph, NodeId};
use serde::Serialize;
use tokio::sync::RwLock;

// ============================================================
// Response Types
// ============================================================

/// Diagnostic info emitted when no callers/callees are found.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct DiagnosticInfo {
    pub node_found: bool,
    pub node_id: String,
    pub symbol_name: String,
    pub total_edges_in_graph: usize,
    pub note: String,
}

/// Result of `get_callers`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CallersResult {
    pub callers: Vec<CallInfo>,
    pub symbol_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<DiagnosticInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

/// Result of `get_callees`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CalleesResult {
    pub callees: Vec<CallInfo>,
    pub symbol_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<DiagnosticInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

// ============================================================
// Domain Functions
// ============================================================

/// Get all callers of a symbol at the given depth.
///
/// Returns `CallersResult` matching the MCP codegraph_get_callers response shape.
pub(crate) async fn get_callers(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    node_id: NodeId,
    depth: u32,
    used_fallback: bool,
    requested_line: Option<u32>,
) -> CallersResult {
    let symbol_name = {
        let g = graph.read().await;
        g.get_node(node_id)
            .ok()
            .map(|n| node_props::name(n).to_string())
            .unwrap_or_default()
    };

    let mut result = query_engine.get_callers(node_id, depth).await;

    // If no callers found, check for same-name variants in other files
    // (handles multi-arch C codebases where the same function exists in
    // arch-specific files like backdoorGcc32.c / backdoorGcc64.c)
    if result.is_empty() && !symbol_name.is_empty() {
        let g = graph.read().await;
        let variants = find_same_name_variants(&g, &symbol_name, node_id);
        drop(g);
        for variant_id in variants {
            let variant_callers = query_engine.get_callers(variant_id, depth).await;
            result.extend(variant_callers);
        }
    }

    let diagnostic = if result.is_empty() {
        let edge_count = {
            let g = graph.read().await;
            g.edge_count()
        };
        Some(DiagnosticInfo {
            node_found: true,
            node_id: node_id.to_string(),
            symbol_name: symbol_name.clone(),
            total_edges_in_graph: edge_count,
            note: "No callers found. This may indicate: (1) the function is not called \
                   anywhere, (2) the language parser doesn't extract call relationships, \
                   or (3) indexes need to be rebuilt."
                .to_string(),
        })
    } else {
        None
    };

    let (used_fallback_field, fallback_message) =
        build_fallback(used_fallback, requested_line, &symbol_name);

    CallersResult {
        callers: result,
        symbol_name,
        diagnostic,
        used_fallback: used_fallback_field,
        fallback_message,
    }
}

/// Get all callees of a symbol at the given depth.
///
/// Returns `CalleesResult` matching the MCP codegraph_get_callees response shape.
pub(crate) async fn get_callees(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    node_id: NodeId,
    depth: u32,
    used_fallback: bool,
    requested_line: Option<u32>,
) -> CalleesResult {
    let symbol_name = {
        let g = graph.read().await;
        g.get_node(node_id)
            .ok()
            .map(|n| node_props::name(n).to_string())
            .unwrap_or_default()
    };

    let mut result = query_engine.get_callees(node_id, depth).await;

    // Check same-name variants for callees (multi-arch dedup)
    if result.is_empty() && !symbol_name.is_empty() {
        let g = graph.read().await;
        let variants = find_same_name_variants(&g, &symbol_name, node_id);
        drop(g);
        for variant_id in variants {
            let variant_callees = query_engine.get_callees(variant_id, depth).await;
            result.extend(variant_callees);
        }
    }

    let diagnostic = if result.is_empty() {
        let edge_count = {
            let g = graph.read().await;
            g.edge_count()
        };
        Some(DiagnosticInfo {
            node_found: true,
            node_id: node_id.to_string(),
            symbol_name: symbol_name.clone(),
            total_edges_in_graph: edge_count,
            note: "No callees found. This may indicate: (1) the function doesn't call \
                   other functions, (2) the language parser doesn't extract call \
                   relationships, or (3) indexes need to be rebuilt."
                .to_string(),
        })
    } else {
        None
    };

    let (used_fallback_field, fallback_message) =
        build_fallback(used_fallback, requested_line, &symbol_name);

    CalleesResult {
        callees: result,
        symbol_name,
        diagnostic,
        used_fallback: used_fallback_field,
        fallback_message,
    }
}

// ============================================================
// Private Helpers
// ============================================================

/// Find other function nodes with the same name but different node ID.
/// Used for multi-arch symbol deduplication (e.g. same function in
/// backdoorGcc32.c and backdoorGcc64.c).
fn find_same_name_variants(graph: &CodeGraph, name: &str, exclude_id: NodeId) -> Vec<NodeId> {
    graph
        .nodes_iter()
        .filter_map(|(&nid, node)| {
            if nid != exclude_id
                && node.node_type == codegraph::NodeType::Function
                && node_props::name(node) == name
            {
                Some(nid)
            } else {
                None
            }
        })
        .collect()
}

fn build_fallback(
    used_fallback: bool,
    requested_line: Option<u32>,
    symbol_name: &str,
) -> (Option<bool>, Option<String>) {
    if used_fallback {
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
    }
}
