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

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{EdgeType, Node, NodeType, PropertyMap, PropertyValue};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn node_with(name: &str, path: &str, is_test: Option<bool>) -> Node {
        let mut props = PropertyMap::new();
        props.insert("name", name);
        props.insert("path", path);
        if let Some(v) = is_test {
            props.insert("is_test", v);
        }
        Node::new(0, NodeType::Function, props)
    }

    // ----- Scaffolding for find_unused_code end-to-end tests -----

    fn str_prop(v: &str) -> PropertyValue {
        PropertyValue::String(v.to_string())
    }

    /// Add a node carrying the given key/value properties, returning its id.
    fn add_node(graph: &mut CodeGraph, ty: NodeType, props: &[(&str, PropertyValue)]) -> NodeId {
        let mut map = PropertyMap::new();
        for (k, v) in props {
            map.insert(k.to_string(), v.clone());
        }
        graph.add_node(ty, map).expect("add_node")
    }

    /// Convenience: a Function node with name + path.
    fn add_fn(graph: &mut CodeGraph, name: &str, path: &str) -> NodeId {
        add_node(
            graph,
            NodeType::Function,
            &[("name", str_prop(name)), ("path", str_prop(path))],
        )
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    /// Wrap a built graph in the Arc<RwLock<>> a QueryEngine owns and build the
    /// caller indexes so get_callers resolves from Calls edges.
    async fn engine_for(g: CodeGraph) -> (Arc<RwLock<CodeGraph>>, QueryEngine) {
        let graph = Arc::new(RwLock::new(g));
        let engine = QueryEngine::new(graph.clone());
        engine.build_indexes().await;
        (graph, engine)
    }

    fn params(
        path: Option<&str>,
        scope: &str,
        include_tests: bool,
        confidence: f64,
    ) -> FindUnusedCodeParams {
        FindUnusedCodeParams {
            path: path.map(str::to_string),
            scope: scope.to_string(),
            include_tests,
            confidence,
        }
    }

    #[tokio::test]
    async fn private_uncalled_function_is_a_candidate() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("dead_helper")),
                ("path", str_prop("src/lib.rs")),
                ("visibility", str_prop("private")),
            ],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/lib.rs"), "file", false, 0.5),
        )
        .await;

        assert_eq!(result.total_checked, 1);
        assert_eq!(result.candidates.len(), 1);
        let c = &result.candidates[0];
        assert_eq!(c.name, "dead_helper");
        assert!(!c.is_public);
        // Private + no callers -> the highest 0.9 confidence bucket.
        assert_eq!(c.confidence, 0.9);
        assert_eq!(result.scope, "file");
        assert_eq!(result.min_confidence, 0.5);
    }

    #[tokio::test]
    async fn function_with_real_caller_is_not_a_candidate() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "used_fn", "src/lib.rs");
        let caller = add_fn(&mut g, "caller", "src/lib.rs");
        edge(&mut g, caller, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/lib.rs"), "file", false, 0.5),
        )
        .await;

        // Both are in scope, but used_fn has a caller and caller has one too? caller is uncalled.
        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"used_fn"));
        assert!(names.contains(&"caller"));
    }

    #[tokio::test]
    async fn function_called_only_by_test_is_skipped_when_tests_excluded() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "helper", "src/lib.rs");
        let test_caller = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("test_helper_works")),
                ("path", str_prop("src/lib.rs")),
                ("is_test", PropertyValue::Bool(true)),
            ],
        );
        edge(&mut g, test_caller, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/lib.rs"), "file", false, 0.5),
        )
        .await;

        // helper has a caller, but its only caller is a test -> test infra, skipped
        // (not reported as dead). The test node itself is filtered from the candidate set.
        assert!(result.candidates.is_empty());
    }

    #[tokio::test]
    async fn usage_edge_keeps_symbol_alive() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "referenced_fn", "src/lib.rs");
        let user = add_fn(&mut g, "user", "other.rs");
        // A References edge (not Calls) still counts as usage.
        edge(&mut g, user, target, EdgeType::References);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/lib.rs"), "file", false, 0.5),
        )
        .await;

        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"referenced_fn"));
    }

    #[tokio::test]
    async fn framework_entry_point_and_trait_method_are_skipped() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "main", "src/main.rs");
        add_fn(&mut g, "fmt", "src/main.rs");
        add_fn(&mut g, "genuinely_dead", "src/main.rs");
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/main.rs"), "file", false, 0.5),
        )
        .await;

        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"main"));
        assert!(!names.contains(&"fmt"));
        assert_eq!(names, vec!["genuinely_dead"]);
    }

    #[tokio::test]
    async fn synthetic_names_are_skipped() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "constructor", "src/x.ts");
        add_fn(&mut g, "arrow_function", "src/x.ts");
        add_fn(&mut g, "anonymous", "src/x.ts");
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", str_prop("")), ("path", str_prop("src/x.ts"))],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/x.ts"), "file", false, 0.5),
        )
        .await;

        assert!(result.candidates.is_empty());
        // All four are still counted as checked.
        assert_eq!(result.total_checked, 4);
    }

    #[tokio::test]
    async fn structural_nodes_are_skipped() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::CodeFile,
            &[
                ("name", str_prop("mod.rs")),
                ("path", str_prop("src/mod.rs")),
            ],
        );
        add_node(
            &mut g,
            NodeType::Module,
            &[
                ("name", str_prop("mymod")),
                ("path", str_prop("src/mod.rs")),
            ],
        );
        add_fn(&mut g, "dead", "src/mod.rs");
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/mod.rs"), "file", false, 0.5),
        )
        .await;

        // File/Module nodes are continue'd past; only the function is a candidate.
        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["dead"]);
    }

    #[tokio::test]
    async fn exported_symbol_filtered_by_confidence_threshold() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("pub_api")),
                ("path", str_prop("src/lib.rs")),
                ("is_public", PropertyValue::Bool(true)),
            ],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        // Exported uncalled symbol scores 0.5; a 0.9 threshold drops it.
        let strict = find_unused_code(
            &guard,
            &engine,
            params(Some("src/lib.rs"), "file", false, 0.9),
        )
        .await;
        assert!(strict.candidates.is_empty());
        assert_eq!(strict.total_checked, 1);

        // A 0.5 threshold keeps it.
        let lenient = find_unused_code(
            &guard,
            &engine,
            params(Some("src/lib.rs"), "file", false, 0.5),
        )
        .await;
        assert_eq!(lenient.candidates.len(), 1);
        assert!(lenient.candidates[0].is_public);
        assert_eq!(lenient.candidates[0].confidence, 0.5);
    }

    #[tokio::test]
    async fn class_with_called_child_method_is_kept() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let class = add_node(
            &mut g,
            NodeType::Class,
            &[("name", str_prop("Widget")), ("path", str_prop("src/w.rs"))],
        );
        let method = add_fn(&mut g, "render", "src/w.rs");
        edge(&mut g, class, method, EdgeType::Contains);
        let external = add_fn(&mut g, "external_caller", "other.rs");
        edge(&mut g, external, method, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/w.rs"), "file", false, 0.5),
        )
        .await;

        // The Widget class has a called child method -> not reported dead.
        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"Widget"));
    }

    #[tokio::test]
    async fn class_with_active_same_file_function_is_kept() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Class,
            &[("name", str_prop("Config")), ("path", str_prop("src/c.rs"))],
        );
        let sibling = add_fn(&mut g, "load", "src/c.rs");
        let external = add_fn(&mut g, "boot", "main.rs");
        edge(&mut g, external, sibling, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/c.rs"), "file", false, 0.5),
        )
        .await;

        // Config shares its file with a called function -> considered in use.
        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"Config"));
    }

    #[tokio::test]
    async fn workspace_scope_excludes_build_output_paths() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "src_dead", "src/lib.rs");
        add_fn(&mut g, "built_dead", "dist/bundle.js");
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(&guard, &engine, params(None, "workspace", false, 0.5)).await;

        // The build-output node is filtered from the candidate set entirely.
        assert_eq!(result.total_checked, 1);
        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["src_dead"]);
    }

    #[tokio::test]
    async fn unknown_scope_without_path_checks_nothing() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "dead", "src/lib.rs");
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_unused_code(&guard, &engine, params(None, "file", false, 0.5)).await;

        // scope "file" with no path resolves to an empty candidate set.
        assert_eq!(result.total_checked, 0);
        assert!(result.candidates.is_empty());
    }

    #[tokio::test]
    async fn include_tests_true_reports_test_helper_as_dead() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "helper", "src/lib.rs");
        let test_caller = add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("some_case")),
                ("path", str_prop("src/lib.rs")),
                ("is_test", PropertyValue::Bool(true)),
            ],
        );
        edge(&mut g, test_caller, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        // With include_tests=true, the test caller counts, so helper is alive
        // and the uncalled test node itself becomes the dead candidate.
        let result = find_unused_code(
            &guard,
            &engine,
            params(Some("src/lib.rs"), "file", true, 0.5),
        )
        .await;

        let names: Vec<&str> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"helper"));
        assert!(names.contains(&"some_case"));
        assert_eq!(result.total_checked, 2);
    }

    #[test]
    fn is_test_node_respects_is_test_property() {
        // The structural marker wins even when name/path look non-test.
        let n = node_with("do_work", "src/lib.rs", Some(true));
        assert!(is_test_node(&n));

        // Explicit false with non-test name/path is not a test.
        let n = node_with("do_work", "src/lib.rs", Some(false));
        assert!(!is_test_node(&n));
    }

    #[test]
    fn is_test_node_detects_by_name() {
        assert!(is_test_node(&node_with("test_parse", "src/lib.rs", None)));
        assert!(is_test_node(&node_with("parse_test", "src/lib.rs", None)));
        assert!(is_test_node(&node_with("TestFixture", "src/lib.rs", None)));
        assert!(!is_test_node(&node_with("parse", "src/lib.rs", None)));
    }

    #[test]
    fn is_test_node_detects_by_path() {
        assert!(is_test_node(&node_with("parse", "src/tests/mod.rs", None)));
        assert!(is_test_node(&node_with("parse", "src/foo.test.ts", None)));
        assert!(is_test_node(&node_with("parse", "src/foo.spec.ts", None)));
        assert!(is_test_node(&node_with("parse", "pkg\\tests\\x.rs", None)));
        assert!(is_test_node(&node_with("parse", "src/foo_test.go", None)));
        assert!(!is_test_node(&node_with("parse", "src/foo.rs", None)));
    }

    #[test]
    fn generate_test_path_patterns_covers_conventions() {
        let patterns = generate_test_path_patterns("/src/foo.ts");
        assert!(patterns.contains(&"/src/foo.test.ts".to_string()));
        assert!(patterns.contains(&"/src/foo.spec.ts".to_string()));
        assert!(patterns.contains(&"/src/foo_test.ts".to_string()));
        assert!(patterns.contains(&"/src/tests/foo.ts".to_string()));
        assert!(patterns.contains(&"/src/__tests__/foo.ts".to_string()));
        assert!(patterns.contains(&"/src/test/foo.ts".to_string()));
        assert!(patterns.contains(&"/src/tests/foo_test.ts".to_string()));
    }

    #[test]
    fn generate_test_path_patterns_handles_missing_extension() {
        // No extension -> no adjacent/subdir patterns are emitted.
        assert!(generate_test_path_patterns("/src/Makefile").is_empty());
        // No file stem at all -> empty.
        assert!(generate_test_path_patterns("/").is_empty());
    }

    #[test]
    fn is_build_output_path_matches_excluded_dirs() {
        assert!(is_build_output_path("project/node_modules/pkg/index.js"));
        assert!(is_build_output_path("a/dist/b.js"));
        assert!(is_build_output_path("target/debug/foo"));
        assert!(is_build_output_path("win\\build\\out.o"));
        assert!(!is_build_output_path("src/domain/unused_code.rs"));
        // Substring within a component must not match.
        assert!(!is_build_output_path("src/distribution/x.rs"));
    }

    #[test]
    fn is_framework_entry_point_recognizes_known_names() {
        assert!(is_framework_entry_point("main"));
        assert!(is_framework_entry_point("activate"));
        assert!(is_framework_entry_point("did_open"));
        assert!(is_framework_entry_point("provideCompletionItems"));
        assert!(!is_framework_entry_point("my_helper"));
    }

    #[test]
    fn is_trait_impl_method_recognizes_known_names() {
        assert!(is_trait_impl_method("fmt"));
        assert!(is_trait_impl_method("serialize"));
        assert!(is_trait_impl_method("into_iter"));
        assert!(is_trait_impl_method("toJSON"));
        assert!(!is_trait_impl_method("business_logic"));
    }

    #[test]
    fn compute_unused_confidence_scores_by_pattern() {
        let dummy = node_with("x", "src/lib.rs", None);

        // Dynamic dispatch patterns -> very low.
        assert_eq!(
            compute_unused_confidence("clickHandler", false, &dummy),
            0.2
        );
        assert_eq!(compute_unused_confidence("myListener", false, &dummy), 0.2);
        // MCP tool builders / serde defaults -> lowest.
        assert_eq!(compute_unused_confidence("search_tool", false, &dummy), 0.1);
        assert_eq!(
            compute_unused_confidence("default_limit", false, &dummy),
            0.1
        );
        // Migration + event handler naming -> low.
        assert_eq!(compute_unused_confidence("migrate_v2", false, &dummy), 0.2);
        assert_eq!(compute_unused_confidence("on_click", false, &dummy), 0.2);
        assert_eq!(compute_unused_confidence("onChange", false, &dummy), 0.2);
        assert_eq!(
            compute_unused_confidence("handleSubmit", false, &dummy),
            0.2
        );
        // Exported vs private plain symbols.
        assert_eq!(compute_unused_confidence("plain", true, &dummy), 0.5);
        assert_eq!(compute_unused_confidence("plain", false, &dummy), 0.9);
    }
}
