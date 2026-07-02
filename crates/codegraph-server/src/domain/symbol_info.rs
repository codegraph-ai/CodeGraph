// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Symbol info assembly — transport-agnostic.
//!
//! Extracts get_symbol_info / get_detailed_symbol from MCP server.

use crate::ai_query::{CallInfo, DetailedSymbolInfo, QueryEngine, SymbolInfo};
use crate::domain::source_code;
use codegraph::{CodeGraph, NodeId};
use serde::Serialize;
use tokio::sync::RwLock;

// ============================================================
// Response Types
// ============================================================

/// Result of `get_symbol_info`.
///
/// Mirrors the fields of `DetailedSymbolInfo` but ref fields are optional so
/// they can be suppressed when `include_refs = false`.
#[derive(Debug, Serialize)]
pub(crate) struct SymbolInfoResult {
    pub symbol: SymbolInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callers: Option<Vec<CallInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callees: Option<Vec<CallInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependents: Option<Vec<String>>,
    pub complexity: Option<u32>,
    pub lines_of_code: usize,
    pub has_tests: bool,
    pub is_public: bool,
    pub is_deprecated: bool,
    pub reference_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

/// Result of `get_detailed_symbol`.
#[derive(Debug, Serialize)]
pub(crate) struct DetailedSymbolResult {
    /// Full symbol info (serializes as a nested object matching `DetailedSymbolInfo` shape).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<DetailedSymbolInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callers: Option<Vec<CallInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callees: Option<Vec<CallInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

// ============================================================
// Domain Functions
// ============================================================

/// Get basic symbol info with optional fallback metadata.
///
/// Wraps query_engine.get_symbol_info() and optionally strips references
/// or adds fallback fields.
pub(crate) async fn get_symbol_info(
    _graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    node_id: NodeId,
    include_refs: bool,
    used_fallback: bool,
    requested_line: Option<u32>,
) -> Option<SymbolInfoResult> {
    let info = query_engine.get_symbol_info(node_id).await?;

    let (used_fallback_field, fallback_message) = if used_fallback {
        let name = &info.symbol.name;
        (
            Some(true),
            Some(format!(
                "No symbol at line {}. Using nearest symbol '{}' instead.",
                requested_line.unwrap_or(0),
                name
            )),
        )
    } else {
        (None, None)
    };

    let (callers, callees, dependencies, dependents) = if include_refs {
        (
            Some(info.callers),
            Some(info.callees),
            Some(info.dependencies),
            Some(info.dependents),
        )
    } else {
        (None, None, None, None)
    };

    Some(SymbolInfoResult {
        symbol: info.symbol,
        callers,
        callees,
        dependencies,
        dependents,
        complexity: info.complexity,
        lines_of_code: info.lines_of_code,
        has_tests: info.has_tests,
        is_public: info.is_public,
        is_deprecated: info.is_deprecated,
        reference_count: info.reference_count,
        used_fallback: used_fallback_field,
        fallback_message,
    })
}

/// Get detailed symbol info: basic info + optional source + callers + callees.
///
/// Returns `DetailedSymbolResult`. Shape matches the MCP codegraph_get_detailed_symbol response.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn get_detailed_symbol(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    node_id: NodeId,
    include_source: bool,
    include_callers: bool,
    include_callees: bool,
    used_fallback: bool,
    requested_line: Option<u32>,
) -> DetailedSymbolResult {
    // Get basic symbol info
    let (symbol, symbol_name) = if let Some(info) = query_engine.get_symbol_info(node_id).await {
        let name = info.symbol.name.clone();
        (Some(info), name)
    } else {
        (None, String::new())
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

    // Get source code if requested
    let source = if include_source {
        let g = graph.read().await;
        source_code::get_symbol_source(&g, node_id)
    } else {
        None
    };

    // Get callers if requested
    let callers = if include_callers {
        Some(query_engine.get_callers(node_id, 1).await)
    } else {
        None
    };

    // Get callees if requested
    let callees = if include_callees {
        Some(query_engine.get_callees(node_id, 1).await)
    } else {
        None
    };

    DetailedSymbolResult {
        symbol,
        source,
        callers,
        callees,
        used_fallback: used_fallback_field,
        fallback_message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{EdgeType, NodeType, PropertyMap, PropertyValue};
    use std::sync::Arc;

    /// Add a node carrying the given key/value string properties, returning its id.
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

    /// Wrap a built graph in the Arc<RwLock<>> a QueryEngine owns.
    fn engine_for(g: CodeGraph) -> (Arc<RwLock<CodeGraph>>, QueryEngine) {
        let graph = Arc::new(RwLock::new(g));
        let engine = QueryEngine::new(graph.clone());
        (graph, engine)
    }

    #[tokio::test]
    async fn get_symbol_info_missing_node_returns_none() {
        let g = CodeGraph::in_memory().expect("in_memory");
        let (graph, engine) = engine_for(g);
        let result = get_symbol_info(&graph, &engine, 999, true, false, None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_symbol_info_include_refs_populates_ref_fields() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(&mut g, NodeType::Function, &[("name", "target")]);
        let (graph, engine) = engine_for(g);

        let result = get_symbol_info(&graph, &engine, f, true, false, None)
            .await
            .expect("symbol info");
        // With include_refs, the four ref collections are present (even if empty).
        assert!(result.callers.is_some());
        assert!(result.callees.is_some());
        assert!(result.dependencies.is_some());
        assert!(result.dependents.is_some());
        assert_eq!(result.symbol.name, "target");
    }

    #[tokio::test]
    async fn get_symbol_info_without_refs_suppresses_ref_fields() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(&mut g, NodeType::Function, &[("name", "target")]);
        let (graph, engine) = engine_for(g);

        let result = get_symbol_info(&graph, &engine, f, false, false, None)
            .await
            .expect("symbol info");
        assert!(result.callers.is_none());
        assert!(result.callees.is_none());
        assert!(result.dependencies.is_none());
        assert!(result.dependents.is_none());
    }

    #[tokio::test]
    async fn get_symbol_info_fallback_sets_message_with_line_and_name() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(&mut g, NodeType::Function, &[("name", "nearby")]);
        let (graph, engine) = engine_for(g);

        let result = get_symbol_info(&graph, &engine, f, false, true, Some(42))
            .await
            .expect("symbol info");
        assert_eq!(result.used_fallback, Some(true));
        assert_eq!(
            result.fallback_message.as_deref(),
            Some("No symbol at line 42. Using nearest symbol 'nearby' instead.")
        );
    }

    #[tokio::test]
    async fn get_symbol_info_no_fallback_leaves_fallback_fields_none() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "plain"), ("line_start", "10"), ("line_end", "13")],
        );
        let (graph, engine) = engine_for(g);

        let result = get_symbol_info(&graph, &engine, f, false, false, None)
            .await
            .expect("symbol info");
        assert_eq!(result.used_fallback, None);
        assert_eq!(result.fallback_message, None);
        // lines_of_code is derived from the inclusive line range (13 - 10 + 1).
        assert_eq!(result.lines_of_code, 4);
    }

    #[tokio::test]
    async fn get_symbol_info_reports_import_dependencies() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(&mut g, NodeType::Function, &[("name", "consumer")]);
        let dep = add_node(&mut g, NodeType::Module, &[("name", "helpers")]);
        edge(&mut g, f, dep, EdgeType::Imports);
        let (graph, engine) = engine_for(g);

        let result = get_symbol_info(&graph, &engine, f, true, false, None)
            .await
            .expect("symbol info");
        assert_eq!(
            result.dependencies.as_deref(),
            Some(&["helpers".to_string()][..])
        );
    }

    #[tokio::test]
    async fn get_detailed_symbol_missing_node_returns_empty_shell() {
        let g = CodeGraph::in_memory().expect("in_memory");
        let (graph, engine) = engine_for(g);
        let result = get_detailed_symbol(&graph, &engine, 999, true, true, true, false, None).await;
        assert!(result.symbol.is_none());
        // Source/callers/callees are still requested but resolve to empty/None for a missing node.
        assert!(result.source.is_none());
    }

    #[tokio::test]
    async fn get_detailed_symbol_includes_inline_source() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "withbody"), ("source", "fn withbody() {}")],
        );
        let (graph, engine) = engine_for(g);

        let result = get_detailed_symbol(&graph, &engine, f, true, false, false, false, None).await;
        assert!(result.symbol.is_some());
        assert_eq!(result.source.as_deref(), Some("fn withbody() {}"));
        // Callers/callees were not requested.
        assert!(result.callers.is_none());
        assert!(result.callees.is_none());
    }

    #[tokio::test]
    async fn get_detailed_symbol_omits_source_when_not_requested() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "withbody"), ("source", "fn withbody() {}")],
        );
        let (graph, engine) = engine_for(g);

        let result = get_detailed_symbol(&graph, &engine, f, false, true, true, false, None).await;
        assert!(result.source.is_none());
        assert!(result.callers.is_some());
        assert!(result.callees.is_some());
    }

    #[tokio::test]
    async fn get_detailed_symbol_fallback_uses_symbol_name() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_node(&mut g, NodeType::Function, &[("name", "nearest")]);
        let (graph, engine) = engine_for(g);

        let result =
            get_detailed_symbol(&graph, &engine, f, false, false, false, true, Some(7)).await;
        assert_eq!(result.used_fallback, Some(true));
        assert_eq!(
            result.fallback_message.as_deref(),
            Some("No symbol at line 7. Using nearest symbol 'nearest' instead.")
        );
    }
}
