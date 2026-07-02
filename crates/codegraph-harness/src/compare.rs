// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Comparison logic. Supports four match modes and an inline tolerance
//! sentinel for floating-point fields.
//!
//! Match modes:
//! - `exact` — full deep-equal after normalisation.
//! - `structural` — every key in expected must exist in actual with a
//!   structural-matching value; extra keys in actual are tolerated.
//!   Arrays must be the same length, element-wise structural match.
//! - `contains` — same as structural for objects; arrays in actual may
//!   be longer than expected as long as every expected element has a
//!   structural-equivalent match somewhere in actual.
//! - `count_only` — every key in expected must exist in actual but
//!   values are not compared; arrays must have matching lengths.
//!
//! Tolerance sentinel:
//! ```yaml
//! score:
//!   __tol__: { value: 0.78, tol: 0.05 }
//! ```
//! When `expected` at any leaf is an object with a single `__tol__`
//! key, the comparison treats it as a numeric range assertion against
//! `actual`. Works in every match mode.

use crate::case::MatchMode;
use anyhow::Result;
use serde_json::Value;

const TOL_SENTINEL: &str = "__tol__";

pub struct Comparison {
    pub passed: bool,
    /// Human-readable diff describing the mismatch. Empty on success.
    pub diff: String,
}

/// Substitute `${fixture}` and `${workspace}` placeholders in a JSON
/// value tree (recurses into arrays + objects). Returns a new tree.
pub fn substitute_placeholders(value: &Value, fixture: &str, workspace: &str) -> Value {
    match value {
        Value::String(s) => Value::String(
            s.replace("${fixture}", fixture)
                .replace("${workspace}", workspace),
        ),
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| substitute_placeholders(v, fixture, workspace))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute_placeholders(v, fixture, workspace)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn compare(actual: &Value, expected: &Value, mode: MatchMode) -> Result<Comparison> {
    let mut path = String::from("$");
    let mut errors = Vec::new();
    walk(&mut path, actual, expected, mode, &mut errors);
    if errors.is_empty() {
        Ok(Comparison { passed: true, diff: String::new() })
    } else {
        let diff = format_diff(&errors, actual, expected, mode);
        Ok(Comparison { passed: false, diff })
    }
}

fn walk(path: &mut String, actual: &Value, expected: &Value, mode: MatchMode, errors: &mut Vec<String>) {
    // Tolerance sentinel: `expected` is `{ "__tol__": { value, tol } }`.
    // Recognised at every depth, every match mode.
    if let Some(band) = parse_tol(expected) {
        match actual.as_f64() {
            Some(a) if (a - band.value).abs() <= band.tol => {}
            Some(a) => errors.push(format!(
                "{}: expected {} ± {} but got {}",
                path, band.value, band.tol, a,
            )),
            None => errors.push(format!(
                "{}: expected number ({} ± {}) but got {}",
                path,
                band.value,
                band.tol,
                short_kind(actual),
            )),
        }
        return;
    }

    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            for (k, ev) in e {
                let len = path.len();
                path.push('.');
                path.push_str(k);
                match a.get(k) {
                    None => errors.push(format!("{}: missing key", path)),
                    Some(av) => {
                        if mode == MatchMode::CountOnly {
                            walk(path, av, ev, mode, errors);
                        } else {
                            walk(path, av, ev, mode, errors);
                        }
                    }
                }
                path.truncate(len);
            }
            // Exact mode also flags extra keys in actual.
            if mode == MatchMode::Exact {
                for k in a.keys() {
                    if !e.contains_key(k) {
                        errors.push(format!("{}.{}: unexpected key in actual", path, k));
                    }
                }
            }
        }
        (Value::Array(a), Value::Array(e)) => match mode {
            MatchMode::Exact | MatchMode::Structural | MatchMode::CountOnly => {
                if a.len() != e.len() {
                    errors.push(format!(
                        "{}: array length {} != {}",
                        path,
                        a.len(),
                        e.len()
                    ));
                    return;
                }
                if mode == MatchMode::CountOnly {
                    return; // values not compared
                }
                for (i, (av, ev)) in a.iter().zip(e.iter()).enumerate() {
                    let len = path.len();
                    path.push_str(&format!("[{}]", i));
                    walk(path, av, ev, mode, errors);
                    path.truncate(len);
                }
            }
            MatchMode::Contains => {
                // Each expected element must structurally-match SOME
                // element in actual. Order doesn't matter.
                for (i, ev) in e.iter().enumerate() {
                    let mut matched = false;
                    for av in a.iter() {
                        let mut probe = String::new();
                        let mut probe_errors = Vec::new();
                        walk(&mut probe, av, ev, MatchMode::Structural, &mut probe_errors);
                        if probe_errors.is_empty() {
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        errors.push(format!(
                            "{}[{}]: no element in actual structurally-matches",
                            path, i,
                        ));
                    }
                }
            }
        },
        (a, e) => match mode {
            MatchMode::CountOnly => {} // scalars ignored under count_only
            _ => {
                if a != e {
                    errors.push(format!(
                        "{}: expected {} but got {}",
                        path,
                        truncate_for_diff(e),
                        truncate_for_diff(a),
                    ));
                }
            }
        },
    }
}

struct TolBand {
    value: f64,
    tol: f64,
}

fn parse_tol(v: &Value) -> Option<TolBand> {
    let obj = v.as_object()?;
    if obj.len() != 1 {
        return None;
    }
    let inner = obj.get(TOL_SENTINEL)?.as_object()?;
    let value = inner.get("value")?.as_f64()?;
    let tol = inner.get("tol")?.as_f64()?;
    Some(TolBand { value, tol })
}

fn short_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn truncate_for_diff(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 80 {
        // UTF-8-safe truncation — raw `&s[..77]` panics when a
        // multi-byte char straddles the boundary (GitHub issue #3
        // class). Inlined here to keep harness dependency-light;
        // codegraph-parser-api ships the canonical helper.
        let mut end = 77;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    } else {
        s
    }
}

fn format_diff(errors: &[String], actual: &Value, expected: &Value, mode: MatchMode) -> String {
    let mut out = format!("match mode: {:?}\n", mode);
    out.push_str("--- mismatches\n");
    for e in errors {
        out.push_str("  ");
        out.push_str(e);
        out.push('\n');
    }
    if mode == MatchMode::Exact {
        let actual_pretty = serde_json::to_string_pretty(actual).unwrap_or_default();
        let expected_pretty = serde_json::to_string_pretty(expected).unwrap_or_default();
        out.push_str("\n--- expected\n");
        out.push_str(&prefix_lines(&expected_pretty, "  "));
        out.push_str("\n+++ actual\n");
        out.push_str(&prefix_lines(&actual_pretty, "  "));
    }
    out
}

fn prefix_lines(s: &str, prefix: &str) -> String {
    s.lines().map(|l| format!("{}{}", prefix, l)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_passes_on_equal() {
        let r = compare(&json!({"a": 1}), &json!({"a": 1}), MatchMode::Exact).unwrap();
        assert!(r.passed, "diff: {}", r.diff);
    }

    #[test]
    fn exact_flags_extra_actual_key() {
        let r = compare(
            &json!({"a": 1, "b": 2}),
            &json!({"a": 1}),
            MatchMode::Exact,
        )
        .unwrap();
        assert!(!r.passed);
        assert!(r.diff.contains("unexpected key"));
    }

    #[test]
    fn structural_tolerates_extra_actual_key() {
        let r = compare(
            &json!({"a": 1, "b": 2}),
            &json!({"a": 1}),
            MatchMode::Structural,
        )
        .unwrap();
        assert!(r.passed, "diff: {}", r.diff);
    }

    #[test]
    fn structural_flags_missing_expected_key() {
        let r = compare(
            &json!({"a": 1}),
            &json!({"a": 1, "b": 2}),
            MatchMode::Structural,
        )
        .unwrap();
        assert!(!r.passed);
        assert!(r.diff.contains("missing key"));
    }

    #[test]
    fn contains_tolerates_extra_actual_array_elements() {
        let r = compare(
            &json!([{"name": "a"}, {"name": "b"}, {"name": "c"}]),
            &json!([{"name": "b"}]),
            MatchMode::Contains,
        )
        .unwrap();
        assert!(r.passed, "diff: {}", r.diff);
    }

    #[test]
    fn contains_flags_unmatched_expected_element() {
        let r = compare(
            &json!([{"name": "a"}, {"name": "b"}]),
            &json!([{"name": "z"}]),
            MatchMode::Contains,
        )
        .unwrap();
        assert!(!r.passed);
        assert!(r.diff.contains("structurally-matches"));
    }

    #[test]
    fn count_only_checks_array_lengths_only() {
        let r = compare(
            &json!({"results": [{"x": 1}, {"x": 2}, {"x": 3}]}),
            &json!({"results": [null, null, null]}),
            MatchMode::CountOnly,
        )
        .unwrap();
        assert!(r.passed, "diff: {}", r.diff);
    }

    #[test]
    fn count_only_flags_array_length_mismatch() {
        let r = compare(
            &json!({"results": [{"x": 1}]}),
            &json!({"results": [null, null]}),
            MatchMode::CountOnly,
        )
        .unwrap();
        assert!(!r.passed);
        assert!(r.diff.contains("array length"));
    }

    #[test]
    fn tolerance_band_passes_within_range() {
        let r = compare(
            &json!({"score": 0.79}),
            &json!({"score": {"__tol__": {"value": 0.78, "tol": 0.05}}}),
            MatchMode::Exact,
        )
        .unwrap();
        assert!(r.passed, "diff: {}", r.diff);
    }

    #[test]
    fn tolerance_band_fails_outside_range() {
        let r = compare(
            &json!({"score": 0.50}),
            &json!({"score": {"__tol__": {"value": 0.78, "tol": 0.05}}}),
            MatchMode::Exact,
        )
        .unwrap();
        assert!(!r.passed);
        assert!(r.diff.contains("0.78"));
        assert!(r.diff.contains("0.05"));
    }

    #[test]
    fn tolerance_band_fails_when_actual_not_a_number() {
        let r = compare(
            &json!({"score": "high"}),
            &json!({"score": {"__tol__": {"value": 0.78, "tol": 0.05}}}),
            MatchMode::Exact,
        )
        .unwrap();
        assert!(!r.passed);
        assert!(r.diff.contains("string"));
    }

    #[test]
    fn substitute_placeholders_replaces_in_nested_structures() {
        // Exercises all four arms of substitute_placeholders: String (both
        // ${fixture} and ${workspace} tokens), Array recursion, Object
        // recursion, and the `other` passthrough for non-string leaves.
        let input = json!({
            "path": "${workspace}/src/${fixture}.rs",
            "list": ["${fixture}", "plain", 7],
            "count": 3,
            "flag": true,
            "nothing": null,
            "nested": { "file": "${workspace}/a" }
        });
        let out = substitute_placeholders(&input, "myfix", "/ws");
        assert_eq!(out["path"], json!("/ws/src/myfix.rs"));
        assert_eq!(out["list"][0], json!("myfix"));
        assert_eq!(out["list"][1], json!("plain"));
        // Non-string leaves pass through unchanged (the `other` arm).
        assert_eq!(out["list"][2], json!(7));
        assert_eq!(out["count"], json!(3));
        assert_eq!(out["flag"], json!(true));
        assert_eq!(out["nothing"], json!(null));
        assert_eq!(out["nested"]["file"], json!("/ws/a"));
    }

    #[test]
    fn parse_tol_accepts_well_formed_sentinel_object() {
        let band = parse_tol(&json!({TOL_SENTINEL: {"value": 10.0, "tol": 0.5}}))
            .expect("well-formed sentinel should parse");
        assert_eq!(band.value, 10.0);
        assert_eq!(band.tol, 0.5);
    }

    #[test]
    fn parse_tol_rejects_non_object_and_wrong_arity() {
        // Not an object at all.
        assert!(parse_tol(&json!(5)).is_none());
        assert!(parse_tol(&json!("x")).is_none());
        // Object but not a single key.
        assert!(parse_tol(&json!({})).is_none());
        assert!(parse_tol(&json!({TOL_SENTINEL: {"value": 1.0, "tol": 0.1}, "extra": 1})).is_none());
    }

    #[test]
    fn parse_tol_rejects_bad_sentinel_and_missing_fields() {
        // Single key but not the sentinel.
        assert!(parse_tol(&json!({"other": {"value": 1.0, "tol": 0.1}})).is_none());
        // Sentinel value is not an object.
        assert!(parse_tol(&json!({TOL_SENTINEL: 3})).is_none());
        // Missing / non-numeric value.
        assert!(parse_tol(&json!({TOL_SENTINEL: {"tol": 0.1}})).is_none());
        assert!(parse_tol(&json!({TOL_SENTINEL: {"value": "x", "tol": 0.1}})).is_none());
        // Missing / non-numeric tol.
        assert!(parse_tol(&json!({TOL_SENTINEL: {"value": 1.0}})).is_none());
        assert!(parse_tol(&json!({TOL_SENTINEL: {"value": 1.0, "tol": "x"}})).is_none());
    }

    #[test]
    fn short_kind_names_every_json_variant() {
        assert_eq!(short_kind(&json!(null)), "null");
        assert_eq!(short_kind(&json!(true)), "bool");
        assert_eq!(short_kind(&json!(42)), "number");
        assert_eq!(short_kind(&json!("s")), "string");
        assert_eq!(short_kind(&json!([1, 2])), "array");
        assert_eq!(short_kind(&json!({"a": 1})), "object");
    }

    #[test]
    fn diff_truncates_long_multibyte_scalar_without_panicking() {
        // 40 euro signs -> ~122 bytes as a JSON string, over the 80-byte
        // truncate_for_diff threshold. Byte 77 lands mid-char, so the
        // char-boundary backoff loop must fire (a raw &s[..77] would panic).
        let long_a = "€".repeat(40);
        let long_b = "£".repeat(40);
        let r = compare(&json!(long_a), &json!(long_b), MatchMode::Exact).unwrap();
        assert!(!r.passed);
        // The truncation marker proves the >80-byte branch ran.
        assert!(r.diff.contains("..."), "diff: {}", r.diff);
    }

    #[test]
    fn truncate_for_diff_passes_short_values_through() {
        // A short scalar stays under the 80-byte threshold, so the else
        // branch returns the JSON rendering verbatim (quotes included).
        assert_eq!(truncate_for_diff(&json!("hello")), "\"hello\"");
        assert_eq!(truncate_for_diff(&json!(42)), "42");
    }

    #[test]
    fn truncate_for_diff_cuts_ascii_at_77_bytes() {
        // A 100-char string renders to 102 bytes with quotes, over 80.
        // Every byte is a char boundary, so the slice is taken at 77:
        // the opening quote plus 76 'a's, then the "..." marker.
        let v = json!("a".repeat(100));
        let expected = format!("\"{}...", "a".repeat(76));
        assert_eq!(truncate_for_diff(&v), expected);
    }

    #[test]
    fn truncate_for_diff_backs_off_to_char_boundary() {
        // 40 euro signs -> 122 bytes with quotes. Byte 77 lands mid-char,
        // so the boundary loop walks back to 76: quote + 25 full euros.
        let v = json!("€".repeat(40));
        let expected = format!("\"{}...", "€".repeat(25));
        assert_eq!(truncate_for_diff(&v), expected);
    }

    #[test]
    fn prefix_lines_prefixes_every_line() {
        assert_eq!(prefix_lines("a\nb\nc", "> "), "> a\n> b\n> c");
        // A single line with no newline still gets the prefix.
        assert_eq!(prefix_lines("solo", "# "), "# solo");
    }

    #[test]
    fn format_diff_includes_value_blocks_only_in_exact_mode() {
        let errors = vec!["root.a: mismatch".to_string()];
        let actual = json!({"a": 2});
        let expected = json!({"a": 1});

        let exact = format_diff(&errors, &actual, &expected, MatchMode::Exact);
        assert!(exact.contains("match mode: Exact"));
        assert!(exact.contains("root.a: mismatch"));
        assert!(exact.contains("--- expected"));
        assert!(exact.contains("+++ actual"));

        // Non-exact modes list mismatches but omit the expected/actual dumps.
        let structural = format_diff(&errors, &actual, &expected, MatchMode::Structural);
        assert!(structural.contains("root.a: mismatch"));
        assert!(!structural.contains("--- expected"));
        assert!(!structural.contains("+++ actual"));
    }
}
