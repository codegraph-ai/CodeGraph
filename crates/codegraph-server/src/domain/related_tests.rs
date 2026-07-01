// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Related test discovery — single source of truth for both LSP and MCP handlers.
//!
//! This module contains the domain logic for finding tests related to a symbol.
//! It has no dependency on tower-lsp, MCP protocol types, or serde_json::Value.

use crate::ai_query::{EntryType, QueryEngine};
use crate::domain::{node_props, unused_code};
use codegraph::{CodeGraph, NodeId, NodeType};

// ==========================================
// Parameters & Results
// ==========================================

pub(crate) struct FindRelatedTestsParams {
    /// File path (not URI) of the source file.
    pub path: String,
    /// Pre-resolved target node. If None, only same-file and adjacent-file searches run.
    pub target_node_id: Option<NodeId>,
    pub limit: usize,
}

pub(crate) struct RelatedTestEntry {
    pub name: String,
    pub node_id: NodeId,
    pub relationship: String,
    /// Raw file path (not URI).
    pub path: String,
}

pub(crate) struct FindRelatedTestsResult {
    pub tests: Vec<RelatedTestEntry>,
}

// ==========================================
// Core Domain Function
// ==========================================

/// Find tests related to a symbol or file.
///
/// Strategy:
/// 1. If a target symbol is found, search for test entry points that call it
///    (via QueryEngine callee traversal, depth 3).
/// 2. Search for test functions in the same file.
/// 3. Search for test functions in adjacent test files (foo.test.ts, tests/foo.rs, etc).
pub(crate) async fn find_related_tests(
    graph: &CodeGraph,
    query_engine: &QueryEngine,
    params: FindRelatedTestsParams,
) -> FindRelatedTestsResult {
    let mut tests = Vec::new();
    let mut seen = std::collections::HashSet::<NodeId>::new();

    // Stage 1: if we have a target, find test entry points that call it
    if let Some(target_id) = params.target_node_id {
        seen.insert(target_id);
        let entry_types = [EntryType::TestEntry];
        let test_entries = query_engine.find_entry_points(&entry_types).await;

        for test in test_entries.iter().take(params.limit * 2) {
            if tests.len() >= params.limit {
                break;
            }
            let callees = query_engine.get_callees(test.node_id, 3).await;
            if callees.iter().any(|c| c.node_id == target_id) && seen.insert(test.node_id) {
                let path = graph
                    .get_node(test.node_id)
                    .ok()
                    .map(|node| node_props::path(node).to_string())
                    .unwrap_or_default();
                tests.push(RelatedTestEntry {
                    name: test.symbol.name.clone(),
                    node_id: test.node_id,
                    relationship: "calls_target".to_string(),
                    path,
                });
            }
        }
    }

    // Stage 2: find test functions in the same file
    if tests.len() < params.limit {
        if let Ok(file_nodes) = graph
            .query()
            .property("path", params.path.as_str())
            .execute()
        {
            for node_id in file_nodes {
                if !seen.insert(node_id) || tests.len() >= params.limit {
                    continue;
                }
                if let Ok(node) = graph.get_node(node_id) {
                    if node.node_type != NodeType::Function {
                        continue;
                    }
                    if unused_code::is_test_node(node) {
                        tests.push(RelatedTestEntry {
                            name: node_props::name(node).to_string(),
                            node_id,
                            relationship: "same_file".to_string(),
                            path: node_props::path(node).to_string(),
                        });
                    }
                }
            }
        }
    }

    // Stage 3: find test functions in adjacent test files
    if tests.len() < params.limit {
        let test_path_patterns = unused_code::generate_test_path_patterns(&params.path);
        for test_path in &test_path_patterns {
            if tests.len() >= params.limit {
                break;
            }
            if let Ok(test_nodes) = graph.query().property("path", test_path.as_str()).execute() {
                for node_id in test_nodes {
                    if !seen.insert(node_id) || tests.len() >= params.limit {
                        continue;
                    }
                    if let Ok(node) = graph.get_node(node_id) {
                        if node.node_type != NodeType::Function {
                            continue;
                        }
                        if unused_code::is_test_node(node) {
                            tests.push(RelatedTestEntry {
                                name: node_props::name(node).to_string(),
                                node_id,
                                relationship: "adjacent_file".to_string(),
                                path: test_path.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    FindRelatedTestsResult { tests }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{EdgeType, PropertyMap, PropertyValue};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Add a node carrying the given key/value string properties, returning its id.
    fn add_node(graph: &mut CodeGraph, ty: NodeType, props: &[(&str, &str)]) -> NodeId {
        let mut map = PropertyMap::new();
        for (k, v) in props {
            map.insert(k.to_string(), PropertyValue::String(v.to_string()));
        }
        graph.add_node(ty, map).expect("add_node")
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    /// Wrap a built graph in the Arc<RwLock<>> a QueryEngine owns, build call
    /// indexes, and hand back both so callers can also grab a read guard for the
    /// `&CodeGraph` argument find_related_tests expects.
    async fn engine_for(g: CodeGraph) -> (Arc<RwLock<CodeGraph>>, QueryEngine) {
        let graph = Arc::new(RwLock::new(g));
        let engine = QueryEngine::new(graph.clone());
        engine.build_indexes().await;
        (graph, engine)
    }

    fn params(path: &str, target: Option<NodeId>, limit: usize) -> FindRelatedTestsParams {
        FindRelatedTestsParams {
            path: path.to_string(),
            target_node_id: target,
            limit,
        }
    }

    #[tokio::test]
    async fn empty_graph_returns_no_tests() {
        let g = CodeGraph::in_memory().expect("in_memory");
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;
        let result = find_related_tests(&guard, &engine, params("/src/foo.rs", None, 10)).await;
        assert!(result.tests.is_empty());
    }

    #[tokio::test]
    async fn stage1_test_entry_calling_target_is_reported() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", "target")]);
        let test_fn = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_target"), ("path", "/src/foo_test.rs")],
        );
        edge(&mut g, test_fn, target, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result =
            find_related_tests(&guard, &engine, params("/src/foo.rs", Some(target), 10)).await;
        assert_eq!(result.tests.len(), 1);
        assert_eq!(result.tests[0].node_id, test_fn);
        assert_eq!(result.tests[0].relationship, "calls_target");
        assert_eq!(result.tests[0].path, "/src/foo_test.rs");
    }

    #[tokio::test]
    async fn stage1_test_entry_not_calling_target_is_ignored() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_node(&mut g, NodeType::Function, &[("name", "target")]);
        // A test entry that calls something else, never the target.
        let other = add_node(&mut g, NodeType::Function, &[("name", "helper")]);
        let test_fn = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_unrelated"), ("path", "/src/other.rs")],
        );
        edge(&mut g, test_fn, other, EdgeType::Calls);
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result =
            find_related_tests(&guard, &engine, params("/src/foo.rs", Some(target), 10)).await;
        assert!(result.tests.is_empty());
    }

    #[tokio::test]
    async fn stage2_same_file_test_function_is_reported() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_thing"), ("path", "/src/foo.rs")],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_related_tests(&guard, &engine, params("/src/foo.rs", None, 10)).await;
        assert_eq!(result.tests.len(), 1);
        assert_eq!(result.tests[0].relationship, "same_file");
        assert_eq!(result.tests[0].name, "test_thing");
    }

    #[tokio::test]
    async fn stage2_non_function_in_same_file_is_excluded() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // A test-named node that is not a Function must not be collected.
        add_node(
            &mut g,
            NodeType::Class,
            &[("name", "TestFixture"), ("path", "/src/foo.rs")],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_related_tests(&guard, &engine, params("/src/foo.rs", None, 10)).await;
        assert!(result.tests.is_empty());
    }

    #[tokio::test]
    async fn stage3_adjacent_test_file_is_reported() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // No node lives in /src/foo.rs, but a generated pattern (/src/foo_test.rs)
        // holds a test function, so stage 3 should surface it.
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_adjacent"), ("path", "/src/foo_test.rs")],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_related_tests(&guard, &engine, params("/src/foo.rs", None, 10)).await;
        assert_eq!(result.tests.len(), 1);
        assert_eq!(result.tests[0].relationship, "adjacent_file");
        assert_eq!(result.tests[0].path, "/src/foo_test.rs");
    }

    #[tokio::test]
    async fn limit_truncates_same_file_results() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_one"), ("path", "/src/foo.rs")],
        );
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_two"), ("path", "/src/foo.rs")],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result = find_related_tests(&guard, &engine, params("/src/foo.rs", None, 1)).await;
        assert_eq!(result.tests.len(), 1);
    }

    #[tokio::test]
    async fn target_is_excluded_from_same_file_results() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // The target itself is a test-named function in the scanned file; because
        // it is seeded into `seen`, stage 2 must skip it and only return the sibling.
        let target = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_target"), ("path", "/src/foo.rs")],
        );
        let sibling = add_node(
            &mut g,
            NodeType::Function,
            &[("name", "test_sibling"), ("path", "/src/foo.rs")],
        );
        let (graph, engine) = engine_for(g).await;
        let guard = graph.read().await;

        let result =
            find_related_tests(&guard, &engine, params("/src/foo.rs", Some(target), 10)).await;
        assert_eq!(result.tests.len(), 1);
        assert_eq!(result.tests[0].node_id, sibling);
        assert_eq!(result.tests[0].relationship, "same_file");
    }
}
