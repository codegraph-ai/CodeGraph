// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Normalisation pipeline. Strips volatile fields, substitutes
//! workspace/fixture paths with stable placeholders, folds Windows
//! backslashes to forward slashes, and (opt-in per case) canonical-
//! sorts arrays-of-objects and rounds floats. Applied to BOTH actual
//! response and expected JSON so authoring is forgiving.

use crate::case::NormalizeOpts;
use serde_json::Value;

/// Fields that are non-deterministic across runs and don't carry
/// signal for regression testing. Stripped recursively from the
/// response before comparison.
///
/// This list is intentionally conservative — fields are stripped
/// anywhere they appear, even nested in unrelated tools' output.
/// Adding a field here is a safe operation; removing one risks
/// breaking established expectations.
const VOLATILE_FIELDS: &[&str] = &[
    // Identifier non-determinism — graph node ids are RocksDB-keyed
    // and shift with index order, but symbols are still uniquely
    // identifiable by (name, file, line) which IS in the response.
    "node_id",
    "node_id_a",
    "node_id_b",
    // Wallclock — every run differs.
    "query_time_ms",
    // Embedding-dependent — model + ONNX runtime drift.
    "score",
    "similarity",
    // Transient state while the embedding pass is still warming up.
    "embedding_status",
    // Counters that depend on indexing state and don't carry
    // tool-correctness signal.
    "scanned_files",
    "total_matches",
    "total_symbols_compared",
    "hits_pre_gate",
    "join_sites_examined",
    "handlers_examined",
    "verifier_sites",
    "default_arg_hits_pre_gate",
    "function_observations",
    "paired_groups",
    "files_with_fd_holder",
    "files_with_path_helper",
    // Reachability counts — depend on entry-point detection which is
    // workspace-shape sensitive.
    "entry_points",
    "reachable_from_request",
    "unreachable_from_request",
    "unknown",
];

/// Run the full pipeline. `workspace_path` and `fixture_path` are
/// substituted with `${workspace}` / `${fixture}` placeholders so
/// expected JSON can be path-agnostic. Other passes are gated on
/// `opts`. Pass an empty `workspace_path` / `fixture_path` (or call
/// with the same value as expected) to use this on the expected side
/// where the substitution is a no-op.
pub fn normalize(
    value: &Value,
    workspace_path: &str,
    fixture_path: &str,
    opts: &NormalizeOpts,
) -> Value {
    let mut v = strip_volatile(value, &opts.extra_volatile, &opts.keep_volatile);
    if !workspace_path.is_empty() || !fixture_path.is_empty() {
        v = substitute_paths(&v, workspace_path, fixture_path);
    }
    v = fold_backslashes(&v);
    if !opts.drop_where.is_empty() {
        v = drop_array_elements_matching(&v, &opts.drop_where);
    }
    if let Some(decimals) = opts.float_decimals {
        v = round_floats(&v, decimals);
    }
    if opts.sort_arrays_on() {
        v = canonical_sort(&v);
    }
    v
}

/// Run the subset of the pipeline that's safe to apply to the
/// expected JSON: volatile field strip (in case the case author
/// pasted a raw response with node_ids), backslash fold, optional
/// float rounding, optional sort. NO path substitution — expected
/// already uses `${fixture}` / `${workspace}` placeholders verbatim.
pub fn normalize_expected(value: &Value, opts: &NormalizeOpts) -> Value {
    let mut v = strip_volatile(value, &opts.extra_volatile, &opts.keep_volatile);
    v = fold_backslashes(&v);
    if !opts.drop_where.is_empty() {
        v = drop_array_elements_matching(&v, &opts.drop_where);
    }
    if let Some(decimals) = opts.float_decimals {
        v = round_floats(&v, decimals);
    }
    if opts.sort_arrays_on() {
        v = canonical_sort(&v);
    }
    v
}

fn strip_volatile(value: &Value, extra: &[String], keep: &[String]) -> Value {
    match value {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let is_global = VOLATILE_FIELDS.iter().any(|f| *f == k.as_str());
                let is_extra = extra.iter().any(|f| f == k);
                let is_kept = keep.iter().any(|f| f == k);
                if (is_global || is_extra) && !is_kept {
                    continue;
                }
                new_map.insert(k.clone(), strip_volatile(v, extra, keep));
            }
            Value::Object(new_map)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| strip_volatile(v, extra, keep))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn substitute_paths(value: &Value, workspace_path: &str, fixture_path: &str) -> Value {
    match value {
        Value::String(s) => {
            // Substitute fixture path FIRST so the more-specific match
            // wins over the workspace-path prefix replacement.
            let mut replaced = s.clone();
            if !fixture_path.is_empty() {
                replaced = replaced.replace(fixture_path, "${fixture}");
            }
            if !workspace_path.is_empty() {
                replaced = replaced.replace(workspace_path, "${workspace}");
            }
            Value::String(replaced)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| substitute_paths(v, workspace_path, fixture_path))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute_paths(v, workspace_path, fixture_path)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Replace `\` with `/` in every string. Cheap insurance for cross-OS
/// runs — a Windows tempdir like `C:\Users\...` becomes consistent
/// with the Unix `${workspace}/...` form.
fn fold_backslashes(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            if s.contains('\\') {
                Value::String(s.replace('\\', "/"))
            } else {
                Value::String(s.clone())
            }
        }
        Value::Array(arr) => Value::Array(arr.iter().map(fold_backslashes).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), fold_backslashes(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Round every float in the tree to `decimals` places. Uses
/// `serde_json::Number::from_f64` for the round-trip; falls back to
/// the original value if the rounded number isn't representable
/// (e.g. NaN, ±inf — which serde_json refuses anyway).
fn round_floats(value: &Value, decimals: u8) -> Value {
    let scale = 10f64.powi(decimals as i32);
    match value {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if n.is_f64() {
                    let rounded = (f * scale).round() / scale;
                    if let Some(num) = serde_json::Number::from_f64(rounded) {
                        return Value::Number(num);
                    }
                }
            }
            value.clone()
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| round_floats(v, decimals)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), round_floats(v, decimals)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Recursively drop array elements that match any of the supplied
/// patterns. An element matches a pattern if every `(key, value)` in
/// the pattern is present and equal in the element. Used to strip
/// non-deterministic result categories — e.g. `match_reason: Semantic`
/// in symbol_search results, which depend on embedding warmup timing.
fn drop_array_elements_matching(value: &Value, patterns: &[Value]) -> Value {
    match value {
        Value::Array(arr) => Value::Array(
            arr.iter()
                .filter(|v| !patterns.iter().any(|p| element_matches(v, p)))
                .map(|v| drop_array_elements_matching(v, patterns))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), drop_array_elements_matching(v, patterns)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn element_matches(element: &Value, pattern: &Value) -> bool {
    match (element, pattern) {
        (Value::Object(e), Value::Object(p)) => p.iter().all(|(k, pv)| match e.get(k) {
            Some(ev) => ev == pv,
            None => false,
        }),
        _ => false,
    }
}

/// Recursively sort every array of values by the canonical JSON
/// string of each element. This guarantees a stable order without
/// needing per-tool sort keys — both actual and expected go through
/// the same pass, so they line up. Sledgehammer, but the harness
/// only runs in CI/dev, never in hot paths.
fn canonical_sort(value: &Value) -> Value {
    match value {
        Value::Array(arr) => {
            let mut sorted: Vec<Value> = arr.iter().map(canonical_sort).collect();
            sorted.sort_by_key(|v| v.to_string());
            Value::Array(sorted)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), canonical_sort(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_volatile_removes_node_id() {
        let input = json!({"name": "foo", "node_id": 42, "score": 0.9});
        let out = strip_volatile(&input, &[], &[]);
        assert_eq!(out, json!({"name": "foo"}));
    }

    #[test]
    fn strip_volatile_recurses_into_arrays() {
        let input = json!({
            "results": [
                {"name": "a", "node_id": 1},
                {"name": "b", "node_id": 2}
            ]
        });
        let out = strip_volatile(&input, &[], &[]);
        assert_eq!(out, json!({"results": [{"name": "a"}, {"name": "b"}]}));
    }

    #[test]
    fn strip_volatile_honors_extra_list() {
        let input = json!({"name": "foo", "trace_id": "abc", "node_id": 1});
        let out = strip_volatile(&input, &["trace_id".to_string()], &[]);
        assert_eq!(out, json!({"name": "foo"}));
    }

    #[test]
    fn strip_volatile_keep_overrides_global() {
        // `score` is global-volatile but case asks to keep it (e.g. for
        // tolerance-band assertion). Should survive the strip pass.
        let input = json!({"name": "foo", "score": 0.78, "node_id": 1});
        let out = strip_volatile(&input, &[], &["score".to_string()]);
        assert_eq!(out, json!({"name": "foo", "score": 0.78}));
    }

    #[test]
    fn strip_volatile_keep_overrides_extra() {
        let input = json!({"name": "foo", "trace_id": "abc"});
        let out = strip_volatile(
            &input,
            &["trace_id".to_string()],
            &["trace_id".to_string()],
        );
        assert_eq!(out, json!({"name": "foo", "trace_id": "abc"}));
    }

    #[test]
    fn substitute_paths_replaces_tempdir_prefix() {
        let input = json!({"file": "/tmp/codegraph-XXX/basic.rs"});
        let out = substitute_paths(&input, "/tmp/codegraph-XXX", "/tmp/codegraph-XXX/basic.rs");
        assert_eq!(out, json!({"file": "${fixture}"}));
    }

    #[test]
    fn substitute_paths_falls_back_to_workspace_when_no_fixture_match() {
        let input = json!({"file": "/tmp/codegraph-XXX/other.rs"});
        let out = substitute_paths(&input, "/tmp/codegraph-XXX", "/tmp/codegraph-XXX/basic.rs");
        assert_eq!(out, json!({"file": "${workspace}/other.rs"}));
    }

    #[test]
    fn fold_backslashes_converts_windows_paths() {
        let input = json!({"file": "C:\\Users\\test\\basic.rs"});
        let out = fold_backslashes(&input);
        assert_eq!(out, json!({"file": "C:/Users/test/basic.rs"}));
    }

    #[test]
    fn round_floats_rounds_to_two_decimals() {
        let input = json!({"score": 0.78234567, "count": 5});
        let out = round_floats(&input, 2);
        assert_eq!(out, json!({"score": 0.78, "count": 5}));
    }

    #[test]
    fn canonical_sort_orders_arrays_stably() {
        let input = json!([
            {"name": "b", "id": 2},
            {"name": "a", "id": 1}
        ]);
        let out = canonical_sort(&input);
        // Sorted by JSON string — `{"id":1,"name":"a"}` < `{"id":2,"name":"b"}`.
        assert_eq!(out, json!([
            {"name": "a", "id": 1},
            {"name": "b", "id": 2}
        ]));
    }

    #[test]
    fn drop_where_removes_matching_array_elements() {
        let input = json!({
            "results": [
                {"name": "a", "match_reason": "SymbolName"},
                {"name": "b", "match_reason": "Semantic"},
                {"name": "c", "match_reason": "SymbolName"},
                {"name": "d", "match_reason": "Semantic"}
            ]
        });
        let patterns = vec![json!({"match_reason": "Semantic"})];
        let out = drop_array_elements_matching(&input, &patterns);
        assert_eq!(
            out,
            json!({
                "results": [
                    {"name": "a", "match_reason": "SymbolName"},
                    {"name": "c", "match_reason": "SymbolName"}
                ]
            })
        );
    }

    #[test]
    fn drop_where_recurses_into_nested_arrays() {
        let input = json!({
            "outer": [{
                "inner": [
                    {"kind": "x"},
                    {"kind": "y"}
                ]
            }]
        });
        let patterns = vec![json!({"kind": "y"})];
        let out = drop_array_elements_matching(&input, &patterns);
        assert_eq!(
            out,
            json!({"outer": [{"inner": [{"kind": "x"}]}]})
        );
    }

    #[test]
    fn full_pipeline_with_opts() {
        // `score` is in VOLATILE_FIELDS — use a non-volatile float
        // (`weight`) so we can observe rounding + sort interaction.
        let input = json!({
            "results": [
                {"name": "b", "weight": 0.78234, "node_id": 1},
                {"name": "a", "weight": 0.91000, "node_id": 2}
            ],
            "query_time_ms": 42
        });
        let opts = NormalizeOpts {
            sort_arrays: Some(true),
            float_decimals: Some(2),
            extra_volatile: vec![],
            keep_volatile: vec![],
            embedding_model: None,
            drop_where: vec![],
        };
        let out = normalize(&input, "", "", &opts);
        assert_eq!(out, json!({
            "results": [
                {"name": "a", "weight": 0.91},
                {"name": "b", "weight": 0.78}
            ]
        }));
    }
}
