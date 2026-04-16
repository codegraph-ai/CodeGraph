// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Dead import detection — transport-agnostic.
//!
//! A "dead import" is an import where file A imports module B but no function
//! in file A has a `Calls` edge (or structural usage) to any entity in module B.
//! External imports (modules not present in the graph) are reported separately
//! as "unresolved" — they cannot be verified and should not be flagged as dead.

use crate::domain::node_props;
use codegraph::{CodeGraph, Direction, EdgeType, NodeId, NodeType};
use serde::Serialize;

// ============================================================
// Result Types
// ============================================================

/// A confirmed dead import — the module is in the graph but never called.
#[derive(Debug, Serialize)]
pub(crate) struct DeadImport {
    /// Absolute path of the importing file.
    pub file: String,
    /// Name of the imported module.
    pub imported_module: String,
    /// Line number of the import statement (0 if unknown).
    pub line: usize,
}

/// An unresolved import — the module is not in the graph (external dependency).
#[derive(Debug, Serialize)]
pub(crate) struct UnresolvedImport {
    /// Absolute path of the importing file.
    pub file: String,
    /// Name of the imported module.
    pub imported_module: String,
}

/// Result of `find_dead_imports`.
#[derive(Debug, Serialize)]
pub(crate) struct DeadImportsResult {
    pub dead_imports: Vec<DeadImport>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub total_imports: usize,
    pub dead_count: usize,
}

// ============================================================
// Domain Function
// ============================================================

/// Find dead imports across the graph (or a single file).
///
/// - `file_path`: when `Some`, restrict to that file; when `None`, scan all files.
///
/// Strategy per import edge (file A --Imports--> module B):
/// 1. If module B has no `Contains` children in the graph → unresolved (external).
/// 2. Collect functions in file A (via `Contains` edges from A's `CodeFile` node).
/// 3. For each function F in A, check whether any outgoing `Calls` edge reaches
///    a node whose path matches module B's name or path.
/// 4. If no such call exists → dead import.
pub(crate) fn find_dead_imports(
    graph: &CodeGraph,
    file_path: Option<&str>,
) -> DeadImportsResult {
    let mut dead_imports = Vec::new();
    let mut unresolved_imports = Vec::new();
    let mut total_imports = 0usize;

    // Collect CodeFile nodes to examine
    let file_node_ids: Vec<NodeId> = graph
        .iter_nodes()
        .filter_map(|(id, node)| {
            if node.node_type != NodeType::CodeFile {
                return None;
            }
            // Optionally restrict to a specific file path
            if let Some(fp) = file_path {
                let node_path = node_props::path(node);
                if node_path != fp {
                    return None;
                }
            }
            Some(id)
        })
        .collect();

    for file_id in file_node_ids {
        let file_node = match graph.get_node(file_id) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let file_str = node_props::path(file_node).to_string();

        // Find all outgoing Imports edges from this file
        let import_targets = collect_import_targets(graph, file_id);

        // Collect functions inside this file (via Contains edges)
        let file_functions = collect_contained_functions(graph, file_id);

        for (module_id, module_name, import_line) in import_targets {
            total_imports += 1;

            // Determine whether this module is external (no children in graph)
            let module_children = collect_contained_nodes(graph, module_id);
            if module_children.is_empty() {
                // Check the external property as a fallback
                let is_external = graph
                    .get_node(module_id)
                    .ok()
                    .and_then(|n| n.properties.get_string("external"))
                    .map(|v| v == "true")
                    .unwrap_or(false);

                if is_external || !module_has_path(graph, module_id) {
                    unresolved_imports.push(UnresolvedImport {
                        file: file_str.clone(),
                        imported_module: module_name,
                    });
                    continue;
                }
            }

            // Build the set of node IDs that "belong" to the imported module
            let module_path = graph
                .get_node(module_id)
                .ok()
                .map(|n| node_props::path(n).to_string())
                .unwrap_or_default();

            // Check whether any function in this file calls any entity in module B
            let is_used = file_functions.iter().any(|&func_id| {
                calls_any_in_module(graph, func_id, module_id, &module_path, &module_name)
            });

            // Also check non-function Contains children of the file (e.g. top-level uses)
            let is_used = is_used
                || file_references_module(graph, file_id, module_id, &module_path, &module_name);

            if !is_used {
                dead_imports.push(DeadImport {
                    file: file_str.clone(),
                    imported_module: module_name,
                    line: import_line,
                });
            }
        }
    }

    let dead_count = dead_imports.len();
    DeadImportsResult {
        dead_imports,
        unresolved_imports,
        total_imports,
        dead_count,
    }
}

// ============================================================
// Private Helpers
// ============================================================

/// Collect `(module_node_id, module_name, line)` for every outgoing `Imports`
/// (or `ImportsFrom`) edge from `file_id`.
fn collect_import_targets(
    graph: &CodeGraph,
    file_id: NodeId,
) -> Vec<(NodeId, String, usize)> {
    let neighbors = match graph.get_neighbors(file_id, Direction::Outgoing) {
        Ok(n) => n,
        Err(_) => return vec![],
    };

    neighbors
        .into_iter()
        .filter_map(|neighbor_id| {
            let edge_ids = graph.get_edges_between(file_id, neighbor_id).ok()?;
            let is_import = edge_ids.iter().any(|&eid| {
                graph
                    .get_edge(eid)
                    .map(|e| {
                        e.edge_type == EdgeType::Imports || e.edge_type == EdgeType::ImportsFrom
                    })
                    .unwrap_or(false)
            });
            if !is_import {
                return None;
            }
            let module_node = graph.get_node(neighbor_id).ok()?;
            let module_name = node_props::name(module_node).to_string();
            let line = node_props::line_start(module_node) as usize;
            Some((neighbor_id, module_name, line))
        })
        .collect()
}

/// Collect `NodeId`s of `Function` nodes directly contained by `parent_id`
/// (via `Contains` edges).
fn collect_contained_functions(graph: &CodeGraph, parent_id: NodeId) -> Vec<NodeId> {
    let neighbors = match graph.get_neighbors(parent_id, Direction::Outgoing) {
        Ok(n) => n,
        Err(_) => return vec![],
    };

    neighbors
        .into_iter()
        .filter(|&child_id| {
            // Must be connected via Contains
            let is_contains = graph
                .get_edges_between(parent_id, child_id)
                .unwrap_or_default()
                .iter()
                .any(|&eid| {
                    graph
                        .get_edge(eid)
                        .map(|e| e.edge_type == EdgeType::Contains)
                        .unwrap_or(false)
                });
            if !is_contains {
                return false;
            }
            graph
                .get_node(child_id)
                .map(|n| n.node_type == NodeType::Function)
                .unwrap_or(false)
        })
        .collect()
}

/// Collect all directly-contained node IDs (any type) of a node.
fn collect_contained_nodes(graph: &CodeGraph, parent_id: NodeId) -> Vec<NodeId> {
    let neighbors = match graph.get_neighbors(parent_id, Direction::Outgoing) {
        Ok(n) => n,
        Err(_) => return vec![],
    };

    neighbors
        .into_iter()
        .filter(|&child_id| {
            graph
                .get_edges_between(parent_id, child_id)
                .unwrap_or_default()
                .iter()
                .any(|&eid| {
                    graph
                        .get_edge(eid)
                        .map(|e| e.edge_type == EdgeType::Contains)
                        .unwrap_or(false)
                })
        })
        .collect()
}

/// Return true when the module node has a non-empty path (i.e. it is a real source file
/// that was indexed, not a stub for an external dependency).
fn module_has_path(graph: &CodeGraph, module_id: NodeId) -> bool {
    graph
        .get_node(module_id)
        .map(|n| !node_props::path(n).is_empty())
        .unwrap_or(false)
}

/// Return true if `func_id` has an outgoing `Calls` edge to any entity whose
/// path or whose parent-module's path/name matches the imported module.
fn calls_any_in_module(
    graph: &CodeGraph,
    func_id: NodeId,
    module_id: NodeId,
    module_path: &str,
    module_name: &str,
) -> bool {
    let callee_neighbors = match graph.get_neighbors(func_id, Direction::Outgoing) {
        Ok(n) => n,
        Err(_) => return false,
    };

    callee_neighbors.iter().any(|&callee_id| {
        // Must be a Calls edge
        let is_call = graph
            .get_edges_between(func_id, callee_id)
            .unwrap_or_default()
            .iter()
            .any(|&eid| {
                graph
                    .get_edge(eid)
                    .map(|e| e.edge_type == EdgeType::Calls)
                    .unwrap_or(false)
            });
        if !is_call {
            return false;
        }
        // Direct match: callee is the module itself
        if callee_id == module_id {
            return true;
        }
        // Path-based match: callee lives in the same file as the module
        if let Ok(callee_node) = graph.get_node(callee_id) {
            let callee_path = node_props::path(callee_node);
            if !module_path.is_empty() && callee_path == module_path {
                return true;
            }
            // Name-based fallback: callee path ends with the module name
            if !module_name.is_empty()
                && (callee_path.ends_with(module_name)
                    || callee_path.contains(&format!("/{module_name}"))
                    || callee_path.contains(&format!("\\{module_name}")))
            {
                return true;
            }
        }
        false
    })
}

/// Check non-call usage: look for `References`, `Uses`, `Instantiates`, `Invokes`,
/// or `Extends`/`Implements` edges from any child of `file_id` to any child of
/// `module_id`, as a secondary signal (catches type-only usages, re-exports, etc.).
fn file_references_module(
    graph: &CodeGraph,
    file_id: NodeId,
    module_id: NodeId,
    module_path: &str,
    module_name: &str,
) -> bool {
    let file_children = match graph.get_neighbors(file_id, Direction::Outgoing) {
        Ok(n) => n,
        Err(_) => return false,
    };

    // Collect module children for direct ID matching
    let module_children: std::collections::HashSet<NodeId> =
        collect_contained_nodes(graph, module_id).into_iter().collect();

    const USAGE_EDGES: &[EdgeType] = &[
        EdgeType::References,
        EdgeType::Uses,
        EdgeType::Instantiates,
        EdgeType::Invokes,
        EdgeType::Extends,
        EdgeType::Implements,
    ];

    file_children.iter().any(|&child_id| {
        let outgoing = match graph.get_neighbors(child_id, Direction::Outgoing) {
            Ok(n) => n,
            Err(_) => return false,
        };
        outgoing.iter().any(|&target_id| {
            let has_usage = graph
                .get_edges_between(child_id, target_id)
                .unwrap_or_default()
                .iter()
                .any(|&eid| {
                    graph
                        .get_edge(eid)
                        .map(|e| USAGE_EDGES.contains(&e.edge_type))
                        .unwrap_or(false)
                });
            if !has_usage {
                return false;
            }
            // Direct ID match
            if target_id == module_id || module_children.contains(&target_id) {
                return true;
            }
            // Path/name match
            if let Ok(target_node) = graph.get_node(target_id) {
                let target_path = node_props::path(target_node);
                if !module_path.is_empty() && target_path == module_path {
                    return true;
                }
                if !module_name.is_empty()
                    && (target_path.ends_with(module_name)
                        || target_path.contains(&format!("/{module_name}"))
                        || target_path.contains(&format!("\\{module_name}")))
                {
                    return true;
                }
            }
            false
        })
    })
}
