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

    let score = direct_callers as f64
        + depth2_new.len() as f64 * 0.5
        + depth3_new.len() as f64 * 0.25;

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
