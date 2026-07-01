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
    use crate::{helpers, PropertyMap, PropertyValue};

    #[test]
    fn test_escape_dot_label() {
        assert_eq!(escape_dot_label("hello"), "hello");
        assert_eq!(escape_dot_label("line\\nbreak"), "line\\\\nbreak");
        assert_eq!(escape_dot_label("quote\"here"), "quote\\\"here");
    }

    #[test]
    fn test_escape_dot_label_real_newline() {
        // An actual newline byte is escaped to the two-character \n sequence.
        assert_eq!(escape_dot_label("a\nb"), "a\\nb");
    }

    #[test]
    fn test_format_property_value_scalars() {
        assert_eq!(
            format_property_value(&PropertyValue::String("hi".to_string())),
            "hi"
        );
        assert_eq!(format_property_value(&PropertyValue::Int(42)), "42");
        assert_eq!(format_property_value(&PropertyValue::Float(1.5)), "1.5");
        assert_eq!(format_property_value(&PropertyValue::Bool(true)), "true");
        // Unlike the CSV exporter (empty string), DOT renders Null as "null".
        assert_eq!(format_property_value(&PropertyValue::Null), "null");
    }

    #[test]
    fn test_format_property_value_lists_joined_with_comma() {
        // The DOT exporter joins list variants with ',' (the CSV one uses ';').
        assert_eq!(
            format_property_value(&PropertyValue::StringList(vec![
                "a".to_string(),
                "b".to_string(),
            ])),
            "a,b"
        );
        assert_eq!(
            format_property_value(&PropertyValue::IntList(vec![1, 2, 3])),
            "1,2,3"
        );
    }

    #[test]
    fn test_export_dot_header() {
        let graph = CodeGraph::in_memory().unwrap();
        let dot = export_dot(&graph).unwrap();
        assert!(dot.starts_with("digraph code_graph {\n"));
        // Default rankdir is LR and nodes are filled.
        assert!(dot.contains("rankdir=LR;"));
        assert!(dot.contains("node [style=filled];"));
        assert!(dot.trim_end().ends_with('}'));
    }

    #[test]
    fn test_export_dot_node_label_and_styling() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let file = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        helpers::add_function(&mut graph, file, "do_thing", 1, 5).unwrap();

        let dot = export_dot(&graph).unwrap();
        // The file node has no "name" prop, so it falls back to its path.
        assert!(dot.contains("n0 [label=\"a.py\", shape=folder, fillcolor=\"#E0E0E0\"];"));
        // The function node uses its name and the Function color/shape defaults.
        assert!(dot.contains("n1 [label=\"do_thing\", shape=box, fillcolor=\"#90CAF9\"];"));
    }

    #[test]
    fn test_export_dot_label_fallback_to_node_id() {
        let mut graph = CodeGraph::in_memory().unwrap();
        // A node with neither "name" nor "path" falls back to "n{id}" and the
        // default white color / box shape for an unstyled node type.
        graph
            .add_node(NodeType::Variable, PropertyMap::new())
            .unwrap();

        let dot = export_dot_styled(&graph, DotOptions::default()).unwrap();
        assert!(dot.contains("n0 [label=\"n0\", shape=ellipse, fillcolor=\"#CE93D8\"];"));
    }

    #[test]
    fn test_export_dot_unstyled_type_defaults() {
        let mut graph = CodeGraph::in_memory().unwrap();
        graph
            .add_node(NodeType::Variable, PropertyMap::new())
            .unwrap();

        // Empty options -> no color/shape entries -> white fill and box shape.
        let opts = DotOptions {
            node_colors: HashMap::new(),
            edge_colors: HashMap::new(),
            node_shapes: HashMap::new(),
            rankdir: "LR".to_string(),
            show_properties: vec![],
        };
        let dot = export_dot_styled(&graph, opts).unwrap();
        assert!(dot.contains("n0 [label=\"n0\", shape=box, fillcolor=\"#FFFFFF\"];"));
    }

    #[test]
    fn test_export_dot_show_properties_appends_to_label() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let file = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        helpers::add_function(&mut graph, file, "f", 10, 20).unwrap();

        let opts = DotOptions {
            show_properties: vec!["line_start".to_string()],
            ..DotOptions::default()
        };
        let dot = export_dot_styled(&graph, opts).unwrap();
        // The requested property is appended to the function label after a \n.
        assert!(dot.contains("label=\"f\\nline_start:10\""));
    }

    #[test]
    fn test_export_dot_edges_rendered_with_label() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();

        let dot = export_dot(&graph).unwrap();
        // The import edge is emitted as a directed edge labeled by its type.
        assert!(dot.contains("n0 -> n1 [label=\"Imports\"];"));
    }

    #[test]
    fn test_export_dot_styled_custom_rankdir_and_edge_color() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();

        let mut edge_colors = HashMap::new();
        edge_colors.insert(EdgeType::Imports, "#FF0000".to_string());
        let opts = DotOptions {
            rankdir: "TB".to_string(),
            edge_colors,
            ..DotOptions::default()
        };
        let dot = export_dot_styled(&graph, opts).unwrap();
        assert!(dot.contains("rankdir=TB;"));
        // A configured edge color is appended to the edge attributes.
        assert!(dot.contains("n0 -> n1 [label=\"Imports\", color=\"#FF0000\"];"));
    }
}
