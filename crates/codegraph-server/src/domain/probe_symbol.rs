// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Cheap symbol probe — transport-agnostic.
//!
//! A lightweight companion to `get_symbol_info`/`get_detailed_symbol` for
//! agents that only need to confirm "did I land on the right symbol?"
//! before paying for a heavier, full-context call.

use crate::domain::{node_props, node_resolution};
use codegraph::{CodeGraph, NodeId};
use serde::Serialize;

/// Result of `codegraph_probe_symbol`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProbeSymbolResult {
    pub name: String,
    #[serde(rename = "type")]
    pub symbol_type: String,
    pub node_id: String,
    pub uri: String,
    pub line_start: i64,
    pub line_end: i64,
    pub used_fallback: bool,
    pub match_confidence: &'static str,
}

/// Build a probe result for an already-resolved node. Cheap by construction:
/// only reads properties already held in memory, no source/callers/callees
/// queries.
pub(crate) fn probe_symbol(
    graph: &CodeGraph,
    node_id: NodeId,
    used_fallback: bool,
) -> Option<ProbeSymbolResult> {
    let node = graph.get_node(node_id).ok()?;
    Some(ProbeSymbolResult {
        name: node_props::name(node).to_string(),
        symbol_type: format!("{:?}", node.node_type).to_lowercase(),
        node_id: node_id.to_string(),
        uri: node_props::path(node).to_string(),
        line_start: node_props::line_start(node) as i64,
        line_end: node_props::line_end(node) as i64,
        used_fallback,
        match_confidence: node_resolution::match_confidence(used_fallback),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{NodeType, PropertyMap};

    #[test]
    fn probes_a_resolved_node() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let props = PropertyMap::new()
            .with("name", "compile_navigation")
            .with("path", "./a/b.py")
            .with("line_start", 122)
            .with("line_end", 154);
        let id = graph.add_node(NodeType::Function, props).unwrap();

        let result = probe_symbol(&graph, id, false).unwrap();
        assert_eq!(result.name, "compile_navigation");
        assert_eq!(result.symbol_type, "function");
        assert_eq!(result.line_start, 122);
        assert_eq!(result.line_end, 154);
        assert!(!result.used_fallback);
        assert_eq!(result.match_confidence, "exact");
    }

    #[test]
    fn probe_marks_fallback_confidence() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let props = PropertyMap::new().with("name", "f").with("path", "./a.py");
        let id = graph.add_node(NodeType::Function, props).unwrap();

        let result = probe_symbol(&graph, id, true).unwrap();
        assert!(result.used_fallback);
        assert_eq!(result.match_confidence, "fallback");
    }

    #[test]
    fn probe_returns_none_for_missing_node() {
        let graph = CodeGraph::in_memory().unwrap();
        assert!(probe_symbol(&graph, 999_999u64, false).is_none());
    }
}
