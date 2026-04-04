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
}
