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
    use crate::domain::complexity::{complexity_grade, file_grade};

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
