// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Hot path detection — transport-agnostic.
//!
//! Finds the most-called functions in a codebase by scoring incoming `Calls` edges
//! at multiple depths: direct callers count 1.0, depth-2 callers 0.5, depth-3 0.25.

use crate::domain::node_props;
use codegraph::{CodeGraph, Direction, EdgeType, NodeId, NodeType};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};

// ============================================================
// Result Types
// ============================================================

#[derive(Debug, Serialize)]
pub(crate) struct HotPathsResult {
    pub functions: Vec<HotFunction>,
    pub total_analyzed: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct HotFunction {
    pub node_id: String,
    pub name: String,
    pub path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub direct_callers: usize,
    pub transitive_callers: usize,
    pub score: f64,
    pub signature: String,
}

// ============================================================
// Domain Function
// ============================================================

/// Find the most-called functions in the graph.
///
/// Scores each `NodeType::Function` node by counting incoming `EdgeType::Calls` edges:
/// - depth-1 (direct) callers: weight 1.0
/// - depth-2 callers: weight 0.5
/// - depth-3 callers: weight 0.25
///
/// Returns the top `limit` functions sorted by score descending.
pub(crate) fn find_hot_paths(graph: &CodeGraph, limit: usize) -> HotPathsResult {
    // Collect all Function node IDs
    let function_ids: Vec<NodeId> = graph
        .nodes_iter()
        .filter_map(|(&id, node)| {
            if node.node_type == NodeType::Function {
                Some(id)
            } else {
                None
            }
        })
        .collect();

    let total_analyzed = function_ids.len();

    let mut hot_functions: Vec<HotFunction> = function_ids
        .iter()
        .filter_map(|&func_id| score_function(graph, func_id))
        .collect();

    // Sort by score descending, break ties by direct_callers then name
    hot_functions.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.direct_callers.cmp(&a.direct_callers))
            .then_with(|| a.name.cmp(&b.name))
    });

    hot_functions.truncate(limit);

    HotPathsResult {
        functions: hot_functions,
        total_analyzed,
    }
}

// ============================================================
// Private Helpers
// ============================================================

/// Compute the hot-path score for a single function node.
///
/// Returns `None` only if the node cannot be retrieved from the graph.
fn score_function(graph: &CodeGraph, func_id: NodeId) -> Option<HotFunction> {
    let node = graph.get_node(func_id).ok()?;

    // Gather callers at each depth level via BFS over incoming Calls edges.
    let depth1 = callers_at_depth(graph, func_id, 1);
    let depth2_all = callers_at_depth(graph, func_id, 2);
    let depth3_all = callers_at_depth(graph, func_id, 3);

    let direct_callers = depth1.len();

    // Transitive callers = nodes reachable at depth 2-3 that aren't direct callers or self
    let transitive: HashSet<NodeId> = depth2_all
        .union(&depth3_all)
        .copied()
        .filter(|id| !depth1.contains(id) && *id != func_id)
        .collect();
    let transitive_callers = transitive.len();

    // Assign each reachable caller to its shallowest depth to avoid double-counting
    let depth2_new: HashSet<NodeId> = depth2_all
        .difference(&depth1)
        .copied()
        .filter(|id| *id != func_id)
        .collect();
    let depth3_new: HashSet<NodeId> = depth3_all
        .difference(&depth2_all)
        .copied()
        .filter(|id| !depth1.contains(id) && *id != func_id)
        .collect();

    let score =
        direct_callers as f64 + depth2_new.len() as f64 * 0.5 + depth3_new.len() as f64 * 0.25;

    let name = node_props::name(node).to_string();
    let path = node_props::path(node).to_string();
    let line_start = node_props::line_start(node) as usize;
    let line_end = node_props::line_end(node) as usize;
    let signature = node
        .properties
        .get_string("signature")
        .unwrap_or("")
        .to_string();

    Some(HotFunction {
        node_id: func_id.to_string(),
        name,
        path,
        line_start,
        line_end,
        direct_callers,
        transitive_callers,
        score,
        signature,
    })
}

/// Return the set of unique Function-type caller nodes reachable within `max_depth`
/// hops over incoming `Calls` edges, excluding `start` itself.
fn callers_at_depth(graph: &CodeGraph, start: NodeId, max_depth: usize) -> HashSet<NodeId> {
    let mut visited: HashSet<NodeId> = HashSet::new();
    visited.insert(start);

    // (node_id, current_depth)
    let mut queue: VecDeque<(NodeId, usize)> = VecDeque::new();
    queue.push_back((start, 0));

    let mut result: HashSet<NodeId> = HashSet::new();

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        let neighbors = match graph.get_neighbors(current, Direction::Incoming) {
            Ok(n) => n,
            Err(_) => continue,
        };

        for neighbor_id in neighbors {
            if visited.contains(&neighbor_id) {
                continue;
            }

            // Only follow actual Calls edges
            let has_calls_edge = graph
                .get_edges_between(neighbor_id, current)
                .map(|edge_ids| {
                    edge_ids.into_iter().any(|eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::Calls)
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            if !has_calls_edge {
                continue;
            }

            visited.insert(neighbor_id);

            // Count only Function nodes as callers
            if graph
                .get_node(neighbor_id)
                .map(|n| n.node_type == NodeType::Function)
                .unwrap_or(false)
            {
                result.insert(neighbor_id);
            }

            queue.push_back((neighbor_id, depth + 1));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{PropertyMap, PropertyValue};

    /// Add a node with a `name` and `path` property, returning its id.
    fn add_node(graph: &mut CodeGraph, ty: NodeType, name: &str, path: &str) -> NodeId {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), PropertyValue::String(name.to_string()));
        props.insert("path".to_string(), PropertyValue::String(path.to_string()));
        graph.add_node(ty, props).expect("add_node")
    }

    /// Add a Function node with explicit line numbers.
    fn add_fn(graph: &mut CodeGraph, name: &str, line_start: i64) -> NodeId {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), PropertyValue::String(name.to_string()));
        props.insert(
            "path".to_string(),
            PropertyValue::String("/src/x.rs".to_string()),
        );
        props.insert("line_start".to_string(), PropertyValue::Int(line_start));
        props.insert("line_end".to_string(), PropertyValue::Int(line_start + 5));
        graph.add_node(NodeType::Function, props).expect("add_node")
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    #[test]
    fn empty_graph_yields_no_functions() {
        let g = CodeGraph::in_memory().expect("in_memory");
        let result = find_hot_paths(&g, 10);
        assert_eq!(result.total_analyzed, 0);
        assert!(result.functions.is_empty());
    }

    #[test]
    fn single_direct_caller_scores_one() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let caller = add_fn(&mut g, "caller", 1);
        let target = add_fn(&mut g, "target", 100);
        edge(&mut g, caller, target, EdgeType::Calls);

        let result = find_hot_paths(&g, 10);
        assert_eq!(result.total_analyzed, 2);
        // Target is called once; caller is called by nobody.
        let target_hot = result
            .functions
            .iter()
            .find(|f| f.name == "target")
            .expect("target present");
        assert_eq!(target_hot.direct_callers, 1);
        assert_eq!(target_hot.transitive_callers, 0);
        assert_eq!(target_hot.score, 1.0);
        assert_eq!(target_hot.line_start, 100);
        assert_eq!(target_hot.line_end, 105);
    }

    #[test]
    fn depth_weighting_along_a_call_chain() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // a -> b -> c -> target  (edges point caller -> callee)
        let a = add_fn(&mut g, "a", 1);
        let b = add_fn(&mut g, "b", 10);
        let c = add_fn(&mut g, "c", 20);
        let target = add_fn(&mut g, "target", 30);
        edge(&mut g, a, b, EdgeType::Calls);
        edge(&mut g, b, c, EdgeType::Calls);
        edge(&mut g, c, target, EdgeType::Calls);

        let result = find_hot_paths(&g, 10);
        let t = result
            .functions
            .iter()
            .find(|f| f.name == "target")
            .expect("target present");
        // direct: c (1.0); depth-2: b (0.5); depth-3: a (0.25) = 1.75
        assert_eq!(t.direct_callers, 1);
        assert_eq!(t.transitive_callers, 2);
        assert_eq!(t.score, 1.75);
    }

    #[test]
    fn non_calls_edges_are_ignored() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let user = add_fn(&mut g, "user", 1);
        let target = add_fn(&mut g, "target", 100);
        // References, not Calls: must not count as a caller.
        edge(&mut g, user, target, EdgeType::References);

        let result = find_hot_paths(&g, 10);
        let t = result
            .functions
            .iter()
            .find(|f| f.name == "target")
            .expect("target present");
        assert_eq!(t.direct_callers, 0);
        assert_eq!(t.score, 0.0);
    }

    #[test]
    fn non_function_callers_are_not_counted() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // A CodeFile "calls" the target: traversed but not counted as a caller.
        let file = add_node(&mut g, NodeType::CodeFile, "a.rs", "/src/a.rs");
        let target = add_fn(&mut g, "target", 100);
        edge(&mut g, file, target, EdgeType::Calls);

        let result = find_hot_paths(&g, 10);
        assert_eq!(result.total_analyzed, 1); // only the Function is analyzed
        let t = result
            .functions
            .iter()
            .find(|f| f.name == "target")
            .expect("target present");
        assert_eq!(t.direct_callers, 0);
        assert_eq!(t.score, 0.0);
    }

    #[test]
    fn self_recursion_does_not_count_as_a_caller() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_fn(&mut g, "recur", 1);
        edge(&mut g, f, f, EdgeType::Calls);

        let result = find_hot_paths(&g, 10);
        let hot = result
            .functions
            .iter()
            .find(|x| x.name == "recur")
            .expect("recur present");
        assert_eq!(hot.direct_callers, 0);
        assert_eq!(hot.score, 0.0);
    }

    #[test]
    fn limit_truncates_and_ranks_by_score() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // hot: two direct callers (score 2.0); cold: one direct caller (score 1.0).
        let hot = add_fn(&mut g, "hot", 100);
        let cold = add_fn(&mut g, "cold", 200);
        let c1 = add_fn(&mut g, "c1", 1);
        let c2 = add_fn(&mut g, "c2", 2);
        let c3 = add_fn(&mut g, "c3", 3);
        edge(&mut g, c1, hot, EdgeType::Calls);
        edge(&mut g, c2, hot, EdgeType::Calls);
        edge(&mut g, c3, cold, EdgeType::Calls);

        let result = find_hot_paths(&g, 1);
        assert_eq!(result.total_analyzed, 5);
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "hot");
        assert_eq!(result.functions[0].direct_callers, 2);
        assert_eq!(result.functions[0].score, 2.0);
    }

    #[test]
    fn equal_scores_break_ties_by_name() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Two targets, each with one direct caller — same score, tie broken by name asc.
        let zeta = add_fn(&mut g, "zeta", 100);
        let alpha = add_fn(&mut g, "alpha", 200);
        let ca = add_fn(&mut g, "ca", 1);
        let cz = add_fn(&mut g, "cz", 2);
        edge(&mut g, cz, zeta, EdgeType::Calls);
        edge(&mut g, ca, alpha, EdgeType::Calls);

        let result = find_hot_paths(&g, 2);
        // Both score 1.0 with 1 direct caller; "alpha" sorts before "zeta".
        assert_eq!(result.functions[0].name, "alpha");
        assert_eq!(result.functions[1].name, "zeta");
    }
}
