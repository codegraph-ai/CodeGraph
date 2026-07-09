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
    pub match_confidence: &'static str,
}

/// Symbol metadata for `get_detailed_symbol`, deliberately *without*
/// `callers`/`callees` — those live once, at the top level of
/// `DetailedSymbolResult`, instead of being duplicated here too. Both this
/// struct and the top-level fields used to be populated from independent
/// depth-1 queries that returned identical data.
#[derive(Debug, Serialize)]
pub(crate) struct DetailedSymbolMeta {
    pub symbol: SymbolInfo,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dependents: Vec<String>,
    pub complexity: Option<u32>,
    pub lines_of_code: usize,
    pub has_tests: bool,
    pub is_public: bool,
    pub is_deprecated: bool,
    pub reference_count: usize,
}

impl From<DetailedSymbolInfo> for DetailedSymbolMeta {
    fn from(info: DetailedSymbolInfo) -> Self {
        DetailedSymbolMeta {
            symbol: info.symbol,
            dependencies: info.dependencies,
            dependents: info.dependents,
            complexity: info.complexity,
            lines_of_code: info.lines_of_code,
            has_tests: info.has_tests,
            is_public: info.is_public,
            is_deprecated: info.is_deprecated,
            reference_count: info.reference_count,
        }
    }
}

/// Result of `get_detailed_symbol`.
#[derive(Debug, Serialize)]
pub(crate) struct DetailedSymbolResult {
    /// Symbol metadata (name, kind, signature, complexity, ... — no callers/callees).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<DetailedSymbolMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callers: Option<Vec<CallInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callees: Option<Vec<CallInfo>>,
    /// Present only when `compact=true` truncated the callers list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callers_truncated: Option<usize>,
    /// Present only when `compact=true` truncated the callees list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callees_truncated: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
    pub match_confidence: &'static str,
}

/// Cap `items` to `max` entries in place, returning how many were dropped.
fn truncate_compact(items: &mut Vec<CallInfo>, max: usize) -> Option<usize> {
    if items.len() > max {
        let dropped = items.len() - max;
        items.truncate(max);
        Some(dropped)
    } else {
        None
    }
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
        match_confidence: crate::domain::node_resolution::match_confidence(used_fallback),
    })
}

/// Get detailed symbol info: basic info + optional source + callers + callees.
///
/// Returns `DetailedSymbolResult`. Shape matches the MCP codegraph_get_detailed_symbol response.
///
/// `compact`, when true, caps `callers`/`callees` at 5 entries each (with
/// `callers_truncated`/`callees_truncated` reporting how many were dropped) —
/// for symbols with hundreds of callers this is the dominant cost driver.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn get_detailed_symbol(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    node_id: NodeId,
    include_source: bool,
    include_callers: bool,
    include_callees: bool,
    compact: bool,
    used_fallback: bool,
    requested_line: Option<u32>,
) -> DetailedSymbolResult {
    // Single depth-1 fetch of symbol info + callers/callees — reused for both
    // the top-level `callers`/`callees` fields and the symbol metadata, so we
    // don't run get_call_chain twice for the same data (see DetailedSymbolMeta).
    let info = query_engine.get_symbol_info(node_id).await;

    let symbol_name = info
        .as_ref()
        .map(|i| i.symbol.name.clone())
        .unwrap_or_default();

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

    const COMPACT_MAX_CALLS: usize = 5;

    let (mut callers, mut callees) = match &info {
        Some(i) => (Some(i.callers.clone()), Some(i.callees.clone())),
        None => (None, None),
    };
    if !include_callers {
        callers = None;
    }
    if !include_callees {
        callees = None;
    }

    let callers_truncated = if compact {
        callers
            .as_mut()
            .and_then(|c| truncate_compact(c, COMPACT_MAX_CALLS))
    } else {
        None
    };
    let callees_truncated = if compact {
        callees
            .as_mut()
            .and_then(|c| truncate_compact(c, COMPACT_MAX_CALLS))
    } else {
        None
    };

    let symbol = info.map(DetailedSymbolMeta::from);

    DetailedSymbolResult {
        symbol,
        source,
        callers,
        callees,
        callers_truncated,
        callees_truncated,
        used_fallback: used_fallback_field,
        fallback_message,
        match_confidence: crate::domain::node_resolution::match_confidence(used_fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_compact;
    use crate::ai_query::{CallInfo, SymbolInfo, SymbolLocation};

    fn dummy_call(n: u32) -> CallInfo {
        CallInfo {
            node_id: n as u64,
            symbol: SymbolInfo {
                name: format!("caller_{n}"),
                kind: "function".to_string(),
                location: SymbolLocation {
                    file: "a.rs".to_string(),
                    line: n,
                    column: 0,
                    end_line: n,
                    end_column: 0,
                },
                signature: None,
                docstring: None,
                is_public: true,
                visibility: "public".to_string(),
            },
            call_site: SymbolLocation {
                file: "a.rs".to_string(),
                line: n,
                column: 0,
                end_line: n,
                end_column: 0,
            },
            depth: 1,
            via_ops_struct: None,
            ops_field: None,
        }
    }

    #[test]
    fn truncate_compact_leaves_short_lists_untouched() {
        let mut items = vec![dummy_call(1), dummy_call(2)];
        assert_eq!(truncate_compact(&mut items, 5), None);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn truncate_compact_caps_and_reports_dropped_count() {
        let mut items: Vec<CallInfo> = (0..8).map(dummy_call).collect();
        let dropped = truncate_compact(&mut items, 5);
        assert_eq!(dropped, Some(3));
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn truncate_compact_exact_length_is_not_truncated() {
        let mut items: Vec<CallInfo> = (0..5).map(dummy_call).collect();
        assert_eq!(truncate_compact(&mut items, 5), None);
        assert_eq!(items.len(), 5);
    }
}
