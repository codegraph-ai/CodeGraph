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
}
