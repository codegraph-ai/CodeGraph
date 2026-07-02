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
pub(crate) fn find_circular_deps(graph: &CodeGraph, max_cycle_length: usize) -> CircularDepsResult {
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
    let node_to_path: HashMap<NodeId, String> = file_nodes.iter().cloned().collect();
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
                        .map(|e| matches!(e.edge_type, EdgeType::Imports | EdgeType::ImportsFrom))
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
        if let Some(cycle) = dfs_find_cycle(
            neighbor,
            target,
            adjacency,
            scc_set,
            visited,
            max_cycle_length,
        ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{PropertyMap, PropertyValue};

    fn adjacency(edges: &[(NodeId, &[NodeId])]) -> HashMap<NodeId, Vec<NodeId>> {
        edges
            .iter()
            .map(|(id, neighbors)| (*id, neighbors.to_vec()))
            .collect()
    }

    fn add_node(graph: &mut CodeGraph, ty: NodeType, name: &str, path: &str) -> NodeId {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), PropertyValue::String(name.to_string()));
        props.insert("path".to_string(), PropertyValue::String(path.to_string()));
        graph.add_node(ty, props).expect("add_node")
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    #[test]
    fn build_import_adjacency_links_files_over_import_edge() {
        // A imports B; both are files in the restriction set.
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::CodeFile, "a.rs", "/src/a.rs");
        let b = add_node(&mut g, NodeType::CodeFile, "b.rs", "/src/b.rs");
        edge(&mut g, a, b, EdgeType::Imports);
        let set: HashSet<NodeId> = [a, b].into_iter().collect();

        let adj = build_import_adjacency(&g, &set);
        // Every file in the set gets an entry; A points at B, B has none.
        assert_eq!(adj.get(&a), Some(&vec![b]));
        assert_eq!(adj.get(&b), Some(&Vec::<NodeId>::new()));
    }

    #[test]
    fn build_import_adjacency_counts_imports_from_edge() {
        // ImportsFrom is treated the same as Imports.
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::CodeFile, "a.rs", "/src/a.rs");
        let b = add_node(&mut g, NodeType::CodeFile, "b.rs", "/src/b.rs");
        edge(&mut g, a, b, EdgeType::ImportsFrom);
        let set: HashSet<NodeId> = [a, b].into_iter().collect();

        let adj = build_import_adjacency(&g, &set);
        assert_eq!(adj.get(&a), Some(&vec![b]));
    }

    #[test]
    fn build_import_adjacency_skips_neighbor_outside_file_set() {
        // A imports module M, but M is not in the restriction set.
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::CodeFile, "a.rs", "/src/a.rs");
        let m = add_node(&mut g, NodeType::Module, "serde", "");
        edge(&mut g, a, m, EdgeType::Imports);
        let set: HashSet<NodeId> = [a].into_iter().collect();

        let adj = build_import_adjacency(&g, &set);
        assert_eq!(adj.get(&a), Some(&Vec::<NodeId>::new()));
    }

    #[test]
    fn build_import_adjacency_ignores_non_import_edges() {
        // A calls B (both files in set) but there is no import edge.
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_node(&mut g, NodeType::CodeFile, "a.rs", "/src/a.rs");
        let b = add_node(&mut g, NodeType::CodeFile, "b.rs", "/src/b.rs");
        edge(&mut g, a, b, EdgeType::Calls);
        let set: HashSet<NodeId> = [a, b].into_iter().collect();

        let adj = build_import_adjacency(&g, &set);
        assert_eq!(adj.get(&a), Some(&Vec::<NodeId>::new()));
    }

    #[test]
    fn empty_result_has_no_cycles() {
        let result = CircularDepsResult::empty();
        assert!(result.cycles.is_empty());
        assert_eq!(result.total_cycles, 0);
        assert!(!result.has_circular_deps);
    }

    #[test]
    fn canonical_cycle_rotates_to_min_first() {
        // [3, 1, 2, 3] -> body [3, 1, 2] -> min (1) at pos 1 -> [1, 2, 3].
        assert_eq!(canonical_cycle(&[3, 1, 2, 3]), vec![1, 2, 3]);
    }

    #[test]
    fn canonical_cycle_already_min_first_is_unchanged() {
        // [1, 2, 3, 1] -> body [1, 2, 3] already starts at min.
        assert_eq!(canonical_cycle(&[1, 2, 3, 1]), vec![1, 2, 3]);
    }

    #[test]
    fn canonical_cycle_normalizes_rotations_to_same_form() {
        // A→B→C and B→C→A canonicalize identically.
        assert_eq!(
            canonical_cycle(&[1, 2, 3, 1]),
            canonical_cycle(&[2, 3, 1, 2])
        );
    }

    #[test]
    fn canonical_cycle_without_repeated_tail() {
        // No repeated last element: whole slice is the body.
        assert_eq!(canonical_cycle(&[2, 3, 1]), vec![1, 2, 3]);
    }

    #[test]
    fn canonical_cycle_empty_is_empty() {
        assert_eq!(canonical_cycle(&[]), Vec::<NodeId>::new());
    }

    #[test]
    fn dfs_finds_two_node_cycle() {
        // 1 -> 2 -> 1
        let adj = adjacency(&[(1, &[2]), (2, &[1])]);
        let scc: HashSet<NodeId> = [1, 2].into_iter().collect();
        let cycle = dfs_find_cycle(1, 1, &adj, &scc, &mut Vec::new(), 10);
        assert_eq!(cycle, Some(vec![1, 2, 1]));
    }

    #[test]
    fn dfs_finds_three_node_cycle() {
        // 1 -> 2 -> 3 -> 1
        let adj = adjacency(&[(1, &[2]), (2, &[3]), (3, &[1])]);
        let scc: HashSet<NodeId> = [1, 2, 3].into_iter().collect();
        let cycle = dfs_find_cycle(1, 1, &adj, &scc, &mut Vec::new(), 10);
        assert_eq!(cycle, Some(vec![1, 2, 3, 1]));
    }

    #[test]
    fn dfs_returns_none_without_cycle() {
        // 1 -> 2, dead end.
        let adj = adjacency(&[(1, &[2]), (2, &[])]);
        let scc: HashSet<NodeId> = [1, 2].into_iter().collect();
        let cycle = dfs_find_cycle(1, 1, &adj, &scc, &mut Vec::new(), 10);
        assert_eq!(cycle, None);
    }

    #[test]
    fn dfs_respects_max_cycle_length() {
        // A 2-node cycle needs one intermediate hop; max length 1 forbids it.
        let adj = adjacency(&[(1, &[2]), (2, &[1])]);
        let scc: HashSet<NodeId> = [1, 2].into_iter().collect();
        let cycle = dfs_find_cycle(1, 1, &adj, &scc, &mut Vec::new(), 1);
        assert_eq!(cycle, None);
    }

    #[test]
    fn dfs_ignores_neighbors_outside_scc() {
        // 1 -> 2 (out of SCC) -> 1: the intermediate node isn't in the SCC set,
        // so no cycle path is followed through it.
        let adj = adjacency(&[(1, &[2]), (2, &[1])]);
        let scc: HashSet<NodeId> = [1].into_iter().collect();
        let cycle = dfs_find_cycle(1, 1, &adj, &scc, &mut Vec::new(), 10);
        assert_eq!(cycle, None);
    }
}
