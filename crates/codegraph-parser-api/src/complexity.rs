// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Code complexity metrics for functions and modules.
//!
//! This module provides structures and utilities for tracking code complexity metrics
//! such as cyclomatic complexity, nesting depth, and decision point counts.

use serde::{Deserialize, Serialize};

/// Complexity metrics for a function or method.
///
/// These metrics help identify code that may be difficult to understand,
/// test, or maintain.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComplexityMetrics {
    /// McCabe's Cyclomatic Complexity (CC)
    ///
    /// CC = 1 + number of decision points
    /// - 1-5: Simple, low risk
    /// - 6-10: Moderate complexity
    /// - 11-20: Complex, moderate risk
    /// - 21-50: Very complex, high risk
    /// - 51+: Untestable, very high risk
    pub cyclomatic_complexity: u32,

    /// Number of branch statements (if, else if, else, switch/match cases)
    pub branches: u32,

    /// Number of loop constructs (for, while, loop, do-while)
    pub loops: u32,

    /// Number of logical operators (&& / || / and / or)
    pub logical_operators: u32,

    /// Maximum nesting depth of control structures
    pub max_nesting_depth: u32,

    /// Number of exception handlers (catch, except, rescue)
    pub exception_handlers: u32,

    /// Number of early returns (return statements not at the end)
    pub early_returns: u32,
}

impl ComplexityMetrics {
    /// Create a new ComplexityMetrics with default values (base complexity of 1)
    pub fn new() -> Self {
        Self {
            cyclomatic_complexity: 1,
            ..Default::default()
        }
    }

    /// Calculate the cyclomatic complexity from the component counts
    ///
    /// CC = 1 + branches + loops + logical_operators + exception_handlers
    pub fn calculate_cyclomatic(&mut self) {
        self.cyclomatic_complexity =
            1 + self.branches + self.loops + self.logical_operators + self.exception_handlers;
    }

    /// Get a letter grade based on cyclomatic complexity
    ///
    /// - A: 1-5 (Simple, low risk)
    /// - B: 6-10 (Moderate complexity)
    /// - C: 11-20 (Complex, moderate risk)
    /// - D: 21-50 (Very complex, high risk)
    /// - F: 51+ (Untestable, very high risk)
    pub fn grade(&self) -> char {
        match self.cyclomatic_complexity {
            1..=5 => 'A',
            6..=10 => 'B',
            11..=20 => 'C',
            21..=50 => 'D',
            _ => 'F',
        }
    }

    /// Check if complexity exceeds a threshold
    pub fn exceeds_threshold(&self, threshold: u32) -> bool {
        self.cyclomatic_complexity > threshold
    }

    /// Check if the function has high nesting (> 4 levels)
    pub fn has_high_nesting(&self) -> bool {
        self.max_nesting_depth > 4
    }

    /// Merge metrics from a nested scope (used when traversing nested functions)
    pub fn merge_nested(&mut self, nested: &ComplexityMetrics) {
        self.branches += nested.branches;
        self.loops += nested.loops;
        self.logical_operators += nested.logical_operators;
        self.exception_handlers += nested.exception_handlers;
        self.early_returns += nested.early_returns;
        // max_nesting_depth should be tracked separately during traversal
    }

    // Builder methods

    pub fn with_branches(mut self, count: u32) -> Self {
        self.branches = count;
        self
    }

    pub fn with_loops(mut self, count: u32) -> Self {
        self.loops = count;
        self
    }

    pub fn with_logical_operators(mut self, count: u32) -> Self {
        self.logical_operators = count;
        self
    }

    pub fn with_nesting_depth(mut self, depth: u32) -> Self {
        self.max_nesting_depth = depth;
        self
    }

    pub fn with_exception_handlers(mut self, count: u32) -> Self {
        self.exception_handlers = count;
        self
    }

    pub fn with_early_returns(mut self, count: u32) -> Self {
        self.early_returns = count;
        self
    }

    /// Finalize and calculate the cyclomatic complexity
    pub fn finalize(mut self) -> Self {
        self.calculate_cyclomatic();
        self
    }
}

/// Builder for incrementally tracking complexity during AST traversal
#[derive(Debug, Default)]
pub struct ComplexityBuilder {
    metrics: ComplexityMetrics,
    current_nesting: u32,
}

impl ComplexityBuilder {
    pub fn new() -> Self {
        Self {
            metrics: ComplexityMetrics::new(),
            current_nesting: 0,
        }
    }

    /// Record a branch (if, else if, case, etc.)
    pub fn add_branch(&mut self) {
        self.metrics.branches += 1;
    }

    /// Record a loop (for, while, loop, etc.)
    pub fn add_loop(&mut self) {
        self.metrics.loops += 1;
    }

    /// Record a logical operator (&& or ||)
    pub fn add_logical_operator(&mut self) {
        self.metrics.logical_operators += 1;
    }

    /// Record an exception handler (catch, except, etc.)
    pub fn add_exception_handler(&mut self) {
        self.metrics.exception_handlers += 1;
    }

    /// Record an early return
    pub fn add_early_return(&mut self) {
        self.metrics.early_returns += 1;
    }

    /// Enter a nested scope (increases nesting depth)
    pub fn enter_scope(&mut self) {
        self.current_nesting += 1;
        if self.current_nesting > self.metrics.max_nesting_depth {
            self.metrics.max_nesting_depth = self.current_nesting;
        }
    }

    /// Exit a nested scope (decreases nesting depth)
    pub fn exit_scope(&mut self) {
        self.current_nesting = self.current_nesting.saturating_sub(1);
    }

    /// Get the current nesting depth
    pub fn current_depth(&self) -> u32 {
        self.current_nesting
    }

    /// Build the final ComplexityMetrics
    pub fn build(mut self) -> ComplexityMetrics {
        self.metrics.calculate_cyclomatic();
        self.metrics
    }

    /// Get a reference to the current metrics (without finalizing)
    pub fn current(&self) -> &ComplexityMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_has_base_complexity() {
        let metrics = ComplexityMetrics::new();
        assert_eq!(metrics.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_grade_simple() {
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 3,
            ..Default::default()
        };
        assert_eq!(metrics.grade(), 'A');
    }

    #[test]
    fn test_grade_moderate() {
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 8,
            ..Default::default()
        };
        assert_eq!(metrics.grade(), 'B');
    }

    #[test]
    fn test_grade_complex() {
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 15,
            ..Default::default()
        };
        assert_eq!(metrics.grade(), 'C');
    }

    #[test]
    fn test_grade_very_complex() {
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 35,
            ..Default::default()
        };
        assert_eq!(metrics.grade(), 'D');
    }

    #[test]
    fn test_grade_untestable() {
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 60,
            ..Default::default()
        };
        assert_eq!(metrics.grade(), 'F');
    }

    #[test]
    fn test_calculate_cyclomatic() {
        let mut metrics = ComplexityMetrics::new()
            .with_branches(3)
            .with_loops(2)
            .with_logical_operators(1);
        metrics.calculate_cyclomatic();
        // CC = 1 + 3 + 2 + 1 = 7
        assert_eq!(metrics.cyclomatic_complexity, 7);
    }

    #[test]
    fn test_builder_basic() {
        let mut builder = ComplexityBuilder::new();
        builder.add_branch();
        builder.add_branch();
        builder.add_loop();

        let metrics = builder.build();
        // CC = 1 + 2 branches + 1 loop = 4
        assert_eq!(metrics.cyclomatic_complexity, 4);
    }

    #[test]
    fn test_builder_nesting() {
        let mut builder = ComplexityBuilder::new();
        builder.enter_scope();
        builder.add_branch();
        builder.enter_scope();
        builder.add_loop();
        builder.enter_scope();
        builder.exit_scope();
        builder.exit_scope();
        builder.exit_scope();

        let metrics = builder.build();
        assert_eq!(metrics.max_nesting_depth, 3);
    }

    #[test]
    fn test_exceeds_threshold() {
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 15,
            ..Default::default()
        };
        assert!(metrics.exceeds_threshold(10));
        assert!(!metrics.exceeds_threshold(20));
    }

    #[test]
    fn test_has_high_nesting() {
        let low_nesting = ComplexityMetrics {
            max_nesting_depth: 3,
            ..Default::default()
        };
        assert!(!low_nesting.has_high_nesting());

        let high_nesting = ComplexityMetrics {
            max_nesting_depth: 5,
            ..Default::default()
        };
        assert!(high_nesting.has_high_nesting());
    }

    #[test]
    fn test_has_high_nesting_boundary_at_four() {
        // > 4 is the threshold, so exactly 4 is NOT high nesting
        let at_boundary = ComplexityMetrics {
            max_nesting_depth: 4,
            ..Default::default()
        };
        assert!(!at_boundary.has_high_nesting());
    }

    #[test]
    fn test_grade_boundaries() {
        // Assert every inclusive edge of the grade ranges.
        let grade_at = |cc: u32| {
            ComplexityMetrics {
                cyclomatic_complexity: cc,
                ..Default::default()
            }
            .grade()
        };
        assert_eq!(grade_at(1), 'A');
        assert_eq!(grade_at(5), 'A');
        assert_eq!(grade_at(6), 'B');
        assert_eq!(grade_at(10), 'B');
        assert_eq!(grade_at(11), 'C');
        assert_eq!(grade_at(20), 'C');
        assert_eq!(grade_at(21), 'D');
        assert_eq!(grade_at(50), 'D');
        assert_eq!(grade_at(51), 'F');
        // A zero cyclomatic complexity falls through to the catch-all 'F' arm.
        assert_eq!(grade_at(0), 'F');
    }

    #[test]
    fn test_calculate_cyclomatic_includes_exception_handlers() {
        // exception_handlers is a decision-point contributor that the existing
        // calculate test omits.
        let mut metrics = ComplexityMetrics::new()
            .with_branches(2)
            .with_loops(1)
            .with_logical_operators(1)
            .with_exception_handlers(3);
        metrics.calculate_cyclomatic();
        // CC = 1 + 2 + 1 + 1 + 3 = 8; early_returns must NOT contribute.
        assert_eq!(metrics.cyclomatic_complexity, 8);

        let with_returns = ComplexityMetrics::new().with_early_returns(5).finalize();
        assert_eq!(with_returns.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_finalize_calculates_and_returns_self() {
        let metrics = ComplexityMetrics::new()
            .with_branches(4)
            .with_nesting_depth(2)
            .finalize();
        // CC = 1 + 4 branches = 5, and the builder-set nesting is preserved.
        assert_eq!(metrics.cyclomatic_complexity, 5);
        assert_eq!(metrics.max_nesting_depth, 2);
    }

    #[test]
    fn test_merge_nested_sums_counts_but_not_cyclomatic_or_nesting() {
        let mut base = ComplexityMetrics::new()
            .with_branches(1)
            .with_loops(1)
            .with_logical_operators(1)
            .with_exception_handlers(1)
            .with_early_returns(1)
            .with_nesting_depth(2);
        base.calculate_cyclomatic();
        let base_cc = base.cyclomatic_complexity;

        let nested = ComplexityMetrics::new()
            .with_branches(2)
            .with_loops(3)
            .with_logical_operators(4)
            .with_exception_handlers(5)
            .with_early_returns(6)
            .with_nesting_depth(9);

        base.merge_nested(&nested);

        assert_eq!(base.branches, 3);
        assert_eq!(base.loops, 4);
        assert_eq!(base.logical_operators, 5);
        assert_eq!(base.exception_handlers, 6);
        assert_eq!(base.early_returns, 7);
        // merge_nested does not touch nesting depth or recompute cyclomatic.
        assert_eq!(base.max_nesting_depth, 2);
        assert_eq!(base.cyclomatic_complexity, base_cc);
    }

    #[test]
    fn test_builder_increment_all_counters() {
        let mut builder = ComplexityBuilder::new();
        builder.add_logical_operator();
        builder.add_exception_handler();
        builder.add_early_return();

        // current() exposes metrics without finalizing, so cyclomatic is still base 1.
        let snapshot = builder.current();
        assert_eq!(snapshot.logical_operators, 1);
        assert_eq!(snapshot.exception_handlers, 1);
        assert_eq!(snapshot.early_returns, 1);
        assert_eq!(snapshot.cyclomatic_complexity, 1);

        let metrics = builder.build();
        // CC = 1 + 1 logical_operator + 1 exception_handler = 3; early_return excluded.
        assert_eq!(metrics.cyclomatic_complexity, 3);
    }

    #[test]
    fn test_builder_enter_scope_retains_peak_on_reentry() {
        // Re-entering a scope after exiting drives enter_scope's
        // `current_nesting > max_nesting_depth` FALSE arm: every prior test
        // only ever entered strictly deeper, always taking the true arm that
        // raises the peak. Here the second entry to depth 2 does not exceed
        // the already-recorded peak of 2, so max_nesting_depth stays 2.
        let mut builder = ComplexityBuilder::new();
        builder.enter_scope(); // depth 1 > 0 -> peak becomes 1
        builder.enter_scope(); // depth 2 > 1 -> peak becomes 2
        builder.exit_scope(); // depth 1
        builder.enter_scope(); // depth 2, 2 > 2 is false -> peak stays 2
        assert_eq!(builder.current_depth(), 2);
        assert_eq!(builder.build().max_nesting_depth, 2);
    }

    #[test]
    fn test_builder_current_depth_and_exit_saturates() {
        let mut builder = ComplexityBuilder::new();
        assert_eq!(builder.current_depth(), 0);
        builder.enter_scope();
        builder.enter_scope();
        assert_eq!(builder.current_depth(), 2);
        builder.exit_scope();
        assert_eq!(builder.current_depth(), 1);
        // Extra exits saturate at zero rather than underflowing.
        builder.exit_scope();
        builder.exit_scope();
        assert_eq!(builder.current_depth(), 0);
        // The peak depth of 2 is retained in the built metrics.
        assert_eq!(builder.build().max_nesting_depth, 2);
    }
}
