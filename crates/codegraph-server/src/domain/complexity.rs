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

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{Node, PropertyMap};

    #[test]
    fn complexity_grade_covers_all_tiers() {
        // Boundary values for each grade band.
        assert_eq!(complexity_grade(1), 'A');
        assert_eq!(complexity_grade(5), 'A');
        assert_eq!(complexity_grade(6), 'B');
        assert_eq!(complexity_grade(10), 'B');
        assert_eq!(complexity_grade(11), 'C');
        assert_eq!(complexity_grade(20), 'C');
        assert_eq!(complexity_grade(21), 'D');
        assert_eq!(complexity_grade(50), 'D');
        assert_eq!(complexity_grade(51), 'F');
        assert_eq!(complexity_grade(1000), 'F');
    }

    #[test]
    fn complexity_grade_zero_falls_through_to_f() {
        // 0 is below the 1..=5 'A' band, so it lands in the catch-all 'F'.
        assert_eq!(complexity_grade(0), 'F');
    }

    #[test]
    fn file_grade_covers_all_tiers() {
        // avg is truncated to u32 before matching, so 5.9 -> 5 -> 'A'.
        assert_eq!(file_grade(0.0), 'A');
        assert_eq!(file_grade(5.9), 'A');
        assert_eq!(file_grade(6.0), 'B');
        assert_eq!(file_grade(10.9), 'B');
        assert_eq!(file_grade(11.0), 'C');
        assert_eq!(file_grade(15.9), 'C');
        assert_eq!(file_grade(16.0), 'D');
        assert_eq!(file_grade(25.9), 'D');
        assert_eq!(file_grade(26.0), 'F');
        assert_eq!(file_grade(100.0), 'F');
    }

    fn function_node(props: PropertyMap) -> Node {
        Node::new(0, NodeType::Function, props)
    }

    #[test]
    fn get_complexity_from_node_defaults_when_absent() {
        // No complexity property -> complexity 1, grade 'A', zeroed details.
        let mut props = PropertyMap::new();
        props.insert("line_start", 10i64);
        props.insert("line_end", 20i64);
        let node = function_node(props);

        let (complexity, details, grade) = get_complexity_from_node(&node);
        assert_eq!(complexity, 1);
        assert_eq!(grade, 'A');
        // lines_of_code = end - start + 1 = 20 - 10 + 1 = 11.
        assert_eq!(details.lines_of_code, 11);
        assert_eq!(details.complexity_branches, 0);
        assert_eq!(details.complexity_loops, 0);
    }

    #[test]
    fn get_complexity_from_node_reads_stored_metrics() {
        let mut props = PropertyMap::new();
        props.insert("line_start", 1i64);
        props.insert("line_end", 30i64);
        props.insert("complexity", 12i64);
        props.insert("complexity_branches", 4i64);
        props.insert("complexity_loops", 2i64);
        props.insert("complexity_logical_ops", 3i64);
        props.insert("complexity_nesting", 5i64);
        props.insert("complexity_exceptions", 1i64);
        props.insert("complexity_early_returns", 2i64);
        let node = function_node(props);

        let (complexity, details, grade) = get_complexity_from_node(&node);
        assert_eq!(complexity, 12);
        // No stored grade -> derived from complexity_grade(12) -> 'C'.
        assert_eq!(grade, 'C');
        assert_eq!(details.complexity_branches, 4);
        assert_eq!(details.complexity_loops, 2);
        assert_eq!(details.complexity_logical_ops, 3);
        assert_eq!(details.complexity_nesting, 5);
        assert_eq!(details.complexity_exceptions, 1);
        assert_eq!(details.complexity_early_returns, 2);
        assert_eq!(details.lines_of_code, 30);
    }

    #[test]
    fn get_complexity_from_node_prefers_stored_grade() {
        // An explicit complexity_grade overrides the derived grade.
        let mut props = PropertyMap::new();
        props.insert("complexity", 3i64);
        props.insert("complexity_grade", "F");
        let node = function_node(props);

        let (_, _, grade) = get_complexity_from_node(&node);
        assert_eq!(grade, 'F');
    }

    // ---- analyze_file_complexity ----

    use codegraph::{CodeGraph, NodeId, NodeType as NT};

    /// Add a Function node to the graph with the given name/lines/complexity.
    fn add_fn(
        g: &mut CodeGraph,
        name: &str,
        line_start: i64,
        line_end: i64,
        complexity: i64,
        nesting: i64,
    ) -> NodeId {
        let mut props = PropertyMap::new();
        props.insert("name", name);
        props.insert("line_start", line_start);
        props.insert("line_end", line_end);
        props.insert("complexity", complexity);
        props.insert("complexity_nesting", nesting);
        g.add_node(NT::Function, props).unwrap()
    }

    #[test]
    fn analyze_empty_node_ids_yields_zeroed_result() {
        let g = CodeGraph::in_memory().unwrap();
        let result = analyze_file_complexity(&g, &[], None, 10);
        assert!(result.functions.is_empty());
        assert_eq!(result.average_complexity, 0.0);
        assert_eq!(result.max_complexity, 0);
        assert_eq!(result.functions_above_threshold, 0);
        // file_grade(0.0) is 'A'.
        assert_eq!(result.overall_grade, 'A');
        assert!(result.recommendations.is_empty());
    }

    #[test]
    fn analyze_sorts_descending_and_aggregates() {
        let mut g = CodeGraph::in_memory().unwrap();
        let a = add_fn(&mut g, "low", 1, 5, 3, 0);
        let b = add_fn(&mut g, "high", 10, 40, 12, 0);
        let c = add_fn(&mut g, "mid", 50, 60, 6, 0);

        let result = analyze_file_complexity(&g, &[a, b, c], None, 10);
        // Sorted by complexity descending.
        assert_eq!(result.functions[0].name, "high");
        assert_eq!(result.functions[1].name, "mid");
        assert_eq!(result.functions[2].name, "low");
        assert_eq!(result.max_complexity, 12);
        // (3 + 12 + 6) / 3 = 7.
        assert_eq!(result.average_complexity, 7.0);
        // Only "high" (12) exceeds threshold 10.
        assert_eq!(result.functions_above_threshold, 1);
        assert_eq!(result.recommendations.len(), 1);
        assert!(result.recommendations[0].contains("high"));
    }

    #[test]
    fn analyze_skips_non_function_and_missing_nodes() {
        let mut g = CodeGraph::in_memory().unwrap();
        let f = add_fn(&mut g, "real", 1, 5, 4, 0);
        // A non-function node with a complexity prop must be ignored.
        let mut cprops = PropertyMap::new();
        cprops.insert("name", "SomeClass");
        cprops.insert("complexity", 99i64);
        let class_id = g.add_node(NT::Class, cprops).unwrap();
        // NodeId 9999 was never inserted -> get_node fails and is skipped.
        let missing: NodeId = 9999;

        let result = analyze_file_complexity(&g, &[f, class_id, missing], None, 10);
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "real");
        assert_eq!(result.max_complexity, 4);
    }

    #[test]
    fn analyze_line_filter_keeps_only_enclosing_function() {
        let mut g = CodeGraph::in_memory().unwrap();
        let a = add_fn(&mut g, "outer", 1, 10, 5, 0);
        let b = add_fn(&mut g, "other", 20, 30, 8, 0);

        // Line 25 falls inside "other" only.
        let result = analyze_file_complexity(&g, &[a, b], Some(25), 100);
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "other");

        // A line outside every function range yields no functions.
        let none = analyze_file_complexity(&g, &[a, b], Some(15), 100);
        assert!(none.functions.is_empty());
    }

    #[test]
    fn analyze_uses_anonymous_for_empty_name() {
        let mut g = CodeGraph::in_memory().unwrap();
        let mut props = PropertyMap::new();
        // name absent -> node_props::name returns "" -> "anonymous".
        props.insert("line_start", 1i64);
        props.insert("line_end", 3i64);
        props.insert("complexity", 2i64);
        let id = g.add_node(NT::Function, props).unwrap();

        let result = analyze_file_complexity(&g, &[id], None, 10);
        assert_eq!(result.functions[0].name, "anonymous");
    }

    #[test]
    fn analyze_emits_high_average_and_deep_nesting_recommendations() {
        let mut g = CodeGraph::in_memory().unwrap();
        // High complexity + deep nesting on a single function.
        let a = add_fn(&mut g, "monster", 1, 100, 30, 6);

        let result = analyze_file_complexity(&g, &[a], None, 10);
        // avg 30.0 > 15 -> high-average recommendation.
        assert!(result
            .recommendations
            .iter()
            .any(|r| r.contains("high average complexity")));
        // nesting 6 > 4 -> deep-nesting recommendation.
        assert!(result
            .recommendations
            .iter()
            .any(|r| r.contains("deep nesting")));
        // file_grade(30.0) -> 'F'.
        assert_eq!(result.overall_grade, 'F');
    }
}
