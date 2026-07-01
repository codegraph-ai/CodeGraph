// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bless mode: rewrite a case YAML's `expect.data` block with the
//! captured actual response. Two workflows share the same code path:
//!
//! - **Record** — author writes a stub case YAML with empty `data: {}`
//!   plus `id`/`setup`/`invoke`/`expect.match`. Runs `--bless --filter
//!   <id>` to populate `expect.data` from the live tool response.
//! - **Re-bless** — existing case fails because the tool's output
//!   intentionally changed. Author runs `--bless --filter <pattern>`
//!   to overwrite expected with current actual.
//!
//! YAML rewrite uses serde_yaml round-trip — comments and blank lines
//! are NOT preserved. Authors should keep their stub YAMLs minimal
//! (id, description, setup, invoke, expect.match) and rely on the
//! bless step to fill in the bulk of the file. Comments can be
//! re-added afterwards.

use anyhow::{anyhow, Context, Result};
use serde_json::Value as Json;
use serde_yaml::Value as Yaml;
use std::path::Path;

/// Rewrite the case file at `path`, replacing `expect.data` with
/// `new_data` (a JSON value, converted to YAML). Other fields in the
/// YAML are preserved structurally — keys keep their order best-
/// effort, but comments are lost.
pub fn rewrite_expect_data(path: &Path, new_data: &Json) -> Result<()> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let mut doc: Yaml = serde_yaml::from_str(&raw)
        .with_context(|| format!("parse {} as YAML", path.display()))?;

    let expect = doc
        .as_mapping_mut()
        .and_then(|m| m.get_mut(Yaml::String("expect".into())))
        .and_then(|v| v.as_mapping_mut())
        .ok_or_else(|| anyhow!("{}: top-level `expect:` mapping not found", path.display()))?;

    let new_yaml = json_to_yaml(new_data);
    expect.insert(Yaml::String("data".into()), new_yaml);

    let serialised = serde_yaml::to_string(&doc)
        .with_context(|| format!("serialise {} after rewrite", path.display()))?;
    std::fs::write(path, serialised)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Convert a `serde_json::Value` to `serde_yaml::Value`. Round-trips
/// via the YAML deserialiser to keep number representations stable
/// (e.g. integers stay integers, not f64-tagged 5.0).
fn json_to_yaml(v: &Json) -> Yaml {
    match v {
        Json::Null => Yaml::Null,
        Json::Bool(b) => Yaml::Bool(*b),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Yaml::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                Yaml::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                Yaml::Number(f.into())
            } else {
                Yaml::String(n.to_string())
            }
        }
        Json::String(s) => Yaml::String(s.clone()),
        Json::Array(arr) => Yaml::Sequence(arr.iter().map(json_to_yaml).collect()),
        Json::Object(map) => Yaml::Mapping(
            map.iter()
                .map(|(k, v)| (Yaml::String(k.clone()), json_to_yaml(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rewrites_expect_data_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foo.case.yml");
        std::fs::write(
            &path,
            "id: t.1\nsetup:\n  fixture: x\ninvoke:\n  tool: t\n  args: {}\nexpect:\n  match: exact\n  data: {}\n",
        )
        .unwrap();

        let new_data = json!({"results": [{"name": "a"}]});
        rewrite_expect_data(&path, &new_data).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        // Round-trip the file through serde_yaml so we can assert on
        // the structured content rather than literal text formatting.
        let parsed: serde_yaml::Value = serde_yaml::from_str(&after).unwrap();
        let data = parsed
            .get("expect")
            .unwrap()
            .get("data")
            .unwrap();
        let expected_yaml: Yaml = serde_yaml::from_str(
            "results:\n  - name: a\n",
        )
        .unwrap();
        assert_eq!(*data, expected_yaml);
    }

    #[test]
    fn errors_on_missing_expect_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.case.yml");
        std::fs::write(&path, "id: t.1\n").unwrap();
        let err = rewrite_expect_data(&path, &json!({})).unwrap_err();
        assert!(err.to_string().contains("expect:"));
    }

    #[test]
    fn errors_when_expect_is_not_a_mapping() {
        // `expect` is present but a scalar, so the `.as_mapping_mut()` arm
        // returns None — a distinct branch from the missing-key case above.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scalar_expect.case.yml");
        std::fs::write(&path, "id: t.1\nexpect: exact\n").unwrap();
        let err = rewrite_expect_data(&path, &json!({})).unwrap_err();
        assert!(err.to_string().contains("expect:"));
    }

    #[test]
    fn json_to_yaml_preserves_scalar_kinds() {
        // Null and Bool arms — neither reached by the object/array happy path.
        assert_eq!(json_to_yaml(&json!(null)), Yaml::Null);
        assert_eq!(json_to_yaml(&json!(true)), Yaml::Bool(true));

        // Integer stays an integer (documented: not f64-tagged 5.0) — the i64 arm.
        match json_to_yaml(&json!(5)) {
            Yaml::Number(ref n) => {
                assert!(n.is_i64());
                assert_eq!(n.as_i64(), Some(5));
            }
            other => panic!("expected integer number, got {other:?}"),
        }

        // A value beyond i64::MAX exercises the u64 arm.
        match json_to_yaml(&json!(u64::MAX)) {
            Yaml::Number(ref n) => assert_eq!(n.as_u64(), Some(u64::MAX)),
            other => panic!("expected u64 number, got {other:?}"),
        }

        // Fractional value stays a float — the f64 arm.
        match json_to_yaml(&json!(2.5)) {
            Yaml::Number(ref n) => {
                assert!(n.is_f64());
                assert_eq!(n.as_f64(), Some(2.5));
            }
            other => panic!("expected float number, got {other:?}"),
        }
    }

    #[test]
    fn rewrite_preserves_integer_numbers_end_to_end() {
        // End-to-end guard on the number-stability contract: an integer in the
        // blessed data must round-trip through the YAML rewrite as an integer.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nums.case.yml");
        std::fs::write(&path, "id: t.1\nexpect:\n  match: exact\n  data: {}\n").unwrap();

        rewrite_expect_data(&path, &json!({"count": 3})).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        let parsed: Yaml = serde_yaml::from_str(&after).unwrap();
        let count = parsed
            .get("expect")
            .unwrap()
            .get("data")
            .unwrap()
            .get("count")
            .unwrap();
        match count {
            Yaml::Number(n) => {
                assert!(n.is_i64());
                assert_eq!(n.as_i64(), Some(3));
            }
            other => panic!("expected integer count, got {other:?}"),
        }
    }
}
