// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Impact analysis — transport-agnostic.
//!
//! Extracts analyze_impact from MCP server.

use crate::ai_query::QueryEngine;
use crate::domain::node_props;
use codegraph::{
    CodeGraph, Direction, EdgeType, NamespacedBackend, NodeId, RocksDBBackend, StorageBackend,
};
use serde::Serialize;
use std::collections::HashSet;
use tokio::sync::RwLock;

// ============================================================
// Response Types
// ============================================================

/// A symbol directly impacted by a change (depth = 1, all edge types).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ImpactedSymbol {
    pub node_id: String,
    pub name: String,
    pub depth: u32,
    /// Semantic impact type: "caller", "reference", "subclass", "implementation".
    pub impact_type: String,
    /// File path on disk (empty string if unknown).
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub col_start: u32,
    pub col_end: u32,
    /// "breaking" | "warning" | "info"
    pub severity: String,
    /// Whether the impacted symbol is a test function or lives in a test file.
    pub is_test: bool,
    /// Raw edge type string for debugging (e.g. "Calls", "References").
    pub edge_type_str: String,
}

/// An indirect impact item (reached via 2-level BFS from direct impacts).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct IndirectImpactItem {
    pub node_id: String,
    pub path: String,
    /// Chain of paths from the changed symbol to this item (for display).
    pub via_path: Vec<String>,
    pub severity: String,
}

/// A consumer of the changed symbol found in another indexed project.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CrossProjectImpact {
    pub project: String,
    /// Name of the consuming function/symbol in the other project.
    pub symbol_name: String,
    pub path: String,
    pub line_start: u32,
    /// How the symbol is consumed: "caller", "includer", "type_reference".
    pub impact_type: String,
    /// Signature of the consuming function (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// "breaking" | "warning" | "info"
    pub severity: String,
}

/// Result of `analyze_impact`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ImpactResult {
    pub symbol_id: String,
    pub symbol_name: String,
    pub change_type: String,
    /// Direct impacts (all incoming edge types, depth = 1).
    pub impacted: Vec<ImpactedSymbol>,
    /// Indirect impacts (2-level BFS from direct impact nodes).
    pub indirect_impacted: Vec<IndirectImpactItem>,
    /// Consumers in other indexed projects.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cross_project_impacts: Vec<CrossProjectImpact>,
    pub total_impacted: usize,
    pub direct_impacted: usize,
    pub risk_level: String,
    pub files_affected: usize,
    pub breaking_changes: usize,
    pub warnings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

// ============================================================
// Domain Function
// ============================================================

/// Analyze the blast radius of a change to a symbol.
///
/// `change_type` is one of: "modify" | "delete" | "rename"
///
/// Computes direct impact from all incoming edge types (not just calls), then BFS
/// 2 levels from each direct impact for indirect impact. Uses `query_engine.get_callers()`
/// for risk-level calculation (broader caller count at depth 3).
pub(crate) async fn analyze_impact(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    start_node: NodeId,
    change_type: &str,
    used_fallback: bool,
    requested_line: Option<u32>,
    current_project_slug: Option<&str>,
) -> ImpactResult {
    let symbol_name = {
        let g = graph.read().await;
        g.get_node(start_node)
            .ok()
            .map(|n| node_props::name(n).to_string())
            .unwrap_or_default()
    };

    // Use query_engine for risk assessment (callers to depth 3, calls edges only)
    let all_callers = query_engine.get_callers(start_node, 3).await;

    // Compute direct impacts via graph reads (all incoming edge types)
    let mut direct_impacts: Vec<(NodeId, ImpactedSymbol)> = Vec::new();
    let mut affected_files: HashSet<String> = HashSet::new();

    {
        let g = graph.read().await;
        if let Ok(neighbors) = g.get_neighbors(start_node, Direction::Incoming) {
            for source_id in neighbors {
                if let Ok(edge_ids) = g.get_edges_between(source_id, start_node) {
                    for edge_id in edge_ids {
                        if let Ok(edge) = g.get_edge(edge_id) {
                            let impact_type = match edge.edge_type {
                                EdgeType::Calls => "caller",
                                EdgeType::References => "reference",
                                EdgeType::Extends => "subclass",
                                EdgeType::Implements => "implementation",
                                _ => "reference",
                            };
                            let severity = match change_type {
                                "delete" | "rename" => "breaking",
                                "modify" => "warning",
                                _ => "info",
                            };
                            if let Ok(ref_node) = g.get_node(source_id) {
                                let name = node_props::name(ref_node).to_string();
                                let path = node_props::path(ref_node).to_string();
                                let line_start = node_props::line_start(ref_node);
                                let line_end = node_props::line_end(ref_node);
                                let col_start =
                                    node_props::col_start_from_props(&ref_node.properties);
                                let col_end = node_props::col_end_from_props(&ref_node.properties);
                                let is_test = crate::domain::unused_code::is_test_node(ref_node);
                                let edge_type_str = format!("{:?}", edge.edge_type);
                                affected_files.insert(path.clone());
                                direct_impacts.push((
                                    source_id,
                                    ImpactedSymbol {
                                        node_id: source_id.to_string(),
                                        name,
                                        depth: 1,
                                        impact_type: impact_type.to_string(),
                                        path,
                                        line_start,
                                        line_end,
                                        col_start,
                                        col_end,
                                        severity: severity.to_string(),
                                        is_test,
                                        edge_type_str,
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // Compute indirect impacts via BFS from each direct impact node
    let mut indirect_impacted: Vec<IndirectImpactItem> = Vec::new();
    let mut indirect_visited: HashSet<NodeId> = HashSet::new();
    indirect_visited.insert(start_node);
    for &(id, _) in &direct_impacts {
        indirect_visited.insert(id);
    }

    {
        let g = graph.read().await;
        for &(direct_id, ref impact) in &direct_impacts {
            if let Ok(indirect_ids) = g.bfs(direct_id, Direction::Incoming, Some(2)) {
                for indirect_id in indirect_ids {
                    if indirect_visited.contains(&indirect_id) {
                        continue;
                    }
                    indirect_visited.insert(indirect_id);
                    if let Ok(ref_node) = g.get_node(indirect_id) {
                        let ref_path = node_props::path(ref_node).to_string();
                        if !affected_files.contains(&ref_path) {
                            indirect_impacted.push(IndirectImpactItem {
                                node_id: indirect_id.to_string(),
                                path: ref_path.clone(),
                                via_path: vec![impact.path.clone(), ref_path],
                                severity: "warning".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    let impacted: Vec<ImpactedSymbol> = direct_impacts.into_iter().map(|(_, sym)| sym).collect();

    let direct_impacted = impacted.len();
    let breaking_changes = impacted.iter().filter(|i| i.severity == "breaking").count();
    let warnings =
        impacted.iter().filter(|i| i.severity == "warning").count() + indirect_impacted.len();

    // Use all_callers (depth 3) for risk_level to account for transitive call exposure
    // Cross-project consumers elevate risk (external breakage is harder to coordinate)
    let caller_count = all_callers.len();
    let risk_level = match (change_type, caller_count) {
        ("delete", n) if n > 10 => "critical",
        ("delete", n) if n > 0 => "high",
        ("rename", n) if n > 10 => "high",
        ("rename", n) if n > 0 => "medium",
        ("modify", n) if n > 20 => "medium",
        ("modify", _) => "low",
        _ => "low",
    };

    let (used_fallback_field, fallback_message) = if used_fallback {
        (
            Some(true),
            Some(format!(
                "No symbol at line {}. Using nearest symbol '{}' instead.",
                requested_line.unwrap_or(0),
                symbol_name
            )),
        )
    } else {
        (None, None)
    };

    // Cross-project impact: search other indexed projects for consumers
    let source_file = {
        let g = graph.read().await;
        g.get_node(start_node)
            .ok()
            .and_then(|n| n.properties.get_string("path").map(|s| s.to_string()))
    };
    let cross_project_impacts = find_cross_project_consumers(
        &symbol_name,
        source_file.as_deref(),
        change_type,
        current_project_slug,
    );

    // Elevate risk when cross-project consumers exist — external breakage is
    // harder to coordinate than in-project changes
    let risk_level = if !cross_project_impacts.is_empty() {
        match risk_level {
            "low" => "medium",
            "medium" => "high",
            _ => risk_level,
        }
    } else {
        risk_level
    };

    let total_impacted = direct_impacted + indirect_impacted.len() + cross_project_impacts.len();

    ImpactResult {
        symbol_id: start_node.to_string(),
        symbol_name,
        change_type: change_type.to_string(),
        impacted,
        indirect_impacted,
        cross_project_impacts,
        total_impacted,
        direct_impacted,
        risk_level: risk_level.to_string(),
        files_affected: affected_files.len(),
        breaking_changes,
        warnings,
        used_fallback: used_fallback_field,
        fallback_message,
    }
}

/// Search other indexed projects for functions that call, reference, or include
/// the given symbol.
///
/// Three search strategies per project:
/// 1. **Unresolved calls** — functions with `unresolved_calls` containing symbol name
/// 2. **Resolved calls** — `Calls` edges where target node name matches
/// 3. **Include/import tracking** — files that import the source file (for header changes)
fn find_cross_project_consumers(
    symbol_name: &str,
    source_file: Option<&str>,
    change_type: &str,
    current_project_slug: Option<&str>,
) -> Vec<CrossProjectImpact> {
    if symbol_name.is_empty() {
        return Vec::new();
    }

    tracing::debug!(
        "[cross-project] Searching for '{}' (source: {:?}, change: {}, current: {:?})",
        symbol_name,
        source_file,
        change_type,
        current_project_slug
    );

    let db_path = match crate::memory::shared_graph_db_path() {
        Ok(p) if p.exists() => p,
        _ => {
            tracing::info!("[cross-project] No shared graph.db found");
            return Vec::new();
        }
    };

    // Open RocksDB, scan registry, then DROP the connection before per-project
    // loading — RocksDB uses exclusive locks, so only one connection at a time.
    let entries = {
        let rocks = match RocksDBBackend::open(&db_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[cross-project] Failed to open graph.db: {}", e);
                return Vec::new();
            }
        };
        match StorageBackend::scan_prefix(&rocks, b"_registry:") {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("[cross-project] Failed to scan registry: {}", e);
                return Vec::new();
            }
        }
        // rocks dropped here — lock released
    };

    tracing::debug!(
        "[cross-project] Found {} registered projects",
        entries.len()
    );

    let severity = match change_type {
        "delete" | "rename" => "breaking",
        "modify" => "warning",
        _ => "info",
    };

    // Extract the base file name from source path for include matching
    // e.g., "/path/to/ice_common.h" → "ice_common.h"
    let source_basename = source_file.and_then(|p| {
        std::path::Path::new(p)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
    });

    let mut results = Vec::new();
    let current_slug = current_project_slug.unwrap_or("");

    for (key, value) in entries {
        let slug = String::from_utf8_lossy(&key)
            .strip_prefix("_registry:")
            .unwrap_or("")
            .to_string();

        if slug == current_slug || slug.is_empty() {
            continue;
        }

        let project_name = serde_json::from_slice::<serde_json::Value>(&value)
            .ok()
            .and_then(|v| {
                v.get("workspace").and_then(|n| n.as_str()).and_then(|p| {
                    std::path::Path::new(p)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                })
            })
            .unwrap_or_else(|| slug.clone());

        let other_rocks = match RocksDBBackend::open(&db_path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let namespaced = NamespacedBackend::new(Box::new(other_rocks), &slug);
        let mut other_graph = match CodeGraph::with_backend(Box::new(namespaced)) {
            Ok(g) => g,
            Err(_) => continue,
        };
        let _ = other_graph.detach_storage();

        let mut seen = HashSet::new();

        // Strategy 1: Unresolved calls containing the symbol name
        for (node_id, node) in other_graph.iter_nodes() {
            if let Some(calls) = node.properties.get_string_list_compat("unresolved_calls") {
                if calls.iter().any(|c| c == symbol_name) && seen.insert(node_id) {
                    results.push(CrossProjectImpact {
                        project: project_name.clone(),
                        symbol_name: node_props::name(node).to_string(),
                        path: node_props::path(node).to_string(),
                        line_start: node_props::line_start(node),
                        impact_type: "caller".to_string(),
                        signature: node
                            .properties
                            .get_string("signature")
                            .map(|s| s.to_string()),
                        severity: severity.to_string(),
                    });
                }
            }

            // Strategy 2: Unresolved type references containing the symbol name
            if let Some(refs) = node
                .properties
                .get_string_list_compat("unresolved_type_refs")
            {
                if refs.iter().any(|r| r == symbol_name) && seen.insert(node_id) {
                    results.push(CrossProjectImpact {
                        project: project_name.clone(),
                        symbol_name: node_props::name(node).to_string(),
                        path: node_props::path(node).to_string(),
                        line_start: node_props::line_start(node),
                        impact_type: "type_reference".to_string(),
                        signature: node
                            .properties
                            .get_string("signature")
                            .map(|s| s.to_string()),
                        severity: severity.to_string(),
                    });
                }
            }
        }

        // Strategy 3: Include/import tracking — find files that #include the source file
        // Follows Imports edges from CodeFile nodes to import nodes whose name matches
        // the source file basename (e.g., ice_common.h)
        if let Some(ref basename) = source_basename {
            for (file_id, file_node) in other_graph.iter_nodes() {
                if file_node.node_type != codegraph::NodeType::CodeFile {
                    continue;
                }
                // Check outgoing Imports edges from this file
                if let Ok(neighbors) = other_graph.get_neighbors(file_id, Direction::Outgoing) {
                    for target_id in neighbors {
                        if let Ok(edges) = other_graph.get_edges_between(file_id, target_id) {
                            for edge_id in edges {
                                if let Ok(edge) = other_graph.get_edge(edge_id) {
                                    if edge.edge_type == EdgeType::Imports {
                                        if let Ok(import_node) = other_graph.get_node(target_id) {
                                            let import_name = node_props::name(import_node);
                                            if import_name.contains(basename.as_str())
                                                && seen.insert(file_id)
                                            {
                                                results.push(CrossProjectImpact {
                                                    project: project_name.clone(),
                                                    symbol_name: node_props::name(file_node)
                                                        .to_string(),
                                                    path: node_props::path(file_node).to_string(),
                                                    line_start: 0,
                                                    impact_type: "includer".to_string(),
                                                    signature: None,
                                                    severity: severity.to_string(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !results.is_empty() {
        tracing::info!(
            "Found {} cross-project consumers of '{}' across {} strategies",
            results.len(),
            symbol_name,
            if source_basename.is_some() { 3 } else { 2 },
        );
    }

    results
}
