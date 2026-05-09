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
        format!("{}...", &s[..77])
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
}
