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

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{Node, NodeType};

    fn node(props: PropertyMap) -> Node {
        Node::new(0, NodeType::Function, props)
    }

    fn props(pairs: &[(&str, i64)]) -> PropertyMap {
        let mut p = PropertyMap::new();
        for (k, v) in pairs {
            p.insert(*k, *v);
        }
        p
    }

    #[test]
    fn line_start_prefers_line_start_then_start_line_then_zero() {
        // line_start wins over start_line when both present.
        assert_eq!(
            line_start(&node(props(&[("line_start", 5), ("start_line", 9)]))),
            5
        );
        // falls back to start_line when line_start absent.
        assert_eq!(line_start(&node(props(&[("start_line", 9)]))), 9);
        // absent -> 0.
        assert_eq!(line_start(&node(props(&[]))), 0);
    }

    #[test]
    fn line_end_prefers_line_end_then_end_line_then_zero() {
        assert_eq!(
            line_end(&node(props(&[("line_end", 12), ("end_line", 20)]))),
            12
        );
        assert_eq!(line_end(&node(props(&[("end_line", 20)]))), 20);
        assert_eq!(line_end(&node(props(&[]))), 0);
    }

    #[test]
    fn line_opt_accessors_return_none_when_absent() {
        assert_eq!(line_start_opt(&node(props(&[]))), None);
        assert_eq!(line_end_opt(&node(props(&[]))), None);
        assert_eq!(line_start_opt(&node(props(&[("start_line", 3)]))), Some(3));
        assert_eq!(line_end_opt(&node(props(&[("line_end", 7)]))), Some(7));
    }

    #[test]
    fn line_from_props_prefers_line_key_then_falls_back_then_zero() {
        // line_start_from_props: line_start wins over start_line.
        assert_eq!(
            line_start_from_props(&props(&[("line_start", 5), ("start_line", 9)])),
            5
        );
        // falls back to start_line when line_start absent.
        assert_eq!(line_start_from_props(&props(&[("start_line", 9)])), 9);
        // absent -> 0.
        assert_eq!(line_start_from_props(&props(&[])), 0);

        // line_end_from_props mirrors the same precedence.
        assert_eq!(
            line_end_from_props(&props(&[("line_end", 12), ("end_line", 20)])),
            12
        );
        assert_eq!(line_end_from_props(&props(&[("end_line", 20)])), 20);
        assert_eq!(line_end_from_props(&props(&[])), 0);
    }

    #[test]
    fn line_opt_from_props_returns_none_when_absent() {
        // Absent -> None (distinct from the u32 accessors that default to 0).
        assert_eq!(line_start_opt_from_props(&props(&[])), None);
        assert_eq!(line_end_opt_from_props(&props(&[])), None);
        // Present via the *_line fallback keys still resolves to Some.
        assert_eq!(
            line_start_opt_from_props(&props(&[("start_line", 3)])),
            Some(3)
        );
        assert_eq!(line_end_opt_from_props(&props(&[("line_end", 7)])), Some(7));
    }

    #[test]
    fn col_start_defaults_to_zero_col_end_defaults_to_ten_thousand() {
        assert_eq!(col_start_from_props(&props(&[("col_start", 4)])), 4);
        assert_eq!(col_start_from_props(&props(&[("start_col", 6)])), 6);
        assert_eq!(col_start_from_props(&props(&[])), 0);

        assert_eq!(col_end_from_props(&props(&[("col_end", 8)])), 8);
        assert_eq!(col_end_from_props(&props(&[("end_col", 11)])), 11);
        // col_end has an unusual 10000 default rather than 0.
        assert_eq!(col_end_from_props(&props(&[])), 10000);
    }

    #[test]
    fn string_accessors_have_expected_defaults() {
        let mut p = PropertyMap::new();
        p.insert("name", "do_work");
        p.insert("path", "src/lib.rs");
        p.insert("language", "rust");
        p.insert("visibility", "private");
        let n = node(p);
        assert_eq!(name(&n), "do_work");
        assert_eq!(path(&n), "src/lib.rs");
        assert_eq!(language(&n), "rust");
        assert_eq!(visibility(&n), "private");

        let empty = node(PropertyMap::new());
        assert_eq!(name(&empty), "");
        assert_eq!(path(&empty), "");
        assert_eq!(language(&empty), "");
        // visibility defaults to "public", not "".
        assert_eq!(visibility(&empty), "public");
    }

    #[test]
    fn is_public_checks_is_public_then_exported_then_visibility() {
        let mut p = PropertyMap::new();
        p.insert("is_public", true);
        assert!(is_public(&node(p)));

        let mut p = PropertyMap::new();
        p.insert("exported", true);
        assert!(is_public(&node(p)));

        // is_public wins over a private visibility fallback.
        let mut p = PropertyMap::new();
        p.insert("is_public", false);
        p.insert("visibility", "public");
        assert!(!is_public(&node(p)));

        // no bools -> falls back to visibility string.
        let mut p = PropertyMap::new();
        p.insert("visibility", "pub");
        assert!(is_public(&node(p)));

        let mut p = PropertyMap::new();
        p.insert("visibility", "private");
        assert!(!is_public(&node(p)));

        // absent visibility defaults to "public" -> public.
        assert!(is_public(&node(PropertyMap::new())));
    }

    #[test]
    fn is_test_reads_structural_marker_and_defaults_false() {
        let mut p = PropertyMap::new();
        p.insert("is_test", true);
        assert!(is_test(&node(p)));

        let mut p = PropertyMap::new();
        p.insert("is_test", false);
        assert!(!is_test(&node(p)));

        // absent -> false.
        assert!(!is_test(&node(PropertyMap::new())));
    }

    #[test]
    fn get_int_parses_string_backed_numeric_props() {
        // get_int accepts a String that parses as an integer, so a
        // string-typed line_start still resolves through the accessor.
        let mut p = PropertyMap::new();
        p.insert("line_start", "42");
        assert_eq!(line_start(&node(p)), 42);
    }
}
