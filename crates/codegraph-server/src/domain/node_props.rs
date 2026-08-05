// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Typed property accessors for graph nodes.
//!
//! These functions eliminate scattered get_int/get_string fallback chains
//! by providing a single canonical accessor for each property.
//!
use codegraph::{Node, PropertyMap};

// Line accessors (from Node)

/// Get the start line of a node. Tries line_start then start_line. Returns 0 if absent.
pub(crate) fn line_start(node: &Node) -> u32 {
    line_start_from_props(&node.properties)
}

/// Get the end line of a node. Tries line_end then end_line. Returns 0 if absent.
pub(crate) fn line_end(node: &Node) -> u32 {
    line_end_from_props(&node.properties)
}

/// Optional variant — returns None when neither key is present.
#[allow(dead_code)]
pub(crate) fn line_start_opt(node: &Node) -> Option<u32> {
    line_start_opt_from_props(&node.properties)
}

/// Optional variant — returns None when neither key is present.
#[allow(dead_code)]
pub(crate) fn line_end_opt(node: &Node) -> Option<u32> {
    line_end_opt_from_props(&node.properties)
}

// Line accessors (from PropertyMap — for callers without a Node)

pub(crate) fn line_start_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("line_start")
        .or_else(|| props.get_int("start_line"))
        .unwrap_or(0) as u32
}

pub(crate) fn line_end_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("line_end")
        .or_else(|| props.get_int("end_line"))
        .unwrap_or(0) as u32
}

pub(crate) fn line_start_opt_from_props(props: &PropertyMap) -> Option<u32> {
    props
        .get_int("line_start")
        .or_else(|| props.get_int("start_line"))
        .map(|v| v as u32)
}

pub(crate) fn line_end_opt_from_props(props: &PropertyMap) -> Option<u32> {
    props
        .get_int("line_end")
        .or_else(|| props.get_int("end_line"))
        .map(|v| v as u32)
}

pub(crate) fn col_start_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("col_start")
        .or_else(|| props.get_int("start_col"))
        .unwrap_or(0) as u32
}

pub(crate) fn col_end_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("col_end")
        .or_else(|| props.get_int("end_col"))
        .unwrap_or(10000) as u32
}

// String property accessors

/// Get the node name. Returns "" when absent.
pub(crate) fn name(node: &Node) -> &str {
    node.properties.get_string("name").unwrap_or("")
}

/// Get the node file path. Returns "" when absent.
pub(crate) fn path(node: &Node) -> &str {
    node.properties.get_string("path").unwrap_or("")
}

/// Get the node visibility string. Returns "public" when absent.
#[allow(dead_code)]
pub(crate) fn visibility(node: &Node) -> &str {
    node.properties.get_string("visibility").unwrap_or("public")
}

/// Get the node language. Returns "" when absent.
pub(crate) fn language(node: &Node) -> &str {
    node.properties.get_string("language").unwrap_or("")
}

// Boolean property accessors

/// Whether the node is public/exported.
/// Checks is_public, then exported, then falls back to visibility string.
pub(crate) fn is_public(node: &Node) -> bool {
    node.properties
        .get_bool("is_public")
        .or_else(|| node.properties.get_bool("exported"))
        .unwrap_or_else(|| matches!(visibility(node), "public" | "pub"))
}

/// Whether the node is a test function, as recorded at index time from the
/// language's test marker (`#[test]`/`#[cfg(test)]`, `@Test`, etc.). This is
/// the structural signal; callers should prefer it over name heuristics, which
/// miss idiomatic test names (Rust `#[cfg(test)] mod tests { fn descriptive() }`).
pub(crate) fn is_test(node: &Node) -> bool {
    node.properties.get_bool("is_test").unwrap_or(false)
}

/// Whether a caller node looks like test code: the structural [`is_test`]
/// marker, or a name/path heuristic for languages that don't record it. Shared
/// by CodeLens per-symbol stats and PR-review coverage so the two classify
/// callers identically and can't silently diverge.
pub(crate) fn is_test_like(node: &Node) -> bool {
    if is_test(node) {
        return true;
    }
    if is_test_name(name(node)) {
        return true;
    }
    is_test_path(node.properties.get_string("path").unwrap_or(""))
}

/// Name heuristic for test functions: `test_foo` or `foo_test`. Anchored to the
/// ends of the name because a substring match would also claim `run_tests`,
/// `setup_test_env` and other harness helpers, which are production code that
/// happens to drive tests - and dropping those from the lens is a silent loss.
fn is_test_name(name: &str) -> bool {
    let name = name.to_lowercase();
    name.starts_with("test_") || name.ends_with("_test")
}

/// Path heuristic for test files: a `tests` directory component, a `test_`
/// prefix, or a file name following one of the suffix conventions the supported
/// languages use. Rust and Python put the marker in front (`test_foo.py`); Go,
/// JavaScript, Ruby and Java put it behind (`foo_test.go`, `foo.test.ts`,
/// `foo_spec.rb`, `FooTest.java`), and recognising only the prefix classified
/// every Go and Java test as a production caller.
///
/// Separators are normalised first because node paths are stored with the
/// indexing host's native separator, so a Windows `tests\foo.rs` would
/// otherwise never match.
fn is_test_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    if path.starts_with("tests/")
        || path.starts_with("test_")
        || path.contains("/tests/")
        || path.contains("/test_")
    {
        return true;
    }

    let file = path.rsplit('/').next().unwrap_or("");
    // Anchored on the separator before the extension so `contest.rs` and
    // `manifest.go` stay production code.
    if ["_test.", ".test.", "_spec.", ".spec."]
        .iter()
        .any(|marker| file.contains(marker))
    {
        return true;
    }
    // Case-sensitive, and on the stem only: `Test`/`Tests` is the Java and C#
    // convention, while a lowercase match would claim `latest.rs`.
    let stem = file.split('.').next().unwrap_or("");
    stem.ends_with("Test") || stem.ends_with("Tests")
}

#[cfg(test)]
mod tests {
    use super::{is_test_name, is_test_path};

    #[test]
    fn test_name_matches_at_word_boundaries() {
        assert!(is_test_name("test_parses_empty_input"));
        assert!(is_test_name("lower_bound_test"));
        assert!(is_test_name("TEST_Uppercase"));
    }

    #[test]
    fn test_name_rejects_harness_helpers() {
        assert!(!is_test_name("run_tests"));
        assert!(!is_test_name("setup_test_env"));
        assert!(!is_test_name("latest_snapshot"));
    }

    #[test]
    fn test_path_matches_windows_separators() {
        assert!(is_test_path(r"C:\repo\tests\navigation.rs"));
        assert!(is_test_path(r"C:\repo\src\test_helpers.rs"));
        assert!(is_test_path(r"tests\navigation.rs"));
    }

    #[test]
    fn test_path_matches_unix_separators() {
        assert!(is_test_path("/repo/tests/navigation.rs"));
        assert!(is_test_path("/repo/src/test_helpers.rs"));
        assert!(is_test_path("tests/navigation.rs"));
    }

    #[test]
    fn test_path_matches_suffix_conventions() {
        // Go, JavaScript/TypeScript, Ruby and Java all put the marker last.
        assert!(is_test_path("/repo/internal/parser_test.go"));
        assert!(is_test_path("/repo/src/parser.test.ts"));
        assert!(is_test_path("/repo/spec/models/user_spec.rb"));
        assert!(is_test_path("/repo/src/parser.spec.js"));
        assert!(is_test_path("/repo/src/main/java/com/x/ParserTest.java"));
        assert!(is_test_path(r"C:\repo\src\ParserTests.cs"));
    }

    #[test]
    fn test_path_rejects_production_paths() {
        assert!(!is_test_path("/repo/src/latest/mod.rs"));
        assert!(!is_test_path(r"C:\repo\src\contest.rs"));
        // Ends in "test" only in lowercase, which is not a suffix convention.
        assert!(!is_test_path("/repo/src/manifest.go"));
        assert!(!is_test_path("/repo/src/latest.rs"));
        assert!(!is_test_path("/repo/src/protest.java"));
    }
}
