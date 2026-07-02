// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! JSON format export for D3.js and web visualization tools.
//!
//! Generates JSON with "nodes" and "links" arrays compatible with D3.js force-directed layouts.

use crate::{CodeGraph, Node, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

/// Export graph to D3.js-compatible JSON format
pub fn export_json(graph: &CodeGraph) -> Result<String> {
    let mut nodes_array = Vec::new();
    let mut links_array = Vec::new();

    // Export all nodes
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            nodes_array.push(node_to_json(node_id, node));
        }
    }

    // Export all edges
    for edge_id in 0..graph.edge_count() as u64 {
        if let Ok(edge) = graph.get_edge(edge_id) {
            links_array.push(json!({
                "id": edge_id,
                "source": edge.source_id,
                "target": edge.target_id,
                "type": format!("{:?}", edge.edge_type),
                "properties": properties_to_json(&edge.properties),
            }));
        }
    }

    let result = json!({
        "nodes": nodes_array,
        "links": links_array,
    });

    // serde_json::to_string_pretty should never fail for our data structures
    Ok(serde_json::to_string_pretty(&result).expect("Failed to serialize JSON"))
}

/// Export filtered subset of graph to JSON
pub fn export_json_filtered(
    graph: &CodeGraph,
    node_filter: impl Fn(&Node) -> bool,
    include_edges: bool,
) -> Result<String> {
    let mut nodes_array = Vec::new();
    let mut filtered_ids = HashSet::new();

    // Export filtered nodes
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            if node_filter(node) {
                nodes_array.push(node_to_json(node_id, node));
                filtered_ids.insert(node_id);
            }
        }
    }

    // Export edges if requested
    let mut links_array = Vec::new();
    if include_edges {
        for edge_id in 0..graph.edge_count() as u64 {
            if let Ok(edge) = graph.get_edge(edge_id) {
                // Only include edges between filtered nodes
                if filtered_ids.contains(&edge.source_id) && filtered_ids.contains(&edge.target_id)
                {
                    links_array.push(json!({
                        "id": edge_id,
                        "source": edge.source_id,
                        "target": edge.target_id,
                        "type": format!("{:?}", edge.edge_type),
                        "properties": properties_to_json(&edge.properties),
                    }));
                }
            }
        }
    }

    let result = json!({
        "nodes": nodes_array,
        "links": links_array,
    });

    // serde_json::to_string_pretty should never fail for our data structures
    Ok(serde_json::to_string_pretty(&result).expect("Failed to serialize JSON"))
}

/// Convert node to JSON object
fn node_to_json(node_id: u64, node: &Node) -> Value {
    json!({
        "id": node_id,
        "type": format!("{:?}", node.node_type),
        "properties": properties_to_json(&node.properties),
    })
}

/// Convert PropertyMap to JSON object
fn properties_to_json(props: &crate::PropertyMap) -> Value {
    let mut obj = serde_json::Map::new();

    for (key, value) in props.iter() {
        let json_value = match value {
            crate::PropertyValue::String(s) => json!(s),
            crate::PropertyValue::Int(i) => json!(i),
            crate::PropertyValue::Float(f) => json!(f),
            crate::PropertyValue::Bool(b) => json!(b),
            crate::PropertyValue::StringList(v) => json!(v),
            crate::PropertyValue::IntList(v) => json!(v),
            crate::PropertyValue::Null => json!(null),
        };
        obj.insert(key.clone(), json_value);
    }

    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{helpers, NodeType, PropertyMap, PropertyValue};

    #[test]
    fn test_properties_to_json() {
        let mut props = PropertyMap::new();
        props.insert("name", "test");
        props.insert("count", 42);

        let json = properties_to_json(&props);
        assert!(json.is_object());
        assert_eq!(json["name"], "test");
        assert_eq!(json["count"], 42);
    }

    #[test]
    fn test_properties_to_json_all_variants() {
        let mut props = PropertyMap::new();
        props.insert("s", PropertyValue::String("hi".to_string()));
        props.insert("i", PropertyValue::Int(7));
        props.insert("f", PropertyValue::Float(1.5));
        props.insert("b", PropertyValue::Bool(true));
        props.insert(
            "sl",
            PropertyValue::StringList(vec!["a".to_string(), "b".to_string()]),
        );
        props.insert("il", PropertyValue::IntList(vec![1, 2, 3]));
        props.insert("n", PropertyValue::Null);

        let json = properties_to_json(&props);
        // Each variant maps to its native JSON type (lists stay arrays, Null -> null).
        assert_eq!(json["s"], "hi");
        assert_eq!(json["i"], 7);
        assert_eq!(json["f"], 1.5);
        assert_eq!(json["b"], true);
        assert_eq!(json["sl"], json!(["a", "b"]));
        assert_eq!(json["il"], json!([1, 2, 3]));
        assert_eq!(json["n"], Value::Null);
    }

    #[test]
    fn test_node_to_json_shape() {
        let props = PropertyMap::new().with("name", "foo");
        let node = Node::new(0, NodeType::Function, props);

        let json = node_to_json(3, &node);
        assert_eq!(json["id"], 3);
        // node_type is rendered via Debug formatting.
        assert_eq!(json["type"], "Function");
        assert_eq!(json["properties"]["name"], "foo");
    }

    #[test]
    fn test_export_json_nodes_and_links() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec!["foo"]).unwrap();

        let out = export_json(&graph).unwrap();
        let value: Value = serde_json::from_str(&out).unwrap();

        let nodes = value["nodes"].as_array().unwrap();
        let links = value["links"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(links.len(), 1);

        let link = &links[0];
        assert_eq!(link["source"], 0);
        assert_eq!(link["target"], 1);
        assert_eq!(link["type"], "Imports");
        assert_eq!(link["properties"]["symbols"], json!(["foo"]));
    }

    #[test]
    fn test_export_json_filtered_selects_nodes() {
        let mut graph = CodeGraph::in_memory().unwrap();
        helpers::add_file(&mut graph, "a.py", "python").unwrap();
        graph
            .add_node(NodeType::Function, PropertyMap::new().with("name", "f"))
            .unwrap();

        let out =
            export_json_filtered(&graph, |n| n.node_type == NodeType::Function, false).unwrap();
        let value: Value = serde_json::from_str(&out).unwrap();

        let nodes = value["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["type"], "Function");
        // include_edges=false always yields an empty links array.
        assert!(value["links"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_export_json_filtered_drops_edges_to_excluded_nodes() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();

        // Keep only the first file; the import edge points at the excluded node.
        let out = export_json_filtered(
            &graph,
            |n| n.properties.get("path") == Some(&PropertyValue::String("a.py".to_string())),
            true,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(value["nodes"].as_array().unwrap().len(), 1);
        // Edge is dropped because its target is not in the filtered set.
        assert!(value["links"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_export_json_filtered_keeps_edges_between_included_nodes() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();

        // Both endpoints are CodeFile nodes, so the edge survives.
        let out =
            export_json_filtered(&graph, |n| n.node_type == NodeType::CodeFile, true).unwrap();
        let value: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(value["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(value["links"].as_array().unwrap().len(), 1);
    }
}
