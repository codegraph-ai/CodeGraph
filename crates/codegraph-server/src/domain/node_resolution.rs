// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Node resolution — unified find_nearest_node.

use codegraph::{CodeGraph, NodeId};

use crate::domain::node_props;

/// Find the nearest function/symbol node at or near a given line in a file.
///
/// Strategy:
/// 1. Exact containment: find nodes whose line range contains target_line (prefer tightest fit)
/// 2. Fallback: find nearest node by proximity (prefer forward-looking, penalize backward)
///
/// Returns (node_id, used_fallback) where used_fallback is true if no exact containment was found.
pub(crate) fn find_nearest_node(
    graph: &CodeGraph,
    file_path: &str,
    target_line: u32,
) -> Option<(NodeId, bool)> {
    // Strategy 1: Exact containment (prefer tightest — smallest range)
    let mut best_exact: Option<(NodeId, u32)> = None; // (id, range_size)

    for (&node_id, node) in graph.nodes_iter() {
        if node_props::path(node) != file_path {
            continue;
        }
        let start = node_props::line_start(node);
        let end = node_props::line_end(node);
        if target_line >= start && target_line <= end {
            let range_size = end.saturating_sub(start);
            if best_exact.is_none() || range_size < best_exact.unwrap().1 {
                best_exact = Some((node_id, range_size));
            }
        }
    }
    if let Some((id, _)) = best_exact {
        return Some((id, false));
    }

    // Strategy 2: Nearest by proximity (prefer forward, penalize backward)
    let mut best_fallback: Option<(NodeId, i64)> = None;
    for (&node_id, node) in graph.nodes_iter() {
        if node_props::path(node) != file_path {
            continue;
        }
        let start = node_props::line_start(node) as i64;
        let end = node_props::line_end(node) as i64;
        let target = target_line as i64;

        let distance = if start > target {
            start - target // Forward — preferred
        } else {
            (target - end) + 1000 // Backward — penalized
        };

        if best_fallback.is_none() || distance < best_fallback.unwrap().1 {
            best_fallback = Some((node_id, distance));
        }
    }

    best_fallback.map(|(id, _)| (id, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{NodeType, PropertyMap, PropertyValue};

    /// Add a Function node with path + line range, returning its id.
    fn add_node(graph: &mut CodeGraph, path: &str, start: i64, end: i64) -> NodeId {
        let mut props = PropertyMap::new();
        props.insert("path".to_string(), PropertyValue::String(path.to_string()));
        props.insert("line_start".to_string(), PropertyValue::Int(start));
        props.insert("line_end".to_string(), PropertyValue::Int(end));
        graph.add_node(NodeType::Function, props).expect("add_node")
    }

    #[test]
    fn returns_none_when_no_node_in_file() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(&mut g, "other.rs", 1, 10);

        assert_eq!(find_nearest_node(&g, "target.rs", 5), None);
    }

    #[test]
    fn exact_containment_returns_no_fallback() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let n = add_node(&mut g, "a.rs", 10, 20);

        assert_eq!(find_nearest_node(&g, "a.rs", 15), Some((n, false)));
    }

    #[test]
    fn containment_prefers_tightest_range() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Outer range 1..100 and inner range 10..20 both contain line 15;
        // the tighter (smaller) range should win.
        let _outer = add_node(&mut g, "a.rs", 1, 100);
        let inner = add_node(&mut g, "a.rs", 10, 20);

        assert_eq!(find_nearest_node(&g, "a.rs", 15), Some((inner, false)));
    }

    #[test]
    fn containment_ignores_other_files() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Same line range but a different file must not match.
        add_node(&mut g, "other.rs", 10, 20);
        let target = add_node(&mut g, "a.rs", 10, 20);

        assert_eq!(find_nearest_node(&g, "a.rs", 15), Some((target, false)));
    }

    #[test]
    fn fallback_prefers_forward_over_backward() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Target line 50 is not contained by either node.
        // Backward node (ends at 40) is penalized by +1000; the forward node
        // (starts at 60) has distance 10 and should win.
        let _backward = add_node(&mut g, "a.rs", 30, 40);
        let forward = add_node(&mut g, "a.rs", 60, 70);

        assert_eq!(find_nearest_node(&g, "a.rs", 50), Some((forward, true)));
    }

    #[test]
    fn fallback_picks_backward_when_only_option() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let backward = add_node(&mut g, "a.rs", 10, 20);

        assert_eq!(find_nearest_node(&g, "a.rs", 50), Some((backward, true)));
    }

    #[test]
    fn fallback_prefers_nearest_forward() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Two forward nodes; the nearer start (55) beats the farther (80).
        let near = add_node(&mut g, "a.rs", 55, 65);
        let _far = add_node(&mut g, "a.rs", 80, 90);

        assert_eq!(find_nearest_node(&g, "a.rs", 50), Some((near, true)));
    }
}
