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
