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
