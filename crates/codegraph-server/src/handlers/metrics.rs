// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Code Metrics Handler - Complexity and quality analysis for AI assistants.

use crate::backend::CodeGraphBackend;
use crate::handlers::ai_context::LocationInfo;
use codegraph::{CodeGraph, NodeId};
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::Url;

// Re-export domain complexity types and functions so existing call sites are unaffected.
pub(crate) use crate::domain::complexity::{analyze_file_complexity, ComplexityDetails};

// ==========================================
// Complexity Analysis Types
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComplexityParams {
    pub uri: String,
    /// Specific line to analyze (optional, analyzes whole file if not provided)
    pub line: Option<u32>,
    /// Complexity threshold for recommendations (default: 10)
    pub threshold: Option<u32>,
    /// Include detailed metrics breakdown
    pub include_metrics: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComplexityResponse {
    pub functions: Vec<FunctionComplexity>,
    pub file_summary: FileSummary,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionComplexity {
    pub name: String,
    pub complexity: u32,
    pub grade: char,
    pub location: LocationInfo,
    pub details: ComplexityDetails,
}

// LocationInfo is imported from ai_context module
// ComplexityDetails, FunctionComplexityEntry, ComplexityAnalysisResult re-exported from domain::complexity above.

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub total_functions: u32,
    pub average_complexity: f64,
    pub max_complexity: u32,
    pub functions_above_threshold: u32,
    pub overall_grade: char,
}

// ==========================================
// LSP Handlers
// ==========================================

impl CodeGraphBackend {
    /// LSP handler — delegates to shared `analyze_file_complexity()`.
    pub async fn handle_analyze_complexity(
        &self,
        params: ComplexityParams,
    ) -> Result<ComplexityResponse> {
        let threshold = params.threshold.unwrap_or(10);
        let graph = self.graph.read().await;
        let file_nodes = self.get_file_node_ids(&graph, &params.uri)?;
        let result = analyze_file_complexity(&graph, &file_nodes, params.line, threshold);

        let mut functions = Vec::new();
        for entry in &result.functions {
            let location = self
                .node_to_location(&graph, entry.node_id)
                .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;
            functions.push(FunctionComplexity {
                name: entry.name.clone(),
                complexity: entry.complexity,
                grade: entry.grade,
                location: LocationInfo {
                    uri: location.uri.to_string(),
                    range: location.range,
                },
                details: entry.details.clone(),
            });
        }

        Ok(ComplexityResponse {
            functions,
            file_summary: FileSummary {
                total_functions: result.functions.len() as u32,
                average_complexity: result.average_complexity,
                max_complexity: result.max_complexity,
                functions_above_threshold: result.functions_above_threshold,
                overall_grade: result.overall_grade,
            },
            recommendations: result.recommendations,
        })
    }

    /// Resolve file URI to node IDs via symbol index.
    fn get_file_node_ids(&self, _graph: &CodeGraph, uri_str: &str) -> Result<Vec<NodeId>> {
        let uri = Url::parse(uri_str)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;
        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;
        Ok(self.symbol_index.get_file_symbols(&path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_query::QueryEngine;
    use crate::domain::complexity::{complexity_grade, file_grade};
    use codegraph::CodeGraph;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Create a test backend with an in-memory graph and empty symbol index.
    async fn create_test_backend() -> CodeGraphBackend {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create in-memory graph"),
        ));
        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        CodeGraphBackend::new_for_test(graph, query_engine)
    }

    #[tokio::test]
    async fn test_get_file_node_ids_invalid_uri() {
        let backend = create_test_backend().await;
        let graph = backend.graph.read().await;
        // A string that is not a parseable URL fails at Url::parse.
        let err = backend
            .get_file_node_ids(&graph, "not a valid uri")
            .expect_err("expected invalid params for unparseable URI");
        assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidParams);
    }

    #[tokio::test]
    async fn test_get_file_node_ids_non_file_scheme() {
        let backend = create_test_backend().await;
        let graph = backend.graph.read().await;
        // A well-formed but non-file URL parses, then fails at to_file_path.
        let err = backend
            .get_file_node_ids(&graph, "http://example.com/foo.rs")
            .expect_err("expected invalid params for non-file scheme");
        assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidParams);
    }

    #[tokio::test]
    async fn test_get_file_node_ids_valid_uri_empty_index() {
        let backend = create_test_backend().await;
        let graph = backend.graph.read().await;
        // Valid file URI, but the symbol index has no entries for it.
        let ids = backend
            .get_file_node_ids(&graph, "file:///tmp/does_not_exist.rs")
            .expect("valid file URI should resolve");
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn test_handle_analyze_complexity_empty_file() {
        let backend = create_test_backend().await;
        // No symbols indexed -> no functions -> zeroed summary with default grade.
        let resp = backend
            .handle_analyze_complexity(ComplexityParams {
                uri: "file:///tmp/does_not_exist.rs".to_string(),
                line: None,
                threshold: None,
                include_metrics: None,
            })
            .await
            .expect("empty-file analysis should succeed");
        assert!(resp.functions.is_empty());
        assert_eq!(resp.file_summary.total_functions, 0);
        assert_eq!(resp.file_summary.max_complexity, 0);
        assert_eq!(resp.file_summary.functions_above_threshold, 0);
    }

    #[tokio::test]
    async fn test_handle_analyze_complexity_invalid_uri_propagates_error() {
        let backend = create_test_backend().await;
        // The handler surfaces get_file_node_ids' invalid-params error.
        let err = backend
            .handle_analyze_complexity(ComplexityParams {
                uri: "http://example.com/foo.rs".to_string(),
                line: None,
                threshold: Some(5),
                include_metrics: Some(true),
            })
            .await
            .expect_err("non-file URI should fail");
        assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidParams);
    }

    #[test]
    fn test_complexity_grade() {
        assert_eq!(complexity_grade(1), 'A');
        assert_eq!(complexity_grade(5), 'A');
        assert_eq!(complexity_grade(6), 'B');
        assert_eq!(complexity_grade(10), 'B');
        assert_eq!(complexity_grade(11), 'C');
        assert_eq!(complexity_grade(20), 'C');
        assert_eq!(complexity_grade(21), 'D');
        assert_eq!(complexity_grade(50), 'D');
        assert_eq!(complexity_grade(51), 'F');
    }

    #[test]
    fn test_file_grade() {
        assert_eq!(file_grade(3.0), 'A');
        assert_eq!(file_grade(8.0), 'B');
        assert_eq!(file_grade(12.0), 'C');
        assert_eq!(file_grade(20.0), 'D');
        assert_eq!(file_grade(30.0), 'F');
    }
}
