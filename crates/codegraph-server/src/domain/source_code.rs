// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Source code access — unified get_symbol_source.

use codegraph::{CodeGraph, NodeId};

use crate::domain::node_props;

/// Read source code for a graph node from disk.
///
/// Reads the file path and line range from node properties, then extracts
/// the corresponding lines from the file. Checks for an inline `source`
/// property first before attempting disk I/O.
pub(crate) fn get_symbol_source(graph: &CodeGraph, node_id: NodeId) -> Option<String> {
    let node = graph.get_node(node_id).ok()?;

    // Check for inline source first
    if let Some(source) = node.properties.get_string("source") {
        return Some(source.to_string());
    }

    let path = node.properties.get_string("path")?;
    let start_line = node_props::line_start_opt(node)? as usize;
    let end_line = node_props::line_end_opt(node)? as usize;

    let content = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    if start_line > 0 && end_line <= lines.len() {
        Some(lines[start_line - 1..end_line].join("\n"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{NodeType, PropertyMap, PropertyValue};
    use std::io::Write;

    /// Add a Function node from a set of properties, returning its id.
    fn add_node(graph: &mut CodeGraph, props: PropertyMap) -> NodeId {
        graph.add_node(NodeType::Function, props).expect("add_node")
    }

    fn props_with(pairs: &[(&str, PropertyValue)]) -> PropertyMap {
        let mut props = PropertyMap::new();
        for (k, v) in pairs {
            props.insert((*k).to_string(), v.clone());
        }
        props
    }

    #[test]
    fn missing_node_returns_none() {
        let g = CodeGraph::in_memory().expect("in_memory");
        // NodeId 999 was never added.
        assert_eq!(get_symbol_source(&g, 999), None);
    }

    #[test]
    fn inline_source_takes_precedence_over_disk() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Both an inline source and a (nonexistent) path are set; the inline
        // source must win without any disk I/O being attempted.
        let n = add_node(
            &mut g,
            props_with(&[
                (
                    "source",
                    PropertyValue::String("fn inline() {}".to_string()),
                ),
                (
                    "path",
                    PropertyValue::String("/nonexistent/file.rs".to_string()),
                ),
                ("line_start", PropertyValue::Int(1)),
                ("line_end", PropertyValue::Int(1)),
            ]),
        );
        assert_eq!(get_symbol_source(&g, n), Some("fn inline() {}".to_string()));
    }

    #[test]
    fn missing_path_returns_none() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // No inline source and no path property.
        let n = add_node(
            &mut g,
            props_with(&[
                ("line_start", PropertyValue::Int(1)),
                ("line_end", PropertyValue::Int(2)),
            ]),
        );
        assert_eq!(get_symbol_source(&g, n), None);
    }

    #[test]
    fn missing_line_range_returns_none() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Path present but no line_start/line_end.
        let n = add_node(
            &mut g,
            props_with(&[(
                "path",
                PropertyValue::String("/tmp/whatever.rs".to_string()),
            )]),
        );
        assert_eq!(get_symbol_source(&g, n), None);
    }

    #[test]
    fn missing_end_line_with_start_present_returns_none() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // path + line_start present, but line_end absent: passes the
        // line_start_opt `?` and must short-circuit at line_end_opt `?`.
        let n = add_node(
            &mut g,
            props_with(&[
                (
                    "path",
                    PropertyValue::String("/tmp/whatever.rs".to_string()),
                ),
                ("line_start", PropertyValue::Int(1)),
            ]),
        );
        assert_eq!(get_symbol_source(&g, n), None);
    }

    #[test]
    fn reads_line_range_from_disk() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(file, "line one\nline two\nline three\nline four").expect("write");
        let path = file.path().to_str().expect("utf8 path").to_string();

        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Lines 2..=3 (1-indexed, inclusive) => "line two\nline three".
        let n = add_node(
            &mut g,
            props_with(&[
                ("path", PropertyValue::String(path)),
                ("line_start", PropertyValue::Int(2)),
                ("line_end", PropertyValue::Int(3)),
            ]),
        );
        assert_eq!(
            get_symbol_source(&g, n),
            Some("line two\nline three".to_string())
        );
    }

    #[test]
    fn end_line_past_eof_returns_none() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(file, "only line").expect("write");
        let path = file.path().to_str().expect("utf8 path").to_string();

        let mut g = CodeGraph::in_memory().expect("in_memory");
        // end_line 5 exceeds the single line in the file.
        let n = add_node(
            &mut g,
            props_with(&[
                ("path", PropertyValue::String(path)),
                ("line_start", PropertyValue::Int(1)),
                ("line_end", PropertyValue::Int(5)),
            ]),
        );
        assert_eq!(get_symbol_source(&g, n), None);
    }

    #[test]
    fn zero_start_line_returns_none() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(file, "a\nb\nc").expect("write");
        let path = file.path().to_str().expect("utf8 path").to_string();

        let mut g = CodeGraph::in_memory().expect("in_memory");
        // start_line 0 fails the `start_line > 0` guard.
        let n = add_node(
            &mut g,
            props_with(&[
                ("path", PropertyValue::String(path)),
                ("line_start", PropertyValue::Int(0)),
                ("line_end", PropertyValue::Int(2)),
            ]),
        );
        assert_eq!(get_symbol_source(&g, n), None);
    }

    #[test]
    fn nonexistent_path_returns_none() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Valid line range but the file does not exist on disk.
        let n = add_node(
            &mut g,
            props_with(&[
                (
                    "path",
                    PropertyValue::String("/nonexistent/does/not/exist.rs".to_string()),
                ),
                ("line_start", PropertyValue::Int(1)),
                ("line_end", PropertyValue::Int(1)),
            ]),
        );
        assert_eq!(get_symbol_source(&g, n), None);
    }
}
