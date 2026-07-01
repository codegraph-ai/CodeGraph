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
    "error",
    "Error",
    "err",
    "exception",
    "Exception",
    "fail",
    "failure",
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

            let body = node.properties.get_string("body_prefix").unwrap_or("");
            let signature = node.properties.get_string("signature").unwrap_or("");
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
    use codegraph::{PropertyMap, PropertyValue};

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

    /// Add a Function node carrying name/path/signature/body_prefix/line props.
    fn add_fn(
        graph: &mut CodeGraph,
        name: &str,
        path: &str,
        signature: &str,
        body: &str,
        line_start: i64,
        line_end: i64,
    ) {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), PropertyValue::String(name.to_string()));
        props.insert("path".to_string(), PropertyValue::String(path.to_string()));
        props.insert(
            "signature".to_string(),
            PropertyValue::String(signature.to_string()),
        );
        props.insert(
            "body_prefix".to_string(),
            PropertyValue::String(body.to_string()),
        );
        props.insert("line_start".to_string(), PropertyValue::Int(line_start));
        props.insert("line_end".to_string(), PropertyValue::Int(line_end));
        graph.add_node(NodeType::Function, props).expect("add_node");
    }

    fn add_node(graph: &mut CodeGraph, ty: NodeType, name: &str, body: &str) {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), PropertyValue::String(name.to_string()));
        props.insert(
            "body_prefix".to_string(),
            PropertyValue::String(body.to_string()),
        );
        graph.add_node(ty, props).expect("add_node");
    }

    #[test]
    fn non_function_nodes_are_ignored() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // A Class node carrying throw-like text must not be reported.
        add_node(&mut g, NodeType::Class, "Boom", "throw new Error()");
        let result = search_by_error(&g, None, "any", 50);
        assert_eq!(result.total_matches, 0);
        assert!(result.functions.is_empty());
    }

    #[test]
    fn clean_function_is_not_matched() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "add", "/a.rs", "fn add()", "return a + b;", 1, 3);
        let result = search_by_error(&g, None, "any", 50);
        assert_eq!(result.total_matches, 0);
    }

    #[test]
    fn throws_only_function_has_throws_role() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(
            &mut g,
            "boom",
            "/a.rs",
            "fn boom()",
            "let x = Err(oops);",
            1,
            3,
        );
        let result = search_by_error(&g, None, "any", 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.functions[0].error_role, "throws");
        assert!(result.functions[0]
            .error_patterns
            .contains(&"Err(".to_string()));
    }

    #[test]
    fn catches_only_function_has_catches_role() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(
            &mut g,
            "safe",
            "/a.rs",
            "fn safe()",
            "} catch(e) { log(e); }",
            1,
            3,
        );
        let result = search_by_error(&g, None, "any", 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.functions[0].error_role, "catches");
    }

    #[test]
    fn function_that_throws_and_catches_has_both_role() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(
            &mut g,
            "wrap",
            "/a.rs",
            "fn wrap()",
            "throw new Error(); } catch(e) {}",
            1,
            5,
        );
        let result = search_by_error(&g, None, "any", 50);
        assert_eq!(result.functions[0].error_role, "both");
    }

    #[test]
    fn general_pattern_fallback_matches_when_no_specific_pattern() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // "error_count" trips only the general patterns, not throw/catch tables.
        add_fn(
            &mut g,
            "count",
            "/a.rs",
            "fn count()",
            "let error_count = 0;",
            1,
            2,
        );
        let result = search_by_error(&g, None, "any", 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.functions[0].error_role, "any");
    }

    #[test]
    fn general_only_match_in_throws_mode_is_classified_as_throws() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Body trips only the general table (no throw/catch pattern). Under
        // "throws" mode, passes_mode's second clause (!has_catches &&
        // !general_hits.is_empty()) keeps it, and the role-classification
        // fallback labels it "throws" rather than "any".
        add_fn(
            &mut g,
            "count",
            "/a.rs",
            "fn count()",
            "let error_count = 0;",
            1,
            2,
        );
        let result = search_by_error(&g, None, "throws", 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.functions[0].error_role, "throws");
        assert_eq!(result.mode, "throws");
    }

    #[test]
    fn general_only_match_in_catches_mode_is_classified_as_catches() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Mirror of the throws case: a general-only body under "catches" mode
        // passes via (!has_throws && !general_hits.is_empty()) and is labeled
        // "catches" by the classification fallback.
        add_fn(
            &mut g,
            "count",
            "/a.rs",
            "fn count()",
            "let error_count = 0;",
            1,
            2,
        );
        let result = search_by_error(&g, None, "catches", 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.functions[0].error_role, "catches");
        assert_eq!(result.mode, "catches");
    }

    #[test]
    fn signature_is_scanned_for_patterns() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Pattern lives in the signature (Result<...>), body is clean.
        add_fn(
            &mut g,
            "load",
            "/a.rs",
            "fn load() -> Result<T, E>",
            "ok",
            1,
            2,
        );
        let result = search_by_error(&g, None, "any", 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.functions[0].error_role, "catches");
    }

    #[test]
    fn error_type_filter_excludes_non_matching_functions() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(
            &mut g,
            "io",
            "/a.rs",
            "fn io()",
            "return Err(IoError);",
            1,
            2,
        );
        add_fn(
            &mut g,
            "net",
            "/b.rs",
            "fn net()",
            "return Err(NetError);",
            1,
            2,
        );
        let result = search_by_error(&g, Some("IoError"), "any", 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.functions[0].name, "io");
        assert_eq!(result.error_type_filter.as_deref(), Some("IoError"));
    }

    #[test]
    fn throws_mode_excludes_catches_only_function() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "safe", "/a.rs", "fn safe()", "} catch(e) {}", 1, 2);
        let result = search_by_error(&g, None, "throws", 50);
        assert_eq!(result.total_matches, 0);
        assert_eq!(result.mode, "throws");
    }

    #[test]
    fn catches_mode_excludes_throws_only_function() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(
            &mut g,
            "boom",
            "/a.rs",
            "fn boom()",
            "let x = Err(e);",
            1,
            2,
        );
        let result = search_by_error(&g, None, "catches", 50);
        assert_eq!(result.total_matches, 0);
        assert_eq!(result.mode, "catches");
    }

    #[test]
    fn unknown_mode_normalizes_to_any() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(
            &mut g,
            "boom",
            "/a.rs",
            "fn boom()",
            "let x = Err(e);",
            1,
            2,
        );
        let result = search_by_error(&g, None, "garbage", 50);
        assert_eq!(result.mode, "any");
        assert_eq!(result.total_matches, 1);
    }

    #[test]
    fn results_sorted_both_then_throws_then_catches() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "c", "/c.rs", "fn c()", "} catch(e) {}", 1, 2);
        add_fn(&mut g, "b", "/b.rs", "fn b()", "let x = Err(e);", 1, 2);
        add_fn(
            &mut g,
            "a",
            "/a.rs",
            "fn a()",
            "throw x; } catch(e) {}",
            1,
            2,
        );
        let result = search_by_error(&g, None, "any", 50);
        let roles: Vec<&str> = result
            .functions
            .iter()
            .map(|f| f.error_role.as_str())
            .collect();
        assert_eq!(roles, vec!["both", "throws", "catches"]);
    }

    #[test]
    fn limit_truncates_functions_but_total_matches_is_full_count() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "a", "/a.rs", "fn a()", "Err(e);", 1, 2);
        add_fn(&mut g, "b", "/b.rs", "fn b()", "Err(e);", 1, 2);
        add_fn(&mut g, "c", "/c.rs", "fn c()", "Err(e);", 1, 2);
        let result = search_by_error(&g, None, "any", 2);
        assert_eq!(result.total_matches, 3);
        assert_eq!(result.functions.len(), 2);
    }

    #[test]
    fn error_patterns_are_sorted_and_deduped() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Two occurrences of `Err(` plus a distinct `panic!(` throw pattern.
        add_fn(
            &mut g,
            "x",
            "/a.rs",
            "fn x()",
            "Err(a); Err(b); panic!(c);",
            1,
            3,
        );
        let result = search_by_error(&g, None, "any", 50);
        let patterns = &result.functions[0].error_patterns;
        assert_eq!(patterns, &vec!["Err(".to_string(), "panic!(".to_string()]);
    }

    #[test]
    fn matched_function_carries_location_metadata() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "boom", "/src/x.rs", "fn boom()", "Err(e);", 10, 20);
        let result = search_by_error(&g, None, "any", 50);
        let f = &result.functions[0];
        assert_eq!(f.name, "boom");
        assert_eq!(f.path, "/src/x.rs");
        assert_eq!(f.signature, "fn boom()");
        assert_eq!(f.line_start, 10);
        assert_eq!(f.line_end, 20);
    }
}
