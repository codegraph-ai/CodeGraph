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
    matches.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line_start.cmp(&b.line_start))
    });

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
    use codegraph::{PropertyMap, PropertyValue};

    /// Add a node from a slice of (key, PropertyValue) props, returning its id.
    fn add_node(
        graph: &mut CodeGraph,
        ty: NodeType,
        props: &[(&str, PropertyValue)],
    ) -> codegraph::NodeId {
        let mut map = PropertyMap::new();
        for (k, v) in props {
            map.insert(k.to_string(), v.clone());
        }
        graph.add_node(ty, map).expect("add_node")
    }

    fn s(v: &str) -> PropertyValue {
        PropertyValue::String(v.to_string())
    }

    fn i(v: i64) -> PropertyValue {
        PropertyValue::Int(v)
    }

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

    #[test]
    fn name_scope_matches_only_name() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("parse_config")),
                ("path", s("/src/a.rs")),
                ("signature", s("fn parse_config() -> Config")),
                ("line_start", i(10)),
                ("line_end", i(20)),
            ],
        );
        let result = search_by_pattern(&g, "parse_", Some("name"), None, 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.scope, "name");
        let m = &result.matches[0];
        assert_eq!(m.name, "parse_config");
        assert_eq!(m.matched_in, "name");
        assert_eq!(m.matched_text, "parse_config");
        assert_eq!(m.kind, "function");
        assert_eq!(m.line_start, 10);
        assert_eq!(m.line_end, 20);
        assert_eq!(m.signature, "fn parse_config() -> Config");
    }

    #[test]
    fn name_scope_does_not_match_signature() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Pattern only appears in the signature, not the name.
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("foo")),
                ("path", s("/src/a.rs")),
                ("signature", s("fn foo(cfg: Config)")),
            ],
        );
        let result = search_by_pattern(&g, "Config", Some("name"), None, 50);
        assert_eq!(result.total_matches, 0);
    }

    #[test]
    fn signature_scope_matches() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("foo")),
                ("path", s("/src/a.rs")),
                ("signature", s("fn foo(cfg: Config)")),
            ],
        );
        let result = search_by_pattern(&g, "Config", Some("signature"), None, 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].matched_in, "signature");
    }

    #[test]
    fn function_body_scope_matches_body_prefix() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("foo")),
                ("path", s("/src/a.rs")),
                ("body_prefix", s("let x = todo_marker();")),
            ],
        );
        let result = search_by_pattern(&g, "todo_marker", Some("function_body"), None, 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].matched_in, "body");
    }

    #[test]
    fn docstring_scope_matches_doc() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("foo")),
                ("path", s("/src/a.rs")),
                ("doc", s("Deprecated: use bar instead")),
            ],
        );
        let result = search_by_pattern(&g, "Deprecated", Some("docstring"), None, 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].matched_in, "docstring");
    }

    #[test]
    fn any_scope_prefers_name_over_other_fields() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // "target" appears in every field; "any" must report the name hit first.
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("target")),
                ("path", s("/src/a.rs")),
                ("signature", s("fn target()")),
                ("body_prefix", s("target();")),
                ("doc", s("the target")),
            ],
        );
        let result = search_by_pattern(&g, "target", None, None, 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].matched_in, "name");
    }

    #[test]
    fn any_scope_falls_through_to_signature_then_body_then_doc() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // signature precedence when name misses
        let mut g2 = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("foo")),
                ("path", s("/src/a.rs")),
                ("signature", s("fn foo(z: Zeta)")),
                ("body_prefix", s("Zeta::new()")),
            ],
        );
        assert_eq!(
            search_by_pattern(&g, "Zeta", None, None, 50).matches[0].matched_in,
            "signature"
        );
        // doc precedence when name/sig/body all miss
        add_node(
            &mut g2,
            NodeType::Function,
            &[
                ("name", s("foo")),
                ("path", s("/src/a.rs")),
                ("doc", s("mentions Omega only")),
            ],
        );
        assert_eq!(
            search_by_pattern(&g2, "Omega", None, None, 50).matches[0].matched_in,
            "docstring"
        );
    }

    #[test]
    fn node_type_filter_restricts_kind() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", s("widget_fn")), ("path", s("/src/a.rs"))],
        );
        add_node(
            &mut g,
            NodeType::Class,
            &[("name", s("widget_cls")), ("path", s("/src/a.rs"))],
        );
        let result = search_by_pattern(&g, "widget", Some("name"), Some("class"), 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].kind, "class");
        assert_eq!(result.matches[0].name, "widget_cls");
    }

    #[test]
    fn any_filter_skips_file_and_module_nodes() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::CodeFile,
            &[
                ("name", s("mod_target.rs")),
                ("path", s("/src/mod_target.rs")),
            ],
        );
        add_node(
            &mut g,
            NodeType::Module,
            &[("name", s("mod_target")), ("path", s("/src/mod_target.rs"))],
        );
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", s("mod_target_fn")), ("path", s("/src/a.rs"))],
        );
        // "any" filter drops the CodeFile/Module hits, keeping only the function.
        let result = search_by_pattern(&g, "mod_target", Some("name"), Some("any"), 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].kind, "function");
    }

    #[test]
    fn explicit_module_filter_includes_module_nodes() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Module,
            &[("name", s("mod_target")), ("path", s("/src/mod_target.rs"))],
        );
        let result = search_by_pattern(&g, "mod_target", Some("name"), Some("module"), 50);
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].kind, "module");
    }

    #[test]
    fn results_sorted_by_path_then_line_start() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("hit_b2")),
                ("path", s("/src/b.rs")),
                ("line_start", i(30)),
            ],
        );
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("hit_a")),
                ("path", s("/src/a.rs")),
                ("line_start", i(99)),
            ],
        );
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("hit_b1")),
                ("path", s("/src/b.rs")),
                ("line_start", i(5)),
            ],
        );
        let result = search_by_pattern(&g, "hit_", Some("name"), None, 50);
        let order: Vec<&str> = result.matches.iter().map(|m| m.name.as_str()).collect();
        // /src/a.rs (any line) before /src/b.rs; within b.rs, line 5 before line 30.
        assert_eq!(order, vec!["hit_a", "hit_b1", "hit_b2"]);
    }

    #[test]
    fn limit_truncates_matches_but_total_reflects_all() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        for n in 0..5 {
            add_node(
                &mut g,
                NodeType::Function,
                &[
                    ("name", s(&format!("hit_{n}"))),
                    ("path", s("/src/a.rs")),
                    ("line_start", i(n)),
                ],
            );
        }
        let result = search_by_pattern(&g, "hit_", Some("name"), None, 2);
        assert_eq!(result.total_matches, 5);
        assert_eq!(result.matches.len(), 2);
    }

    #[test]
    fn matched_text_is_truncated_to_200_chars() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let long_body = format!("start_{}", "x".repeat(300));
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", s("foo")),
                ("path", s("/src/a.rs")),
                ("body_prefix", s(&long_body)),
            ],
        );
        let result = search_by_pattern(&g, "start_", Some("function_body"), None, 50);
        assert_eq!(result.total_matches, 1);
        let text = &result.matches[0].matched_text;
        assert!(text.ends_with('…'));
        assert_eq!(text.chars().count(), 201);
    }

    #[test]
    fn no_match_returns_empty_with_scope_echoed() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", s("foo")), ("path", s("/src/a.rs"))],
        );
        let result = search_by_pattern(&g, "no_such_symbol", Some("name"), None, 50);
        assert_eq!(result.total_matches, 0);
        assert!(result.matches.is_empty());
        assert_eq!(result.scope, "name");
        assert_eq!(result.pattern, "no_such_symbol");
    }

    #[test]
    fn empty_node_type_filter_treated_like_any_but_still_scans_functions() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", s("scan_me")), ("path", s("/src/a.rs"))],
        );
        // An empty filter string is not "any", so the file/module skip guard
        // (which is gated on == "any") does not trip; functions still match.
        let result = search_by_pattern(&g, "scan_me", Some("name"), Some(""), 50);
        assert_eq!(result.total_matches, 1);
    }
}
