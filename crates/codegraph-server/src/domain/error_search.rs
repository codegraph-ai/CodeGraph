// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Error pattern search — transport-agnostic.
//!
//! Finds functions that throw, catch, or handle errors by scanning
//! `body_prefix` and `signature` node properties for language-specific patterns.

use crate::domain::node_props;
use codegraph::{CodeGraph, NodeType};
use serde::Serialize;

// ============================================================
// Response Types
// ============================================================

#[derive(Debug, Serialize)]
pub(crate) struct ErrorSearchResult {
    pub functions: Vec<ErrorFunction>,
    pub total_matches: usize,
    pub error_type_filter: Option<String>,
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorFunction {
    pub node_id: String,
    pub name: String,
    pub path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: String,
    /// Which patterns matched in this function's body/signature.
    pub error_patterns: Vec<String>,
    /// "throws", "catches", or "both"
    pub error_role: String,
}

// ============================================================
// Pattern Tables
// ============================================================

/// Patterns that indicate a function *produces* errors (throws/raises/panics).
const THROW_PATTERNS: &[&str] = &[
    // Rust
    "Err(",
    "panic!(",
    ".unwrap()",
    ".expect(",
    "anyhow::",
    "thiserror",
    // Python
    "raise ",
    // TypeScript/JS/Java/Kotlin/C#/Go
    "throw ",
    "errors.New(",
    "fmt.Errorf(",
    "reject(",
];

/// Patterns that indicate a function *handles* errors (catch/except/recover).
const CATCH_PATTERNS: &[&str] = &[
    // Rust — `?` propagates but also *handles* in the sense of short-circuiting
    "Result<",
    "?",
    // Python
    "except ",
    "try:",
    // TypeScript/JS
    "catch(",
    ".catch(",
    // Go
    "if err != nil",
    // Java/Kotlin/C#
    "catch (",
    "catch(",
];

/// General patterns used for broad language-agnostic matching.
const GENERAL_PATTERNS: &[&str] = &[
    "error", "Error", "err", "exception", "Exception", "fail", "failure",
];

// ============================================================
// Domain Function
// ============================================================

/// Find functions that throw, catch, or handle errors.
///
/// - `error_type`: optional specific type string to narrow results (e.g. "IoError")
/// - `mode`: "throws" | "catches" | "any" (default)
/// - `limit`: maximum results to return (default 50)
pub(crate) fn search_by_error(
    graph: &CodeGraph,
    error_type: Option<&str>,
    mode: &str,
    limit: usize,
) -> ErrorSearchResult {
    let mode_str = match mode {
        "throws" | "catches" => mode.to_string(),
        _ => "any".to_string(),
    };

    let mut functions: Vec<ErrorFunction> = graph
        .nodes_iter()
        .filter_map(|(&node_id, node)| {
            if node.node_type != NodeType::Function {
                return None;
            }

            let body = node
                .properties
                .get_string("body_prefix")
                .unwrap_or("");
            let signature = node
                .properties
                .get_string("signature")
                .unwrap_or("");
            let haystack = format!("{}\n{}", signature, body);

            // If a specific error type was requested, the haystack must mention it.
            if let Some(et) = error_type {
                if !haystack.contains(et) {
                    return None;
                }
            }

            let throw_hits: Vec<String> = THROW_PATTERNS
                .iter()
                .filter(|&&p| haystack.contains(p))
                .map(|&p| p.to_string())
                .collect();

            let catch_hits: Vec<String> = CATCH_PATTERNS
                .iter()
                .filter(|&&p| haystack.contains(p))
                .map(|&p| p.to_string())
                .collect();

            let has_throws = !throw_hits.is_empty();
            let has_catches = !catch_hits.is_empty();

            // Fall back to general patterns when no specific match found.
            let general_hits: Vec<String> = if !has_throws && !has_catches {
                GENERAL_PATTERNS
                    .iter()
                    .filter(|&&p| haystack.contains(p))
                    .map(|&p| p.to_string())
                    .collect()
            } else {
                vec![]
            };

            let has_any = has_throws || has_catches || !general_hits.is_empty();
            if !has_any {
                return None;
            }

            // Apply mode filter.
            let passes_mode = match mode_str.as_str() {
                "throws" => has_throws || (!has_catches && !general_hits.is_empty()),
                "catches" => has_catches || (!has_throws && !general_hits.is_empty()),
                _ => true,
            };
            if !passes_mode {
                return None;
            }

            let error_role = if has_throws && has_catches {
                "both".to_string()
            } else if has_throws {
                "throws".to_string()
            } else if has_catches {
                "catches".to_string()
            } else {
                // Only general patterns matched — classify by mode or default.
                match mode_str.as_str() {
                    "throws" => "throws".to_string(),
                    "catches" => "catches".to_string(),
                    _ => "any".to_string(),
                }
            };

            let mut error_patterns = throw_hits;
            error_patterns.extend(catch_hits);
            error_patterns.extend(general_hits);
            error_patterns.sort();
            error_patterns.dedup();

            let name = node_props::name(node).to_string();
            let path = node_props::path(node).to_string();
            let line_start = node_props::line_start(node) as usize;
            let line_end = node_props::line_end(node) as usize;
            let sig = signature.to_string();

            Some(ErrorFunction {
                node_id: node_id.to_string(),
                name,
                path,
                line_start,
                line_end,
                signature: sig,
                error_patterns,
                error_role,
            })
        })
        .collect();

    // Sort: "both" first, then "throws", then "catches"/"any"; then alphabetically by path+name.
    functions.sort_by(|a, b| {
        role_rank(&a.error_role)
            .cmp(&role_rank(&b.error_role))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.name.cmp(&b.name))
    });

    let total_matches = functions.len();
    functions.truncate(limit);

    ErrorSearchResult {
        functions,
        total_matches,
        error_type_filter: error_type.map(|s| s.to_string()),
        mode: mode_str,
    }
}

// ============================================================
// Private Helpers
// ============================================================

fn role_rank(role: &str) -> u8 {
    match role {
        "both" => 0,
        "throws" => 1,
        "catches" => 2,
        _ => 3,
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_rank_ordering() {
        assert!(role_rank("both") < role_rank("throws"));
        assert!(role_rank("throws") < role_rank("catches"));
        assert!(role_rank("catches") < role_rank("any"));
    }

    #[test]
    fn test_throw_patterns_non_empty() {
        assert!(!THROW_PATTERNS.is_empty());
        assert!(!CATCH_PATTERNS.is_empty());
        assert!(!GENERAL_PATTERNS.is_empty());
    }
}
