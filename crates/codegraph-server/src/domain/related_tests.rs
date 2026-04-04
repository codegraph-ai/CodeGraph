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
