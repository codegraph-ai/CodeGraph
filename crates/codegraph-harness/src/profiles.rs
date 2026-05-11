// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Per-tool default normalisation profiles. The harness looks up a
//! profile by tool name before applying the case's `expect.normalize`
//! block; the case wins on every field. Lets new cases inherit
//! sensible defaults — sort_arrays for collection-returning tools,
//! float_decimals + keep_volatile for cosine-returning tools — without
//! every YAML restating the same boilerplate.
//!
//! Adding a new tool: pick the family it best fits in
//! `default_for` and add a name match. Outliers can keep an explicit
//! per-case override; that's why the merge is overlay-wins.

use crate::case::NormalizeOpts;

/// Tool family for reporting / rollup. Matches the same buckets used
/// for default profiles, but split out so navigation tools (which
/// have no profile because their order is contractual) are visible
/// in the report instead of bucketed as "Other".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Search,
    Similarity,
    Navigation,
    AnalysisCollection,
    Security,
    Memory,
    Git,
    Other,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Search => "search",
            Family::Similarity => "similarity",
            Family::Navigation => "navigation",
            Family::AnalysisCollection => "analysis",
            Family::Security => "security",
            Family::Memory => "memory",
            Family::Git => "git",
            Family::Other => "other",
        }
    }
}

/// Bucket a tool by family for reporting.
pub fn family_of(tool: &str) -> Family {
    if SEARCH_TOOLS.contains(&tool) {
        return Family::Search;
    }
    if SIMILARITY_TOOLS.contains(&tool) {
        return Family::Similarity;
    }
    if NAVIGATION_TOOLS.contains(&tool) {
        return Family::Navigation;
    }
    if ANALYSIS_COLLECTION_TOOLS.contains(&tool) {
        return Family::AnalysisCollection;
    }
    if SECURITY_TOOLS.contains(&tool) || tool.starts_with("codegraph_security_") {
        return Family::Security;
    }
    if MEMORY_TOOLS.contains(&tool) || tool.starts_with("codegraph_memory_") {
        return Family::Memory;
    }
    if GIT_TOOLS.contains(&tool) {
        return Family::Git;
    }
    Family::Other
}

/// Look up the baseline profile for a tool name. Returns
/// `NormalizeOpts::default()` (all `None`/empty) for tools we don't
/// have an opinion about — those behave exactly like P3.
pub fn default_for(tool: &str) -> NormalizeOpts {
    if SEARCH_TOOLS.contains(&tool) {
        return search_profile();
    }
    if SIMILARITY_TOOLS.contains(&tool) {
        return similarity_profile();
    }
    if ANALYSIS_COLLECTION_TOOLS.contains(&tool) {
        return collection_profile();
    }
    if SECURITY_TOOLS.contains(&tool) || tool.starts_with("codegraph_security_") {
        return security_profile();
    }
    if MEMORY_TOOLS.contains(&tool) || tool.starts_with("codegraph_memory_") {
        return memory_profile();
    }
    if GIT_TOOLS.contains(&tool) {
        return git_profile();
    }
    NormalizeOpts::default()
}

/// Search-family tools return result lists where order is not part of
/// the tool's contract. Sort them canonically so authoring is order-
/// agnostic. Also drop `match_reason: Semantic` elements — these
/// come from the embedding pipeline and arrive non-deterministically
/// depending on warmup timing, so they can't be asserted on without
/// a separate embedding-stable test path. Cases that DO want to
/// verify semantic matching can override `drop_where: []` to keep
/// them.
fn search_profile() -> NormalizeOpts {
    NormalizeOpts {
        sort_arrays: Some(true),
        drop_where: vec![serde_json::json!({"match_reason": "Semantic"})],
        ..Default::default()
    }
}

/// Similarity / clustering tools return cosine scores that drift in
/// the last few digits between ONNX runtime versions. Round to 2
/// decimals AND keep `score`/`similarity` (which the global volatile
/// list would otherwise strip) so cases can assert on them via
/// tolerance bands or rounded equality.
fn similarity_profile() -> NormalizeOpts {
    NormalizeOpts {
        sort_arrays: Some(true),
        float_decimals: Some(2),
        keep_volatile: vec!["score".to_string(), "similarity".to_string()],
        ..Default::default()
    }
}

/// Analysis / context tools that return collections (lists of files,
/// symbols, modules, etc.) where order isn't contractual.
fn collection_profile() -> NormalizeOpts {
    NormalizeOpts {
        sort_arrays: Some(true),
        ..Default::default()
    }
}

/// Security scanners return finding lists; order may shift with
/// detector iteration. Sort and strip detector-side timing
/// artifacts.
fn security_profile() -> NormalizeOpts {
    NormalizeOpts {
        sort_arrays: Some(true),
        extra_volatile: vec![
            "scan_duration_ms".to_string(),
            "detector_duration_ms".to_string(),
        ],
        ..Default::default()
    }
}

/// Memory tools have timestamps and IDs that change every run.
fn memory_profile() -> NormalizeOpts {
    NormalizeOpts {
        sort_arrays: Some(true),
        extra_volatile: vec![
            "id".to_string(),
            "created_at".to_string(),
            "updated_at".to_string(),
            "timestamp".to_string(),
        ],
        ..Default::default()
    }
}

/// Git history tools return commits with hashes / dates that change.
/// Even with `init_git` pinning author + committer dates, response
/// metadata like `queryTime` (wallclock) and `date` strings (which
/// may render in the local timezone) are still volatile. Strip them.
fn git_profile() -> NormalizeOpts {
    NormalizeOpts {
        sort_arrays: Some(true),
        extra_volatile: vec![
            // Hash-shaped fields under various names tools use.
            "sha".to_string(),
            "commit_hash".to_string(),
            "hash".to_string(),
            "fullHash".to_string(),
            // Date-shaped fields under various names.
            "author_date".to_string(),
            "commit_date".to_string(),
            "timestamp".to_string(),
            "date".to_string(),
            // Wallclock metadata.
            "queryTime".to_string(),
        ],
        ..Default::default()
    }
}

const NAVIGATION_TOOLS: &[&str] = &[
    "codegraph_get_callers",
    "codegraph_get_callees",
    "codegraph_get_symbol_info",
    "codegraph_get_detailed_symbol",
];

const SEARCH_TOOLS: &[&str] = &[
    "codegraph_symbol_search",
    "codegraph_search_by_pattern",
    "codegraph_search_by_error",
    "codegraph_cross_project_search",
    "codegraph_find_by_imports",
    "codegraph_find_by_signature",
];

const SIMILARITY_TOOLS: &[&str] = &[
    "codegraph_find_similar",
    "codegraph_find_duplicates",
    "codegraph_cluster_symbols",
    "codegraph_compare_symbols",
];

const ANALYSIS_COLLECTION_TOOLS: &[&str] = &[
    "codegraph_analyze_complexity",
    "codegraph_analyze_coupling",
    "codegraph_analyze_impact",
    "codegraph_find_circular_deps",
    "codegraph_find_dead_imports",
    "codegraph_find_entry_points",
    "codegraph_find_hot_paths",
    "codegraph_find_implementors",
    "codegraph_find_related_tests",
    "codegraph_find_unused_code",
    "codegraph_get_call_graph",
    "codegraph_get_dependency_graph",
    "codegraph_traverse_graph",
    "codegraph_get_ai_context",
    "codegraph_get_curated_context",
    "codegraph_get_edit_context",
    "codegraph_get_module_summary",
];

const SECURITY_TOOLS: &[&str] = &[
    "codegraph_security_scan",
    "codegraph_security_orchestrated_scan",
    "codegraph_security_audit_deps",
    "codegraph_security_export_sarif",
    "codegraph_security_generate_sbom",
    "codegraph_security_scan_iac",
    "codegraph_security_codeql_warmup",
    "codegraph_security_control_flow",
    "codegraph_security_detect_injection",
    "codegraph_security_trace_data_flow",
];

const MEMORY_TOOLS: &[&str] = &[
    "codegraph_memory_store",
    "codegraph_memory_get",
    "codegraph_memory_list",
    "codegraph_memory_search",
    "codegraph_memory_invalidate",
    "codegraph_memory_stats",
    "codegraph_memory_context",
];

const GIT_TOOLS: &[&str] = &[
    "codegraph_mine_git_history",
    "codegraph_mine_git_history_for_file",
    "codegraph_search_git_history",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_tool_inherits_sort_arrays() {
        let p = default_for("codegraph_symbol_search");
        assert_eq!(p.sort_arrays, Some(true));
    }

    #[test]
    fn similarity_tool_inherits_score_keep_and_rounding() {
        let p = default_for("codegraph_find_similar");
        assert_eq!(p.sort_arrays, Some(true));
        assert_eq!(p.float_decimals, Some(2));
        assert!(p.keep_volatile.iter().any(|s| s == "score"));
        assert!(p.keep_volatile.iter().any(|s| s == "similarity"));
    }

    #[test]
    fn navigation_tool_has_no_default_profile() {
        // Single-symbol tools — order is contractual, no profile.
        let p = default_for("codegraph_get_callers");
        assert_eq!(p.sort_arrays, None);
        assert_eq!(p.float_decimals, None);
        assert!(p.extra_volatile.is_empty());
        assert!(p.keep_volatile.is_empty());
    }

    #[test]
    fn unknown_tool_falls_through_to_default() {
        let p = default_for("codegraph_made_up_tool");
        assert_eq!(p.sort_arrays, None);
    }

    #[test]
    fn security_prefix_match_works_for_unlisted() {
        // A security tool not in SECURITY_TOOLS still matches by prefix.
        let p = default_for("codegraph_security_check_jwt_completeness");
        assert_eq!(p.sort_arrays, Some(true));
    }

    #[test]
    fn merge_overlay_wins_for_some() {
        let base = NormalizeOpts {
            sort_arrays: Some(true),
            ..Default::default()
        };
        let overlay = NormalizeOpts {
            sort_arrays: Some(false),
            ..Default::default()
        };
        let merged = NormalizeOpts::merge(base, overlay);
        assert_eq!(merged.sort_arrays, Some(false));
    }

    #[test]
    fn merge_overlay_none_inherits_base() {
        let base = NormalizeOpts {
            sort_arrays: Some(true),
            ..Default::default()
        };
        let overlay = NormalizeOpts::default();
        let merged = NormalizeOpts::merge(base, overlay);
        assert_eq!(merged.sort_arrays, Some(true));
    }

    #[test]
    fn merge_concatenates_vec_fields_dedup() {
        let base = NormalizeOpts {
            extra_volatile: vec!["a".to_string(), "b".to_string()],
            ..Default::default()
        };
        let overlay = NormalizeOpts {
            extra_volatile: vec!["b".to_string(), "c".to_string()],
            ..Default::default()
        };
        let merged = NormalizeOpts::merge(base, overlay);
        assert_eq!(merged.extra_volatile, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn merge_case_extra_volatile_beats_profile_keep_volatile() {
        // Profile says "keep similarity". Case says "strip similarity".
        // The case wins — without this, find_duplicates can't be
        // de-flaked when embedding similarity drifts.
        let base = NormalizeOpts {
            keep_volatile: vec!["similarity".to_string(), "score".to_string()],
            ..Default::default()
        };
        let overlay = NormalizeOpts {
            extra_volatile: vec!["similarity".to_string()],
            ..Default::default()
        };
        let merged = NormalizeOpts::merge(base, overlay);
        assert!(merged.extra_volatile.contains(&"similarity".to_string()));
        // `score` stays kept (profile only); `similarity` is removed.
        assert_eq!(merged.keep_volatile, vec!["score".to_string()]);
    }
}
