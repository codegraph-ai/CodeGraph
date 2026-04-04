// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Shared complexity analysis — single source of truth for both LSP and MCP handlers.
//!
//! This module contains the domain logic for cyclomatic complexity analysis.
//! It has no dependency on tower-lsp, MCP protocol types, or serde_json::Value.

use super::node_props;
use codegraph::{CodeGraph, NodeId, NodeType};
use serde::{Deserialize, Serialize};

// ==========================================
// Shared Types
// ==========================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ComplexityDetails {
    /// Number of if/else/switch branches
    pub complexity_branches: u32,
    /// Number of for/while/loop constructs
    pub complexity_loops: u32,
    /// Number of && / || logical operators
    pub complexity_logical_ops: u32,
    /// Maximum nesting depth
    pub complexity_nesting: u32,
    /// Number of try/catch/except handlers
    pub complexity_exceptions: u32,
    /// Number of early return/break/continue statements
    pub complexity_early_returns: u32,
    /// Lines of code in the function
    pub lines_of_code: u32,
}

pub(crate) struct FunctionComplexityEntry {
    pub node_id: NodeId,
    pub name: String,
    pub complexity: u32,
    pub grade: char,
    pub line_start: u32,
    pub line_end: u32,
    pub details: ComplexityDetails,
}

pub(crate) struct ComplexityAnalysisResult {
    pub functions: Vec<FunctionComplexityEntry>,
    pub threshold: u32,
    pub average_complexity: f64,
    pub max_complexity: u32,
    pub functions_above_threshold: u32,
    pub overall_grade: char,
    pub recommendations: Vec<String>,
}

// ==========================================
// Complexity Calculation
// ==========================================

/// Calculate cyclomatic complexity grade from score.
/// Uses same thresholds as upstream codegraph-parser-api ComplexityMetrics::grade().
pub(crate) fn complexity_grade(complexity: u32) -> char {
    match complexity {
        1..=5 => 'A',   // Simple, low risk
        6..=10 => 'B',  // Moderate complexity
        11..=20 => 'C', // Complex, moderate risk
        21..=50 => 'D', // Very complex, high risk
        _ => 'F',       // Untestable, very high risk
    }
}

/// Calculate overall file grade from average complexity.
pub(crate) fn file_grade(avg_complexity: f64) -> char {
    match avg_complexity as u32 {
        0..=5 => 'A',
        6..=10 => 'B',
        11..=15 => 'C',
        16..=25 => 'D',
        _ => 'F',
    }
}

/// Extract complexity metrics from a graph node's properties.
pub(crate) fn get_complexity_from_node(node: &codegraph::Node) -> (u32, ComplexityDetails, char) {
    let start = node_props::line_start(node);
    let end = node_props::line_end(node);
    let lines_of_code = end.saturating_sub(start) + 1;

    if let Some(parsed_complexity) = node.properties.get_int("complexity") {
        let complexity = parsed_complexity as u32;
        let grade = node
            .properties
            .get_string("complexity_grade")
            .and_then(|s| s.chars().next())
            .unwrap_or_else(|| complexity_grade(complexity));
        let details = ComplexityDetails {
            complexity_branches: node.properties.get_int("complexity_branches").unwrap_or(0) as u32,
            complexity_loops: node.properties.get_int("complexity_loops").unwrap_or(0) as u32,
            complexity_logical_ops: node
                .properties
                .get_int("complexity_logical_ops")
                .unwrap_or(0) as u32,
            complexity_nesting: node.properties.get_int("complexity_nesting").unwrap_or(0) as u32,
            complexity_exceptions: node
                .properties
                .get_int("complexity_exceptions")
                .unwrap_or(0) as u32,
            complexity_early_returns: node
                .properties
                .get_int("complexity_early_returns")
                .unwrap_or(0) as u32,
            lines_of_code,
        };
        (complexity, details, grade)
    } else {
        let details = ComplexityDetails {
            complexity_branches: 0,
            complexity_loops: 0,
            complexity_logical_ops: 0,
            complexity_nesting: 0,
            complexity_exceptions: 0,
            complexity_early_returns: 0,
            lines_of_code,
        };
        (1, details, 'A')
    }
}

/// Core complexity analysis — single source of truth for both LSP and MCP handlers.
/// Takes a graph reference and pre-resolved node IDs (from symbol index or graph query).
pub(crate) fn analyze_file_complexity(
    graph: &CodeGraph,
    node_ids: &[NodeId],
    line: Option<u32>,
    threshold: u32,
) -> ComplexityAnalysisResult {
    let mut functions: Vec<FunctionComplexityEntry> = Vec::new();

    for &node_id in node_ids {
        if let Ok(node) = graph.get_node(node_id) {
            if node.node_type != NodeType::Function {
                continue;
            }

            let start = node_props::line_start(node);
            let end = node_props::line_end(node);

            if let Some(target_line) = line {
                if target_line < start || target_line > end {
                    continue;
                }
            }

            let name = node_props::name(node);
            let name = if name.is_empty() {
                "anonymous".to_string()
            } else {
                name.to_string()
            };

            let (complexity, details, grade) = get_complexity_from_node(node);

            functions.push(FunctionComplexityEntry {
                node_id,
                name,
                complexity,
                grade,
                line_start: start,
                line_end: end,
                details,
            });
        }
    }

    functions.sort_by(|a, b| b.complexity.cmp(&a.complexity));

    let total: u32 = functions.iter().map(|f| f.complexity).sum();
    let count = functions.len();
    let average_complexity = if count > 0 {
        total as f64 / count as f64
    } else {
        0.0
    };
    let max_complexity = functions.iter().map(|f| f.complexity).max().unwrap_or(0);
    let functions_above_threshold = functions
        .iter()
        .filter(|f| f.complexity > threshold)
        .count() as u32;

    let mut recommendations = Vec::new();
    for f in functions.iter().filter(|f| f.complexity > threshold) {
        recommendations.push(format!(
            "Consider refactoring '{}' (complexity: {}, grade: {}). Break into smaller functions.",
            f.name, f.complexity, f.grade
        ));
    }
    if average_complexity > 15.0 {
        recommendations.push(
            "File has high average complexity. Consider splitting into multiple modules."
                .to_string(),
        );
    }
    let deep_nesting = functions
        .iter()
        .filter(|f| f.details.complexity_nesting > 4)
        .count();
    if deep_nesting > 0 {
        recommendations.push(format!(
            "{} function(s) have deep nesting (>4 levels). Use early returns or extract methods.",
            deep_nesting
        ));
    }

    ComplexityAnalysisResult {
        functions,
        threshold,
        average_complexity,
        max_complexity,
        functions_above_threshold,
        overall_grade: file_grade(average_complexity),
        recommendations,
    }
}
