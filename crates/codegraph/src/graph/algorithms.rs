// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Graph traversal and analysis algorithms.
//!
//! Provides BFS, DFS, cycle detection (Tarjan's SCC), and path finding algorithms
//! optimized for code dependency analysis.

use crate::error::Result;
use crate::graph::{CodeGraph, Direction, NodeId};
use std::collections::{HashMap, HashSet, VecDeque};

/// Breadth-First Search traversal from a starting node.
///
/// Returns all reachable nodes within the specified depth limit.
///
/// # Parameters
/// - `graph`: The graph to traverse
/// - `start`: Starting node ID
/// - `direction`: Follow outgoing or incoming edges
/// - `max_depth`: Optional maximum depth (None for unlimited)
///
/// # Returns
/// Vec of reachable node IDs (excluding the start node)
pub fn bfs(
    graph: &CodeGraph,
    start: NodeId,
    direction: Direction,
    max_depth: Option<usize>,
) -> Result<Vec<NodeId>> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut result = Vec::new();

    visited.insert(start);
    queue.push_back((start, 0)); // (node_id, depth)

    while let Some((current, depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(max) = max_depth {
            if depth >= max {
                continue;
            }
        }

        // Get neighbors
        let neighbors = graph.get_neighbors(current, direction)?;

        for neighbor_id in neighbors {
            if !visited.contains(&neighbor_id) {
                visited.insert(neighbor_id);
                result.push(neighbor_id);
                queue.push_back((neighbor_id, depth + 1));
            }
        }
    }

    Ok(result)
}

/// Depth-First Search traversal from a starting node (iterative implementation).
///
/// Uses an iterative approach to avoid stack overflow on deep graphs.
///
/// # Parameters
/// - `graph`: The graph to traverse
/// - `start`: Starting node ID
/// - `direction`: Follow outgoing or incoming edges
/// - `max_depth`: Optional maximum depth (None for unlimited)
///
/// # Returns
/// Vec of reachable node IDs (excluding the start node)
pub fn dfs(
    graph: &CodeGraph,
    start: NodeId,
    direction: Direction,
    max_depth: Option<usize>,
) -> Result<Vec<NodeId>> {
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    let mut result = Vec::new();

    visited.insert(start);
    stack.push((start, 0)); // (node_id, depth)

    while let Some((current, depth)) = stack.pop() {
        // Check depth limit
        if let Some(max) = max_depth {
            if depth >= max {
                continue;
            }
        }

        // Get neighbors
        let neighbors = graph.get_neighbors(current, direction)?;

        for neighbor_id in neighbors {
            if !visited.contains(&neighbor_id) {
                visited.insert(neighbor_id);
                result.push(neighbor_id);
                stack.push((neighbor_id, depth + 1));
            }
        }
    }

    Ok(result)
}

/// Find all strongly connected components using Tarjan's algorithm.
///
/// A strongly connected component is a maximal set of nodes where every node
/// is reachable from every other node. In code graphs, these represent
/// circular dependencies.
///
/// # Parameters
/// - `graph`: The graph to analyze
///
/// # Returns
/// Vec of SCCs, where each SCC is a Vec of node IDs
pub fn find_strongly_connected_components(graph: &CodeGraph) -> Result<Vec<Vec<NodeId>>> {
    let mut index = 0;
    let mut stack = Vec::new();
    let mut indices: HashMap<NodeId, usize> = HashMap::new();
    let mut lowlinks: HashMap<NodeId, usize> = HashMap::new();
    let mut on_stack: HashSet<NodeId> = HashSet::new();
    let mut sccs = Vec::new();

    // Process all nodes to handle disconnected components
    for node_id in 0..graph.node_count() as u64 {
        if graph.get_node(node_id).is_ok() && !indices.contains_key(&node_id) {
            strongconnect(
                graph,
                node_id,
                &mut index,
                &mut indices,
                &mut lowlinks,
                &mut stack,
                &mut on_stack,
                &mut sccs,
            )?;
        }
    }

    // Filter to only return SCCs with more than one node (actual cycles)
    Ok(sccs.into_iter().filter(|scc| scc.len() > 1).collect())
}

/// Helper function for Tarjan's algorithm
#[allow(clippy::too_many_arguments)]
fn strongconnect(
    graph: &CodeGraph,
    v: NodeId,
    index: &mut usize,
    indices: &mut HashMap<NodeId, usize>,
    lowlinks: &mut HashMap<NodeId, usize>,
    stack: &mut Vec<NodeId>,
    on_stack: &mut HashSet<NodeId>,
    sccs: &mut Vec<Vec<NodeId>>,
) -> Result<()> {
    indices.insert(v, *index);
    lowlinks.insert(v, *index);
    *index += 1;
    stack.push(v);
    on_stack.insert(v);

    // Consider successors of v
    let neighbors = graph.get_neighbors(v, Direction::Outgoing)?;
    for w in neighbors {
        if !indices.contains_key(&w) {
            // Successor w has not yet been visited; recurse on it
            strongconnect(graph, w, index, indices, lowlinks, stack, on_stack, sccs)?;
            let w_lowlink = *lowlinks.get(&w).unwrap();
            let v_lowlink = *lowlinks.get(&v).unwrap();
            lowlinks.insert(v, v_lowlink.min(w_lowlink));
        } else if on_stack.contains(&w) {
            // Successor w is in stack and hence in the current SCC
            let w_index = *indices.get(&w).unwrap();
            let v_lowlink = *lowlinks.get(&v).unwrap();
            lowlinks.insert(v, v_lowlink.min(w_index));
        }
    }

    // If v is a root node, pop the stack and generate an SCC
    if lowlinks.get(&v) == indices.get(&v) {
        let mut scc = Vec::new();
        loop {
            let w = stack.pop().unwrap();
            on_stack.remove(&w);
            scc.push(w);
            if w == v {
                break;
            }
        }
        sccs.push(scc);
    }

    Ok(())
}

/// Find all paths between two nodes up to a maximum depth.
///
/// Uses DFS to enumerate all possible paths. Depth limit prevents
/// infinite loops in cyclic graphs.
///
/// # Parameters
/// - `graph`: The graph to search
/// - `start`: Starting node ID
/// - `end`: Target node ID
/// - `max_depth`: Maximum path length (required)
///
/// # Returns
/// Vec of paths, where each path is a Vec of node IDs from start to end
pub fn find_all_paths(
    graph: &CodeGraph,
    start: NodeId,
    end: NodeId,
    max_depth: Option<usize>,
) -> Result<Vec<Vec<NodeId>>> {
    let max_depth = max_depth.unwrap_or(100); // Default limit to prevent infinite loops
    let mut paths = Vec::new();
    let mut current_path = vec![start];
    let mut visited = HashSet::new();
    visited.insert(start);

    find_paths_recursive(
        graph,
        start,
        end,
        &mut current_path,
        &mut visited,
        &mut paths,
        max_depth,
    )?;

    Ok(paths)
}

/// Recursive helper for path finding
fn find_paths_recursive(
    graph: &CodeGraph,
    current: NodeId,
    end: NodeId,
    current_path: &mut Vec<NodeId>,
    visited: &mut HashSet<NodeId>,
    paths: &mut Vec<Vec<NodeId>>,
    max_depth: usize,
) -> Result<()> {
    // Check depth limit
    if current_path.len() >= max_depth {
        return Ok(());
    }

    // Check if we reached the target
    if current == end {
        paths.push(current_path.clone());
        return Ok(());
    }

    // Explore neighbors
    let neighbors = graph.get_neighbors(current, Direction::Outgoing)?;
    for neighbor in neighbors {
        if !visited.contains(&neighbor) {
            visited.insert(neighbor);
            current_path.push(neighbor);

            find_paths_recursive(
                graph,
                neighbor,
                end,
                current_path,
                visited,
                paths,
                max_depth,
            )?;

            current_path.pop();
            visited.remove(&neighbor);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers;

    #[test]
    fn test_bfs_simple_chain() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();

        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, c, vec![]).unwrap();

        let result = bfs(&graph, a, Direction::Outgoing, None).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&b));
        assert!(result.contains(&c));
    }

    #[test]
    fn test_dfs_simple_chain() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();

        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, c, vec![]).unwrap();

        let result = dfs(&graph, a, Direction::Outgoing, None).unwrap();
        assert_eq!(result.len(), 2);
    }

    /// Build the acyclic chain a -> b -> c and return (graph, a, b, c).
    fn chain() -> (CodeGraph, NodeId, NodeId, NodeId) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, c, vec![]).unwrap();
        (graph, a, b, c)
    }

    #[test]
    fn test_bfs_respects_max_depth() {
        let (graph, a, b, _c) = chain();
        // Depth 1 expands only the start's direct neighbors.
        let result = bfs(&graph, a, Direction::Outgoing, Some(1)).unwrap();
        assert_eq!(result, vec![b]);
    }

    #[test]
    fn test_bfs_incoming_direction() {
        let (graph, a, b, c) = chain();
        // Walking incoming edges from the tail reaches both ancestors.
        let result = bfs(&graph, c, Direction::Incoming, None).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&a));
        assert!(result.contains(&b));
    }

    #[test]
    fn test_bfs_cycle_terminates_and_dedups() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, a, vec![]).unwrap();
        // The start node is pre-marked visited, so the cycle back to `a`
        // does not re-add it; only `b` is returned.
        let result = bfs(&graph, a, Direction::Outgoing, None).unwrap();
        assert_eq!(result, vec![b]);
    }

    #[test]
    fn test_dfs_respects_max_depth() {
        let (graph, a, b, _c) = chain();
        let result = dfs(&graph, a, Direction::Outgoing, Some(1)).unwrap();
        assert_eq!(result, vec![b]);
    }

    #[test]
    fn test_dfs_incoming_direction() {
        // Mirror of the bfs incoming test: dfs walking incoming edges from the
        // tail must reach both ancestors. bfs exercised Direction::Incoming but
        // dfs only ever ran Outgoing, leaving its direction arg unpinned.
        let (graph, a, b, c) = chain();
        let result = dfs(&graph, c, Direction::Incoming, None).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&a));
        assert!(result.contains(&b));
    }

    #[test]
    fn test_dfs_cycle_terminates_and_dedups() {
        // dfs mirror of the bfs cycle test: the start node is pre-marked
        // visited, so the back edge to `a` does not re-add it and traversal
        // terminates. dfs's visited-set dedup on a cycle was previously untested.
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, a, vec![]).unwrap();
        let result = dfs(&graph, a, Direction::Outgoing, None).unwrap();
        assert_eq!(result, vec![b]);
    }

    #[test]
    fn test_bfs_max_depth_zero_returns_empty() {
        // The depth guard is `depth >= max`, so max_depth Some(0) fires on the
        // very first pop (depth 0 >= 0) before any neighbor is expanded. Prior
        // tests only used Some(1), never the depth==max boundary at zero.
        let (graph, a, _b, _c) = chain();
        assert!(bfs(&graph, a, Direction::Outgoing, Some(0))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_dfs_max_depth_zero_returns_empty() {
        let (graph, a, _b, _c) = chain();
        assert!(dfs(&graph, a, Direction::Outgoing, Some(0))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_scc_no_cycle_is_empty() {
        let (graph, _a, _b, _c) = chain();
        assert!(find_strongly_connected_components(&graph)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_scc_detects_two_node_cycle() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();
        // a <-> b is a cycle; c only depends on a (no back edge).
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, a, vec![]).unwrap();
        helpers::add_import(&mut graph, c, a, vec![]).unwrap();

        let sccs = find_strongly_connected_components(&graph).unwrap();
        assert_eq!(sccs.len(), 1);
        let scc = &sccs[0];
        assert_eq!(scc.len(), 2);
        assert!(scc.contains(&a) && scc.contains(&b));
    }

    #[test]
    fn test_scc_detects_multiple_disconnected_cycles() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();
        let d = helpers::add_file(&mut graph, "d.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, a, vec![]).unwrap();
        helpers::add_import(&mut graph, c, d, vec![]).unwrap();
        helpers::add_import(&mut graph, d, c, vec![]).unwrap();

        let sccs = find_strongly_connected_components(&graph).unwrap();
        assert_eq!(sccs.len(), 2);
        assert!(sccs.iter().all(|scc| scc.len() == 2));
    }

    #[test]
    fn test_find_all_paths_single_path() {
        let (graph, a, b, c) = chain();
        let paths = find_all_paths(&graph, a, c, None).unwrap();
        assert_eq!(paths, vec![vec![a, b, c]]);
    }

    #[test]
    fn test_find_all_paths_start_equals_end() {
        // When start == end, find_paths_recursive hits the `current == end`
        // target check on the very first call (current_path == [start]) and
        // returns a single length-1 path without traversing any edge. Every
        // prior test used distinct start/end, so this immediate-hit branch
        // (ordered before neighbor exploration) was never exercised.
        let (graph, a, _b, _c) = chain();
        let paths = find_all_paths(&graph, a, a, None).unwrap();
        assert_eq!(paths, vec![vec![a]]);
    }

    #[test]
    fn test_find_all_paths_diamond_multiple() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();
        let d = helpers::add_file(&mut graph, "d.py", "python").unwrap();
        // a -> b -> d and a -> c -> d: two distinct paths.
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, a, c, vec![]).unwrap();
        helpers::add_import(&mut graph, b, d, vec![]).unwrap();
        helpers::add_import(&mut graph, c, d, vec![]).unwrap();

        let paths = find_all_paths(&graph, a, d, None).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths
            .iter()
            .all(|p| p.first() == Some(&a) && p.last() == Some(&d)));
    }

    #[test]
    fn test_find_all_paths_no_path_returns_empty() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        // c is unreachable from a.
        let paths = find_all_paths(&graph, a, c, None).unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_find_all_paths_depth_check_preempts_target() {
        let (graph, a, _b, c) = chain();
        // The [a, b, c] path has length 3. The depth guard fires when
        // current_path.len() >= max_depth *before* the target check, so
        // max_depth must exceed the path length: 3 finds nothing, 4 finds it.
        assert!(find_all_paths(&graph, a, c, Some(3)).unwrap().is_empty());
        let paths = find_all_paths(&graph, a, c, Some(4)).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].first(), Some(&a));
        assert_eq!(paths[0].last(), Some(&c));
    }

    #[test]
    fn test_find_all_paths_skips_back_edge_to_ancestor() {
        // a -> b -> c plus a back edge b -> a. Searching a..c, when the recursion
        // reaches b it explores both neighbors: c (the target) and a. Because a is
        // the start and is already in `visited`, the `!visited.contains(&neighbor)`
        // guard takes its false arm and the back edge is skipped, so exactly one
        // path is enumerated. Every prior path test was acyclic or returned at the
        // target before any back edge, leaving that visited-skip arm unexercised.
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, c, vec![]).unwrap();
        helpers::add_import(&mut graph, b, a, vec![]).unwrap();

        let paths = find_all_paths(&graph, a, c, None).unwrap();
        assert_eq!(paths, vec![vec![a, b, c]]);
    }

    #[test]
    fn test_find_all_paths_terminates_on_deep_cycle() {
        // a -> b -> c -> a is a 3-node cycle; c also branches to d. Searching a..d,
        // when the recursion reaches c it hits the back edge c -> a (a already on the
        // path/visited) and skips it, then follows c -> d to the target. This drives
        // the visited-skip arm one level deeper than the two-node case and confirms
        // the traversal terminates rather than looping the cycle forever.
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        let c = helpers::add_file(&mut graph, "c.py", "python").unwrap();
        let d = helpers::add_file(&mut graph, "d.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();
        helpers::add_import(&mut graph, b, c, vec![]).unwrap();
        helpers::add_import(&mut graph, c, a, vec![]).unwrap();
        helpers::add_import(&mut graph, c, d, vec![]).unwrap();

        let paths = find_all_paths(&graph, a, d, None).unwrap();
        assert_eq!(paths, vec![vec![a, b, c, d]]);
    }
}
