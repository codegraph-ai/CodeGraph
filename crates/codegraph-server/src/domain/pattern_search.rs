// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Regex-based pattern search across graph nodes — transport-agnostic.
//!
//! Searches node properties (name, signature, body_prefix, doc) using a compiled
//! `regex::Regex`. Supports optional scope narrowing and node-type filtering.

use crate::domain::node_props;
use codegraph::{CodeGraph, NodeType};
use regex::Regex;
use serde::Serialize;

// ============================================================
// Result Types
// ============================================================

#[derive(Debug, Serialize)]
pub(crate) struct PatternSearchResult {
    pub matches: Vec<PatternMatch>,
    pub total_matches: usize,
    pub pattern: String,
    pub scope: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PatternMatch {
    pub node_id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line_start: usize,
    pub line_end: usize,
    /// Which scope field matched: "name", "signature", "body", or "docstring".
    pub matched_in: String,
    /// The snippet of the matched text, truncated to ~200 chars.
    pub matched_text: String,
    pub signature: String,
}

// ============================================================
// Domain Function
// ============================================================

/// Search graph nodes whose properties match `pattern` (a regex string).
///
/// `scope` controls which property is searched:
/// - `"name"` — node name only
/// - `"signature"` — signature property only
/// - `"function_body"` — body_prefix property only
/// - `"docstring"` — doc property only
/// - `"any"` (default) — all of the above; the first matching scope is reported
///
/// `node_type_filter` restricts to a specific `NodeType` string (e.g. `"function"`,
/// `"class"`). Pass `"any"` or an empty string to search all node types.
///
/// `limit` caps the number of returned matches (default 50).
pub(crate) fn search_by_pattern(
    graph: &CodeGraph,
    pattern: &str,
    scope: Option<&str>,
    node_type_filter: Option<&str>,
    limit: usize,
) -> PatternSearchResult {
    let scope = scope.unwrap_or("any");
    let node_type_filter = node_type_filter.unwrap_or("any");

    // Compile the regex — return empty result on invalid pattern
    let re = match Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => {
            return PatternSearchResult {
                matches: vec![],
                total_matches: 0,
                pattern: pattern.to_string(),
                scope: scope.to_string(),
            }
        }
    };

    let mut matches: Vec<PatternMatch> = Vec::new();

    for (&node_id, node) in graph.nodes_iter() {
        // Node-type filter
        if !node_type_filter.is_empty() && node_type_filter != "any" {
            let kind_str = format!("{:?}", node.node_type).to_lowercase();
            if kind_str != node_type_filter {
                continue;
            }
        }

        // Skip file/module nodes unless the user explicitly requested them
        if matches!(node.node_type, NodeType::CodeFile | NodeType::Module)
            && node_type_filter == "any"
        {
            continue;
        }

        let name = node_props::name(node);
        let signature = node.properties.get_string("signature").unwrap_or("");
        let body_prefix = node.properties.get_string("body_prefix").unwrap_or("");
        let doc = node.properties.get_string("doc").unwrap_or("");
        let kind = format!("{:?}", node.node_type).to_lowercase();
        let path = node_props::path(node).to_string();
        let line_start = node_props::line_start(node) as usize;
        let line_end = node_props::line_end(node) as usize;
        let sig_str = signature.to_string();

        if let Some(pm) = try_match(
            &re,
            scope,
            node_id.to_string(),
            name,
            &kind,
            &path,
            line_start,
            line_end,
            &sig_str,
            signature,
            body_prefix,
            doc,
        ) {
            matches.push(pm);
        }
    }

    // Sort for stable, useful ordering: path then line_start
    matches.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line_start.cmp(&b.line_start)));

    let total_matches = matches.len();
    matches.truncate(limit);

    PatternSearchResult {
        matches,
        total_matches,
        pattern: pattern.to_string(),
        scope: scope.to_string(),
    }
}

// ============================================================
// Private Helpers
// ============================================================

/// Try to match `re` against the properties governed by `scope`.
///
/// Returns `Some(PatternMatch)` for the first scope that produces a match, or `None`.
#[allow(clippy::too_many_arguments)]
fn try_match(
    re: &Regex,
    scope: &str,
    node_id: String,
    name: &str,
    kind: &str,
    path: &str,
    line_start: usize,
    line_end: usize,
    sig_str: &str,
    signature: &str,
    body_prefix: &str,
    doc: &str,
) -> Option<PatternMatch> {
    let make_match = |matched_in: &str, text: &str| PatternMatch {
        node_id: node_id.clone(),
        name: name.to_string(),
        kind: kind.to_string(),
        path: path.to_string(),
        line_start,
        line_end,
        matched_in: matched_in.to_string(),
        matched_text: truncate(text, 200),
        signature: sig_str.to_string(),
    };

    match scope {
        "name" => {
            if re.is_match(name) {
                Some(make_match("name", name))
            } else {
                None
            }
        }
        "signature" => {
            if re.is_match(signature) {
                Some(make_match("signature", signature))
            } else {
                None
            }
        }
        "function_body" => {
            if re.is_match(body_prefix) {
                Some(make_match("body", body_prefix))
            } else {
                None
            }
        }
        "docstring" => {
            if re.is_match(doc) {
                Some(make_match("docstring", doc))
            } else {
                None
            }
        }
        // "any" or anything else — first matching scope wins
        _ => {
            if re.is_match(name) {
                Some(make_match("name", name))
            } else if re.is_match(signature) {
                Some(make_match("signature", signature))
            } else if re.is_match(body_prefix) {
                Some(make_match("body", body_prefix))
            } else if re.is_match(doc) {
                Some(make_match("docstring", doc))
            } else {
                None
            }
        }
    }
}

/// Truncate `s` to at most `max_chars` Unicode scalar values.
fn truncate(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}…", collected)
    } else {
        collected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 200), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let long: String = "a".repeat(300);
        let result = truncate(&long, 200);
        // 200 chars + ellipsis
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 201);
    }

    #[test]
    fn test_invalid_pattern_returns_empty() {
        // Build a minimal in-memory graph
        let graph = codegraph::CodeGraph::in_memory().expect("in-memory graph");
        let result = search_by_pattern(&graph, "[invalid(", None, None, 50);
        assert_eq!(result.total_matches, 0);
        assert!(result.matches.is_empty());
    }
}
