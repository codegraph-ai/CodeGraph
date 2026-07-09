// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Node resolution — unified find_nearest_node.

use codegraph::{CodeGraph, NodeId};

use crate::domain::node_props;

/// Compare two file paths for the "same file" relation, tolerant of one side being
/// relative (as stored by the indexer, e.g. "./crates/foo/bar.rs") and the other
/// absolute (as produced by resolving a `file://` URI from an IDE/MCP client).
///
/// Falls back to a suffix match on path-component boundaries so `"./a/b.rs"`
/// matches `"/workspace/a/b.rs"` without requiring filesystem canonicalization.
pub(crate) fn paths_match(node_path: &str, query_path: &str) -> bool {
    let node_norm = node_path.trim_start_matches("./");
    let query_norm = query_path.trim_start_matches("./");
    if node_norm.is_empty() || query_norm.is_empty() {
        return false;
    }
    if node_norm == query_norm {
        return true;
    }
    if let Some(rest) = query_norm.strip_suffix(node_norm) {
        return rest.is_empty() || rest.ends_with('/');
    }
    if let Some(rest) = node_norm.strip_suffix(query_norm) {
        return rest.is_empty() || rest.ends_with('/');
    }
    false
}

/// Resolve a `uri` argument to a path string usable for graph lookups.
///
/// MCP clients are expected to send LSP-style `file://` URIs, but plain
/// filesystem paths (relative or absolute) are also accepted — this matters
/// for `--run-tool` one-shot invocations and simpler MCP clients that don't
/// bother with URI encoding.
pub(crate) fn resolve_uri_to_path(uri: &str) -> Option<String> {
    if let Ok(url) = tower_lsp::lsp_types::Url::parse(uri) {
        if let Ok(path) = url.to_file_path() {
            return Some(path.to_string_lossy().to_string());
        }
        // Parsed as a URL but not a file:// URI we can resolve to a path — invalid.
        if url.scheme() != "file" {
            return None;
        }
    }
    if uri.is_empty() {
        return None;
    }
    Some(uri.to_string())
}

/// Find the nearest function/symbol node at or near a given line in a file.
///
/// Strategy:
/// 1. Exact containment: find nodes whose line range contains target_line (prefer tightest fit)
/// 2. Fallback: find nearest node by proximity (prefer forward-looking, penalize backward)
///
/// Returns (node_id, used_fallback) where used_fallback is true if no exact containment was found.
pub(crate) fn find_nearest_node(
    graph: &CodeGraph,
    file_path: &str,
    target_line: u32,
) -> Option<(NodeId, bool)> {
    // Strategy 1: Exact containment (prefer tightest — smallest range)
    // Only considers nodes with an explicit line range. Container nodes like
    // `CodeFile` are often indexed without `line_start`/`line_end` at all; treating
    // that absence as a `0..0` range would make them win every containment check
    // for `target_line == 0` (range_size 0 beats any real symbol's range) and
    // masquerade as the nearest symbol, even though they have no real source span.
    let mut best_exact: Option<(NodeId, u32)> = None; // (id, range_size)

    for (&node_id, node) in graph.nodes_iter() {
        if !paths_match(node_props::path(node), file_path) {
            continue;
        }
        let (Some(start), Some(end)) =
            (node_props::line_start_opt(node), node_props::line_end_opt(node))
        else {
            continue;
        };
        if target_line >= start && target_line <= end {
            let range_size = end.saturating_sub(start);
            if best_exact.is_none() || range_size < best_exact.unwrap().1 {
                best_exact = Some((node_id, range_size));
            }
        }
    }
    if let Some((id, _)) = best_exact {
        return Some((id, false));
    }

    // Strategy 2: Nearest by proximity (prefer forward, penalize backward)
    let mut best_fallback: Option<(NodeId, i64)> = None;
    for (&node_id, node) in graph.nodes_iter() {
        if !paths_match(node_props::path(node), file_path) {
            continue;
        }
        let (Some(start), Some(end)) =
            (node_props::line_start_opt(node), node_props::line_end_opt(node))
        else {
            continue;
        };
        let start = start as i64;
        let end = end as i64;
        let target = target_line as i64;

        let distance = if start > target {
            start - target // Forward — preferred
        } else {
            (target - end) + 1000 // Backward — penalized
        };

        if best_fallback.is_none() || distance < best_fallback.unwrap().1 {
            best_fallback = Some((node_id, distance));
        }
    }

    best_fallback.map(|(id, _)| (id, true))
}

/// Structured reason why `find_nearest_node` (or a `uri` resolution) failed to
/// find any node, replacing the previous generic "Could not find symbol"
/// message with something an agent can act on without another round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolNotFoundReason {
    /// `uri` could not be resolved to a filesystem path at all (e.g. a
    /// non-`file://` URI or an unparseable one).
    InvalidUri,
    /// The graph has no nodes at all — nothing has been indexed yet.
    WorkspaceNotIndexed,
    /// The graph has nodes, but none of them belong to the requested file.
    FileNotIndexed,
}

impl SymbolNotFoundReason {
    pub(crate) fn code(self) -> &'static str {
        match self {
            SymbolNotFoundReason::InvalidUri => "invalid_uri",
            SymbolNotFoundReason::WorkspaceNotIndexed => "workspace_not_indexed",
            SymbolNotFoundReason::FileNotIndexed => "file_not_indexed",
        }
    }

    pub(crate) fn hint(self) -> &'static str {
        match self {
            SymbolNotFoundReason::InvalidUri => {
                "Pass a file:// URI or a plain filesystem path (relative or absolute)."
            }
            SymbolNotFoundReason::WorkspaceNotIndexed => {
                "Run codegraph_reindex_workspace(force=true) before querying symbols."
            }
            SymbolNotFoundReason::FileNotIndexed => {
                "This file has no nodes in the graph yet. Run codegraph_index_files \
                 for it, or codegraph_reindex_workspace(force=true) if it should \
                 already be covered by the workspace root."
            }
        }
    }

    /// Concrete next tool calls likely to resolve this failure — the same
    /// guidance as `hint()`, but as machine-actionable tool names instead of
    /// prose, so an agent doesn't have to parse English to pick a next step.
    pub(crate) fn suggested_next_queries(self) -> &'static [&'static str] {
        match self {
            SymbolNotFoundReason::InvalidUri => &[],
            SymbolNotFoundReason::WorkspaceNotIndexed => {
                &["codegraph_reindex_workspace(force=true)"]
            }
            SymbolNotFoundReason::FileNotIndexed => &[
                "codegraph_index_files",
                "codegraph_reindex_workspace(force=true)",
            ],
        }
    }
}

/// Confidence label for a resolved symbol: `"exact"` when the requested line
/// fell inside the symbol's own range, `"fallback"` when the nearest symbol
/// was substituted instead. Lets an agent decide whether to trust a response
/// without re-deriving it from `used_fallback` itself.
pub(crate) fn match_confidence(used_fallback: bool) -> &'static str {
    if used_fallback {
        "fallback"
    } else {
        "exact"
    }
}

/// Diagnose why a `uri`-based lookup found no node, for use in error responses.
pub(crate) fn diagnose_not_found(graph: &CodeGraph, uri: &str) -> SymbolNotFoundReason {
    let Some(path) = resolve_uri_to_path(uri) else {
        return SymbolNotFoundReason::InvalidUri;
    };
    if graph.nodes_iter().next().is_none() {
        return SymbolNotFoundReason::WorkspaceNotIndexed;
    }
    let file_has_nodes = graph
        .nodes_iter()
        .any(|(_, node)| paths_match(node_props::path(node), &path));
    if !file_has_nodes {
        return SymbolNotFoundReason::FileNotIndexed;
    }
    // File has nodes but find_nearest_node still returned None — shouldn't
    // happen given the fallback strategy has no distance limit, but keep a
    // safe default rather than panicking on an invariant we can't fully prove.
    SymbolNotFoundReason::FileNotIndexed
}

#[cfg(test)]
mod tests {
    use super::{diagnose_not_found, find_nearest_node, match_confidence, paths_match, SymbolNotFoundReason};
    use codegraph::{CodeGraph, NodeType, PropertyMap};

    #[test]
    fn suggested_next_queries_nonempty_for_actionable_reasons() {
        assert!(SymbolNotFoundReason::InvalidUri
            .suggested_next_queries()
            .is_empty());
        assert_eq!(
            SymbolNotFoundReason::WorkspaceNotIndexed.suggested_next_queries(),
            &["codegraph_reindex_workspace(force=true)"]
        );
        assert_eq!(
            SymbolNotFoundReason::FileNotIndexed
                .suggested_next_queries()
                .len(),
            2
        );
    }

    #[test]
    fn match_confidence_exact_vs_fallback() {
        assert_eq!(match_confidence(false), "exact");
        assert_eq!(match_confidence(true), "fallback");
    }

    #[test]
    fn diagnose_not_found_invalid_uri() {
        let graph = CodeGraph::in_memory().unwrap();
        assert_eq!(
            diagnose_not_found(&graph, "not-a-uri://???"),
            SymbolNotFoundReason::InvalidUri
        );
    }

    #[test]
    fn diagnose_not_found_empty_workspace() {
        let graph = CodeGraph::in_memory().unwrap();
        assert_eq!(
            diagnose_not_found(&graph, "/workspace/src/lib.rs"),
            SymbolNotFoundReason::WorkspaceNotIndexed
        );
    }

    #[test]
    fn diagnose_not_found_file_not_indexed() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let props = PropertyMap::new()
            .with("name", "other")
            .with("path", "./crates/foo/other.rs");
        graph.add_node(NodeType::Function, props).unwrap();

        assert_eq!(
            diagnose_not_found(&graph, "/workspace/crates/foo/missing.rs"),
            SymbolNotFoundReason::FileNotIndexed
        );
    }

    #[test]
    fn find_nearest_node_skips_codefile_node_without_line_range() {
        // Regression test: a `CodeFile` node indexed without `line_start`/`line_end`
        // (e.g. Python's module node, see codegraph-python/src/parser_impl.rs) must
        // not shadow a real function when querying line 0 — its absent range used
        // to default to 0..0, which trivially "contains" line 0 with the smallest
        // possible range and won the containment check over any real symbol.
        let mut graph = CodeGraph::in_memory().unwrap();
        let file_props = PropertyMap::new()
            .with("name", "init_arch_workflow")
            .with("path", "./app/services/init_arch_workflow.py")
            .with("language", "python");
        graph.add_node(NodeType::CodeFile, file_props).unwrap();

        let fn_props = PropertyMap::new()
            .with("name", "init_arch_workflow")
            .with("path", "./app/services/init_arch_workflow.py")
            .with("line_start", "1")
            .with("line_end", "40");
        let fn_id = graph.add_node(NodeType::Function, fn_props).unwrap();

        let (node_id, used_fallback) =
            find_nearest_node(&graph, "./app/services/init_arch_workflow.py", 0)
                .expect("expected a match");
        assert_eq!(node_id, fn_id);
        assert!(used_fallback, "line 0 is outside the function's own range");
    }

    #[test]
    fn find_nearest_node_returns_none_when_only_codefile_node_present() {
        // If a file has no indexed symbols at all — only its `CodeFile` node — the
        // caller should get a proper "not found" instead of a fake match on the
        // file node's fabricated 0..0 range.
        let mut graph = CodeGraph::in_memory().unwrap();
        let file_props = PropertyMap::new()
            .with("name", "empty_module")
            .with("path", "./app/empty_module.py")
            .with("language", "python");
        graph.add_node(NodeType::CodeFile, file_props).unwrap();

        assert!(find_nearest_node(&graph, "./app/empty_module.py", 0).is_none());
    }

    #[test]
    fn identical_paths_match() {
        assert!(paths_match("./crates/foo/bar.rs", "./crates/foo/bar.rs"));
        assert!(paths_match("crates/foo/bar.rs", "crates/foo/bar.rs"));
    }

    #[test]
    fn relative_matches_absolute_on_boundary() {
        assert!(paths_match(
            "./crates/foo/bar.rs",
            "/workspace/crates/foo/bar.rs"
        ));
        assert!(paths_match(
            "/workspace/crates/foo/bar.rs",
            "./crates/foo/bar.rs"
        ));
    }

    #[test]
    fn does_not_match_on_partial_component() {
        // "bar.rs" must not match "notbar.rs" — boundary must fall on a '/'
        assert!(!paths_match("./crates/foo/notbar.rs", "bar.rs"));
        assert!(!paths_match(
            "./crates/other/bar.rs",
            "./crates/foo/bar.rs"
        ));
    }

    #[test]
    fn empty_paths_never_match() {
        assert!(!paths_match("", ""));
        assert!(!paths_match("./crates/foo/bar.rs", ""));
        assert!(!paths_match("", "./crates/foo/bar.rs"));
    }
}
