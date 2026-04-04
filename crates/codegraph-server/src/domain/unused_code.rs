// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unused code detection — single source of truth for both LSP and MCP handlers.
//!
//! This module contains the domain logic for finding unused code symbols.
//! It has no dependency on tower-lsp, MCP protocol types, or serde_json::Value.

use crate::ai_query::QueryEngine;
use crate::domain::node_props;
use codegraph::{CodeGraph, NodeId, NodeType};

// ==========================================
// Parameters & Results
// ==========================================

pub(crate) struct FindUnusedCodeParams {
    /// Optional file path (not URI). If None, scope determines range.
    pub path: Option<String>,
    /// "file", "module", or "workspace"
    pub scope: String,
    pub include_tests: bool,
    pub confidence: f64,
}

pub(crate) struct UnusedCodeCandidate {
    pub name: String,
    pub node_id: NodeId,
    pub node_type: NodeType,
    pub confidence: f64,
    pub is_public: bool,
    pub line_start: u32,
    pub line_end: u32,
}

pub(crate) struct FindUnusedCodeResult {
    pub candidates: Vec<UnusedCodeCandidate>,
    pub total_checked: usize,
    pub scope: String,
    pub min_confidence: f64,
}

// ==========================================
// Core Domain Function
// ==========================================

/// Find unused code symbols in the graph.
///
/// Uses the richer detection strategy: checks callers via QueryEngine (respects
/// test filtering), structural usage (child methods, sibling functions), and
/// confidence scoring with framework-specific heuristics.
pub(crate) async fn find_unused_code(
    graph: &CodeGraph,
    query_engine: &QueryEngine,
    params: FindUnusedCodeParams,
) -> FindUnusedCodeResult {
    // Collect candidate nodes based on scope / path
    let mut nodes_to_check: Vec<NodeId> = if let Some(ref path) = params.path {
        graph
            .query()
            .property("path", path.as_str())
            .execute()
            .unwrap_or_default()
    } else if params.scope == "workspace" || params.scope == "module" {
        let mut all = Vec::new();
        for node_type in &[
            NodeType::Function,
            NodeType::Class,
            NodeType::Variable,
            NodeType::Type,
            NodeType::Interface,
        ] {
            if let Ok(ids) = graph.query().node_type(*node_type).execute() {
                all.extend(ids);
            }
        }
        // Exclude build output directories to avoid counting compiled duplicates
        all.retain(|&node_id| {
            graph
                .get_node(node_id)
                .map(|node| !is_build_output_path(node_props::path(node)))
                .unwrap_or(true)
        });
        all.into_iter().take(2000).collect()
    } else {
        vec![]
    };

    // When include_tests is false, filter out test nodes from the candidate set
    if !params.include_tests {
        nodes_to_check.retain(|&node_id| {
            graph
                .get_node(node_id)
                .map(|node| !is_test_node(node))
                .unwrap_or(true)
        });
    }

    let total_checked = nodes_to_check.len();
    let mut candidates = Vec::new();

    for node_id in nodes_to_check {
        if let Ok(node) = graph.get_node(node_id) {
            // Skip structural node types (files, modules)
            if node.node_type == NodeType::CodeFile || node.node_type == NodeType::Module {
                continue;
            }

            let name = node_props::name(node);

            // Skip anonymous/synthetic names
            if name == "arrow_function"
                || name.is_empty()
                || name == "anonymous"
                || name == "constructor"
            {
                continue;
            }

            // Skip well-known entry points and lifecycle hooks
            if is_framework_entry_point(name) {
                continue;
            }

            // Skip well-known trait impl methods (called by Rust/language framework dispatch)
            if is_trait_impl_method(name) {
                continue;
            }

            // Check for callers (via Calls edges)
            let callers = query_engine.get_callers(node_id, 1).await;
            let total_callers = callers.len();

            // When include_tests is false, filter out callers that are test functions
            let effective_callers = if !params.include_tests {
                callers
                    .iter()
                    .filter(|c| {
                        graph
                            .get_node(c.node_id)
                            .map(|n| !is_test_node(n))
                            .unwrap_or(true)
                    })
                    .count()
            } else {
                total_callers
            };

            // Test helper detection: if a function has callers but ALL are
            // test functions, it's test infrastructure — not dead production code
            if !params.include_tests && effective_callers == 0 && total_callers > 0 {
                continue;
            }

            // Struct/class-used-via-methods: if a struct has child methods
            // (via Contains edges) that have callers, OR if sibling functions
            // in the same file are called, the struct itself is in use
            if matches!(node.node_type, NodeType::Class | NodeType::Type)
                && (has_called_child_methods(graph, node_id)
                    || has_active_same_file_functions(graph, node_id))
            {
                continue;
            }

            // Check for usage edges (excluding structural Contains/Defines edges)
            let has_usage_edge = graph
                .get_neighbors(node_id, codegraph::Direction::Incoming)
                .map(|neighbors| {
                    neighbors.iter().any(|&neighbor_id| {
                        if !params.include_tests {
                            if let Ok(n) = graph.get_node(neighbor_id) {
                                if is_test_node(n) {
                                    return false;
                                }
                            }
                        }
                        graph
                            .get_edges_between(neighbor_id, node_id)
                            .unwrap_or_default()
                            .iter()
                            .any(|&edge_id| {
                                graph
                                    .get_edge(edge_id)
                                    .map(|e| {
                                        matches!(
                                            e.edge_type,
                                            codegraph::EdgeType::References
                                                | codegraph::EdgeType::Uses
                                                | codegraph::EdgeType::Invokes
                                                | codegraph::EdgeType::Instantiates
                                                | codegraph::EdgeType::Extends
                                                | codegraph::EdgeType::Implements
                                                | codegraph::EdgeType::Imports
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                    })
                })
                .unwrap_or(false);

            if effective_callers == 0 && !has_usage_edge {
                let is_exported = node_props::is_public(node);
                let item_confidence = compute_unused_confidence(name, is_exported, node);

                if item_confidence >= params.confidence {
                    candidates.push(UnusedCodeCandidate {
                        name: name.to_string(),
                        node_id,
                        node_type: node.node_type,
                        confidence: item_confidence,
                        is_public: is_exported,
                        line_start: node_props::line_start(node),
                        line_end: node_props::line_end(node),
                    });
                }
            }
        }
    }

    FindUnusedCodeResult {
        candidates,
        total_checked,
        scope: params.scope,
        min_confidence: params.confidence,
    }
}

// ==========================================
// Shared Helpers (pub(crate) for related_tests)
// ==========================================

/// Check if a node is a test function or lives in a test file.
pub(crate) fn is_test_node(node: &codegraph::Node) -> bool {
    // Check is_test property (set by Rust parser for #[test] functions)
    if node.properties.get_bool("is_test").unwrap_or(false) {
        return true;
    }

    let name = node_props::name(node);
    let path = node_props::path(node);

    let name_is_test = name.starts_with("test_")
        || name.ends_with("_test")
        || name.contains("test ")
        || name.starts_with("Test");

    let path_is_test = path.contains("/test")
        || path.contains("/tests")
        || path.contains("\\test")
        || path.contains("\\tests")
        || path.contains(".test.")
        || path.contains(".spec.")
        || path.contains("_test.");

    name_is_test || path_is_test
}

/// Generate candidate test file paths for a source file.
/// Given `/src/foo.ts`, generates patterns like `/src/foo.test.ts`, `/src/foo.spec.ts`,
/// `/src/tests/foo.ts`, `/src/__tests__/foo.ts`, `/src/foo_test.rs`, etc.
pub(crate) fn generate_test_path_patterns(source_path: &str) -> Vec<String> {
    let path = std::path::Path::new(source_path);
    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return vec![],
    };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let dir = path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut patterns = Vec::new();

    if !ext.is_empty() {
        // Adjacent test files: foo.test.ts, foo.spec.ts
        patterns.push(format!("{dir}/{stem}.test.{ext}"));
        patterns.push(format!("{dir}/{stem}.spec.{ext}"));
        // Rust/Go convention: foo_test.rs
        patterns.push(format!("{dir}/{stem}_test.{ext}"));
        // Subdirectory conventions: tests/foo.ts, __tests__/foo.ts, test/foo.ts
        patterns.push(format!("{dir}/tests/{stem}.{ext}"));
        patterns.push(format!("{dir}/__tests__/{stem}.{ext}"));
        patterns.push(format!("{dir}/test/{stem}.{ext}"));
        // Test file with _test suffix in subdirectory
        patterns.push(format!("{dir}/tests/{stem}_test.{ext}"));
    }

    patterns
}

// ==========================================
// Private Helpers
// ==========================================

/// Check if a path is inside a build output directory.
fn is_build_output_path(path: &str) -> bool {
    const EXCLUDED_DIRS: &[&str] = &["out", "dist", "target", "node_modules", "build"];
    path.split(['/', '\\'])
        .any(|component| EXCLUDED_DIRS.contains(&component))
}

/// Check if a struct/class has child methods (via Contains edges) that are called.
fn has_called_child_methods(graph: &CodeGraph, node_id: NodeId) -> bool {
    let children = match graph.get_neighbors(node_id, codegraph::Direction::Outgoing) {
        Ok(c) => c,
        Err(_) => return false,
    };
    for &child_id in &children {
        let is_contained_fn = graph
            .get_edges_between(node_id, child_id)
            .unwrap_or_default()
            .iter()
            .any(|&eid| {
                graph
                    .get_edge(eid)
                    .map(|e| e.edge_type == codegraph::EdgeType::Contains)
                    .unwrap_or(false)
            });
        if !is_contained_fn {
            continue;
        }
        if let Ok(child) = graph.get_node(child_id) {
            if child.node_type != NodeType::Function {
                continue;
            }
        }
        if let Ok(neighbors) = graph.get_neighbors(child_id, codegraph::Direction::Incoming) {
            for &neighbor_id in &neighbors {
                let has_call = graph
                    .get_edges_between(neighbor_id, child_id)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == codegraph::EdgeType::Calls)
                            .unwrap_or(false)
                    });
                if has_call {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if a struct/class shares its file with functions that have callers.
fn has_active_same_file_functions(graph: &CodeGraph, node_id: NodeId) -> bool {
    let path = match graph.get_node(node_id) {
        Ok(n) => {
            let p = node_props::path(n).to_string();
            if p.is_empty() {
                return false;
            }
            p
        }
        Err(_) => return false,
    };
    let file_functions = graph
        .query()
        .node_type(NodeType::Function)
        .property("path", path)
        .execute()
        .unwrap_or_default();
    for &func_id in &file_functions {
        if func_id == node_id {
            continue;
        }
        if let Ok(neighbors) = graph.get_neighbors(func_id, codegraph::Direction::Incoming) {
            for &neighbor_id in &neighbors {
                let has_call = graph
                    .get_edges_between(neighbor_id, func_id)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == codegraph::EdgeType::Calls)
                            .unwrap_or(false)
                    });
                if has_call {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if a name is a well-known framework entry point or lifecycle hook.
fn is_framework_entry_point(name: &str) -> bool {
    matches!(
        name,
        // Rust/general
        "main" | "setup" | "Args"
        // JS test frameworks
        | "it"
        | "describe"
        | "beforeEach"
        | "afterEach"
        | "beforeAll"
        | "afterAll"
        // VS Code extension API
        | "activate"
        | "deactivate"
        // VS Code TreeDataProvider / WebviewProvider
        | "getTreeItem"
        | "getChildren"
        | "getParent"
        | "resolveTreeItem"
        | "resolveWebviewView"
        // VS Code FollowupProvider / ChatParticipant
        | "provideFollowups"
        | "provideCodeContext"
        | "buildEnhancedPrompt"
        // VS Code CodeActionProvider / CodeLensProvider
        | "provideCodeActions"
        | "provideCodeLenses"
        | "resolveCodeLens"
        // VS Code CompletionItemProvider
        | "provideCompletionItems"
        | "resolveCompletionItem"
        // VS Code LanguageModelTool
        | "invoke"
        | "prepareInvocation"
        // VS Code Disposable / lifecycle
        | "dispose"
        | "refresh"
        | "getIcon"
        // LSP protocol methods (called by LSP framework dispatch)
        | "initialized"
        | "shutdown"
        | "did_open"
        | "did_change"
        | "did_save"
        | "did_close"
        | "goto_definition"
        | "references"
        | "hover"
        | "document_symbol"
        | "prepare_call_hierarchy"
        | "incoming_calls"
        | "outgoing_calls"
        | "execute_command"
        | "completion"
        | "code_action"
        | "code_lens"
        | "formatting"
        | "rename"
        | "did_change_configuration"
    )
}

/// Check if a name is a well-known trait impl method (Rust/JS framework dispatch).
fn is_trait_impl_method(name: &str) -> bool {
    matches!(
        name,
        // Rust std trait impls
        "default"
            | "fmt"
            | "from"
            | "into"
            | "clone"
            | "clone_from"
            | "eq"
            | "ne"
            | "partial_cmp"
            | "cmp"
            | "hash"
            | "drop"
            | "deref"
            | "deref_mut"
            | "as_ref"
            | "as_mut"
            | "try_from"
            | "try_into"
            | "from_str"
            | "to_string"
            | "next"
            | "size_hint"
            // Serde
            | "serialize"
            | "deserialize"
            | "visit_str"
            | "visit_map"
            | "visit_seq"
            | "expecting"
            // Iterator/IntoIterator
            | "into_iter"
            | "from_iter"
            // Display/Debug/Error
            | "source"
            | "description"
            // Embedding/ML trait methods
            | "embed"
            | "embed_batch"
            | "dimension"
            | "encode"
            // Index/collection/metric trait methods
            | "insert"
            | "remove"
            | "get"
            | "contains"
            | "len"
            | "is_empty"
            | "iter"
            | "clear"
            | "distance"
            // Conversion/builder
            | "build"
            | "parse"
            | "new"
            // JS built-ins called by runtime
            | "toString"
            | "valueOf"
            | "toJSON"
            | "Symbol.iterator"
            | "[Symbol.iterator]"
    )
}

/// Compute confidence score for an unused code candidate.
/// Lower confidence = more likely a false positive.
fn compute_unused_confidence(name: &str, is_exported: bool, _node: &codegraph::Node) -> f64 {
    // Dynamic dispatch patterns — very likely called at runtime
    if name.contains("handler")
        || name.contains("Handler")
        || name.contains("callback")
        || name.contains("Callback")
        || name.contains("listener")
        || name.contains("Listener")
        || name.contains("middleware")
        || name.contains("Middleware")
    {
        return 0.2;
    }

    // MCP tool builder functions (called via collected vec, not direct call edges)
    if name.ends_with("_tool") {
        return 0.1;
    }

    // Serde default functions (referenced by #[serde(default = "...")] attribute)
    if name.starts_with("default_") {
        return 0.1;
    }

    // Migration functions (called by migration framework/runner)
    if name.starts_with("migrate_") || name.starts_with("migration_") {
        return 0.2;
    }

    // Event handler patterns (on_click, on_change, handleSubmit, etc.)
    if name.starts_with("on_")
        || (name.starts_with("on") && name.chars().nth(2).is_some_and(|c| c.is_uppercase()))
    {
        return 0.2;
    }
    if name.starts_with("handle") && name.chars().nth(6).is_some_and(|c| c.is_uppercase()) {
        return 0.2;
    }

    // Exported symbols — might be used by consumers outside the indexed workspace
    if is_exported {
        return 0.5;
    }

    // Private/unexported symbols with no callers — very likely unused
    0.9
}
