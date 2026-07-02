// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Convenience helpers for common code entities and relationships.
//!
//! This module provides higher-level abstractions for working with code graphs,
//! reducing boilerplate for common operations like adding files, functions, classes,
//! and tracking relationships between them.

use crate::error::Result;
use crate::graph::{CodeGraph, Direction, EdgeId, EdgeType, NodeId, NodeType, PropertyMap};

/// Metadata for a function with extended properties.
pub struct FunctionMetadata<'a> {
    /// Function name
    pub name: &'a str,
    /// Starting line number
    pub line_start: i64,
    /// Ending line number
    pub line_end: i64,
    /// Visibility modifier (e.g., "public", "private")
    pub visibility: &'a str,
    /// Function signature string
    pub signature: &'a str,
    /// Whether the function is async
    pub is_async: bool,
    /// Whether the function is a test
    pub is_test: bool,
}

/// Add a file node to the graph.
///
/// Creates a CodeFile node with path and language properties.
///
/// # Arguments
///
/// * `graph` - The code graph to add the file to
/// * `path` - File path (e.g., "src/main.rs")
/// * `language` - Programming language (e.g., "rust", "python")
///
/// # Returns
///
/// The ID of the created file node.
pub fn add_file(graph: &mut CodeGraph, path: &str, language: &str) -> Result<NodeId> {
    let props = PropertyMap::new()
        .with("path", path)
        .with("language", language);

    graph.add_node(NodeType::CodeFile, props)
}

/// Add a function node and automatically link it to a file.
///
/// Creates a Function node and a Contains edge from the file to the function.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The ID of the file containing this function
/// * `name` - Function name
/// * `line_start` - Starting line number
/// * `line_end` - Ending line number
///
/// # Returns
///
/// The ID of the created function node.
pub fn add_function(
    graph: &mut CodeGraph,
    file_id: NodeId,
    name: &str,
    line_start: i64,
    line_end: i64,
) -> Result<NodeId> {
    let props = PropertyMap::new()
        .with("name", name)
        .with("line_start", line_start)
        .with("line_end", line_end);

    let func_id = graph.add_node(NodeType::Function, props)?;

    // Auto-create Contains edge
    graph.add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())?;

    Ok(func_id)
}

/// Add a function node with extended metadata.
///
/// Creates a Function node with additional properties like visibility, signature, etc.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The ID of the file containing this function
/// * `metadata` - Function metadata including name, lines, visibility, etc.
///
/// # Returns
///
/// The ID of the created function node.
pub fn add_function_with_metadata(
    graph: &mut CodeGraph,
    file_id: NodeId,
    metadata: FunctionMetadata,
) -> Result<NodeId> {
    let props = PropertyMap::new()
        .with("name", metadata.name)
        .with("line_start", metadata.line_start)
        .with("line_end", metadata.line_end)
        .with("visibility", metadata.visibility)
        .with("signature", metadata.signature)
        .with("is_async", metadata.is_async)
        .with("is_test", metadata.is_test);

    let func_id = graph.add_node(NodeType::Function, props)?;

    // Auto-create Contains edge
    graph.add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())?;

    Ok(func_id)
}

/// Add a class node and automatically link it to a file.
///
/// Creates a Class node and a Contains edge from the file to the class.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The ID of the file containing this class
/// * `name` - Class name
/// * `line_start` - Starting line number
/// * `line_end` - Ending line number
///
/// # Returns
///
/// The ID of the created class node.
pub fn add_class(
    graph: &mut CodeGraph,
    file_id: NodeId,
    name: &str,
    line_start: i64,
    line_end: i64,
) -> Result<NodeId> {
    let props = PropertyMap::new()
        .with("name", name)
        .with("line_start", line_start)
        .with("line_end", line_end);

    let class_id = graph.add_node(NodeType::Class, props)?;

    // Auto-create Contains edge
    graph.add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())?;

    Ok(class_id)
}

/// Add a method node and link it to a class.
///
/// Creates a Function node and a Contains edge from the class to the method.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `class_id` - The ID of the class containing this method
/// * `name` - Method name
/// * `line_start` - Starting line number
/// * `line_end` - Ending line number
///
/// # Returns
///
/// The ID of the created method node.
pub fn add_method(
    graph: &mut CodeGraph,
    class_id: NodeId,
    name: &str,
    line_start: i64,
    line_end: i64,
) -> Result<NodeId> {
    let props = PropertyMap::new()
        .with("name", name)
        .with("line_start", line_start)
        .with("line_end", line_end);

    let method_id = graph.add_node(NodeType::Function, props)?;

    // Link to class
    graph.add_edge(class_id, method_id, EdgeType::Contains, PropertyMap::new())?;

    Ok(method_id)
}

/// Add a module node to the graph.
///
/// Creates a Module node with name and path properties.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `name` - Module name
/// * `path` - Module path
///
/// # Returns
///
/// The ID of the created module node.
pub fn add_module(graph: &mut CodeGraph, name: &str, path: &str) -> Result<NodeId> {
    let props = PropertyMap::new().with("name", name).with("path", path);

    graph.add_node(NodeType::Module, props)
}

/// Add a function call relationship with line metadata.
///
/// Creates a Calls edge from caller to callee with the line number where the call occurs.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `caller_id` - The ID of the calling function
/// * `callee_id` - The ID of the called function
/// * `line` - Line number where the call occurs
///
/// # Returns
///
/// The ID of the created Calls edge.
pub fn add_call(
    graph: &mut CodeGraph,
    caller_id: NodeId,
    callee_id: NodeId,
    line: i64,
) -> Result<EdgeId> {
    let props = PropertyMap::new().with("line", line);
    graph.add_edge(caller_id, callee_id, EdgeType::Calls, props)
}

/// Add an import relationship with imported symbols.
///
/// Creates an Imports edge from one file to another with a list of imported symbols.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `from_file_id` - The ID of the file doing the import
/// * `to_file_id` - The ID of the file being imported
/// * `symbols` - List of imported symbol names
///
/// # Returns
///
/// The ID of the created Imports edge.
pub fn add_import(
    graph: &mut CodeGraph,
    from_file_id: NodeId,
    to_file_id: NodeId,
    symbols: Vec<&str>,
) -> Result<EdgeId> {
    let symbol_strings: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
    let props = PropertyMap::new().with("symbols", symbol_strings);
    graph.add_edge(from_file_id, to_file_id, EdgeType::Imports, props)
}

/// Create a generic Contains edge between two nodes.
///
/// This is useful for linking any entity to a file.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `container_id` - The ID of the containing node (e.g., file)
/// * `contained_id` - The ID of the contained node
///
/// # Returns
///
/// The ID of the created Contains edge.
pub fn link_to_file(
    graph: &mut CodeGraph,
    container_id: NodeId,
    contained_id: NodeId,
) -> Result<EdgeId> {
    graph.add_edge(
        container_id,
        contained_id,
        EdgeType::Contains,
        PropertyMap::new(),
    )
}

/// Get all functions that call the given function.
///
/// Returns the node IDs of all functions with incoming Calls edges.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `function_id` - The ID of the function to find callers for
///
/// # Returns
///
/// Vector of node IDs of functions that call this function.
pub fn get_callers(graph: &CodeGraph, function_id: NodeId) -> Result<Vec<NodeId>> {
    let incoming = graph.get_neighbors(function_id, Direction::Incoming)?;

    let mut callers = Vec::new();
    for neighbor_id in incoming {
        // Check if the edge is a Calls edge
        let edges = graph.get_edges_between(neighbor_id, function_id)?;
        for edge_id in edges {
            let edge = graph.get_edge(edge_id)?;
            if edge.edge_type == EdgeType::Calls {
                callers.push(neighbor_id);
                break;
            }
        }
    }

    Ok(callers)
}

/// Get all functions called by the given function.
///
/// Returns the node IDs of all functions with outgoing Calls edges.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `function_id` - The ID of the function to find callees for
///
/// # Returns
///
/// Vector of node IDs of functions called by this function.
pub fn get_callees(graph: &CodeGraph, function_id: NodeId) -> Result<Vec<NodeId>> {
    let outgoing = graph.get_neighbors(function_id, Direction::Outgoing)?;

    let mut callees = Vec::new();
    for neighbor_id in outgoing {
        // Check if the edge is a Calls edge
        let edges = graph.get_edges_between(function_id, neighbor_id)?;
        for edge_id in edges {
            let edge = graph.get_edge(edge_id)?;
            if edge.edge_type == EdgeType::Calls {
                callees.push(neighbor_id);
                break;
            }
        }
    }

    Ok(callees)
}

/// Get all functions contained in a file.
///
/// Returns the node IDs of all Function nodes connected to the file via Contains edges.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The ID of the file to find functions in
///
/// # Returns
///
/// Vector of node IDs of functions in this file.
pub fn get_functions_in_file(graph: &CodeGraph, file_id: NodeId) -> Result<Vec<NodeId>> {
    let contained = graph.get_neighbors(file_id, Direction::Outgoing)?;

    let mut functions = Vec::new();
    for node_id in contained {
        let node = graph.get_node(node_id)?;
        // Only include Function nodes
        if node.node_type == NodeType::Function {
            functions.push(node_id);
        }
    }

    Ok(functions)
}

/// Get all files that a file depends on (imports from).
///
/// Returns the node IDs of all files connected via outgoing Imports or ImportsFrom edges.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The ID of the file to find dependencies for
///
/// # Returns
///
/// Vector of node IDs of files that this file imports.
pub fn get_file_dependencies(graph: &CodeGraph, file_id: NodeId) -> Result<Vec<NodeId>> {
    let outgoing = graph.get_neighbors(file_id, Direction::Outgoing)?;

    let mut dependencies = Vec::new();
    for neighbor_id in outgoing {
        // Check if the edge is Imports or ImportsFrom
        let edges = graph.get_edges_between(file_id, neighbor_id)?;
        for edge_id in edges {
            let edge = graph.get_edge(edge_id)?;
            if edge.edge_type == EdgeType::Imports || edge.edge_type == EdgeType::ImportsFrom {
                dependencies.push(neighbor_id);
                break;
            }
        }
    }

    Ok(dependencies)
}

/// Get all files that depend on this file (import this file).
///
/// Returns the node IDs of all files connected via incoming Imports or ImportsFrom edges.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The ID of the file to find dependents for
///
/// # Returns
///
/// Vector of node IDs of files that import this file.
pub fn get_file_dependents(graph: &CodeGraph, file_id: NodeId) -> Result<Vec<NodeId>> {
    let incoming = graph.get_neighbors(file_id, Direction::Incoming)?;

    let mut dependents = Vec::new();
    for neighbor_id in incoming {
        // Check if the edge is Imports or ImportsFrom
        let edges = graph.get_edges_between(neighbor_id, file_id)?;
        for edge_id in edges {
            let edge = graph.get_edge(edge_id)?;
            if edge.edge_type == EdgeType::Imports || edge.edge_type == EdgeType::ImportsFrom {
                dependents.push(neighbor_id);
                break;
            }
        }
    }

    Ok(dependents)
}

// ===== File Lookup Helpers =====

/// Find a file node by its path.
///
/// Searches for a CodeFile node whose "path" property matches the given path string.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `path` - The file path to search for (e.g., "src/main.rs")
///
/// # Returns
///
/// `Some(NodeId)` if a matching file node is found, `None` otherwise.
pub fn find_file_by_path(graph: &CodeGraph, path: &str) -> Result<Option<NodeId>> {
    let results = graph
        .query()
        .node_type(NodeType::CodeFile)
        .property("path", path)
        .limit(1)
        .execute()?;

    Ok(results.into_iter().next())
}

/// Convert a slice of node IDs to their corresponding file paths.
///
/// Looks up each node and extracts the "path" property. Nodes that don't exist
/// or don't have a path property are silently skipped.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `node_ids` - Slice of node IDs to resolve
///
/// # Returns
///
/// Vector of `(NodeId, String)` tuples for each successfully resolved node.
pub fn node_ids_to_paths(graph: &CodeGraph, node_ids: &[NodeId]) -> Result<Vec<(NodeId, String)>> {
    let mut result = Vec::with_capacity(node_ids.len());

    for &id in node_ids {
        if let Ok(node) = graph.get_node(id) {
            if let Some(path) = node.properties.get_string("path") {
                result.push((id, path.to_string()));
            }
        }
    }

    Ok(result)
}

// ===== Transitive Dependency Analysis =====

/// Find all transitive dependencies of a file (what it imports, directly or indirectly).
///
/// Uses BFS to follow Imports/ImportsFrom edges to find all files that this file
/// depends on, directly or transitively. Handles cycles gracefully.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The starting file node ID
/// * `max_depth` - Optional maximum depth to traverse (None for unlimited)
///
/// # Returns
///
/// Vector of node IDs of all files this file depends on (transitively).
pub fn transitive_dependencies(
    graph: &CodeGraph,
    file_id: NodeId,
    max_depth: Option<usize>,
) -> Result<Vec<NodeId>> {
    use std::collections::{HashSet, VecDeque};

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut result = Vec::new();

    visited.insert(file_id);
    queue.push_back((file_id, 0));

    while let Some((current, depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(max) = max_depth {
            if depth >= max {
                continue;
            }
        }

        // Get direct dependencies
        let deps = get_file_dependencies(graph, current)?;

        for dep_id in deps {
            if !visited.contains(&dep_id) {
                visited.insert(dep_id);
                result.push(dep_id);
                queue.push_back((dep_id, depth + 1));
            }
        }
    }

    Ok(result)
}

/// Find all transitive dependents of a file (what imports it, directly or indirectly).
///
/// Uses reverse BFS to follow incoming Imports/ImportsFrom edges to find all files
/// that depend on this file, directly or transitively. Handles cycles gracefully.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `file_id` - The starting file node ID
/// * `max_depth` - Optional maximum depth to traverse (None for unlimited)
///
/// # Returns
///
/// Vector of node IDs of all files that depend on this file (transitively).
pub fn transitive_dependents(
    graph: &CodeGraph,
    file_id: NodeId,
    max_depth: Option<usize>,
) -> Result<Vec<NodeId>> {
    use std::collections::{HashSet, VecDeque};

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut result = Vec::new();

    visited.insert(file_id);
    queue.push_back((file_id, 0));

    while let Some((current, depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(max) = max_depth {
            if depth >= max {
                continue;
            }
        }

        // Get direct dependents
        let dependents = get_file_dependents(graph, current)?;

        for dependent_id in dependents {
            if !visited.contains(&dependent_id) {
                visited.insert(dependent_id);
                result.push(dependent_id);
                queue.push_back((dependent_id, depth + 1));
            }
        }
    }

    Ok(result)
}

/// Find all call chains (paths) between two functions.
///
/// Uses path finding to discover all possible ways one function can reach another
/// through intermediate function calls.
///
/// # Arguments
///
/// * `graph` - The code graph
/// * `from_func` - Starting function node ID
/// * `to_func` - Target function node ID
/// * `max_depth` - Maximum path length (recommended to prevent infinite search)
///
/// # Returns
///
/// Vector of call chains, where each chain is a Vec of node IDs from start to end.
pub fn call_chain(
    graph: &CodeGraph,
    from_func: NodeId,
    to_func: NodeId,
    max_depth: Option<usize>,
) -> Result<Vec<Vec<NodeId>>> {
    graph.find_all_paths(from_func, to_func, max_depth)
}

/// Detect circular dependencies in file imports.
///
/// Uses Tarjan's strongly connected components algorithm to find groups of files
/// that form circular import chains.
///
/// # Arguments
///
/// * `graph` - The code graph
///
/// # Returns
///
/// Vector of circular dependency groups, where each group is a Vec of file node IDs
/// that form a cycle.
pub fn circular_deps(graph: &CodeGraph) -> Result<Vec<Vec<NodeId>>> {
    // Find all SCCs in the graph
    let sccs = graph.find_strongly_connected_components()?;

    // Filter to only include SCCs that contain CodeFile nodes
    let mut file_cycles = Vec::new();

    for scc in sccs {
        // Check if this SCC contains file nodes
        let mut file_nodes = Vec::new();
        for node_id in &scc {
            if let Ok(node) = graph.get_node(*node_id) {
                if node.node_type == NodeType::CodeFile {
                    file_nodes.push(*node_id);
                }
            }
        }

        // If we found file nodes in this SCC, it's a circular dependency
        if file_nodes.len() > 1 {
            file_cycles.push(file_nodes);
        }
    }

    Ok(file_cycles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::CodeGraph;

    fn graph() -> CodeGraph {
        CodeGraph::in_memory().expect("in-memory graph")
    }

    #[test]
    fn add_function_creates_contains_edge_from_file() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let func = add_function(&mut g, file, "foo", 1, 5).unwrap();

        // The function is contained by the file.
        assert_eq!(get_functions_in_file(&g, file).unwrap(), vec![func]);
        let node = g.get_node(func).unwrap();
        assert_eq!(node.node_type, NodeType::Function);
        assert_eq!(node.properties.get_string("name"), Some("foo"));
        assert_eq!(node.properties.get_int("line_start"), Some(1));
        assert_eq!(node.properties.get_int("line_end"), Some(5));
    }

    #[test]
    fn add_function_with_metadata_stores_all_fields() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let func = add_function_with_metadata(
            &mut g,
            file,
            FunctionMetadata {
                name: "bar",
                line_start: 10,
                line_end: 20,
                visibility: "public",
                signature: "fn bar()",
                is_async: true,
                is_test: true,
            },
        )
        .unwrap();

        let node = g.get_node(func).unwrap();
        assert_eq!(node.properties.get_string("visibility"), Some("public"));
        assert_eq!(node.properties.get_string("signature"), Some("fn bar()"));
        assert_eq!(node.properties.get_bool("is_async"), Some(true));
        assert_eq!(node.properties.get_bool("is_test"), Some(true));
        // Auto-linked to its file.
        assert_eq!(get_functions_in_file(&g, file).unwrap(), vec![func]);
    }

    #[test]
    fn add_method_links_to_class_and_is_a_function() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let class = add_class(&mut g, file, "Widget", 1, 30).unwrap();
        let method = add_method(&mut g, class, "render", 5, 10).unwrap();

        assert_eq!(g.get_node(class).unwrap().node_type, NodeType::Class);
        // A method is a Function node contained by the class, so scanning the
        // class for functions surfaces it.
        assert_eq!(get_functions_in_file(&g, class).unwrap(), vec![method]);
    }

    #[test]
    fn add_module_has_name_and_path() {
        let mut g = graph();
        let m = add_module(&mut g, "utils", "src/utils.rs").unwrap();
        let node = g.get_node(m).unwrap();
        assert_eq!(node.node_type, NodeType::Module);
        assert_eq!(node.properties.get_string("name"), Some("utils"));
        assert_eq!(node.properties.get_string("path"), Some("src/utils.rs"));
    }

    #[test]
    fn get_callers_and_callees_only_follow_calls_edges() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let caller = add_function(&mut g, file, "caller", 1, 5).unwrap();
        let callee = add_function(&mut g, file, "callee", 6, 10).unwrap();
        add_call(&mut g, caller, callee, 3).unwrap();

        assert_eq!(get_callers(&g, callee).unwrap(), vec![caller]);
        assert_eq!(get_callees(&g, caller).unwrap(), vec![callee]);
        // No incoming Calls edge to the caller, no outgoing from the callee.
        assert!(get_callers(&g, caller).unwrap().is_empty());
        assert!(get_callees(&g, callee).unwrap().is_empty());
    }

    #[test]
    fn get_callees_ignores_non_calls_edges() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        // The file Contains the function, but a Contains edge is not a Calls edge.
        let func = add_function(&mut g, file, "foo", 1, 5).unwrap();
        assert!(get_callees(&g, file).unwrap().is_empty());
        assert!(get_callers(&g, func).unwrap().is_empty());
    }

    #[test]
    fn get_functions_in_file_excludes_non_functions() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let func = add_function(&mut g, file, "foo", 1, 5).unwrap();
        let class = add_class(&mut g, file, "Widget", 6, 30).unwrap();
        // Both are contained, but only the Function is returned.
        let funcs = get_functions_in_file(&g, file).unwrap();
        assert_eq!(funcs, vec![func]);
        assert!(!funcs.contains(&class));
    }

    #[test]
    fn file_dependencies_and_dependents_follow_import_edges() {
        let mut g = graph();
        let a = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let b = add_file(&mut g, "src/b.rs", "rust").unwrap();
        add_import(&mut g, a, b, vec!["thing"]).unwrap();

        assert_eq!(get_file_dependencies(&g, a).unwrap(), vec![b]);
        assert_eq!(get_file_dependents(&g, b).unwrap(), vec![a]);
        // Reverse directions are empty.
        assert!(get_file_dependencies(&g, b).unwrap().is_empty());
        assert!(get_file_dependents(&g, a).unwrap().is_empty());
    }

    #[test]
    fn find_file_by_path_matches_and_misses() {
        let mut g = graph();
        let a = add_file(&mut g, "src/a.rs", "rust").unwrap();
        assert_eq!(find_file_by_path(&g, "src/a.rs").unwrap(), Some(a));
        assert_eq!(find_file_by_path(&g, "src/missing.rs").unwrap(), None);
    }

    #[test]
    fn node_ids_to_paths_skips_missing_and_pathless_nodes() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        // A function has no "path" property, and 9999 is a nonexistent id.
        let func = add_function(&mut g, file, "foo", 1, 5).unwrap();
        let resolved = node_ids_to_paths(&g, &[file, func, 9999]).unwrap();
        assert_eq!(resolved, vec![(file, "src/a.rs".to_string())]);
    }

    #[test]
    fn transitive_dependencies_walks_chain_and_respects_depth() {
        let mut g = graph();
        let a = add_file(&mut g, "a.rs", "rust").unwrap();
        let b = add_file(&mut g, "b.rs", "rust").unwrap();
        let c = add_file(&mut g, "c.rs", "rust").unwrap();
        add_import(&mut g, a, b, vec![]).unwrap();
        add_import(&mut g, b, c, vec![]).unwrap();

        let mut all = transitive_dependencies(&g, a, None).unwrap();
        all.sort_unstable();
        let mut expected = vec![b, c];
        expected.sort_unstable();
        assert_eq!(all, expected);

        // Depth 1 only reaches the direct dependency.
        assert_eq!(transitive_dependencies(&g, a, Some(1)).unwrap(), vec![b]);
    }

    #[test]
    fn transitive_dependents_reverse_walks_chain() {
        let mut g = graph();
        let a = add_file(&mut g, "a.rs", "rust").unwrap();
        let b = add_file(&mut g, "b.rs", "rust").unwrap();
        let c = add_file(&mut g, "c.rs", "rust").unwrap();
        add_import(&mut g, a, b, vec![]).unwrap();
        add_import(&mut g, b, c, vec![]).unwrap();

        // c is imported by b, which is imported by a.
        let mut all = transitive_dependents(&g, c, None).unwrap();
        all.sort_unstable();
        let mut expected = vec![a, b];
        expected.sort_unstable();
        assert_eq!(all, expected);
    }

    #[test]
    fn transitive_dependencies_handles_cycles() {
        let mut g = graph();
        let a = add_file(&mut g, "a.rs", "rust").unwrap();
        let b = add_file(&mut g, "b.rs", "rust").unwrap();
        add_import(&mut g, a, b, vec![]).unwrap();
        add_import(&mut g, b, a, vec![]).unwrap();

        // Cycle must not loop forever; a's only dependency is b.
        assert_eq!(transitive_dependencies(&g, a, None).unwrap(), vec![b]);
    }

    #[test]
    fn circular_deps_reports_file_cycles_only() {
        let mut g = graph();
        let a = add_file(&mut g, "a.rs", "rust").unwrap();
        let b = add_file(&mut g, "b.rs", "rust").unwrap();
        add_import(&mut g, a, b, vec![]).unwrap();
        add_import(&mut g, b, a, vec![]).unwrap();

        let cycles = circular_deps(&g).unwrap();
        assert_eq!(cycles.len(), 1);
        let mut cycle = cycles[0].clone();
        cycle.sort_unstable();
        let mut expected = vec![a, b];
        expected.sort_unstable();
        assert_eq!(cycle, expected);
    }

    #[test]
    fn circular_deps_empty_without_cycle() {
        let mut g = graph();
        let a = add_file(&mut g, "a.rs", "rust").unwrap();
        let b = add_file(&mut g, "b.rs", "rust").unwrap();
        add_import(&mut g, a, b, vec![]).unwrap();
        assert!(circular_deps(&g).unwrap().is_empty());
    }

    #[test]
    fn call_chain_finds_path_between_functions() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let a = add_function(&mut g, file, "a", 1, 5).unwrap();
        let b = add_function(&mut g, file, "b", 6, 10).unwrap();
        let c = add_function(&mut g, file, "c", 11, 15).unwrap();
        add_call(&mut g, a, b, 2).unwrap();
        add_call(&mut g, b, c, 7).unwrap();

        let chains = call_chain(&g, a, c, Some(10)).unwrap();
        assert_eq!(chains, vec![vec![a, b, c]]);
    }

    #[test]
    fn add_file_sets_codefile_type_and_language() {
        // Prior tests use add_file only for setup and check `path` indirectly via
        // find_file_by_path; the node type and the `language` property were never
        // asserted directly.
        let mut g = graph();
        let file = add_file(&mut g, "src/lib.rs", "rust").unwrap();
        let node = g.get_node(file).unwrap();
        assert_eq!(node.node_type, NodeType::CodeFile);
        assert_eq!(node.properties.get_string("path"), Some("src/lib.rs"));
        assert_eq!(node.properties.get_string("language"), Some("rust"));
    }

    #[test]
    fn add_class_stores_props_and_links_to_file() {
        // add_method_links_to_class asserts the Class node_type but not its
        // name/line properties, and add_class's own Contains edge from the file
        // is never pinned separately from add_method's setup.
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let class = add_class(&mut g, file, "Widget", 3, 42).unwrap();
        let node = g.get_node(class).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        assert_eq!(node.properties.get_string("name"), Some("Widget"));
        assert_eq!(node.properties.get_int("line_start"), Some(3));
        assert_eq!(node.properties.get_int("line_end"), Some(42));
        // The file Contains the class (outgoing neighbor).
        let contained = g.get_neighbors(file, Direction::Outgoing).unwrap();
        assert!(contained.contains(&class));
    }

    #[test]
    fn add_call_stores_line_on_calls_edge() {
        // The Calls edge's `line` property is the only payload add_call adds
        // beyond the edge itself, and no existing test reads it back.
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let caller = add_function(&mut g, file, "caller", 1, 5).unwrap();
        let callee = add_function(&mut g, file, "callee", 6, 10).unwrap();
        let edge_id = add_call(&mut g, caller, callee, 7).unwrap();
        let edge = g.get_edge(edge_id).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Calls);
        assert_eq!(edge.properties.get_int("line"), Some(7));
    }

    #[test]
    fn add_import_stores_symbols_on_edge() {
        // add_import's `symbols` list property is never asserted; existing import
        // tests only check the resulting dependency direction.
        let mut g = graph();
        let a = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let b = add_file(&mut g, "src/b.rs", "rust").unwrap();
        let edge_id = add_import(&mut g, a, b, vec!["foo", "bar"]).unwrap();
        let edge = g.get_edge(edge_id).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert_eq!(
            edge.properties.get_string_list("symbols"),
            Some(["foo".to_string(), "bar".to_string()].as_slice())
        );
    }

    #[test]
    fn file_dependencies_follow_imports_from_edges() {
        // get_file_dependencies/dependents accept both Imports AND ImportsFrom,
        // but add_import only ever creates Imports edges, so the ImportsFrom arm
        // is unexercised. Wire one manually to cover it.
        let mut g = graph();
        let a = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let b = add_file(&mut g, "src/b.rs", "rust").unwrap();
        g.add_edge(a, b, EdgeType::ImportsFrom, PropertyMap::new())
            .unwrap();
        assert_eq!(get_file_dependencies(&g, a).unwrap(), vec![b]);
        assert_eq!(get_file_dependents(&g, b).unwrap(), vec![a]);
    }

    #[test]
    fn transitive_dependencies_depth_zero_is_empty() {
        // Some(0) hits the `depth >= max` guard on the very first node, so no
        // dependency is ever collected - a boundary the Some(1)/None tests miss.
        let mut g = graph();
        let a = add_file(&mut g, "a.rs", "rust").unwrap();
        let b = add_file(&mut g, "b.rs", "rust").unwrap();
        add_import(&mut g, a, b, vec![]).unwrap();
        assert!(transitive_dependencies(&g, a, Some(0)).unwrap().is_empty());
        assert!(transitive_dependents(&g, b, Some(0)).unwrap().is_empty());
    }

    #[test]
    fn call_chain_returns_empty_when_no_path() {
        // Two functions with no connecting Calls edge yield no chains.
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        let a = add_function(&mut g, file, "a", 1, 5).unwrap();
        let b = add_function(&mut g, file, "b", 6, 10).unwrap();
        assert!(call_chain(&g, a, b, Some(10)).unwrap().is_empty());
    }

    #[test]
    fn link_to_file_creates_contains_edge() {
        let mut g = graph();
        let file = add_file(&mut g, "src/a.rs", "rust").unwrap();
        // A bare Function node with no auto-link, then wire it manually.
        let func = g
            .add_node(
                NodeType::Function,
                PropertyMap::new().with("name", "orphan"),
            )
            .unwrap();
        assert!(get_functions_in_file(&g, file).unwrap().is_empty());
        link_to_file(&mut g, file, func).unwrap();
        assert_eq!(get_functions_in_file(&g, file).unwrap(), vec![func]);
    }

    #[test]
    fn circular_deps_ignores_cycle_without_file_nodes() {
        // A Calls cycle between two Function nodes forms a genuine 2-node SCC, but
        // it contains no CodeFile nodes, so file_nodes stays empty and the
        // `file_nodes.len() > 1` guard rejects it via its len==0 arm. Every prior
        // circular_deps test either builds a real file cycle (len 2) or produces
        // only single-file singleton SCCs (len 1), so the empty-file_nodes false
        // arm - together with the inner CodeFile type filter being false for both
        // members of a cyclic SCC - was never reached.
        let mut g = graph();
        let a = g
            .add_node(NodeType::Function, PropertyMap::new().with("name", "a"))
            .unwrap();
        let b = g
            .add_node(NodeType::Function, PropertyMap::new().with("name", "b"))
            .unwrap();
        g.add_edge(a, b, EdgeType::Calls, PropertyMap::new())
            .unwrap();
        g.add_edge(b, a, EdgeType::Calls, PropertyMap::new())
            .unwrap();

        // The cycle exists at the graph level...
        let sccs = g.find_strongly_connected_components().unwrap();
        assert!(sccs
            .iter()
            .any(|s| s.len() == 2 && s.contains(&a) && s.contains(&b)));
        // ...but circular_deps reports nothing because neither node is a file.
        assert!(circular_deps(&g).unwrap().is_empty());
    }

    #[test]
    fn circular_deps_ignores_multi_node_scc_with_single_file() {
        // A cycle through exactly one CodeFile and one non-file node (a Module)
        // yields a multi-node SCC whose file_nodes has length 1, so the `> 1`
        // guard still rejects it. This drives the inner CodeFile type filter both
        // ways within one cyclic SCC (true for the file, false for the module) and
        // hits the len==1 false arm from a genuine multi-node cycle - existing
        // len==1 coverage comes only from lone-file singleton SCCs, never a cycle.
        let mut g = graph();
        let file = add_file(&mut g, "a.rs", "rust").unwrap();
        let m = add_module(&mut g, "m", "m.rs").unwrap();
        g.add_edge(file, m, EdgeType::Contains, PropertyMap::new())
            .unwrap();
        g.add_edge(m, file, EdgeType::References, PropertyMap::new())
            .unwrap();

        // The file and module form a real 2-node SCC...
        let sccs = g.find_strongly_connected_components().unwrap();
        assert!(sccs
            .iter()
            .any(|s| s.len() == 2 && s.contains(&file) && s.contains(&m)));
        // ...but only one file participates, so no circular file dependency is reported.
        assert!(circular_deps(&g).unwrap().is_empty());
    }
}
