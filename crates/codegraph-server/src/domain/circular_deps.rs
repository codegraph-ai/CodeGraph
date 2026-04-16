// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Circular dependency detection — transport-agnostic.
//!
//! Uses Tarjan's SCC algorithm (via `codegraph::helpers::circular_deps`) to find
//! groups of files that mutually import each other, then reconstructs explicit
//! cycle paths via DFS for human-readable output.

use crate::domain::node_props;
use codegraph::{CodeGraph, Direction, EdgeType, NodeId, NodeType};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

// ============================================================
// Response Types
// ============================================================

/// A single circular dependency chain.
#[derive(Debug, Serialize)]
pub(crate) struct DependencyCycle {
    /// File paths forming the cycle, with the first file repeated at the end.
    /// Example: `["a.rs", "b.rs", "c.rs", "a.rs"]`
    pub files: Vec<String>,
    /// Number of distinct files in the cycle (files.len() - 1).
    pub length: usize,
}

/// Result of `find_circular_deps`.
#[derive(Debug, Serialize)]
pub(crate) struct CircularDepsResult {
    pub cycles: Vec<DependencyCycle>,
    pub total_cycles: usize,
    pub has_circular_deps: bool,
}

// ============================================================
// Domain Function
// ============================================================

/// Find circular import chains in the code graph.
///
/// Uses Tarjan's SCC algorithm to discover file groups involved in cycles, then
/// runs DFS within each SCC to reconstruct explicit cycle paths.
///
/// `max_cycle_length` caps the longest reported chain (default 10). Cycles longer
/// than the limit are omitted to keep output manageable.
pub(crate) fn find_circular_deps(
    graph: &CodeGraph,
    max_cycle_length: usize,
) -> CircularDepsResult {
    // Collect all CodeFile nodes with their file paths.
    let file_nodes: Vec<(NodeId, String)> = {
        match graph.query().node_type(NodeType::CodeFile).execute() {
            Ok(ids) => ids
                .into_iter()
                .filter_map(|id| {
                    graph.get_node(id).ok().map(|n| {
                        let path = node_props::path(n).to_string();
                        (id, path)
                    })
                })
                .filter(|(_, path)| !path.is_empty())
                .collect(),
            Err(_) => return CircularDepsResult::empty(),
        }
    };

    if file_nodes.is_empty() {
        return CircularDepsResult::empty();
    }

    // Build an adjacency map: node_id -> [neighbor_ids via Imports edges]
    let node_to_path: HashMap<NodeId, String> =
        file_nodes.iter().cloned().collect();
    let file_id_set: HashSet<NodeId> = node_to_path.keys().copied().collect();

    let adjacency = build_import_adjacency(graph, &file_id_set);

    // Detect self-imports first (a file that imports itself).
    let mut cycles: Vec<DependencyCycle> = Vec::new();
    for (&node_id, neighbors) in &adjacency {
        if neighbors.contains(&node_id) {
            if let Some(path) = node_to_path.get(&node_id) {
                cycles.push(DependencyCycle {
                    files: vec![path.clone(), path.clone()],
                    length: 1,
                });
            }
        }
    }

    // Use the existing codegraph helper to get SCCs (groups of files in cycles).
    let scc_groups = match codegraph::helpers::circular_deps(graph) {
        Ok(g) => g,
        Err(_) => return CircularDepsResult::empty(),
    };

    // For each SCC group, run DFS to find representative cycle paths.
    for scc in &scc_groups {
        let scc_set: HashSet<NodeId> = scc.iter().copied().collect();
        // Only keep nodes that are CodeFiles (the helper may include non-file nodes).
        let scc_files: Vec<NodeId> = scc
            .iter()
            .copied()
            .filter(|id| file_id_set.contains(id))
            .collect();

        if scc_files.len() < 2 {
            continue;
        }

        // Find one cycle path starting from each node in the SCC, deduplicated by
        // canonical rotation so we don't return the same cycle multiple times.
        let mut seen_canonical: HashSet<Vec<NodeId>> = HashSet::new();

        for &start in &scc_files {
            if let Some(path_ids) = dfs_find_cycle(
                start,
                start,
                &adjacency,
                &scc_set,
                &mut Vec::new(),
                max_cycle_length,
            ) {
                // Canonicalize: rotate to smallest-id-first, so A→B→C and B→C→A are the same.
                let canonical = canonical_cycle(&path_ids);
                if seen_canonical.insert(canonical) {
                    let file_paths: Vec<String> = path_ids
                        .iter()
                        .filter_map(|id| node_to_path.get(id).cloned())
                        .collect();
                    if file_paths.len() == path_ids.len() {
                        let length = file_paths.len().saturating_sub(1);
                        cycles.push(DependencyCycle {
                            files: file_paths,
                            length,
                        });
                    }
                }
            }
        }
    }

    let total_cycles = cycles.len();
    CircularDepsResult {
        has_circular_deps: total_cycles > 0,
        cycles,
        total_cycles,
    }
}

// ============================================================
// Helpers
// ============================================================

impl CircularDepsResult {
    fn empty() -> Self {
        CircularDepsResult {
            cycles: vec![],
            total_cycles: 0,
            has_circular_deps: false,
        }
    }
}

/// Build a map of node_id -> Vec<neighbor_id> for Imports edges restricted to CodeFile nodes.
fn build_import_adjacency(
    graph: &CodeGraph,
    file_id_set: &HashSet<NodeId>,
) -> HashMap<NodeId, Vec<NodeId>> {
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for &node_id in file_id_set {
        let neighbors = match graph.get_neighbors(node_id, Direction::Outgoing) {
            Ok(n) => n,
            Err(_) => continue,
        };

        let mut import_neighbors: Vec<NodeId> = Vec::new();
        for neighbor_id in neighbors {
            if !file_id_set.contains(&neighbor_id) {
                continue;
            }
            // Check if there is at least one Imports edge between node_id and neighbor_id.
            let has_import = graph
                .get_edges_between(node_id, neighbor_id)
                .unwrap_or_default()
                .iter()
                .any(|&edge_id| {
                    graph
                        .get_edge(edge_id)
                        .map(|e| {
                            matches!(e.edge_type, EdgeType::Imports | EdgeType::ImportsFrom)
                        })
                        .unwrap_or(false)
                });
            if has_import {
                import_neighbors.push(neighbor_id);
            }
        }

        adjacency.insert(node_id, import_neighbors);
    }

    adjacency
}

/// DFS that looks for a path from `current` back to `target` within the SCC.
///
/// Returns the cycle path including `target` at both start and end, e.g.
/// `[target, a, b, target]`, or `None` if no cycle within `max_cycle_length`.
fn dfs_find_cycle(
    current: NodeId,
    target: NodeId,
    adjacency: &HashMap<NodeId, Vec<NodeId>>,
    scc_set: &HashSet<NodeId>,
    visited: &mut Vec<NodeId>,
    max_cycle_length: usize,
) -> Option<Vec<NodeId>> {
    // Exceeded length limit (visited does not include the start/end target node yet).
    if visited.len() >= max_cycle_length {
        return None;
    }

    let neighbors = adjacency.get(&current)?;

    for &neighbor in neighbors {
        // Found a cycle back to target.
        if neighbor == target && !visited.is_empty() {
            let mut cycle = vec![target];
            cycle.extend_from_slice(visited);
            cycle.push(target);
            return Some(cycle);
        }

        // Only follow edges within the SCC, avoid revisiting nodes.
        if !scc_set.contains(&neighbor) || visited.contains(&neighbor) || neighbor == target {
            continue;
        }

        visited.push(neighbor);
        if let Some(cycle) = dfs_find_cycle(neighbor, target, adjacency, scc_set, visited, max_cycle_length) {
            return Some(cycle);
        }
        visited.pop();
    }

    None
}

/// Produce a canonical form of a cycle (without the repeated tail element) by
/// rotating so the minimum NodeId comes first.
fn canonical_cycle(cycle: &[NodeId]) -> Vec<NodeId> {
    // cycle is [a, b, c, ..., a]; strip the repeated last element.
    let body = if cycle.last() == cycle.first() {
        &cycle[..cycle.len().saturating_sub(1)]
    } else {
        cycle
    };

    if body.is_empty() {
        return vec![];
    }

    let min_pos = body
        .iter()
        .enumerate()
        .min_by_key(|(_, &id)| id)
        .map(|(i, _)| i)
        .unwrap_or(0);

    let mut rotated = body[min_pos..].to_vec();
    rotated.extend_from_slice(&body[..min_pos]);
    rotated
}
