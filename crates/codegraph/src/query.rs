// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Query builder for fluent graph queries.
//!
//! Provides a fluent interface for constructing and executing complex graph queries
//! with multiple filters and optimizations.

use crate::error::Result;
use crate::graph::{CodeGraph, Direction, Node, NodeId, NodeType, PropertyValue};

/// A filter predicate that can be applied to nodes.
type FilterFn = Box<dyn Fn(&Node) -> bool>;

/// Fluent query builder for graph operations.
///
/// Allows chaining multiple filters to find specific nodes in the graph.
///
/// # Examples
///
/// ```
/// use codegraph::{CodeGraph, NodeType};
///
/// # fn example() -> codegraph::Result<()> {
/// let mut graph = CodeGraph::in_memory()?;
/// // ... populate graph ...
///
/// // Find all public functions in a specific file
/// let results = graph.query()
///     .node_type(NodeType::Function)
///     .in_file("src/main.rs")
///     .property("visibility", "public")
///     .execute()?;
/// # Ok(())
/// # }
/// ```
pub struct QueryBuilder<'a> {
    graph: &'a CodeGraph,
    filters: Vec<FilterFn>,
    limit_value: Option<usize>,
    in_file_filter: Option<String>,
}

impl<'a> QueryBuilder<'a> {
    /// Create a new query builder for the given graph.
    pub fn new(graph: &'a CodeGraph) -> Self {
        Self {
            graph,
            filters: Vec::new(),
            limit_value: None,
            in_file_filter: None,
        }
    }

    /// Filter nodes by type.
    ///
    /// # Examples
    ///
    /// ```
    /// # use codegraph::{CodeGraph, NodeType};
    /// # fn example() -> codegraph::Result<()> {
    /// # let graph = CodeGraph::in_memory()?;
    /// let functions = graph.query()
    ///     .node_type(NodeType::Function)
    ///     .execute()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn node_type(mut self, node_type: NodeType) -> Self {
        self.filters
            .push(Box::new(move |node| node.node_type == node_type));
        self
    }

    /// Filter nodes that are contained in a specific file.
    ///
    /// Looks up the file by path and finds all nodes connected via Contains edges.
    pub fn in_file(mut self, file_path: &str) -> Self {
        self.in_file_filter = Some(file_path.to_string());
        self
    }

    /// Filter files by glob pattern.
    ///
    /// Supports wildcards: `*` matches any characters, `**` matches directories.
    ///
    /// # Examples
    ///
    /// - `src/*.rs` - All Rust files in src/
    /// - `**/*.py` - All Python files recursively
    /// - `tests/**/*.rs` - All Rust files under tests/
    pub fn file_pattern(mut self, pattern: &str) -> Self {
        let pattern = pattern.to_string();
        self.filters.push(Box::new(move |node| {
            if let Some(path) = node.properties.get_string("path") {
                glob_match(&pattern, path)
            } else {
                false
            }
        }));
        self
    }

    /// Filter nodes by exact property match.
    ///
    /// Supports string, int, float, and bool property values.
    pub fn property<V: Into<PropertyValue>>(mut self, key: &str, value: V) -> Self {
        let key = key.to_string();
        let value = value.into();

        self.filters.push(Box::new(move |node| {
            if let Some(prop_value) = node.properties.get(&key) {
                match (&value, prop_value) {
                    (PropertyValue::String(v1), PropertyValue::String(v2)) => v1 == v2,
                    (PropertyValue::Int(v1), PropertyValue::Int(v2)) => v1 == v2,
                    (PropertyValue::Float(v1), PropertyValue::Float(v2)) => {
                        (v1 - v2).abs() < f64::EPSILON
                    }
                    (PropertyValue::Bool(v1), PropertyValue::Bool(v2)) => v1 == v2,
                    _ => false,
                }
            } else {
                false
            }
        }));
        self
    }

    /// Filter nodes that have a specific property (regardless of value).
    pub fn property_exists(mut self, key: &str) -> Self {
        let key = key.to_string();
        self.filters
            .push(Box::new(move |node| node.properties.contains_key(&key)));
        self
    }

    /// Filter nodes by name containing a substring (case-insensitive).
    pub fn name_contains(mut self, substring: &str) -> Self {
        let substring = substring.to_lowercase();
        self.filters.push(Box::new(move |node| {
            if let Some(name) = node.properties.get_string("name") {
                name.to_lowercase().contains(&substring)
            } else {
                false
            }
        }));
        self
    }

    /// Filter nodes by name matching a regex pattern.
    pub fn name_matches(mut self, pattern: &str) -> Self {
        let pattern = pattern.to_string();
        self.filters.push(Box::new(move |node| {
            if let Some(name) = node.properties.get_string("name") {
                // Simple regex: support ^ for start, $ for end, * for wildcard
                regex_match(&pattern, name)
            } else {
                false
            }
        }));
        self
    }

    /// Filter nodes using a custom predicate function.
    ///
    /// # Examples
    ///
    /// ```
    /// # use codegraph::{CodeGraph, NodeType};
    /// # fn example() -> codegraph::Result<()> {
    /// # let graph = CodeGraph::in_memory()?;
    /// // Find functions longer than 50 lines
    /// let results = graph.query()
    ///     .node_type(NodeType::Function)
    ///     .custom(|node| {
    ///         if let (Some(start), Some(end)) = (
    ///             node.properties.get_int("line_start"),
    ///             node.properties.get_int("line_end")
    ///         ) {
    ///             (end - start) > 50
    ///         } else {
    ///             false
    ///         }
    ///     })
    ///     .execute()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn custom<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&Node) -> bool + 'static,
    {
        self.filters.push(Box::new(predicate));
        self
    }

    /// Limit the number of results returned.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit_value = Some(n);
        self
    }

    /// Execute the query and return matching node IDs.
    pub fn execute(&self) -> Result<Vec<NodeId>> {
        let mut results = Vec::new();
        let limit = self.limit_value.unwrap_or(usize::MAX);

        // If in_file filter is set, only search nodes in that file
        if let Some(file_path) = &self.in_file_filter {
            for node_id in self.get_nodes_in_file(file_path)? {
                if results.len() >= limit {
                    break;
                }
                if let Ok(node) = self.graph.get_node(node_id) {
                    if self.matches_filters(node) {
                        results.push(node_id);
                    }
                }
            }
        } else {
            for (&node_id, node) in self.graph.nodes_iter() {
                if results.len() >= limit {
                    break;
                }
                if self.matches_filters(node) {
                    results.push(node_id);
                }
            }
        }

        Ok(results)
    }

    /// Count the number of matching nodes without allocating a result vector.
    pub fn count(&self) -> Result<usize> {
        let mut count = 0;

        if let Some(file_path) = &self.in_file_filter {
            for node_id in self.get_nodes_in_file(file_path)? {
                if let Ok(node) = self.graph.get_node(node_id) {
                    if self.matches_filters(node) {
                        count += 1;
                    }
                }
            }
        } else {
            for (_, node) in self.graph.nodes_iter() {
                if self.matches_filters(node) {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Check if any nodes match the query (short-circuits on first match).
    pub fn exists(&self) -> Result<bool> {
        if let Some(file_path) = &self.in_file_filter {
            for node_id in self.get_nodes_in_file(file_path)? {
                if let Ok(node) = self.graph.get_node(node_id) {
                    if self.matches_filters(node) {
                        return Ok(true);
                    }
                }
            }
        } else {
            for (_, node) in self.graph.nodes_iter() {
                if self.matches_filters(node) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Get all nodes contained in a specific file.
    fn get_nodes_in_file(&self, file_path: &str) -> Result<Vec<NodeId>> {
        // First find the file node
        for (&node_id, node) in self.graph.nodes_iter() {
            if node.node_type == NodeType::CodeFile {
                if let Some(path) = node.properties.get_string("path") {
                    if path == file_path {
                        // Found the file, now get all nodes it contains
                        return self.graph.get_neighbors(node_id, Direction::Outgoing);
                    }
                }
            }
        }

        // File not found
        Ok(Vec::new())
    }

    /// Check if a node matches all filters.
    fn matches_filters(&self, node: &Node) -> bool {
        self.filters.iter().all(|filter| filter(node))
    }
}

/// Simple glob pattern matching.
///
/// Supports * (any characters) and ** (directories).
fn glob_match(pattern: &str, path: &str) -> bool {
    // Handle ** for directory matching
    if pattern.contains("**") {
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            let suffix = parts[1].trim_start_matches('/');

            // Check prefix
            if !prefix.is_empty() && !path.starts_with(prefix) {
                return false;
            }

            // If suffix contains *, we need to handle it recursively
            if suffix.contains('*') {
                // Get the part after the last /
                if let Some(last_slash) = path.rfind('/') {
                    let filename = &path[last_slash + 1..];
                    return glob_match(suffix, filename);
                } else {
                    return glob_match(suffix, path);
                }
            }

            // Simple suffix match
            if !suffix.is_empty() && !path.ends_with(suffix) {
                return false;
            }
            return true;
        }
    }

    // Handle * for simple wildcard matching
    let pattern_parts: Vec<&str> = pattern.split('*').collect();
    if pattern_parts.len() == 1 {
        // No wildcards - exact match
        return pattern == path;
    }

    // Check if path matches the pattern
    let mut pos = 0;
    for (i, part) in pattern_parts.iter().enumerate() {
        if part.is_empty() {
            // Empty part from consecutive * or at start/end
            continue;
        }

        if i == 0 {
            // First part must match start
            if !path[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == pattern_parts.len() - 1 {
            // Last part must match end
            return path[pos..].ends_with(part);
        } else {
            // Middle parts must exist in order
            if let Some(index) = path[pos..].find(part) {
                pos += index + part.len();
            } else {
                return false;
            }
        }
    }

    true
}

/// Simple regex pattern matching.
///
/// Supports ^ (start), $ (end), and basic literals.
fn regex_match(pattern: &str, text: &str) -> bool {
    let starts_with = pattern.starts_with('^');
    let ends_with = pattern.ends_with('$');

    let pattern = pattern.trim_start_matches('^').trim_end_matches('$');

    if starts_with && ends_with {
        text == pattern
    } else if starts_with {
        text.starts_with(pattern)
    } else if ends_with {
        text.ends_with(pattern)
    } else {
        text.contains(pattern)
    }
}
