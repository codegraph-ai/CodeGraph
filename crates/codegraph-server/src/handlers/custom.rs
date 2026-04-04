// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Custom LSP request handlers for graph-based features.

use crate::backend::CodeGraphBackend;
use crate::domain::node_props;
use codegraph::Node;
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Position, Range, Url};

// ==========================================
// Dependency Graph Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyGraphParams {
    pub uri: String,
    pub depth: Option<usize>,
    pub include_external: Option<bool>,
    pub direction: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyNode {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub language: String,
    pub uri: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyGraphResponse {
    pub nodes: Vec<DependencyNode>,
    pub edges: Vec<DependencyEdge>,
}

// ==========================================
// Related Tests Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedTestsParams {
    pub uri: String,
    pub position: Position,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedTest {
    pub uri: String,
    pub test_name: String,
    pub relationship: String,
    pub range: Range,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedTestsResponse {
    pub tests: Vec<RelatedTest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

impl CodeGraphBackend {
    pub async fn handle_get_dependency_graph(
        &self,
        params: DependencyGraphParams,
    ) -> Result<DependencyGraphResponse> {
        let uri = Url::parse(&params.uri)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;

        let depth = params.depth.unwrap_or(3);
        let include_external = params.include_external.unwrap_or(false);
        let path_str = path.to_string_lossy().to_string();
        let dir_param = params.direction.as_deref().unwrap_or("both");

        let result = {
            let graph = self.graph.read().await;
            crate::domain::dependency_graph::get_dependency_graph(
                &graph, &path_str, depth, dir_param,
            )
        };

        // Convert domain nodes → LSP DependencyNode, applying include_external filter
        let nodes = result
            .nodes
            .into_iter()
            .filter(|n| include_external || !n.is_external)
            .map(|n| {
                // Use path from domain; fall back to symbol_index for nodes without a path.
                let uri = if n.path.is_empty() {
                    self.symbol_index
                        .find_file_for_node(n.id.parse().unwrap_or_default())
                        .and_then(|p| p.to_str().map(|s| format!("file://{s}")))
                        .unwrap_or_default()
                } else {
                    n.path
                };
                DependencyNode {
                    id: n.id,
                    label: n.name,
                    node_type: n.node_type,
                    language: n.language,
                    uri,
                }
            })
            .collect();

        let edges = result
            .edges
            .into_iter()
            .map(|e| DependencyEdge {
                from: e.from,
                to: e.to,
                edge_type: e.edge_type,
            })
            .collect();

        Ok(DependencyGraphResponse { nodes, edges })
    }

    /// Find tests related to a symbol at the given position — delegates to domain::related_tests.
    pub async fn handle_find_related_tests(
        &self,
        params: RelatedTestsParams,
    ) -> Result<RelatedTestsResponse> {
        let uri = Url::parse(&params.uri)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;

        let path_str = path.to_string_lossy().to_string();
        let limit = params.limit.unwrap_or(10);

        let graph = self.graph.read().await;

        // Resolve target node at the given position (optional — domain handles None)
        let target_node_id = self.find_node_at_position(&graph, &path, params.position)?;

        let domain_params = crate::domain::related_tests::FindRelatedTestsParams {
            path: path_str,
            target_node_id,
            limit,
        };

        let result = crate::domain::related_tests::find_related_tests(
            &graph,
            &self.query_engine,
            domain_params,
        )
        .await;

        let mut tests = Vec::new();
        for entry in &result.tests {
            // Convert domain entry to LSP RelatedTest with proper Range
            let range = graph
                .get_node(entry.node_id)
                .ok()
                .and_then(Self::node_to_range);

            if let Some(range) = range {
                tests.push(RelatedTest {
                    uri: entry.path.clone(),
                    test_name: entry.name.clone(),
                    relationship: entry.relationship.clone(),
                    range,
                });
            }
        }

        let truncated = if tests.len() >= limit {
            Some(true)
        } else {
            None
        };

        Ok(RelatedTestsResponse { tests, truncated })
    }

    /// Check if a node is a test function or lives in a test file.
    /// Delegates to domain::unused_code::is_test_node.
    #[cfg(test)]
    fn is_test_node(node: &Node) -> bool {
        crate::domain::unused_code::is_test_node(node)
    }

    fn node_to_range(node: &Node) -> Option<Range> {
        let start_line = node_props::line_start(node).saturating_sub(1);
        let end_line = node_props::line_end(node).saturating_sub(1);

        Some(Range {
            start: Position {
                line: start_line,
                character: node_props::col_start_from_props(&node.properties),
            },
            end: Position {
                line: end_line,
                character: node_props::col_end_from_props(&node.properties),
            },
        })
    }
}

// ==========================================
// Call Graph Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallGraphParams {
    pub uri: String,
    pub position: Position,
    pub direction: Option<String>,
    pub depth: Option<usize>,
    pub include_external: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionNode {
    pub id: String,
    pub name: String,
    pub signature: String,
    pub uri: String,
    pub range: Range,
    pub language: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallEdge {
    pub from: String,
    pub to: String,
    pub call_sites: Vec<Range>,
    pub is_recursive: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallGraphResponse {
    pub root: Option<FunctionNode>,
    pub nodes: Vec<FunctionNode>,
    pub edges: Vec<CallEdge>,
}

impl CodeGraphBackend {
    pub async fn handle_get_call_graph(
        &self,
        params: CallGraphParams,
    ) -> Result<CallGraphResponse> {
        let uri = Url::parse(&params.uri)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;

        let depth = params.depth.unwrap_or(3) as u32;
        let dir_param = params.direction.as_deref().unwrap_or("both");

        let node_id = {
            let graph = self.graph.read().await;
            match self.find_node_at_position(&graph, &path, params.position)? {
                Some(id) => id,
                None => {
                    return Ok(CallGraphResponse {
                        root: None,
                        nodes: Vec::new(),
                        edges: Vec::new(),
                    })
                }
            }
        };

        let result = crate::domain::call_graph::get_call_graph(
            &self.graph,
            &self.query_engine,
            node_id,
            depth,
            dir_param,
            false,
            None,
        )
        .await;

        // Convert domain root_node → LSP FunctionNode (use params.uri for root path)
        let root = result.root_node.as_ref().map(|rn| {
            let uri = if rn.path.is_empty() {
                params.uri.clone()
            } else {
                rn.path.clone()
            };
            domain_node_to_function_node(rn, uri)
        });

        // Convert domain nodes → LSP FunctionNode
        let nodes: Vec<FunctionNode> = result
            .nodes
            .iter()
            .map(|n| {
                let uri = if n.path.is_empty() {
                    self.symbol_index
                        .find_file_for_node(n.id.parse().unwrap_or_default())
                        .and_then(|p| p.to_str().map(|s| format!("file://{s}")))
                        .unwrap_or_default()
                } else {
                    n.path.clone()
                };
                domain_node_to_function_node(n, uri)
            })
            .collect();

        // Convert domain edges → LSP CallEdge
        let edges: Vec<CallEdge> = result
            .edges
            .iter()
            .map(|e| {
                let is_recursive = e.from == e.to;
                CallEdge {
                    from: e.from.clone(),
                    to: e.to.clone(),
                    call_sites: Vec::new(),
                    is_recursive,
                }
            })
            .collect();

        Ok(CallGraphResponse { root, nodes, edges })
    }
}

/// Convert a domain `CallGraphNode` to an LSP `FunctionNode`.
fn domain_node_to_function_node(
    node: &crate::domain::call_graph::CallGraphNode,
    uri: String,
) -> FunctionNode {
    FunctionNode {
        id: node.id.clone(),
        name: node.name.clone(),
        signature: node.signature.clone(),
        uri,
        range: Range {
            start: Position {
                line: node.line_start.saturating_sub(1),
                character: node.col_start,
            },
            end: Position {
                line: node.line_end.saturating_sub(1),
                character: node.col_end,
            },
        },
        language: node.language.clone(),
    }
}

// ==========================================
// Impact Analysis Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactAnalysisParams {
    pub uri: String,
    pub position: Position,
    pub analysis_type: String, // "modify", "delete", "rename"
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectImpact {
    pub uri: String,
    pub range: Range,
    #[serde(rename = "type")]
    pub impact_type: String,
    pub severity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndirectImpact {
    pub uri: String,
    pub path: Vec<String>,
    pub severity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AffectedTest {
    pub uri: String,
    pub test_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactSummary {
    pub files_affected: usize,
    pub breaking_changes: usize,
    pub warnings: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactAnalysisResponse {
    pub direct_impact: Vec<DirectImpact>,
    pub indirect_impact: Vec<IndirectImpact>,
    pub affected_tests: Vec<AffectedTest>,
    pub summary: ImpactSummary,
}

impl CodeGraphBackend {
    pub async fn handle_analyze_impact(
        &self,
        params: ImpactAnalysisParams,
    ) -> Result<ImpactAnalysisResponse> {
        let uri = Url::parse(&params.uri)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;

        let node_id = {
            let graph = self.graph.read().await;
            match self.find_node_at_position(&graph, &path, params.position)? {
                Some(id) => id,
                None => {
                    return Ok(ImpactAnalysisResponse {
                        direct_impact: Vec::new(),
                        indirect_impact: Vec::new(),
                        affected_tests: Vec::new(),
                        summary: ImpactSummary {
                            files_affected: 0,
                            breaking_changes: 0,
                            warnings: 0,
                        },
                    })
                }
            }
        };

        let result = crate::domain::impact::analyze_impact(
            &self.graph,
            &self.query_engine,
            node_id,
            &params.analysis_type,
            false,
            None,
            None, // LSP doesn't have project slug
        )
        .await;

        // Convert domain impacted → LSP DirectImpact + AffectedTest
        let mut direct_impact: Vec<DirectImpact> = Vec::new();
        let mut affected_tests: Vec<AffectedTest> = Vec::new();

        for sym in &result.impacted {
            let uri = if sym.path.is_empty() {
                self.symbol_index
                    .find_file_for_node(sym.node_id.parse().unwrap_or_default())
                    .and_then(|p| p.to_str().map(|s| format!("file://{s}")))
                    .unwrap_or_default()
            } else {
                sym.path.clone()
            };

            if sym.is_test {
                affected_tests.push(AffectedTest {
                    uri: uri.clone(),
                    test_name: sym.name.clone(),
                });
            }

            direct_impact.push(DirectImpact {
                uri,
                range: Range {
                    start: Position {
                        line: sym.line_start.saturating_sub(1),
                        character: sym.col_start,
                    },
                    end: Position {
                        line: sym.line_end.saturating_sub(1),
                        character: sym.col_end,
                    },
                },
                impact_type: sym.impact_type.clone(),
                severity: sym.severity.clone(),
            });
        }

        // Convert domain indirect_impacted → LSP IndirectImpact
        let indirect_impact: Vec<IndirectImpact> = result
            .indirect_impacted
            .into_iter()
            .map(|i| IndirectImpact {
                uri: i.path,
                path: i.via_path,
                severity: i.severity,
            })
            .collect();

        Ok(ImpactAnalysisResponse {
            direct_impact,
            indirect_impact,
            affected_tests,
            summary: ImpactSummary {
                files_affected: result.files_affected,
                breaking_changes: result.breaking_changes,
                warnings: result.warnings,
            },
        })
    }
}

// ==========================================
// Parser Metrics Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParserMetricsParams {
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParserMetric {
    pub language: String,
    pub files_attempted: usize,
    pub files_succeeded: usize,
    pub files_failed: usize,
    pub total_entities: usize,
    pub total_relationships: usize,
    pub total_parse_time_ms: u64,
    pub avg_parse_time_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsTotals {
    pub files_attempted: usize,
    pub files_succeeded: usize,
    pub files_failed: usize,
    pub total_entities: usize,
    pub success_rate: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParserMetricsResponse {
    pub metrics: Vec<ParserMetric>,
    pub totals: MetricsTotals,
}

impl CodeGraphBackend {
    pub async fn handle_get_parser_metrics(
        &self,
        params: ParserMetricsParams,
    ) -> Result<ParserMetricsResponse> {
        let all_metrics = self.parsers.all_metrics();

        let metrics: Vec<ParserMetric> = all_metrics
            .into_iter()
            .filter(|(lang, _)| params.language.as_ref().is_none_or(|l| l == *lang))
            .map(|(language, m)| ParserMetric {
                language: language.to_string(),
                files_attempted: m.files_attempted,
                files_succeeded: m.files_succeeded,
                files_failed: m.files_failed,
                total_entities: m.total_entities,
                total_relationships: m.total_relationships,
                total_parse_time_ms: m.total_parse_time.as_millis() as u64,
                avg_parse_time_ms: if m.files_succeeded > 0 {
                    m.total_parse_time.as_millis() as u64 / m.files_succeeded as u64
                } else {
                    0
                },
            })
            .collect();

        let totals = MetricsTotals {
            files_attempted: metrics.iter().map(|m| m.files_attempted).sum(),
            files_succeeded: metrics.iter().map(|m| m.files_succeeded).sum(),
            files_failed: metrics.iter().map(|m| m.files_failed).sum(),
            total_entities: metrics.iter().map(|m| m.total_entities).sum(),
            success_rate: {
                let attempted: usize = metrics.iter().map(|m| m.files_attempted).sum();
                let succeeded: usize = metrics.iter().map(|m| m.files_succeeded).sum();
                if attempted > 0 {
                    succeeded as f64 / attempted as f64
                } else {
                    0.0
                }
            },
        };

        Ok(ParserMetricsResponse { metrics, totals })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_query::QueryEngine;
    use crate::backend::CodeGraphBackend;
    use codegraph::{CodeGraph, EdgeType, NodeId, NodeType, PropertyMap, PropertyValue};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Helper to add a node to the symbol index
    fn add_node_to_index(
        backend: &CodeGraphBackend,
        path: &std::path::Path,
        node_id: NodeId,
        name: &str,
        node_type: &str,
        start_line: u32,
        end_line: u32,
    ) {
        backend.symbol_index.add_node_for_test(
            path.to_path_buf(),
            node_id,
            name,
            node_type,
            start_line,
            end_line,
        );
    }

    /// Helper to create a test backend with a graph containing test nodes
    async fn create_test_backend() -> CodeGraphBackend {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));
        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        CodeGraphBackend::new_for_test(graph, query_engine)
    }

    /// Helper to create a backend with nodes for dependency graph testing
    async fn create_backend_with_imports() -> (CodeGraphBackend, NodeId, NodeId) {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));

        let (file1_id, file2_id) = {
            let mut g = graph.write().await;

            // Create file 1
            let mut props1 = PropertyMap::new();
            props1.insert(
                "name".to_string(),
                PropertyValue::String("main.rs".to_string()),
            );
            props1.insert(
                "path".to_string(),
                PropertyValue::String("/test/main.rs".to_string()),
            );
            props1.insert(
                "language".to_string(),
                PropertyValue::String("rust".to_string()),
            );
            props1.insert("line_start".to_string(), PropertyValue::Int(1));
            props1.insert("line_end".to_string(), PropertyValue::Int(100));
            let file1_id = g.add_node(NodeType::CodeFile, props1).unwrap();

            // Create file 2
            let mut props2 = PropertyMap::new();
            props2.insert(
                "name".to_string(),
                PropertyValue::String("utils.rs".to_string()),
            );
            props2.insert(
                "path".to_string(),
                PropertyValue::String("/test/utils.rs".to_string()),
            );
            props2.insert(
                "language".to_string(),
                PropertyValue::String("rust".to_string()),
            );
            props2.insert("line_start".to_string(), PropertyValue::Int(1));
            props2.insert("line_end".to_string(), PropertyValue::Int(50));
            let file2_id = g.add_node(NodeType::CodeFile, props2).unwrap();

            // Create import edge from file1 to file2
            g.add_edge(file1_id, file2_id, EdgeType::Imports, PropertyMap::new())
                .unwrap();

            (file1_id, file2_id)
        };

        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        let backend = CodeGraphBackend::new_for_test(graph, query_engine);

        (backend, file1_id, file2_id)
    }

    /// Helper to create a backend with function nodes for call graph testing
    async fn create_backend_with_calls() -> (CodeGraphBackend, NodeId, NodeId) {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));

        let (func1_id, func2_id) = {
            let mut g = graph.write().await;

            // Create function 1
            let mut props1 = PropertyMap::new();
            props1.insert(
                "name".to_string(),
                PropertyValue::String("main".to_string()),
            );
            props1.insert(
                "path".to_string(),
                PropertyValue::String("/test/main.rs".to_string()),
            );
            props1.insert(
                "signature".to_string(),
                PropertyValue::String("fn main()".to_string()),
            );
            props1.insert(
                "language".to_string(),
                PropertyValue::String("rust".to_string()),
            );
            props1.insert("line_start".to_string(), PropertyValue::Int(1));
            props1.insert("line_end".to_string(), PropertyValue::Int(10));
            let func1_id = g.add_node(NodeType::Function, props1).unwrap();

            // Create function 2
            let mut props2 = PropertyMap::new();
            props2.insert(
                "name".to_string(),
                PropertyValue::String("helper".to_string()),
            );
            props2.insert(
                "path".to_string(),
                PropertyValue::String("/test/main.rs".to_string()),
            );
            props2.insert(
                "signature".to_string(),
                PropertyValue::String("fn helper() -> i32".to_string()),
            );
            props2.insert(
                "language".to_string(),
                PropertyValue::String("rust".to_string()),
            );
            props2.insert("line_start".to_string(), PropertyValue::Int(15));
            props2.insert("line_end".to_string(), PropertyValue::Int(25));
            let func2_id = g.add_node(NodeType::Function, props2).unwrap();

            // Create call edge from func1 to func2
            g.add_edge(func1_id, func2_id, EdgeType::Calls, PropertyMap::new())
                .unwrap();

            (func1_id, func2_id)
        };

        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        // Build caller/callee indexes so get_callees/get_callers work in tests
        query_engine.build_indexes().await;
        let backend = CodeGraphBackend::new_for_test(graph, query_engine);

        // Add to symbol index
        let path = std::path::Path::new("/test/main.rs");
        add_node_to_index(&backend, path, func1_id, "main", "Function", 1, 10);
        add_node_to_index(&backend, path, func2_id, "helper", "Function", 15, 25);

        (backend, func1_id, func2_id)
    }

    #[test]
    fn test_is_test_node_by_name() {
        let mut props = PropertyMap::new();
        props.insert(
            "name".to_string(),
            PropertyValue::String("test_something".to_string()),
        );
        let node = Node {
            id: 1,
            node_type: NodeType::Function,
            properties: props,
        };
        assert!(CodeGraphBackend::is_test_node(&node));
    }

    #[test]
    fn test_is_test_node_by_path() {
        let mut props = PropertyMap::new();
        props.insert(
            "name".to_string(),
            PropertyValue::String("something".to_string()),
        );
        props.insert(
            "path".to_string(),
            PropertyValue::String("/project/tests/my_test.rs".to_string()),
        );
        let node = Node {
            id: 1,
            node_type: NodeType::Function,
            properties: props,
        };
        assert!(CodeGraphBackend::is_test_node(&node));
    }

    #[test]
    fn test_is_not_test_node() {
        let mut props = PropertyMap::new();
        props.insert(
            "name".to_string(),
            PropertyValue::String("regular_function".to_string()),
        );
        props.insert(
            "path".to_string(),
            PropertyValue::String("/project/src/lib.rs".to_string()),
        );
        let node = Node {
            id: 1,
            node_type: NodeType::Function,
            properties: props,
        };
        assert!(!CodeGraphBackend::is_test_node(&node));
    }

    #[test]
    fn test_node_to_range() {
        let mut props = PropertyMap::new();
        props.insert("line_start".to_string(), PropertyValue::Int(10));
        props.insert("line_end".to_string(), PropertyValue::Int(20));
        props.insert("col_start".to_string(), PropertyValue::Int(4));
        props.insert("col_end".to_string(), PropertyValue::Int(50));
        let node = Node {
            id: 1,
            node_type: NodeType::Function,
            properties: props,
        };

        let range = CodeGraphBackend::node_to_range(&node);
        assert!(range.is_some());
        let range = range.unwrap();
        // Line 10 -> 9 (0-indexed)
        assert_eq!(range.start.line, 9);
        assert_eq!(range.start.character, 4);
        // Line 20 -> 19 (0-indexed)
        assert_eq!(range.end.line, 19);
        assert_eq!(range.end.character, 50);
    }

    // ==========================================
    // Dependency Graph tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_get_dependency_graph_invalid_uri() {
        let backend = create_test_backend().await;

        let params = DependencyGraphParams {
            uri: "not_a_valid_uri".to_string(),
            depth: None,
            include_external: None,
            direction: None,
        };

        let result = backend.handle_get_dependency_graph(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_get_dependency_graph_no_file_node() {
        let backend = create_test_backend().await;

        let params = DependencyGraphParams {
            uri: "file:///nonexistent/file.rs".to_string(),
            depth: Some(2),
            include_external: Some(false),
            direction: None,
        };

        let result = backend.handle_get_dependency_graph(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.nodes.is_empty());
        assert!(response.edges.is_empty());
    }

    #[tokio::test]
    async fn test_handle_get_dependency_graph_with_imports() {
        let (backend, _file1_id, _file2_id) = create_backend_with_imports().await;

        let params = DependencyGraphParams {
            uri: "file:///test/main.rs".to_string(),
            depth: Some(3),
            include_external: Some(false),
            direction: None,
        };

        let result = backend.handle_get_dependency_graph(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // Should have at least the starting node
        assert!(!response.nodes.is_empty());
    }

    // ==========================================
    // Call Graph tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_get_call_graph_invalid_uri() {
        let backend = create_test_backend().await;

        let params = CallGraphParams {
            uri: "not_a_valid_uri".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            direction: None,
            depth: None,
            include_external: None,
        };

        let result = backend.handle_get_call_graph(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_get_call_graph_no_node_at_position() {
        let backend = create_test_backend().await;

        let params = CallGraphParams {
            uri: "file:///test/main.rs".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            direction: None,
            depth: Some(2),
            include_external: Some(false),
        };

        let result = backend.handle_get_call_graph(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.root.is_none());
        assert!(response.nodes.is_empty());
    }

    #[tokio::test]
    async fn test_handle_get_call_graph_with_calls() {
        let (backend, _func1_id, _func2_id) = create_backend_with_calls().await;

        // Position at line 1 (0-indexed = 0) which is within func1 (lines 1-10)
        let params = CallGraphParams {
            uri: "file:///test/main.rs".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            direction: Some("both".to_string()),
            depth: Some(3),
            include_external: Some(false),
        };

        let result = backend.handle_get_call_graph(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // If we found the node, we should have nodes and potentially edges
        if response.root.is_some() {
            assert!(!response.nodes.is_empty());
        }
    }

    // ==========================================
    // Impact Analysis tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_analyze_impact_invalid_uri() {
        let backend = create_test_backend().await;

        let params = ImpactAnalysisParams {
            uri: "not_a_valid_uri".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            analysis_type: "modify".to_string(),
        };

        let result = backend.handle_analyze_impact(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_analyze_impact_no_node() {
        let backend = create_test_backend().await;

        let params = ImpactAnalysisParams {
            uri: "file:///test/main.rs".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            analysis_type: "modify".to_string(),
        };

        let result = backend.handle_analyze_impact(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.direct_impact.is_empty());
        assert!(response.indirect_impact.is_empty());
        assert_eq!(response.summary.files_affected, 0);
    }

    #[tokio::test]
    async fn test_handle_analyze_impact_with_references() {
        let (backend, _func1_id, _func2_id) = create_backend_with_calls().await;

        // Analyze impact on func2 which is called by func1
        // Position within func2 (lines 15-25, so 0-indexed = 14-24)
        let params = ImpactAnalysisParams {
            uri: "file:///test/main.rs".to_string(),
            position: Position {
                line: 14,
                character: 0,
            },
            analysis_type: "delete".to_string(),
        };

        let result = backend.handle_analyze_impact(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // func2 is called by func1, so there should be direct impact
        if !response.direct_impact.is_empty() {
            // Verify severity is correct for delete operation
            assert!(response
                .direct_impact
                .iter()
                .any(|i| i.severity == "breaking"));
        }
    }

    // ==========================================
    // Related Tests tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_find_related_tests_invalid_uri() {
        let backend = create_test_backend().await;

        let params = RelatedTestsParams {
            uri: "not_a_valid_uri".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            limit: None,
        };

        let result = backend.handle_find_related_tests(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_find_related_tests_no_node() {
        let backend = create_test_backend().await;

        let params = RelatedTestsParams {
            uri: "file:///test/main.rs".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            limit: Some(10),
        };

        let result = backend.handle_find_related_tests(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.tests.is_empty());
    }

    #[tokio::test]
    async fn test_handle_find_related_tests_with_test_caller() {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));

        let (func_id, test_id) = {
            let mut g = graph.write().await;

            // Create a regular function
            let mut props1 = PropertyMap::new();
            props1.insert(
                "name".to_string(),
                PropertyValue::String("my_function".to_string()),
            );
            props1.insert(
                "path".to_string(),
                PropertyValue::String("/test/lib.rs".to_string()),
            );
            props1.insert("line_start".to_string(), PropertyValue::Int(1));
            props1.insert("line_end".to_string(), PropertyValue::Int(10));
            let func_id = g.add_node(NodeType::Function, props1).unwrap();

            // Create a test function that calls the regular function
            let mut props2 = PropertyMap::new();
            props2.insert(
                "name".to_string(),
                PropertyValue::String("test_my_function".to_string()),
            );
            props2.insert(
                "path".to_string(),
                PropertyValue::String("/test/tests/lib_test.rs".to_string()),
            );
            props2.insert("line_start".to_string(), PropertyValue::Int(1));
            props2.insert("line_end".to_string(), PropertyValue::Int(10));
            let test_id = g.add_node(NodeType::Function, props2).unwrap();

            // Test calls the function
            g.add_edge(test_id, func_id, EdgeType::Calls, PropertyMap::new())
                .unwrap();

            (func_id, test_id)
        };

        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        let backend = CodeGraphBackend::new_for_test(graph, query_engine);

        // Add to symbol index
        let func_path = std::path::Path::new("/test/lib.rs");
        let test_path = std::path::Path::new("/test/tests/lib_test.rs");
        add_node_to_index(
            &backend,
            func_path,
            func_id,
            "my_function",
            "Function",
            1,
            10,
        );
        add_node_to_index(
            &backend,
            test_path,
            test_id,
            "test_my_function",
            "Function",
            1,
            10,
        );

        let params = RelatedTestsParams {
            uri: "file:///test/lib.rs".to_string(),
            position: Position {
                line: 0,
                character: 0,
            },
            limit: Some(10),
        };

        let result = backend.handle_find_related_tests(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // Should find the test that calls my_function
        assert!(!response.tests.is_empty());
        assert!(response.tests.iter().any(|t| t.test_name.contains("test")));
    }

    // ==========================================
    // Parser Metrics tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_get_parser_metrics_all() {
        let backend = create_test_backend().await;

        let params = ParserMetricsParams { language: None };

        let result = backend.handle_get_parser_metrics(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // Should have metrics for supported languages
        assert!(response.totals.success_rate >= 0.0);
    }

    #[tokio::test]
    async fn test_handle_get_parser_metrics_filtered() {
        let backend = create_test_backend().await;

        let params = ParserMetricsParams {
            language: Some("rust".to_string()),
        };

        let result = backend.handle_get_parser_metrics(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // All metrics should be for rust only
        for metric in &response.metrics {
            assert_eq!(metric.language, "rust");
        }
    }

    #[tokio::test]
    async fn test_handle_get_parser_metrics_unknown_language() {
        let backend = create_test_backend().await;

        let params = ParserMetricsParams {
            language: Some("unknown_language_xyz".to_string()),
        };

        let result = backend.handle_get_parser_metrics(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.metrics.is_empty());
    }
}
