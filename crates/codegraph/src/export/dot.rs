// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! DOT format export for Graphviz visualization.
//!
//! Generates Graphviz DOT format for rendering graphs as images or interactive visualizations.

use crate::{CodeGraph, EdgeType, NodeType, Result};
use std::collections::HashMap;

/// Options for styling DOT export
#[derive(Debug, Clone)]
pub struct DotOptions {
    /// Node colors by type (hex color codes)
    pub node_colors: HashMap<NodeType, String>,
    /// Edge colors by type (hex color codes)
    pub edge_colors: HashMap<EdgeType, String>,
    /// Node shapes by type (box, circle, folder, etc.)
    pub node_shapes: HashMap<NodeType, String>,
    /// Graph layout direction: LR, TB, RL, BT
    pub rankdir: String,
    /// Property names to show in node labels
    pub show_properties: Vec<String>,
}

impl Default for DotOptions {
    fn default() -> Self {
        let mut node_colors = HashMap::new();
        node_colors.insert(NodeType::CodeFile, "#E0E0E0".to_string());
        node_colors.insert(NodeType::Function, "#90CAF9".to_string());
        node_colors.insert(NodeType::Class, "#FFE082".to_string());
        node_colors.insert(NodeType::Variable, "#CE93D8".to_string());
        node_colors.insert(NodeType::Interface, "#FFAB91".to_string());
        node_colors.insert(NodeType::Module, "#BCAAA4".to_string());

        let mut node_shapes = HashMap::new();
        node_shapes.insert(NodeType::CodeFile, "folder".to_string());
        node_shapes.insert(NodeType::Function, "box".to_string());
        node_shapes.insert(NodeType::Class, "component".to_string());
        node_shapes.insert(NodeType::Variable, "ellipse".to_string());
        node_shapes.insert(NodeType::Interface, "diamond".to_string());
        node_shapes.insert(NodeType::Module, "folder".to_string());

        DotOptions {
            node_colors,
            edge_colors: HashMap::new(),
            node_shapes,
            rankdir: "LR".to_string(),
            show_properties: vec![],
        }
    }
}

/// Export graph to Graphviz DOT format
pub fn export_dot(graph: &CodeGraph) -> Result<String> {
    export_dot_styled(graph, DotOptions::default())
}

/// Export graph to Graphviz DOT format with custom styling
pub fn export_dot_styled(graph: &CodeGraph, options: DotOptions) -> Result<String> {
    let mut output = String::new();

    // Header
    output.push_str("digraph code_graph {\n");
    output.push_str(&format!("    rankdir={};\n", options.rankdir));
    output.push_str("    node [style=filled];\n\n");

    // Export nodes - iterate through all node IDs
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            // Build label
            let mut label = if let Some(name) = node.properties.get_string("name") {
                escape_dot_label(name)
            } else if let Some(path) = node.properties.get_string("path") {
                escape_dot_label(path)
            } else {
                format!("n{node_id}")
            };

            // Add properties to label if requested
            for prop_name in &options.show_properties {
                if let Some(value) = node.properties.get(prop_name) {
                    label.push_str(&format!(
                        "\\n{}:{}",
                        prop_name,
                        format_property_value(value)
                    ));
                }
            }

            // Get styling
            let color = options
                .node_colors
                .get(&node.node_type)
                .map(|s| s.as_str())
                .unwrap_or("#FFFFFF");

            let shape = options
                .node_shapes
                .get(&node.node_type)
                .map(|s| s.as_str())
                .unwrap_or("box");

            output.push_str(&format!(
                "    n{node_id} [label=\"{label}\", shape={shape}, fillcolor=\"{color}\"];\n"
            ));
        }
    }

    output.push('\n');

    // Export edges - iterate through all edge IDs
    for edge_id in 0..graph.edge_count() as u64 {
        if let Ok(edge) = graph.get_edge(edge_id) {
            let edge_label = format!("{:?}", edge.edge_type);

            let color = options
                .edge_colors
                .get(&edge.edge_type)
                .map(|c| format!(", color=\"{c}\""))
                .unwrap_or_default();

            output.push_str(&format!(
                "    n{} -> n{} [label=\"{}\"{}];\n",
                edge.source_id, edge.target_id, edge_label, color
            ));
        }
    }

    output.push_str("}\n");

    Ok(output)
}

/// Escape special characters for DOT labels
fn escape_dot_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Format property value for display
fn format_property_value(value: &crate::PropertyValue) -> String {
    match value {
        crate::PropertyValue::String(s) => s.clone(),
        crate::PropertyValue::Int(i) => i.to_string(),
        crate::PropertyValue::Float(f) => f.to_string(),
        crate::PropertyValue::Bool(b) => b.to_string(),
        crate::PropertyValue::StringList(v) => v.join(","),
        crate::PropertyValue::IntList(v) => v
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(","),
        crate::PropertyValue::Null => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_dot_label() {
        assert_eq!(escape_dot_label("hello"), "hello");
        assert_eq!(escape_dot_label("line\\nbreak"), "line\\\\nbreak");
        assert_eq!(escape_dot_label("quote\"here"), "quote\\\"here");
    }
}
