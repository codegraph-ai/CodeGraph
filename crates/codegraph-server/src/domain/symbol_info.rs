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
