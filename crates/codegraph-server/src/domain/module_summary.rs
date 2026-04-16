// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Module summary — transport-agnostic.
//!
//! Aggregates stats for all files whose path starts with a given directory prefix.
//! Synchronous (takes &CodeGraph).

use crate::domain::node_props;
use codegraph::{CodeGraph, NodeType};
use serde::Serialize;
use std::collections::HashMap;

// ============================================================
// Response Types
// ============================================================

/// Per-language file and function counts.
#[derive(Debug, Serialize)]
pub(crate) struct LanguageBreakdown {
    pub language: String,
    pub files: usize,
    pub functions: usize,
}

/// A function with notable cyclomatic complexity.
#[derive(Debug, Serialize)]
pub(crate) struct ComplexFunction {
    pub name: String,
    pub path: String,
    pub complexity: u32,
    pub line_start: usize,
}

/// Aggregated summary for a directory (module) in the graph.
#[derive(Debug, Serialize)]
pub(crate) struct ModuleSummaryResult {
    pub directory: String,
    pub files: usize,
    pub total_functions: usize,
    pub total_classes: usize,
    pub total_imports: usize,
    pub total_lines: usize,
    pub languages: Vec<LanguageBreakdown>,
    pub top_complex_functions: Vec<ComplexFunction>,
    pub external_deps: Vec<String>,
}

// ============================================================
// Domain Function
// ============================================================

/// Build a high-level summary of all nodes whose `path` starts with `directory`.
///
/// Counts files, functions, classes, interface nodes, and total lines.
/// Groups by language.  Collects external module names.
/// Returns the top `top_n` functions ranked by complexity (descending).
pub(crate) fn get_module_summary(
    graph: &CodeGraph,
    directory: &str,
    top_n: usize,
) -> ModuleSummaryResult {
    // Normalise: strip trailing slash so prefix matching is consistent.
    let prefix = directory.trim_end_matches('/');

    let mut files: usize = 0;
    let mut total_functions: usize = 0;
    let mut total_classes: usize = 0;
    let mut total_imports: usize = 0;
    let mut total_lines: usize = 0;

    // language -> (file_count, function_count)
    let mut lang_map: HashMap<String, (usize, usize)> = HashMap::new();

    // All functions with a complexity score, for later sorting.
    let mut complex_fns: Vec<ComplexFunction> = Vec::new();

    // External module names (deduplicated via HashMap key).
    let mut ext_deps: HashMap<String, ()> = HashMap::new();

    for (_node_id, node) in graph.iter_nodes() {
        let path = node_props::path(node);

        match node.node_type {
            NodeType::CodeFile => {
                // Only include files under the requested directory prefix.
                if !path_matches(path, prefix) {
                    continue;
                }

                files += 1;

                let language = {
                    let l = node_props::language(node);
                    if l.is_empty() { "unknown" } else { l }
                };

                let line_count = node
                    .properties
                    .get_int("line_count")
                    .unwrap_or(0) as usize;
                total_lines += line_count;

                let entry = lang_map.entry(language.to_string()).or_insert((0, 0));
                entry.0 += 1;
            }

            NodeType::Function => {
                if !path_matches(path, prefix) {
                    continue;
                }

                total_functions += 1;

                // Count toward language breakdown (keyed by file path language —
                // functions carry the same language as their containing file).
                let language = {
                    let l = node_props::language(node);
                    if l.is_empty() { "unknown" } else { l }
                };
                lang_map.entry(language.to_string()).or_insert((0, 0)).1 += 1;

                // Collect complexity info.
                let complexity = node
                    .properties
                    .get_int("complexity")
                    .unwrap_or(0) as u32;
                let name = node_props::name(node).to_string();
                let line_start = node_props::line_start(node) as usize;

                complex_fns.push(ComplexFunction {
                    name,
                    path: path.to_string(),
                    complexity,
                    line_start,
                });
            }

            NodeType::Class | NodeType::Interface => {
                if !path_matches(path, prefix) {
                    continue;
                }
                total_classes += 1;
            }

            NodeType::Module => {
                // Check is_external — path may be empty for external modules.
                let is_external = node
                    .properties
                    .get_string("is_external")
                    .map(|v| v == "true")
                    .unwrap_or(false)
                    || node
                        .properties
                        .get_bool("is_external")
                        .unwrap_or(false);

                if is_external {
                    // External deps are workspace-wide, not path-filtered.
                    let name = node_props::name(node);
                    if !name.is_empty() {
                        ext_deps.insert(name.to_string(), ());
                    }
                } else if path_matches(path, prefix) {
                    // Count internal import nodes that live inside the directory.
                    total_imports += 1;
                }
            }

            _ => {}
        }
    }

    // Sort complex functions descending by complexity, then take top_n.
    complex_fns.sort_by(|a, b| b.complexity.cmp(&a.complexity));
    complex_fns.truncate(top_n);

    // Build language breakdown vec sorted descending by file count.
    let mut languages: Vec<LanguageBreakdown> = lang_map
        .into_iter()
        .map(|(language, (file_count, fn_count))| LanguageBreakdown {
            language,
            files: file_count,
            functions: fn_count,
        })
        .collect();
    languages.sort_by(|a, b| b.files.cmp(&a.files).then(a.language.cmp(&b.language)));

    let mut external_deps: Vec<String> = ext_deps.into_keys().collect();
    external_deps.sort();

    ModuleSummaryResult {
        directory: directory.to_string(),
        files,
        total_functions,
        total_classes,
        total_imports,
        total_lines,
        languages,
        top_complex_functions: complex_fns,
        external_deps,
    }
}

// ============================================================
// Helpers
// ============================================================

/// Returns true when `path` starts with `prefix`.
///
/// Handles the edge case where `prefix` is empty (matches everything) and ensures
/// we don't accidentally match `/foo/bar_baz` with prefix `/foo/bar`.
#[inline]
fn path_matches(path: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    if path == prefix {
        return true;
    }
    // Require that the prefix is followed by a path separator so that
    // e.g. prefix "src/foo" does not match "src/foobar/...".
    path.starts_with(prefix)
        && path[prefix.len()..].starts_with('/')
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matches_exact() {
        assert!(path_matches("/a/b", "/a/b"));
    }

    #[test]
    fn test_path_matches_child() {
        assert!(path_matches("/a/b/c.rs", "/a/b"));
    }

    #[test]
    fn test_path_matches_no_partial_dir() {
        // "src/foobar/x" must NOT match prefix "src/foo"
        assert!(!path_matches("/src/foobar/x.rs", "/src/foo"));
    }

    #[test]
    fn test_path_matches_empty_prefix() {
        assert!(path_matches("/anything", ""));
    }

    #[test]
    fn test_path_no_match() {
        assert!(!path_matches("/other/path.rs", "/src"));
    }
}
